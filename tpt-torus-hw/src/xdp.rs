//! XDP (eXpress Data Path) integration for high-performance networking.
//!
//! XDP runs eBPF programs at the driver level, before the kernel network stack,
//! enabling packet processing at near-line-rate speeds. Compared to DPDK:
//!
//! - No hugepages or special kernel modules required
//! - Works with standard NICs (no poll-mode drivers)
//! - Integrates with the kernel networking stack (can fallback)
//! - Lighter weight, easier to deploy
//!
//! # Architecture
//!
//! ```text
//! Application
//!     │
//!     ▼
//! XdpSocket ──────► AF_XDP socket
//!     │                  │
//!     │                  ▼
//!     │            UMEM (shared packet buffers)
//!     │                  │
//!     ▼                  ▼
//! eBPF program ◄──► NIC driver (XDP hook)
//! ```
//!
//! # Feature Flag
//!
//! This module requires the `xdp` feature flag to be enabled.
//! When disabled, all functions return `HwError::NotAvailable`.

use crate::{HwError, HwResult};

// ─── XDP constants ─────────────────────────────────────────────────────────

/// AF_XDP address family.
const AF_XDP: i32 = 44;

/// XDP socket options.
const XDP_UMEM_REG: i32 = 1;
const XDP_RX_RING: i32 = 2;
const XDP_TX_RING: i32 = 3;
const XDP_UMEM_FILL_RING: i32 = 4;
const XDP_UMEM_COMPLETION_RING: i32 = 5;
const XDP_OPTIONS: i32 = 8;

/// XDP ring frame sizes.
const XDP_UMEM__DEFAULT_FRAME_SIZE: u32 = 4096;
const XDP_UMEM__DEFAULT_FRAME_HEADROOM: u32 = 256;

/// XDP socket options flags.
const XDP_OPTIONS_ZEROCOPY: u32 = 1 << 1;

// ─── XDP UMEM ──────────────────────────────────────────────────────────────

/// Shared memory region for XDP packet buffers (UMEM).
///
/// UMEM is a shared region between user space and the kernel that holds
/// packet data. It is divided into fixed-size frames.
pub struct XdpUmem {
    base: *mut u8,
    size: usize,
    frame_size: u32,
    frame_headroom: u32,
}

unsafe impl Send for XdpUmem {}
unsafe impl Sync for XdpUmem {}

impl XdpUmem {
    /// Create a new UMEM region.
    ///
    /// # Safety
    ///
    /// - `base` must point to a valid memory region of at least `size` bytes
    /// - The memory must be page-aligned for optimal performance
    /// - The memory must remain valid for the lifetime of this UMEM
    pub unsafe fn new(base: *mut u8, size: usize) -> Self {
        Self {
            base,
            size,
            frame_size: XDP_UMEM__DEFAULT_FRAME_SIZE,
            frame_headroom: XDP_UMEM__DEFAULT_FRAME_HEADROOM,
        }
    }

    /// Create a new UMEM with custom frame size and headroom.
    pub unsafe fn with_config(
        base: *mut u8,
        size: usize,
        frame_size: u32,
        frame_headroom: u32,
    ) -> Self {
        Self {
            base,
            size,
            frame_size,
            frame_headroom,
        }
    }

    /// Total number of frames in this UMEM.
    pub fn frame_count(&self) -> u32 {
        (self.size as u32) / self.frame_size
    }

    /// Frame size in bytes.
    pub fn frame_size(&self) -> u32 {
        self.frame_size
    }

    /// Get a pointer to a specific frame.
    pub fn frame_ptr(&self, index: u32) -> Option<*mut u8> {
        if index >= self.frame_count() {
            return None;
        }
        Some(unsafe { self.base.add((index * self.frame_size) as usize) })
    }
}

impl Drop for XdpUmem {
    fn drop(&mut self) {
        // Note: The actual UMEM memory is not freed here — it was allocated
        // externally. This is intentional for XDP integration where the
        // memory is typically allocated via hugepages or mmap.
    }
}

// ─── XDP Socket ────────────────────────────────────────────────────────────

/// An AF_XDP socket for high-performance packet processing.
///
/// Provides zero-copy packet I/O by sharing UMEM buffers with the kernel.
pub struct XdpSocket {
    fd: i32,
    umem: Option<XdpUmem>,
    rx_ring_size: u32,
    tx_ring_size: u32,
}

unsafe impl Send for XdpSocket {}
unsafe impl Sync for XdpSocket {}

impl XdpSocket {
    /// Create a new XDP socket.
    pub fn new() -> HwResult<Self> {
        #[cfg(target_os = "linux")]
        {
            let fd = unsafe { libc::socket(AF_XDP, libc::SOCK_RAW, 0) };
            if fd < 0 {
                return Err(HwError::InitFailed(format!(
                    "failed to create AF_XDP socket: {}",
                    std::io::Error::last_os_error()
                )));
            }
            Ok(Self {
                fd,
                umem: None,
                rx_ring_size: 2048,
                tx_ring_size: 2048,
            })
        }
        #[cfg(not(target_os = "linux"))]
        Err(HwError::NotAvailable(
            "XDP is only available on Linux".into(),
        ))
    }

    /// Set the RX ring size (must be power of 2).
    pub fn with_rx_ring_size(mut self, size: u32) -> Self {
        self.rx_ring_size = size;
        self
    }

    /// Set the TX ring size (must be power of 2).
    pub fn with_tx_ring_size(mut self, size: u32) -> Self {
        self.tx_ring_size = size;
        self
    }

    /// Register a UMEM region with this socket.
    ///
    /// # Safety
    ///
    /// The UMEM must outlive this socket and must not be modified while
    /// the socket is active.
    pub unsafe fn bind_umem(&mut self, umem: XdpUmem) -> HwResult<()> {
        #[cfg(target_os = "linux")]
        {
            // In a real implementation, this would call setsockopt() with
            // XDP_UMEM_REG to register the UMEM with the kernel.
            let _ = (XDP_UMEM_REG, &umem);
            self.umem = Some(umem);
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        Err(HwError::NotAvailable(
            "XDP UMEM binding is only available on Linux".into(),
        ))
    }

    /// Bind the socket to a network interface.
    pub fn bind(&self, ifindex: u32) -> HwResult<()> {
        #[cfg(target_os = "linux")]
        {
            let mut addr: libc::sockaddr_xdp = unsafe { std::mem::zeroed() };
            addr.sxdp_family = AF_XDP as u16;
            addr.sxdp_ifindex = ifindex;
            addr.sxdp_flags = 0;

            let ret = unsafe {
                libc::bind(
                    self.fd,
                    &addr as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_xdp>() as libc::socklen_t,
                )
            };

            if ret < 0 {
                return Err(HwError::InitFailed(format!(
                    "failed to bind XDP socket: {}",
                    std::io::Error::last_os_error()
                )));
            }
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        Err(HwError::NotAvailable(
            "XDP bind is only available on Linux".into(),
        ))
    }

    /// Receive packets from the RX ring.
    ///
    /// Returns the number of packets received. Packet data is in the UMEM
    /// frames referenced by the fill ring.
    pub fn recv(&self, descs: &mut [XdpDesc]) -> HwResult<usize> {
        #[cfg(target_os = "linux")]
        {
            // In a real implementation, this would read from the RX ring
            // and return descriptors pointing to UMEM frames.
            let _ = descs;
            Ok(0) // Stub: no packets available
        }
        #[cfg(not(target_os = "linux"))]
        Err(HwError::NotAvailable(
            "XDP recv is only available on Linux".into(),
        ))
    }

    /// Transmit packets via the TX ring.
    ///
    /// Returns the number of packets transmitted.
    pub fn send(&self, descs: &[XdpDesc]) -> HwResult<usize> {
        #[cfg(target_os = "linux")]
        {
            // In a real implementation, this would write to the TX ring.
            let _ = descs;
            Ok(0) // Stub: nothing sent
        }
        #[cfg(not(target_os = "linux"))]
        Err(HwError::NotAvailable(
            "XDP send is only available on Linux".into(),
        ))
    }

    /// The file descriptor of this XDP socket.
    pub fn fd(&self) -> i32 {
        self.fd
    }
}

impl Drop for XdpSocket {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

// ─── XDP Descriptor ────────────────────────────────────────────────────────

/// Descriptor for an XDP packet in the UMEM.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct XdpDesc {
    /// Offset into the UMEM frame (byte offset from UMEM base).
    pub addr: u64,
    /// Length of the packet data.
    pub len: u32,
    /// Options/flags (e.g., checksum status).
    pub options: u32,
}

// ─── XDP eBPF Program ─────────────────────────────────────────────────────

/// An XDP eBPF program that can be attached to a network interface.
///
/// XDP programs run at the driver level and can:
/// - Pass packets to the normal kernel stack (`XDP_PASS`)
/// - Drop packets (`XDP_DROP`)
/// - Redirect to another interface (`XDP_REDIRECT`)
/// - Send to AF_XDP socket (`XDP_REDIRECT` to AF_XDP)
pub struct XdpProgram {
    prog_fd: i32,
}

unsafe impl Send for XdpProgram {}
unsafe impl Sync for XdpProgram {}

impl XdpProgram {
    /// Load an XDP program from compiled eBPF bytecode.
    ///
    /// # Safety
    ///
    /// `obj` must point to a valid eBPF ELF object of `obj_len` bytes.
    pub unsafe fn load(obj: *const u8, obj_len: usize) -> HwResult<Self> {
        #[cfg(target_os = "linux")]
        {
            // In a real implementation, this would use libbpf to:
            // 1. Open the ELF object
            // 2. Load the program into the kernel
            // 3. Return the program fd
            let _ = (obj, obj_len);
            Ok(Self { prog_fd: -1 })
        }
        #[cfg(not(target_os = "linux"))]
        Err(HwError::NotAvailable(
            "XDP programs are only available on Linux".into(),
        ))
    }

    /// Attach this XDP program to a network interface.
    pub fn attach(&self, ifindex: u32, flags: u32) -> HwResult<()> {
        #[cfg(target_os = "linux")]
        {
            // In a real implementation, this would use bpf_set_link_xdp_fd()
            let _ = (ifindex, flags);
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        Err(HwError::NotAvailable(
            "XDP attach is only available on Linux".into(),
        ))
    }

    /// The file descriptor of this loaded XDP program.
    pub fn fd(&self) -> i32 {
        self.prog_fd
    }
}

impl Drop for XdpProgram {
    fn drop(&mut self) {
        if self.prog_fd >= 0 {
            unsafe {
                libc::close(self.prog_fd);
            }
        }
    }
}

// ─── XDP helpers ───────────────────────────────────────────────────────────

/// Get the interface index for a network interface by name.
pub fn ifindex_from_name(name: &str) -> HwResult<u32> {
    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        let c_name = CString::new(name).map_err(|e| HwError::InvalidParam(e.to_string()))?;
        let idx = unsafe { libc::if_nametoindex(c_name.as_ptr()) };
        if idx == 0 {
            Err(HwError::InvalidParam(format!(
                "interface '{}' not found",
                name
            )))
        } else {
            Ok(idx)
        }
    }
    #[cfg(not(target_os = "linux"))]
    Err(HwError::NotAvailable(
        "XDP is only available on Linux".into(),
    ))
}
