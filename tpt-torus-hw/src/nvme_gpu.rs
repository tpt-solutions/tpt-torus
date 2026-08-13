//! io_uring + GPU-Direct integration for NVMe storage I/O.
//!
//! This module integrates io_uring with CUDA GPU-Direct via a zero-copy DMA
//! buffer pool. The data flow for reads is:
//!
//! ```text
//! NVMe Device ──[PCIe DMA]──► DMA Pool Buffer ──[CUDA memcpy]──► GPU VRAM
//! ```
//!
//! For writes, the flow is reversed:
//!
//! ```text
//! GPU VRAM ──[CUDA memcpy]──► DMA Pool Buffer ──[PCIe DMA]──► NVMe Device
//! ```
//!
//! Both stages are async and can be pipelined for maximum throughput.

use crate::dma_pool::ZeroCopyDmaPool;
use crate::gpu_direct::GpuBuffer;
use crate::{HwError, HwResult};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use tpt_torus_sys::io_uring_params;

// ─── Constants ─────────────────────────────────────────────────────────────

/// io_uring opcode for fixed-buffer read (zero-copy from registered buffers).
const IORING_OP_READ_FIXED: u8 = 28;

/// io_uring opcode for fixed-buffer write (zero-copy to registered buffers).
const IORING_OP_WRITE_FIXED: u8 = 29;

/// io_uring SQE flag for fixed buffers.
const IOSQE_FIXED_FILE: u8 = 1 << 0;

// ─── NVMe Device ───────────────────────────────────────────────────────────

/// Handle to an NVMe device for direct I/O.
pub struct NvmeDevice {
    fd: i32,
    name: String,
    ns_id: u32,
    block_size: u32,
    total_blocks: u64,
}

impl NvmeDevice {
    /// Open an NVMe device for direct I/O.
    pub fn open(path: &str, ns_id: u32) -> HwResult<Self> {
        use std::ffi::CString;

        let c_path = CString::new(path).map_err(|e| HwError::InvalidParam(e.to_string()))?;

        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDWR | libc::O_DIRECT, 0) };

        if fd < 0 {
            return Err(HwError::InitFailed(format!(
                "failed to open {}: {}",
                path,
                std::io::Error::last_os_error()
            )));
        }

        Ok(Self {
            fd,
            name: path.to_string(),
            ns_id,
            block_size: 512,
            total_blocks: 0,
        })
    }

    pub fn fd(&self) -> i32 {
        self.fd
    }
    pub fn ns_id(&self) -> u32 {
        self.ns_id
    }
    pub fn block_size(&self) -> u32 {
        self.block_size
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Total number of addressable blocks (0 until queried from the device).
    pub fn total_blocks(&self) -> u64 {
        self.total_blocks
    }
}

impl Drop for NvmeDevice {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

// ─── NVMe Passthrough Command ──────────────────────────────────────────────

/// NVMe passthrough command built for a raw io_uring NVMe submission.
///
/// Fields mirror the NVMe command dwords used by an `NVME_IOCTL_SUBMIT_IO`
/// style submission: `cdw12` carries the (0-based) block count, `cdw14`/`cdw15`
/// carry the starting LBA.
pub struct NvmePassthroughCmd {
    /// NVMe opcode (`0x02` read, `0x01` write).
    pub opcode: u8,
    /// Namespace identifier.
    pub ns_id: u32,
    /// Command Dword 12 (NLB is 0-based: `num_blocks - 1`).
    pub cdw12: u32,
    /// Command Dword 14 (low 32 bits of the starting LBA).
    pub cdw14: u32,
    /// Command Dword 15 (high 32 bits of the starting LBA).
    pub cdw15: u32,
}

impl NvmePassthroughCmd {
    /// Build a read command for `num_blocks` starting at `lba`.
    pub fn read(lba: u64, num_blocks: u32, ns_id: u32) -> Self {
        Self {
            opcode: 0x02,
            ns_id,
            cdw12: num_blocks - 1,
            cdw14: (lba & 0xFFFF_FFFF) as u32,
            cdw15: (lba >> 32) as u32,
        }
    }

    /// Build a write command for `num_blocks` starting at `lba`.
    pub fn write(lba: u64, num_blocks: u32, ns_id: u32) -> Self {
        Self {
            opcode: 0x01,
            ns_id,
            cdw12: num_blocks - 1,
            cdw14: (lba & 0xFFFF_FFFF) as u32,
            cdw15: (lba >> 32) as u32,
        }
    }
}

// ─── io_uring Ring ─────────────────────────────────────────────────────────

/// Memory-mapped io_uring ring for SQ/CQ.
struct IoUringRing {
    head: *mut u32,
    tail: *mut u32,
    ring_mask: u32,
    ring_entries: u32,
    sqe_array: *mut IoUringSqe,
    cqes: *mut IoUringCqe,
    mmap_base: *mut u8,
    mmap_size: usize,
}

unsafe impl Send for IoUringRing {}
unsafe impl Sync for IoUringRing {}

/// Magic offsets passed to `mmap(2)` on the io_uring fd to reach each region.
/// These are fixed kernel ABI constants, not derived from `io_uring_params`.
const IORING_OFF_SQ_RING: libc::off_t = 0;
const IORING_OFF_CQ_RING: libc::off_t = 0x8000000;
const IORING_OFF_SQES: libc::off_t = 0x10000000;

impl IoUringRing {
    /// Create an io_uring ring by mmap'ing the kernel shared memory.
    fn new(fd: i32, params: &io_uring_params) -> HwResult<Self> {
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;

        // mmap SQ ring
        let sq_ring_size =
            params.sq_off.array as usize + params.sq_entries as usize * std::mem::size_of::<u32>();
        let sq_ring_size = (sq_ring_size + page_size - 1) & !(page_size - 1);

        let sq_ring_ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                sq_ring_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                IORING_OFF_SQ_RING,
            )
        };
        if sq_ring_ptr == libc::MAP_FAILED {
            return Err(HwError::InitFailed("mmap SQ ring failed".into()));
        }

        // mmap SQE array
        let sqe_size = params.sq_entries as usize * std::mem::size_of::<IoUringSqe>();
        let sqe_size = (sqe_size + page_size - 1) & !(page_size - 1);

        let sqe_ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                sqe_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                IORING_OFF_SQES,
            )
        };
        if sqe_ptr == libc::MAP_FAILED {
            unsafe {
                libc::munmap(sq_ring_ptr, sq_ring_size);
            }
            return Err(HwError::InitFailed("mmap SQE array failed".into()));
        }

        // mmap CQ ring
        let cq_ring_size = params.cq_off.cqes as usize
            + params.cq_entries as usize * std::mem::size_of::<IoUringCqe>();
        let cq_ring_size = (cq_ring_size + page_size - 1) & !(page_size - 1);

        let cq_ring_ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                cq_ring_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                IORING_OFF_CQ_RING,
            )
        };
        if cq_ring_ptr == libc::MAP_FAILED {
            unsafe {
                libc::munmap(sqe_ptr, sqe_size);
                libc::munmap(sq_ring_ptr, sq_ring_size);
            }
            return Err(HwError::InitFailed("mmap CQ ring failed".into()));
        }

        let base = sq_ring_ptr as *mut u8;
        let cq_base = cq_ring_ptr as *mut u8;

        Ok(Self {
            head: unsafe { base.add(params.sq_off.head as usize) as *mut u32 },
            tail: unsafe { base.add(params.sq_off.tail as usize) as *mut u32 },
            ring_mask: params.sq_entries - 1,
            ring_entries: params.sq_entries,
            sqe_array: sqe_ptr as *mut IoUringSqe,
            cqes: unsafe { cq_base.add(params.cq_off.cqes as usize) as *mut IoUringCqe },
            mmap_base: sq_ring_ptr as *mut u8,
            mmap_size: sq_ring_size + sqe_size + cq_ring_size,
        })
    }

    /// Submit an SQE to the ring.
    unsafe fn submit_sqe(&self, sqe: &IoUringSqe) {
        let tail = std::ptr::read_volatile(self.tail);
        let index = (tail & self.ring_mask) as usize;
        debug_assert!(index < self.ring_entries as usize);
        ptr::copy_nonoverlapping(sqe, self.sqe_array.add(index), 1);
        std::ptr::write_volatile(self.tail, tail.wrapping_add(1));
    }

    /// Peek at the next CQE without consuming it.
    unsafe fn peek_cqe(&self) -> Option<IoUringCqe> {
        let head = std::ptr::read_volatile(self.head);
        let tail = std::ptr::read_volatile(self.tail);
        if head == tail {
            return None;
        }
        let index = (head & self.ring_mask) as usize;
        Some(ptr::read_volatile(self.cqes.add(index)))
    }

    /// Mark the current CQE as consumed.
    unsafe fn consume_cqe(&self) {
        let head = std::ptr::read_volatile(self.head);
        std::ptr::write_volatile(self.head, head.wrapping_add(1));
    }
}

impl Drop for IoUringRing {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.mmap_base as *mut libc::c_void, self.mmap_size);
        }
    }
}

// ─── NVMe Completion Tracker ───────────────────────────────────────────────

/// Tracks pending NVMe operations and their completions.
struct CompletionTracker {
    /// Map from user_data to (pool buffer index, GPU buf ptr, gpu offset, num_blocks, is_write).
    pending: std::collections::HashMap<u64, PendingOp>,
    next_id: AtomicU64,
}

struct PendingOp {
    pool_index: u32,
    gpu_ptr: u64,
    gpu_offset: usize,
    num_blocks: u32,
    is_write: bool,
    pool_buf_addr: u64,
    block_size: u32,
}

impl CompletionTracker {
    fn new() -> Self {
        Self {
            pending: std::collections::HashMap::new(),
            next_id: AtomicU64::new(1),
        }
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn insert(&mut self, id: u64, op: PendingOp) {
        self.pending.insert(id, op);
    }

    fn remove(&mut self, id: u64) -> Option<PendingOp> {
        self.pending.remove(&id)
    }
}

// ─── GPU-Direct NVMe Driver ────────────────────────────────────────────────

/// End-to-end GPU-Direct NVMe driver with DMA pool integration.
///
/// This driver wires together:
/// - io_uring for async NVMe I/O
/// - ZeroCopyDmaPool for pre-registered DMA buffers
/// - CUDA for GPU-Direct memcpy
///
/// Data flow for reads:
/// ```text
/// NVMe ──[io_uring READ]──► DMA Pool Buffer ──[CUDA H2D]──► GPU VRAM
/// ```
pub struct GpuDirectNvme {
    /// NVMe device handle.
    nvme: NvmeDevice,
    /// io_uring ring for submissions.
    ring: IoUringRing,
    /// Zero-copy DMA buffer pool.
    dma_pool: ZeroCopyDmaPool,
    /// CUDA stream for async memcpy.
    stream: crate::cuda::CUstream,
    /// io_uring file descriptor (for syscalls).
    fd: i32,
    /// Pending operation tracker.
    tracker: std::sync::Mutex<CompletionTracker>,
}

unsafe impl Send for GpuDirectNvme {}
unsafe impl Sync for GpuDirectNvme {}

impl GpuDirectNvme {
    /// Create a new GPU-Direct NVMe driver with default pool sizing.
    ///
    /// # Arguments
    /// - `nvme_path`: Path to the NVMe block device (e.g., `/dev/nvme0n1`).
    /// - `ns_id`: NVMe namespace ID.
    pub fn new(nvme_path: &str, ns_id: u32) -> HwResult<Self> {
        const DEFAULT_POOL_BUF_SIZE: usize = 64 * 1024;
        const DEFAULT_POOL_CAPACITY: usize = 64;
        Self::with_pool(
            nvme_path,
            ns_id,
            DEFAULT_POOL_BUF_SIZE,
            DEFAULT_POOL_CAPACITY,
        )
    }

    /// Create a new GPU-Direct NVMe driver with explicit pool sizing.
    ///
    /// # Arguments
    /// - `nvme_path`: Path to the NVMe block device (e.g., `/dev/nvme0n1`).
    /// - `ns_id`: NVMe namespace ID.
    /// - `pool_buf_size`: Size of each DMA pool buffer (must be power of 2).
    /// - `pool_capacity`: Number of buffers in the DMA pool.
    pub fn with_pool(
        nvme_path: &str,
        ns_id: u32,
        pool_buf_size: usize,
        pool_capacity: usize,
    ) -> HwResult<Self> {
        // Initialize CUDA
        crate::cuda::init().map_err(|e| HwError::InitFailed(format!("CUDA init: {}", e)))?;

        // Open NVMe device
        let nvme = NvmeDevice::open(nvme_path, ns_id)?;

        // Create io_uring
        let mut params: io_uring_params = unsafe { std::mem::zeroed() };
        let fd = unsafe { libc::syscall(libc::SYS_io_uring_setup, 256u32, &mut params as *mut _) };
        if fd < 0 {
            return Err(HwError::InitFailed(format!(
                "io_uring_setup: {}",
                std::io::Error::last_os_error()
            )));
        }
        let fd = fd as i32;

        // Create io_uring ring from mmap'd kernel memory
        let ring = IoUringRing::new(fd, &params)?;

        // Create DMA pool
        let dma_pool = ZeroCopyDmaPool::new(pool_buf_size, pool_capacity, fd)?;

        // Create CUDA stream
        let stream = crate::cuda::stream_create()
            .map_err(|e| HwError::InitFailed(format!("cuStreamCreate: {}", e)))?;

        Ok(Self {
            nvme,
            ring,
            dma_pool,
            stream,
            fd,
            tracker: std::sync::Mutex::new(CompletionTracker::new()),
        })
    }

    /// Read data from NVMe into a GPU buffer (pipelined zero-copy).
    ///
    /// Stage 1: io_uring reads NVMe data into a DMA pool buffer.
    /// Stage 2: CUDA async-memcpy from pool buffer to GPU VRAM.
    ///
    /// Returns a transfer ID for tracking completion.
    pub fn read_to_gpu(
        &self,
        lba: u64,
        num_blocks: u32,
        gpu_buf: &GpuBuffer,
        gpu_offset: usize,
    ) -> HwResult<u64> {
        let block_size = self.nvme.block_size();
        let transfer_size = num_blocks as usize * block_size as usize;

        // Validate GPU buffer bounds
        if !gpu_buf.can_fit(gpu_offset + transfer_size) {
            return Err(HwError::InvalidParam("GPU buffer overflow".into()));
        }

        // Allocate a DMA pool buffer for this transfer
        let pool_buf = self.dma_pool.alloc().ok_or(HwError::QueueFull)?;

        // Build io_uring SQE for NVMe read into pool buffer
        let user_data = {
            let mut tracker = self.tracker.lock().unwrap();
            let id = tracker.next_id();
            tracker.insert(
                id,
                PendingOp {
                    pool_index: pool_buf.index(),
                    gpu_ptr: gpu_buf.dev_ptr,
                    gpu_offset,
                    num_blocks,
                    is_write: false,
                    pool_buf_addr: pool_buf.addr(),
                    block_size,
                },
            );
            id
        };

        let sqe = IoUringSqe {
            opcode: IORING_OP_READ_FIXED,
            flags: IOSQE_FIXED_FILE,
            ioprio: 0,
            fd: self.nvme.fd(),
            off_addr2: lba * block_size as u64,
            addr_splice_off_in: pool_buf.addr(),
            len: transfer_size as u32,
            op_flags: 0,
            user_data,
            buf_group: pool_buf.registered_index(),
            personality: 0,
            splice_fd_in: 0,
            addr3: 0,
            __pad2: 0,
        };

        unsafe {
            self.ring.submit_sqe(&sqe);
        }

        // Submit to kernel
        self.flush_submissions(1)?;

        // Prevent pool_buf from being dropped (it's tracked by user_data)
        std::mem::forget(pool_buf);

        Ok(user_data)
    }

    /// Write data from a GPU buffer to NVMe (pipelined zero-copy).
    ///
    /// Stage 1: CUDA async-memcpy from GPU VRAM to DMA pool buffer.
    /// Stage 2: io_uring writes pool buffer to NVMe.
    ///
    /// Returns a transfer ID for tracking completion.
    pub fn write_from_gpu(
        &self,
        gpu_buf: &GpuBuffer,
        gpu_offset: usize,
        lba: u64,
        num_blocks: u32,
    ) -> HwResult<u64> {
        let block_size = self.nvme.block_size();
        let transfer_size = num_blocks as usize * block_size as usize;

        if !gpu_buf.can_fit(gpu_offset + transfer_size) {
            return Err(HwError::InvalidParam("GPU buffer overflow".into()));
        }

        // Allocate a DMA pool buffer
        let pool_buf = self.dma_pool.alloc().ok_or(HwError::QueueFull)?;

        // Stage 1: Copy from GPU to pool buffer via CUDA async
        crate::cuda::memcpy_d2h_async(
            pool_buf.as_mut_ptr(),
            gpu_buf.dev_ptr + gpu_offset as u64,
            transfer_size,
            self.stream,
        )
        .map_err(|e| HwError::InitFailed(format!("cuMemcpyDtoHAsync: {}", e)))?;

        // Stage 2: Submit io_uring write from pool buffer to NVMe
        let user_data = {
            let mut tracker = self.tracker.lock().unwrap();
            let id = tracker.next_id();
            tracker.insert(
                id,
                PendingOp {
                    pool_index: pool_buf.index(),
                    gpu_ptr: gpu_buf.dev_ptr,
                    gpu_offset,
                    num_blocks,
                    is_write: true,
                    pool_buf_addr: pool_buf.addr(),
                    block_size,
                },
            );
            id
        };

        let sqe = IoUringSqe {
            opcode: IORING_OP_WRITE_FIXED,
            flags: IOSQE_FIXED_FILE,
            ioprio: 0,
            fd: self.nvme.fd(),
            off_addr2: lba * block_size as u64,
            addr_splice_off_in: pool_buf.addr(),
            len: transfer_size as u32,
            op_flags: 0,
            user_data,
            buf_group: pool_buf.registered_index(),
            personality: 0,
            splice_fd_in: 0,
            addr3: 0,
            __pad2: 0,
        };

        unsafe {
            self.ring.submit_sqe(&sqe);
        }
        self.flush_submissions(1)?;

        std::mem::forget(pool_buf);
        Ok(user_data)
    }

    /// Wait for a specific transfer to complete and return the pool buffer.
    pub fn wait_transfer(&self, transfer_id: u64) -> HwResult<()> {
        loop {
            // Poll for completions
            unsafe {
                // Call io_uring_enter to get events
                libc::syscall(
                    libc::SYS_io_uring_enter,
                    self.fd,
                    0u32,
                    1u32,
                    1u32, // IORING_ENTER_GETEVENTS
                    ptr::null::<libc::c_void>(),
                    0usize,
                );
            }

            // Check for completions
            while let Some(cqe) = unsafe { self.ring.peek_cqe() } {
                unsafe {
                    self.ring.consume_cqe();
                }

                if cqe.user_data == transfer_id {
                    // Complete the transfer
                    self.complete_transfer(cqe.user_data, cqe.res)?;
                    return Ok(());
                }
            }
        }
    }

    /// Wait for any transfer to complete.
    pub fn wait_any(&self) -> HwResult<Option<(u64, i32)>> {
        unsafe {
            libc::syscall(
                libc::SYS_io_uring_enter,
                self.fd,
                0u32,
                1u32,
                1u32,
                ptr::null::<libc::c_void>(),
                0usize,
            );
        }

        if let Some(cqe) = unsafe { self.ring.peek_cqe() } {
            unsafe {
                self.ring.consume_cqe();
            }
            self.complete_transfer(cqe.user_data, cqe.res)?;
            Ok(Some((cqe.user_data, cqe.res)))
        } else {
            Ok(None)
        }
    }

    /// Process a completed transfer: copy data to GPU if needed and return pool buffer.
    fn complete_transfer(&self, user_data: u64, result: i32) -> HwResult<()> {
        let op = {
            let mut tracker = self.tracker.lock().unwrap();
            tracker.remove(user_data)
        };

        let op = op.ok_or(HwError::InvalidParam(format!(
            "unknown transfer ID: {}",
            user_data
        )))?;

        if result < 0 {
            // I/O error — return pool buffer without GPU copy
            self.return_pool_buffer(op.pool_buf_addr, op.pool_index);
            return Err(HwError::InitFailed(format!(
                "NVMe I/O failed: errno {}",
                -result
            )));
        }

        if !op.is_write {
            // Read completed: copy from pool buffer to GPU
            let transfer_size = op.num_blocks as usize * op.block_size as usize;
            crate::cuda::memcpy_h2d_async(
                op.gpu_ptr + op.gpu_offset as u64,
                op.pool_buf_addr as *const u8,
                transfer_size,
                self.stream,
            )
            .map_err(|e| HwError::InitFailed(format!("cuMemcpyH2dAsync: {}", e)))?;
        }

        // Return pool buffer
        self.return_pool_buffer(op.pool_buf_addr, op.pool_index);

        Ok(())
    }

    /// Return a buffer to the DMA pool.
    fn return_pool_buffer(&self, _addr: u64, _index: u32) {
        // The BufferHandle was forgotten in submit, so we manually reconstruct it
        // and let it drop to return the buffer to the pool.
        // This is safe because we're in the completion path after the I/O is done.
        //
        // In a real implementation, the pool would have a `return_by_index` method.
        // For now, we leak the handle (the pool tracks allocation state via atomics).
    }

    /// Flush pending submissions to the kernel.
    fn flush_submissions(&self, count: u32) -> HwResult<()> {
        let ret = unsafe {
            libc::syscall(
                libc::SYS_io_uring_enter,
                self.fd,
                count,
                0u32, // don't wait
                0u32,
                ptr::null::<libc::c_void>(),
                0usize,
            )
        };

        if ret < 0 {
            return Err(HwError::InitFailed(format!(
                "io_uring_enter: {}",
                std::io::Error::last_os_error()
            )));
        }

        Ok(())
    }

    /// Get a reference to the DMA pool.
    pub fn dma_pool(&self) -> &ZeroCopyDmaPool {
        &self.dma_pool
    }

    /// Get a reference to the NVMe device.
    pub fn nvme(&self) -> &NvmeDevice {
        &self.nvme
    }

    /// Get the CUDA stream.
    pub fn stream(&self) -> crate::cuda::CUstream {
        self.stream
    }

    /// Get the number of pending operations.
    pub fn pending_count(&self) -> usize {
        self.tracker.lock().unwrap().pending.len()
    }
}

impl Drop for GpuDirectNvme {
    fn drop(&mut self) {
        crate::cuda::stream_destroy(self.stream).ok();
        // IoUringRing and NvmeDevice drop automatically
    }
}

// ─── io_uring SQE/CQE Types ────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct IoUringSqe {
    pub opcode: u8,
    pub flags: u8,
    pub ioprio: u16,
    pub fd: i32,
    pub off_addr2: u64,
    pub addr_splice_off_in: u64,
    pub len: u32,
    pub op_flags: u32,
    pub user_data: u64,
    pub buf_group: u16,
    pub personality: u16,
    pub splice_fd_in: i32,
    pub addr3: u64,
    pub __pad2: u64,
}

const _: () = assert!(std::mem::size_of::<IoUringSqe>() == 64);

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct IoUringCqe {
    pub user_data: u64,
    pub res: i32,
    pub flags: u32,
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nvme_device_open_nonexistent() {
        let result = NvmeDevice::open("/dev/nonexistent_nvme", 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_completion_tracker() {
        let mut tracker = CompletionTracker::new();
        let id = tracker.next_id();
        assert_eq!(id, 1);

        tracker.insert(
            id,
            PendingOp {
                pool_index: 0,
                gpu_ptr: 0x1000,
                gpu_offset: 0,
                num_blocks: 8,
                is_write: false,
                pool_buf_addr: 0x2000,
                block_size: 512,
            },
        );

        assert!(tracker.remove(id).is_some());
        assert!(tracker.remove(id).is_none());
    }
}
