//! Hardware Bypass layer for TPT Torus.
//!
//! This crate provides direct user-space access to storage and networking hardware,
//! bypassing the OS kernel entirely for maximum performance.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                 Application Layer                       │
//! │  (TorusAsync / Flow / Result API)                      │
//! ├─────────────────────────────────────────────────────────┤
//! │              Hardware Bypass Layer                      │
//! │  ┌───────────┐  ┌───────────┐  ┌───────────────────┐  │
//! │  │    SPDK   │  │    DPDK   │  │   GPU-Direct      │  │
//! │  │  (NVMe)   │  │ (Network) │  │  (DMA Orchest.)   │  │
//! │  └───────────┘  └───────────┘  └───────────────────┘  │
//! ├─────────────────────────────────────────────────────────┤
//! │           Virtual Torus (Core Abstraction)              │
//! ├─────────────────────────────────────────────────────────┤
//! │     Native Backends (io_uring / IOCP / kqueue)          │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! # Feature Flags
//!
//! - `spdk`: Enable SPDK integration (requires SPDK installed)
//! - `dpdk`: Enable DPDK integration (requires DPDK installed)
//! - `gpu_direct`: Enable GPU-Direct orchestration (requires CUDA)

#[cfg(feature = "gpu_direct")]
pub mod cuda;
#[cfg(all(unix, feature = "gpu_direct"))]
pub mod dma_pool;
pub mod dpdk;
#[cfg(feature = "gpu_direct")]
pub mod gpu_direct;
#[cfg(all(target_os = "linux", feature = "gpu_direct"))]
pub mod nvme_gpu;
pub mod spdk;
#[cfg(feature = "xdp")]
pub mod xdp;

// Re-export key types
pub use dpdk::{Mbuf, Mempool};
#[cfg(feature = "gpu_direct")]
pub use gpu_direct::{GpuBuffer, GpuDirect, TransferDirection};
pub use spdk::{
    Controller, NvmeCmd, NvmeNamespace, Spdk, SpdkNvmeCpl, SpdkNvmeTransportId,
};

/// Errors specific to hardware bypass operations.
#[derive(Debug)]
pub enum HwError {
    /// The hardware bypass library is not available.
    NotAvailable(String),
    /// Failed to initialize the hardware.
    InitFailed(String),
    /// The operation timed out.
    Timeout,
    /// Invalid parameter.
    InvalidParam(String),
    /// Hardware queue is full.
    QueueFull,
    /// Memory allocation failed.
    OutOfMemory,
    /// The device is not supported.
    UnsupportedDevice(String),
}

impl std::fmt::Display for HwError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HwError::NotAvailable(msg) => write!(f, "hardware bypass not available: {}", msg),
            HwError::InitFailed(msg) => write!(f, "hardware initialization failed: {}", msg),
            HwError::Timeout => write!(f, "operation timed out"),
            HwError::InvalidParam(msg) => write!(f, "invalid parameter: {}", msg),
            HwError::QueueFull => write!(f, "hardware queue is full"),
            HwError::OutOfMemory => write!(f, "memory allocation failed"),
            HwError::UnsupportedDevice(msg) => write!(f, "unsupported device: {}", msg),
        }
    }
}

impl std::error::Error for HwError {}

/// Result type for hardware bypass operations.
pub type HwResult<T> = Result<T, HwError>;
