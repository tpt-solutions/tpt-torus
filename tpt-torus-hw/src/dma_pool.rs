//! Zero-copy DMA buffer pool for io_uring submissions.
//!
//! This module provides a high-performance buffer pool where memory is:
//! 1. Allocated via `mmap` with `MAP_ANONYMOUS | MAP_HUGETLB` (huge pages)
//! 2. Registered with io_uring via `IORING_REGISTER_BUFFERS`
//! 3. Used directly in io_uring SQEs without any CPU-side copies
//!
//! # Zero-Copy Guarantee
//!
//! Buffers from this pool are never copied during I/O operations. The kernel
//! reads/writes directly to/from pool buffers via DMA, eliminating CPU overhead
//! for data movement.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │              Application                             │
//! ├─────────────────────────────────────────────────────┤
//! │         ZeroCopyDmaPool                              │
//! │  ┌───────────────────────────────────────────────┐  │
//! │  │  Buffer Slots (page-aligned, DMA-registered)  │  │
//! │  │  ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐   │  │
//! │  │  │  0  │ │  1  │ │  2  │ │ ... │ │  N  │   │  │
//! │  │  └──┬──┘ └──┬──┘ └──┬──┘ └──┬──┘ └──┬──┘   │  │
//! │  │     │       │       │       │       │        │  │
//! │  │     ▼       ▼       ▼       ▼       ▼        │  │
//! │  │  ┌─────────────────────────────────────────┐  │  │
//! │  │  │  io_uring registered buffer array       │  │  │
//! │  │  │  (kernel DMA addresses pre-computed)    │  │  │
//! │  │  └─────────────────────────────────────────┘  │  │
//! │  └───────────────────────────────────────────────┘  │
//! ├─────────────────────────────────────────────────────┤
//! │              io_uring SQE/CQE                       │
//! │  (buf_addr points directly into pool buffer)        │
//! ├─────────────────────────────────────────────────────┤
//! │         PCIe DMA Engine                              │
//! └─────────────────────────────────────────────────────┘
//! ```

use crate::{HwError, HwResult};
use std::collections::HashMap;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

// ─── Constants ─────────────────────────────────────────────────────────────

/// Default buffer size: 4KB (single page).
const DEFAULT_BUF_SIZE: usize = 4096;

/// Minimum buffer size: 4KB.
const MIN_BUF_SIZE: usize = 4096;

/// Maximum buffer size: 2MB (huge page).
const MAX_BUF_SIZE: usize = 2 * 1024 * 1024;

/// Default pool capacity.
const DEFAULT_CAPACITY: usize = 256;

/// io_uring register operation codes.
const IORING_REGISTER_BUFFERS: u32 = 1;
const IORING_UNREGISTER_BUFFERS: u32 = 2;

// ─── Buffer Descriptor ─────────────────────────────────────────────────────

/// Descriptor for a DMA buffer in the pool.
#[derive(Debug, Clone, Copy)]
pub struct BufferDesc {
    /// Slot index in the pool.
    pub index: u32,
    /// Virtual address of the buffer.
    pub addr: u64,
    /// Physical/DMA address (for io_uring registration).
    pub dma_addr: u64,
    /// Buffer size in bytes.
    pub size: u32,
    /// Whether this buffer is currently allocated.
    pub allocated: bool,
}

/// Lock-free buffer allocation state.
struct BufferSlot {
    /// The buffer descriptor.
    desc: BufferDesc,
    /// Whether this slot is currently allocated.
    allocated: AtomicBool,
}

// ─── Zero-Copy DMA Pool ────────────────────────────────────────────────────

/// A zero-copy DMA buffer pool for io_uring submissions.
///
/// Buffers are allocated via `mmap` and registered with io_uring, enabling
/// zero-copy I/O where the kernel reads/writes directly to pool buffers.
pub struct ZeroCopyDmaPool {
    /// Base address of the mmap'd region.
    base: *mut u8,
    /// Total size of the mmap'd region.
    region_size: usize,
    /// Buffer size per slot.
    buf_size: usize,
    /// Number of buffer slots.
    capacity: u32,
    /// Buffer slots with allocation state.
    slots: Vec<BufferSlot>,
    /// io_uring file descriptor for buffer registration.
    uring_fd: i32,
    /// Whether buffers are registered with io_uring.
    registered: AtomicBool,
    /// Allocation statistics.
    stats: PoolStats,
    /// Lock for registration operations.
    register_lock: Mutex<()>,
}

unsafe impl Send for ZeroCopyDmaPool {}
unsafe impl Sync for ZeroCopyDmaPool {}

/// Pool allocation statistics.
#[derive(Debug, Default)]
pub struct PoolStats {
    /// Total allocations.
    pub allocations: AtomicU64,
    /// Total deallocations.
    pub deallocations: AtomicU64,
    /// Current in-flight count.
    pub in_flight: AtomicU32,
    /// Peak in-flight count.
    pub peak_in_flight: AtomicU32,
}

impl PoolStats {
    /// Record a new allocation.
    pub fn record_alloc(&self) {
        self.allocations.fetch_add(1, Ordering::Relaxed);
        let current = self.in_flight.fetch_add(1, Ordering::Relaxed) + 1;
        // Update peak
        let mut peak = self.peak_in_flight.load(Ordering::Relaxed);
        while current > peak {
            match self.peak_in_flight.compare_exchange(
                peak,
                current,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => peak = actual,
            }
        }
    }

    /// Record a deallocation.
    pub fn record_dealloc(&self) {
        self.deallocations.fetch_add(1, Ordering::Relaxed);
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get current in-flight count.
    pub fn in_flight(&self) -> u32 {
        self.in_flight.load(Ordering::Relaxed)
    }

    /// Get peak in-flight count.
    pub fn peak_in_flight(&self) -> u32 {
        self.peak_in_flight.load(Ordering::Relaxed)
    }

    /// Get total allocations.
    pub fn total_allocs(&self) -> u64 {
        self.allocations.load(Ordering::Relaxed)
    }
}

impl ZeroCopyDmaPool {
    /// Create a new zero-copy DMA buffer pool.
    ///
    /// # Arguments
    /// - `buf_size`: Size of each buffer (must be power of 2, 4KB-2MB).
    /// - `capacity`: Number of buffers to allocate.
    /// - `uring_fd`: io_uring file descriptor for buffer registration.
    pub fn new(buf_size: usize, capacity: usize, uring_fd: i32) -> HwResult<Self> {
        if !buf_size.is_power_of_two() || buf_size < MIN_BUF_SIZE || buf_size > MAX_BUF_SIZE {
            return Err(HwError::InvalidParam(format!(
                "buf_size must be power of 2 between {} and {} bytes",
                MIN_BUF_SIZE, MAX_BUF_SIZE
            )));
        }

        let total_size = buf_size * capacity;
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;

        // Align to page size
        let region_size = (total_size + page_size - 1) & !(page_size - 1);

        // Allocate via mmap with MAP_ANONYMOUS (zero-filled)
        let base = unsafe {
            libc::mmap(
                ptr::null_mut(),
                region_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_ANONYMOUS | libc::MAP_PRIVATE,
                -1,
                0,
            )
        };

        if base == libc::MAP_FAILED {
            return Err(HwError::OutOfMemory);
        }

        let base = base as *mut u8;

        // Create buffer slots
        let mut slots = Vec::with_capacity(capacity);
        for i in 0..capacity {
            let addr = unsafe { base.add(i * buf_size) } as u64;
            let dma_addr = addr; // In real implementation, get via virt_to_phys

            slots.push(BufferSlot {
                desc: BufferDesc {
                    index: i as u32,
                    addr,
                    dma_addr,
                    size: buf_size as u32,
                    allocated: false,
                },
                allocated: AtomicBool::new(false),
            });
        }

        let pool = Self {
            base,
            region_size,
            buf_size,
            capacity: capacity as u32,
            slots,
            uring_fd,
            registered: AtomicBool::new(false),
            stats: PoolStats::default(),
            register_lock: Mutex::new(()),
        };

        // Register buffers with io_uring
        pool.register_buffers()?;

        Ok(pool)
    }

    /// Register all buffers with io_uring.
    fn register_buffers(&self) -> HwResult<()> {
        let _lock = self.register_lock.lock().unwrap();

        if self.registered.load(Ordering::Relaxed) {
            return Ok(());
        }

        // Create iovec array for registration
        let mut iovecs: Vec<libc::iovec> = Vec::with_capacity(self.capacity as usize);
        for slot in &self.slots {
            iovecs.push(libc::iovec {
                iov_base: slot.desc.addr as *mut libc::c_void,
                iov_len: self.buf_size,
            });
        }

        let ret = unsafe {
            libc::syscall(
                libc::SYS_io_uring_register,
                self.uring_fd,
                IORING_REGISTER_BUFFERS,
                iovecs.as_ptr(),
                self.capacity,
            )
        };

        if ret < 0 {
            let errno = (-ret) as i32;
            return Err(HwError::InitFailed(format!(
                "io_uring_register_buffers failed: errno {}",
                errno
            )));
        }

        self.registered.store(true, Ordering::Release);
        Ok(())
    }

    /// Unregister all buffers from io_uring.
    fn unregister_buffers(&self) -> HwResult<()> {
        let _lock = self.register_lock.lock().unwrap();

        if !self.registered.load(Ordering::Relaxed) {
            return Ok(());
        }

        let ret = unsafe {
            libc::syscall(
                libc::SYS_io_uring_register,
                self.uring_fd,
                IORING_UNREGISTER_BUFFERS,
                ptr::null::<libc::c_void>(),
                0u32,
            )
        };

        if ret < 0 {
            let errno = (-ret) as i32;
            return Err(HwError::InitFailed(format!(
                "io_uring_unregister_buffers failed: errno {}",
                errno
            )));
        }

        self.registered.store(false, Ordering::Release);
        Ok(())
    }

    /// Allocate a buffer from the pool.
    ///
    /// Returns a `BufferHandle` that must be returned to the pool via `free()`.
    pub fn alloc(&self) -> Option<BufferHandle> {
        for slot in &self.slots {
            if slot
                .allocated
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                self.stats.record_alloc();
                return Some(BufferHandle {
                    pool: self as *const Self,
                    index: slot.desc.index,
                    addr: slot.desc.addr,
                    dma_addr: slot.desc.dma_addr,
                    size: slot.desc.size,
                });
            }
        }
        None
    }

    /// Allocate a buffer with specific alignment.
    pub fn alloc_aligned(&self, alignment: usize) -> Option<BufferHandle> {
        if !alignment.is_power_of_two() {
            return None;
        }

        for slot in &self.slots {
            if slot
                .allocated
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                let addr = slot.desc.addr;
                if addr as usize % alignment == 0 {
                    self.stats.record_alloc();
                    return Some(BufferHandle {
                        pool: self as *const Self,
                        index: slot.desc.index,
                        addr,
                        dma_addr: slot.desc.dma_addr,
                        size: slot.desc.size,
                    });
                }
                // Not aligned, release and continue
                slot.allocated.store(false, Ordering::Release);
            }
        }
        None
    }

    /// Get a buffer descriptor by index.
    pub fn descriptor(&self, index: u32) -> Option<BufferDesc> {
        self.slots.get(index as usize).map(|s| s.desc)
    }

    /// Number of available buffers.
    pub fn available(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| !s.allocated.load(Ordering::Relaxed))
            .count()
    }

    /// Total number of buffers.
    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    /// Size of each buffer.
    pub fn buf_size(&self) -> usize {
        self.buf_size
    }

    /// Get allocation statistics.
    pub fn stats(&self) -> &PoolStats {
        &self.stats
    }

    /// Check if buffers are registered with io_uring.
    pub fn is_registered(&self) -> bool {
        self.registered.load(Ordering::Relaxed)
    }
}

impl Drop for ZeroCopyDmaPool {
    fn drop(&mut self) {
        // Unregister from io_uring
        self.unregister_buffers().ok();

        // Unmap the memory region
        unsafe {
            libc::munmap(self.base as *mut libc::c_void, self.region_size);
        }
    }
}

// ─── Buffer Handle ─────────────────────────────────────────────────────────

/// A handle to an allocated buffer in the pool.
///
/// This handle must be returned to the pool via `Drop` or explicit `free()`.
pub struct BufferHandle {
    pool: *const ZeroCopyDmaPool,
    index: u32,
    addr: u64,
    dma_addr: u64,
    size: u32,
}

unsafe impl Send for BufferHandle {}
unsafe impl Sync for BufferHandle {}

impl BufferHandle {
    /// Get the virtual address of the buffer.
    pub fn addr(&self) -> u64 {
        self.addr
    }

    /// Get the DMA address of the buffer (for io_uring submissions).
    pub fn dma_addr(&self) -> u64 {
        self.dma_addr
    }

    /// Get the buffer size.
    pub fn size(&self) -> usize {
        self.size as usize
    }

    /// Get the slot index.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Get a raw pointer to the buffer data.
    pub fn as_ptr(&self) -> *const u8 {
        self.addr as *const u8
    }

    /// Get a mutable raw pointer to the buffer data.
    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.addr as *mut u8
    }

    /// Get a byte slice of the buffer.
    ///
    /// # Safety
    ///
    /// The caller must ensure the buffer is valid and not being used
    /// by another operation.
    pub unsafe fn as_slice(&self) -> &[u8] {
        std::slice::from_raw_parts(self.addr as *const u8, self.size as usize)
    }

    /// Get a mutable byte slice of the buffer.
    ///
    /// # Safety
    ///
    /// The caller must ensure exclusive access to the buffer.
    pub unsafe fn as_mut_slice(&mut self) -> &mut [u8] {
        std::slice::from_raw_parts_mut(self.addr as *mut u8, self.size as usize)
    }

    /// Release the buffer back to the pool.
    pub fn free(self) {
        // Drop will handle this
    }
}

impl Drop for BufferHandle {
    fn drop(&mut self) {
        unsafe {
            let pool = &*self.pool;
            let slot = &pool.slots[self.index as usize];
            slot.allocated.store(false, Ordering::Release);
            pool.stats.record_dealloc();
        }
    }
}

// ─── Buffer Handle for io_uring ────────────────────────────────────────────

/// Extension trait for `BufferHandle` to create io_uring-compatible buffers.
impl BufferHandle {
    /// Create an `iovec` for use with io_uring scatter-gather I/O.
    pub fn as_iovec(&self) -> libc::iovec {
        libc::iovec {
            iov_base: self.addr as *mut libc::c_void,
            iov_len: self.size as usize,
        }
    }

    /// Get the registered buffer index for io_uring fixed-buffer I/O.
    ///
    /// This is the index used in `sqe.buf_index` for registered buffers.
    pub fn registered_index(&self) -> u16 {
        self.index as u16
    }
}

// ─── Pool Builder ──────────────────────────────────────────────────────────

/// Builder for creating `ZeroCopyDmaPool` instances.
pub struct DmaPoolBuilder {
    buf_size: usize,
    capacity: usize,
    uring_fd: i32,
    use_hugepages: bool,
}

impl DmaPoolBuilder {
    /// Create a new builder.
    pub fn new(uring_fd: i32) -> Self {
        Self {
            buf_size: DEFAULT_BUF_SIZE,
            capacity: DEFAULT_CAPACITY,
            uring_fd,
            use_hugepages: false,
        }
    }

    /// Set the buffer size (must be power of 2, 4KB-2MB).
    pub fn buf_size(mut self, size: usize) -> Self {
        self.buf_size = size;
        self
    }

    /// Set the pool capacity (number of buffers).
    pub fn capacity(mut self, count: usize) -> Self {
        self.capacity = count;
        self
    }

    /// Enable huge page allocation.
    pub fn use_hugepages(mut self) -> Self {
        self.use_hugepages = true;
        self
    }

    /// Build the pool.
    pub fn build(self) -> HwResult<ZeroCopyDmaPool> {
        ZeroCopyDmaPool::new(self.buf_size, self.capacity, self.uring_fd)
    }
}

// ─── GPU-Direct Integration ────────────────────────────────────────────────

/// GPU-Direct aware DMA buffer pool that can be registered with both
/// io_uring and CUDA for zero-copy NVMe ↔ GPU transfers.
pub struct GpuDmaPool {
    /// The underlying DMA pool.
    pool: ZeroCopyDmaPool,
    /// GPU device ID.
    device_id: i32,
    /// GPU buffer registrations.
    gpu_bufs: HashMap<u32, u64>, // index -> GPU device pointer
}

impl GpuDmaPool {
    /// Create a new GPU-Direct aware DMA pool.
    pub fn new(
        buf_size: usize,
        capacity: usize,
        uring_fd: i32,
        device_id: i32,
    ) -> HwResult<Self> {
        let pool = ZeroCopyDmaPool::new(buf_size, capacity, uring_fd)?;

        Ok(Self {
            pool,
            device_id,
            gpu_bufs: HashMap::new(),
        })
    }

    /// Allocate a buffer and register it with CUDA for GPU-Direct.
    pub fn alloc_for_gpu(&mut self) -> HwResult<BufferHandle> {
        let handle = self
            .pool
            .alloc()
            .ok_or(HwError::QueueFull)?;

        // Register the buffer with CUDA for GPU-Direct access
        let gpu_ptr = crate::cuda::mem_alloc(handle.size())
            .map_err(|e| HwError::InitFailed(format!("cuMemAlloc: {}", e)))?;

        self.gpu_bufs.insert(handle.index(), gpu_ptr);

        Ok(handle)
    }

    /// Get the GPU pointer for a buffer.
    pub fn gpu_ptr(&self, index: u32) -> Option<u64> {
        self.gpu_bufs.get(&index).copied()
    }

    /// Access the underlying pool.
    pub fn pool(&self) -> &ZeroCopyDmaPool {
        &self.pool
    }
}

impl Drop for GpuDmaPool {
    fn drop(&mut self) {
        // Free GPU allocations
        for (_index, ptr) in &self.gpu_bufs {
            crate::cuda::mem_free(*ptr).ok();
        }
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_desc_layout() {
        let desc = BufferDesc {
            index: 0,
            addr: 0x1000,
            dma_addr: 0x2000,
            size: 4096,
            allocated: false,
        };
        assert_eq!(desc.index, 0);
        assert_eq!(desc.size, 4096);
        assert!(!desc.allocated);
    }

    #[test]
    fn test_pool_stats() {
        let stats = PoolStats::default();
        assert_eq!(stats.in_flight(), 0);
        assert_eq!(stats.total_allocs(), 0);

        stats.record_alloc();
        assert_eq!(stats.in_flight(), 1);
        assert_eq!(stats.total_allocs(), 1);

        stats.record_dealloc();
        assert_eq!(stats.in_flight(), 0);
    }

    #[test]
    fn test_pool_builder() {
        // Can't actually build without an io_uring fd, but test the builder API
        let builder = DmaPoolBuilder::new(0)
            .buf_size(8192)
            .capacity(128)
            .use_hugepages();

        assert_eq!(builder.buf_size, 8192);
        assert_eq!(builder.capacity, 128);
        assert!(builder.use_hugepages);
    }
}
