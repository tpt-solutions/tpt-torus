//! SPDK (Storage Performance Development Kit) integration for user-space NVMe I/O.
//!
//! SPDK provides a user-space NVMe driver that bypasses the kernel entirely,
//! enabling direct access to NVMe devices from user space. This module wraps
//! SPDK's core functionality behind safe Rust abstractions.
//!
//! # Architecture
//!
//! ```text
//! Application
//!     │
//!     ▼
//! SpdkNvme ──────► NVMe Namespace
//!     │                  │
//!     │                  ▼
//!     │            NVMe Controller (user-space)
//!     │                  │
//!     ▼                  ▼
//! DMA Buffers ◄──► NVMe Device
//! ```
//!
//! # Feature Flag
//!
//! This module requires the `spdk` feature flag to be enabled.
//! When disabled, all functions return `HwError::NotAvailable`.

use crate::{HwError, HwResult};
use std::os::raw::c_void;

// ─── SPDK FFI bindings ─────────────────────────────────────────────────────

/// Opaque SPDK NVMe controller handle.
pub struct SpdkNvmeQpair {
    _private: [u8; 0],
}

/// Opaque SPDK NVMe namespace handle.
pub struct SpdkNvmeNs {
    _private: [u8; 0],
}

/// Opaque SPDK NVMe controller handle.
pub struct SpdkNvmeCtrlr {
    _private: [u8; 0],
}

/// SPDK NVMe completion status.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SpdkNvmeCpl {
    /// Status code type.
    pub cdw0: u32,
    /// Status code.
    pub sc: u8,
    /// SC bit.
    pub sc_bit: u8,
    /// Additional status code information.
    pub sct: u8,
    /// Phase bit.
    pub phase: u8,
    /// Command specific.
    pub cdw2: u32,
    pub cdw3: u32,
}

impl SpdkNvmeCpl {
    /// Check if the completion was successful.
    pub fn is_success(&self) -> bool {
        self.sc == 0 && self.sct == 0
    }

    /// Get the NVMe status code.
    pub fn status_code(&self) -> u16 {
        ((self.sct as u16) << 8) | (self.sc as u16)
    }
}

// ─── SPDK NVMe Namespace ───────────────────────────────────────────────────

/// An NVMe namespace accessed via SPDK.
///
/// Provides user-space read/write operations that bypass the kernel.
pub struct NvmeNamespace {
    ns: *mut SpdkNvmeNs,
    block_size: u32,
    total_blocks: u64,
}

unsafe impl Send for NvmeNamespace {}
unsafe impl Sync for NvmeNamespace {}

impl NvmeNamespace {
    /// Create a new NVMe namespace wrapper.
    ///
    /// # Safety
    ///
    /// `ns` must be a valid SPDK NVMe namespace handle.
    pub unsafe fn new(ns: *mut SpdkNvmeNs, block_size: u32, total_blocks: u64) -> Self {
        Self {
            ns,
            block_size,
            total_blocks,
        }
    }

    /// Get the block size in bytes.
    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    /// Get the total number of blocks.
    pub fn total_blocks(&self) -> u64 {
        self.total_blocks
    }

    /// Get the namespace size in bytes.
    pub fn size_bytes(&self) -> u64 {
        self.total_blocks * self.block_size as u64
    }

    /// Submit a read command.
    ///
    /// # Arguments
    /// - `lba`: Starting logical block address.
    /// - `num_blocks`: Number of blocks to read.
    /// - `buf`: DMA-capable buffer to read into.
    /// - `cb`: Completion callback.
    /// - `cb_arg`: Callback argument.
    ///
    /// # Safety
    ///
    /// - `buf` must be a valid DMA-capable buffer of at least `num_blocks * block_size` bytes.
    /// - The buffer must remain valid until the callback is invoked.
    pub unsafe fn read(
        &self,
        lba: u64,
        num_blocks: u32,
        buf: *mut u8,
        cb: Option<unsafe extern "C" fn(*mut c_void, *const SpdkNvmeCpl)>,
        cb_arg: *mut c_void,
    ) -> HwResult<()> {
        if self.ns.is_null() {
            return Err(HwError::NotAvailable("SPDK not initialized".into()));
        }

        // In a real implementation, this would call spdk_nvme_ns_cmd_read()
        // For now, this is a stub that would be linked against SPDK
        let _ = (lba, num_blocks, buf, cb, cb_arg);
        Err(HwError::NotAvailable(
            "SPDK integration requires SPDK to be installed and linked".into(),
        ))
    }

    /// Submit a write command.
    ///
    /// # Safety
    ///
    /// - `buf` must be a valid DMA-capable buffer of at least `num_blocks * block_size` bytes.
    /// - The buffer must remain valid until the callback is invoked.
    pub unsafe fn write(
        &self,
        lba: u64,
        num_blocks: u32,
        buf: *const u8,
        cb: Option<unsafe extern "C" fn(*mut c_void, *const SpdkNvmeCpl)>,
        cb_arg: *mut c_void,
    ) -> HwResult<()> {
        if self.ns.is_null() {
            return Err(HwError::NotAvailable("SPDK not initialized".into()));
        }

        let _ = (lba, num_blocks, buf, cb, cb_arg);
        Err(HwError::NotAvailable(
            "SPDK integration requires SPDK to be installed and linked".into(),
        ))
    }

    /// Submit a flush command.
    ///
    /// # Safety
    /// The callback must be a valid function pointer.
    pub unsafe fn flush(
        &self,
        cb: Option<unsafe extern "C" fn(*mut c_void, *const SpdkNvmeCpl)>,
        cb_arg: *mut c_void,
    ) -> HwResult<()> {
        let _ = (cb, cb_arg);
        Err(HwError::NotAvailable(
            "SPDK integration requires SPDK to be installed and linked".into(),
        ))
    }
}

// ─── SPDK NVMe Command Builder ─────────────────────────────────────────────

/// Builder for SPDK NVMe commands.
pub struct NvmeCmd {
    /// Opcode.
    pub opcode: u8,
    /// Flags.
    pub flags: u8,
    /// Namespace ID.
    pub ns_id: u32,
    /// CDW10 (LBA or command specific).
    pub cdw10: u32,
    /// CDW11.
    pub cdw11: u32,
    /// CDW12 (NLB or command specific).
    pub cdw12: u32,
    /// CDW13.
    pub cdw13: u32,
    /// CDW14 (PRP1 or SGL).
    pub cdw14: u64,
    /// CDW15 (PRP2).
    pub cdw15: u64,
}

impl Default for NvmeCmd {
    fn default() -> Self {
        Self::new()
    }
}

impl NvmeCmd {
    /// Create a new empty NVMe command.
    pub fn new() -> Self {
        Self {
            opcode: 0,
            flags: 0,
            ns_id: 0,
            cdw10: 0,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// Create a read command.
    pub fn read(lba: u64, num_blocks: u32, prp1: u64, prp2: u64, ns_id: u32) -> Self {
        Self {
            opcode: 0x02, // NVME_OPC_READ
            flags: 0,
            ns_id,
            cdw10: (lba & 0xFFFFFFFF) as u32,
            cdw11: ((lba >> 32) & 0xFFFFFFFF) as u32,
            cdw12: num_blocks - 1, // NLB is 0-based
            cdw13: 0,
            cdw14: prp1,
            cdw15: prp2,
        }
    }

    /// Create a write command.
    pub fn write(lba: u64, num_blocks: u32, prp1: u64, prp2: u64, ns_id: u32) -> Self {
        Self {
            opcode: 0x01, // NVME_OPC_WRITE
            flags: 0,
            ns_id,
            cdw10: (lba & 0xFFFFFFFF) as u32,
            cdw11: ((lba >> 32) & 0xFFFFFFFF) as u32,
            cdw12: num_blocks - 1,
            cdw13: 0,
            cdw14: prp1,
            cdw15: prp2,
        }
    }

    /// Create a flush command.
    pub fn flush(ns_id: u32) -> Self {
        Self {
            opcode: 0x0C, // NVME_OPC_FLUSH
            flags: 0,
            ns_id,
            cdw10: 0,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// Create a dataset management (trim/deallocate) command.
    pub fn deallocate(lba: u64, num_blocks: u32, ns_id: u32) -> Self {
        Self {
            opcode: 0x09, // NVME_OPC_DSM
            flags: 0,
            ns_id,
            cdw10: num_blocks - 1,
            cdw11: 0x01, // Deallocate attribute
            cdw12: (lba & 0xFFFFFFFF) as u32,
            cdw13: ((lba >> 32) & 0xFFFFFFFF) as u32,
            cdw14: 0,
            cdw15: 0,
        }
    }
}

// ─── SPDK DMA Buffer Pool ──────────────────────────────────────────────────

/// A pool of DMA-capable buffers for SPDK operations.
pub struct DmaPool {
    base: *mut u8,
    size: usize,
    stride: usize,
    count: usize,
    free_list: Vec<usize>,
}

unsafe impl Send for DmaPool {}
unsafe impl Sync for DmaPool {}

impl DmaPool {
    /// Create a new DMA buffer pool.
    ///
    /// # Safety
    ///
    /// - `base` must point to a DMA-capable memory region of at least `size` bytes.
    /// - The memory must be page-aligned for optimal performance.
    pub unsafe fn new(base: *mut u8, size: usize, stride: usize, count: usize) -> Self {
        let mut free_list = Vec::with_capacity(count);
        for i in 0..count {
            free_list.push(i);
        }

        Self {
            base,
            size,
            stride,
            count,
            free_list,
        }
    }

    /// Allocate a buffer from the pool.
    pub fn alloc(&mut self) -> Option<*mut u8> {
        self.free_list
            .pop()
            .map(|i| unsafe { self.base.add(i * self.stride) })
    }

    /// Return a buffer to the pool.
    pub fn free(&mut self, buf: *mut u8) {
        let offset = buf as usize - self.base as usize;
        if offset < self.size && offset.is_multiple_of(self.stride) {
            let index = offset / self.stride;
            self.free_list.push(index);
        }
    }

    /// Number of available buffers.
    pub fn available(&self) -> usize {
        self.free_list.len()
    }

    /// Total number of buffers.
    pub fn total(&self) -> usize {
        self.count
    }
}

impl Drop for DmaPool {
    fn drop(&mut self) {
        // Note: The actual DMA memory is not freed here — it was allocated
        // externally. This is intentional for SPDK integration where the
        // memory is typically allocated via spdk_dma_malloc or hugepages.
    }
}
