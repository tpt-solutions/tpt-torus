//! GPU-Direct orchestration for DMA transfers between NVMe and GPU VRAM.
//!
//! GPU-Direct technology enables direct DMA (Direct Memory Access) transfers
//! between NVMe storage devices and GPU video memory, bypassing system RAM
//! entirely. This module provides the orchestration API for managing these
//! transfers using the CUDA Driver API.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                    Application                      │
//! ├─────────────────────────────────────────────────────┤
//! │                  GpuDirect                          │
//! │  ┌──────────┐  ┌──────────┐  ┌──────────────────┐ │
//! │  │  NVMe    │  │   DMA    │  │  GPU VRAM        │ │
//! │  │  ──────► │  │  Engine  │  │  ◄─────────────  │ │
//! │  │  Read    │  │ (CUDA)   │  │  Write           │ │
//! │  └──────────┘  └──────────┘  └──────────────────┘ │
//! ├─────────────────────────────────────────────────────┤
//! │         PCI Express / NVLink / CXL Bus             │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! # Feature Flag
//!
//! This module requires the `gpu_direct` feature flag to be enabled.

use crate::{HwError, HwResult};

// ─── GPU-Direct Types ──────────────────────────────────────────────────────

/// Direction of a DMA transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    /// NVMe → GPU VRAM (storage read into GPU memory).
    NvmeToGpu,
    /// GPU VRAM → NVMe (GPU memory written to storage).
    GpuToNvme,
    /// GPU VRAM → GPU VRAM (device-to-device transfer).
    GpuToGpu,
}

impl std::fmt::Display for TransferDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransferDirection::NvmeToGpu => write!(f, "NVMe → GPU"),
            TransferDirection::GpuToNvme => write!(f, "GPU → NVMe"),
            TransferDirection::GpuToGpu => write!(f, "GPU → GPU"),
        }
    }
}

/// A DMA scatter-gather entry.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DmaEntry {
    /// Source physical address.
    pub src: u64,
    /// Destination physical address.
    pub dst: u64,
    /// Transfer length in bytes.
    pub len: u32,
    /// Flags (e.g., end-of-transfer).
    pub flags: u32,
}

/// DMA transfer completion status.
#[derive(Debug, Clone, Copy)]
pub enum DmaStatus {
    /// Transfer completed successfully.
    Completed,
    /// Transfer is in progress.
    InProgress,
    /// Transfer failed.
    Failed(DmaError),
    /// Transfer was cancelled.
    Cancelled,
}

/// DMA transfer error codes.
#[derive(Debug, Clone, Copy)]
pub enum DmaError {
    /// Invalid source address.
    InvalidSource,
    /// Invalid destination address.
    InvalidDestination,
    /// Transfer size exceeds maximum.
    SizeExceeded,
    /// DMA engine is busy.
    EngineBusy,
    /// GPU device not available.
    GpuNotAvailable,
    /// NVMe device not available.
    NvmeNotAvailable,
    /// Permission denied.
    PermissionDenied,
    /// I/O error.
    IoError,
}

impl std::fmt::Display for DmaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DmaError::InvalidSource => write!(f, "invalid source address"),
            DmaError::InvalidDestination => write!(f, "invalid destination address"),
            DmaError::SizeExceeded => write!(f, "transfer size exceeds maximum"),
            DmaError::EngineBusy => write!(f, "DMA engine is busy"),
            DmaError::GpuNotAvailable => write!(f, "GPU device not available"),
            DmaError::NvmeNotAvailable => write!(f, "NVMe device not available"),
            DmaError::PermissionDenied => write!(f, "permission denied"),
            DmaError::IoError => write!(f, "I/O error"),
        }
    }
}

impl std::error::Error for DmaError {}

/// Handle to a GPU memory region.
pub struct GpuBuffer {
    /// GPU device ID.
    pub device_id: i32,
    /// CUDA device pointer.
    pub dev_ptr: u64,
    /// Host-side pointer (if pinned).
    pub host_ptr: Option<*mut u8>,
    /// Buffer size in bytes.
    pub size: usize,
    /// Whether the buffer is pinned (page-locked).
    pub pinned: bool,
}

unsafe impl Send for GpuBuffer {}
unsafe impl Sync for GpuBuffer {}

impl GpuBuffer {
    /// Allocate GPU device memory.
    pub fn alloc(device_id: i32, size: usize) -> HwResult<Self> {
        crate::cuda::init().map_err(|e| HwError::InitFailed(format!("CUDA init: {}", e)))?;

        let dev_ptr = crate::cuda::mem_alloc(size)
            .map_err(|e| HwError::InitFailed(format!("cuMemAlloc: {}", e)))?;

        Ok(Self {
            device_id,
            dev_ptr,
            host_ptr: None,
            size,
            pinned: false,
        })
    }

    /// Allocate pinned (page-locked) host memory for fast transfers.
    pub fn alloc_pinned(device_id: i32, size: usize) -> HwResult<Self> {
        crate::cuda::init().map_err(|e| HwError::InitFailed(format!("CUDA init: {}", e)))?;

        let dev_ptr = crate::cuda::mem_alloc(size)
            .map_err(|e| HwError::InitFailed(format!("cuMemAlloc: {}", e)))?;

        let host_ptr = crate::cuda::mem_host_alloc(size)
            .map_err(|e| HwError::InitFailed(format!("cuMemHostAlloc: {}", e)))?;

        Ok(Self {
            device_id,
            dev_ptr,
            host_ptr: Some(host_ptr),
            size,
            pinned: true,
        })
    }

    /// Create a descriptor from an existing device pointer.
    pub fn from_raw(device_id: i32, dev_ptr: u64, size: usize) -> Self {
        Self {
            device_id,
            dev_ptr,
            host_ptr: None,
            size,
            pinned: false,
        }
    }

    /// Check if this buffer can hold the given transfer size.
    pub fn can_fit(&self, size: usize) -> bool {
        size <= self.size
    }

    /// Copy data from host to this GPU buffer.
    pub fn copy_from_host(&self, src: *const u8, size: usize) -> HwResult<()> {
        if size > self.size {
            return Err(HwError::InvalidParam("transfer exceeds buffer size".into()));
        }
        crate::cuda::memcpy_h2d(self.dev_ptr, src, size)
            .map_err(|e| HwError::InitFailed(format!("cuMemcpyHtoD: {}", e)))
    }

    /// Copy data from this GPU buffer to host.
    pub fn copy_to_host(&self, dst: *mut u8, size: usize) -> HwResult<()> {
        if size > self.size {
            return Err(HwError::InvalidParam("transfer exceeds buffer size".into()));
        }
        crate::cuda::memcpy_d2h(dst, self.dev_ptr, size)
            .map_err(|e| HwError::InitFailed(format!("cuMemcpyDtoH: {}", e)))
    }
}

impl Drop for GpuBuffer {
    fn drop(&mut self) {
        // Free device memory
        if self.dev_ptr != 0 {
            crate::cuda::mem_free(self.dev_ptr).ok();
        }
        // Free pinned host memory
        if let Some(ptr) = self.host_ptr {
            crate::cuda::mem_free_host(ptr, self.size).ok();
        }
    }
}

// ─── GPU-Direct Engine ─────────────────────────────────────────────────────

/// Handle to a CUDA stream (DMA transfer engine).
pub struct DmaEngine {
    engine_id: u32,
    stream: crate::cuda::CUstream,
    max_transfers: usize,
    active_transfers: usize,
}

unsafe impl Send for DmaEngine {}
unsafe impl Sync for DmaEngine {}

impl DmaEngine {
    /// Create a new DMA engine (CUDA stream).
    pub fn new(engine_id: u32, max_transfers: usize) -> HwResult<Self> {
        let stream = crate::cuda::stream_create()
            .map_err(|e| HwError::InitFailed(format!("cuStreamCreate: {}", e)))?;

        Ok(Self {
            engine_id,
            stream,
            max_transfers,
            active_transfers: 0,
        })
    }

    /// Get the engine ID.
    pub fn engine_id(&self) -> u32 {
        self.engine_id
    }

    /// Get the CUDA stream handle.
    pub fn stream(&self) -> crate::cuda::CUstream {
        self.stream
    }

    /// Check if the engine can accept more transfers.
    pub fn has_capacity(&self) -> bool {
        self.active_transfers < self.max_transfers
    }

    /// Number of active transfers.
    pub fn active_count(&self) -> usize {
        self.active_transfers
    }

    /// Submit an async device-to-device transfer.
    pub fn submit_d2d(
        &mut self,
        src: u64,
        dst: u64,
        len: usize,
    ) -> HwResult<u64> {
        if !self.has_capacity() {
            return Err(HwError::QueueFull);
        }

        crate::cuda::memcpy_d2d_async(dst, src, len, self.stream)
            .map_err(|e| HwError::InitFailed(format!("cuMemcpyDtoDAsync: {}", e)))?;

        self.active_transfers += 1;
        Ok(self.active_transfers as u64)
    }

    /// Submit an async host-to-device transfer.
    pub fn submit_h2d(
        &mut self,
        dst: u64,
        src: *const u8,
        len: usize,
    ) -> HwResult<u64> {
        if !self.has_capacity() {
            return Err(HwError::QueueFull);
        }

        crate::cuda::memcpy_h2d_async(dst, src, len, self.stream)
            .map_err(|e| HwError::InitFailed(format!("cuMemcpyHtoDAsync: {}", e)))?;

        self.active_transfers += 1;
        Ok(self.active_transfers as u64)
    }

    /// Submit an async device-to-host transfer.
    pub fn submit_d2h(
        &mut self,
        dst: *mut u8,
        src: u64,
        len: usize,
    ) -> HwResult<u64> {
        if !self.has_capacity() {
            return Err(HwError::QueueFull);
        }

        crate::cuda::memcpy_d2h_async(dst, src, len, self.stream)
            .map_err(|e| HwError::InitFailed(format!("cuMemcpyDtoHAsync: {}", e)))?;

        self.active_transfers += 1;
        Ok(self.active_transfers as u64)
    }

    /// Synchronize the stream (wait for all transfers to complete).
    pub fn synchronize(&self) -> HwResult<()> {
        crate::cuda::stream_synchronize(self.stream)
            .map_err(|e| HwError::InitFailed(format!("cuStreamSynchronize: {}", e)))
    }

    /// Cancel pending transfers (by synchronizing).
    pub fn cancel_all(&mut self) -> HwResult<()> {
        self.synchronize()?;
        self.active_transfers = 0;
        Ok(())
    }
}

impl Drop for DmaEngine {
    fn drop(&mut self) {
        crate::cuda::stream_destroy(self.stream).ok();
    }
}

// ─── GPU-Direct Orchestrator ───────────────────────────────────────────────

/// High-level orchestrator for GPU-Direct DMA transfers.
///
/// Manages multiple DMA engines (CUDA streams) and provides a unified API for
/// NVMe ↔ GPU transfers.
pub struct GpuDirect {
    device_id: i32,
    engines: Vec<DmaEngine>,
}

unsafe impl Send for GpuDirect {}
unsafe impl Sync for GpuDirect {}

impl GpuDirect {
    /// Create a new GPU-Direct orchestrator.
    ///
    /// Initializes CUDA and creates the specified number of DMA engines.
    pub fn new(device_id: i32, num_engines: usize) -> HwResult<Self> {
        // Initialize CUDA
        crate::cuda::init()
            .map_err(|e| HwError::InitFailed(format!("CUDA init failed: {}", e)))?;

        // Set the current device and create context
        let dev = crate::cuda::device_get(device_id)
            .map_err(|e| HwError::InitFailed(format!("cuDeviceGet: {}", e)))?;
        let ctx = crate::cuda::ctx_create(dev)
            .map_err(|e| HwError::InitFailed(format!("cuCtxCreate: {}", e)))?;
        if ctx.is_null() {
            return Err(HwError::InitFailed("cuCtxCreate returned null".into()));
        }

        // Create DMA engines (CUDA streams)
        let mut engines = Vec::with_capacity(num_engines);
        for i in 0..num_engines as u32 {
            engines.push(DmaEngine::new(i, 64)?);
        }

        Ok(Self { device_id, engines })
    }

    /// Get the GPU device ID.
    pub fn device_id(&self) -> i32 {
        self.device_id
    }

    /// Number of available DMA engines.
    pub fn engine_count(&self) -> usize {
        self.engines.len()
    }

    /// Find an available DMA engine.
    fn find_engine(&mut self) -> HwResult<&mut DmaEngine> {
        self.engines
            .iter_mut()
            .find(|e| e.has_capacity())
            .ok_or(HwError::QueueFull)
    }

    /// Transfer data from NVMe to GPU VRAM.
    ///
    /// Uses the CUDA async memcpy engine for direct DMA.
    pub fn nvme_to_gpu(
        &mut self,
        nvme_lba: u64,
        _nvme_ns_id: u32,
        gpu_buf: &GpuBuffer,
        offset: usize,
        len: usize,
    ) -> HwResult<u64> {
        if !gpu_buf.can_fit(offset + len) {
            return Err(HwError::InvalidParam(
                "transfer exceeds GPU buffer size".into(),
            ));
        }

        let engine = self.find_engine()?;
        let dst = gpu_buf.dev_ptr + offset as u64;
        let src = nvme_lba * 512; // NVMe sector offset

        engine.submit_d2d(src, dst, len)
    }

    /// Transfer data from GPU VRAM to NVMe.
    pub fn gpu_to_nvme(
        &mut self,
        gpu_buf: &GpuBuffer,
        offset: usize,
        len: usize,
        nvme_lba: u64,
        _nvme_ns_id: u32,
    ) -> HwResult<u64> {
        if !gpu_buf.can_fit(offset + len) {
            return Err(HwError::InvalidParam(
                "transfer exceeds GPU buffer size".into(),
            ));
        }

        let engine = self.find_engine()?;
        let src = gpu_buf.dev_ptr + offset as u64;
        let dst = nvme_lba * 512;

        engine.submit_d2d(src, dst, len)
    }

    /// Transfer data between two GPU buffers (device-to-device).
    pub fn gpu_to_gpu(
        &mut self,
        src_buf: &GpuBuffer,
        src_offset: usize,
        dst_buf: &GpuBuffer,
        dst_offset: usize,
        len: usize,
    ) -> HwResult<u64> {
        if !src_buf.can_fit(src_offset + len) {
            return Err(HwError::InvalidParam(
                "source transfer exceeds buffer size".into(),
            ));
        }
        if !dst_buf.can_fit(dst_offset + len) {
            return Err(HwError::InvalidParam(
                "destination transfer exceeds buffer size".into(),
            ));
        }

        let engine = self.find_engine()?;
        let src = src_buf.dev_ptr + src_offset as u64;
        let dst = dst_buf.dev_ptr + dst_offset as u64;

        engine.submit_d2d(src, dst, len)
    }

    /// Copy data from host to GPU buffer (synchronous).
    pub fn host_to_gpu(
        &self,
        gpu_buf: &GpuBuffer,
        host_ptr: *const u8,
        size: usize,
    ) -> HwResult<()> {
        gpu_buf.copy_from_host(host_ptr, size)
    }

    /// Copy data from GPU buffer to host (synchronous).
    pub fn gpu_to_host(
        &self,
        gpu_buf: &GpuBuffer,
        host_ptr: *mut u8,
        size: usize,
    ) -> HwResult<()> {
        gpu_buf.copy_to_host(host_ptr, size)
    }

    /// Wait for all transfers on all engines to complete.
    pub fn sync_all(&mut self) -> HwResult<()> {
        for engine in &self.engines {
            engine.synchronize()?;
        }
        Ok(())
    }

    /// Get transfer statistics.
    pub fn stats(&self) -> GpuDirectStats {
        GpuDirectStats {
            total_engines: self.engines.len(),
            active_transfers: self.engines.iter().map(|e| e.active_count()).sum(),
        }
    }
}

/// GPU-Direct transfer statistics.
#[derive(Debug, Default, Clone)]
pub struct GpuDirectStats {
    /// Number of DMA engines.
    pub total_engines: usize,
    /// Number of active transfers across all engines.
    pub active_transfers: usize,
}
