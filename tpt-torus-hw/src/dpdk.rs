//! DPDK (Data Plane Development Kit) integration for user-space networking.
//!
//! DPDK provides a user-space networking stack that bypasses the kernel,
//! enabling high-performance packet processing directly from user space.
//! This module wraps DPDK's core functionality behind safe Rust abstractions.
//!
//! # Architecture
//!
//! ```text
//! Application
//!     │
//!     ▼
//! Dpdk ──────► Mempool (mbufs)
//!     │                  │
//!     │                  ▼
//!     │            Packet Rings (TX/RX)
//!     │                  │
//!     ▼                  ▼
//! Ethernet NIC ◄──► Poll Mode Driver
//! ```
//!
//! # Feature Flag
//!
//! This module requires the `dpdk` feature flag to be enabled. When the
//! feature is disabled, every operation returns [`crate::HwError::NotAvailable`].
//! When enabled, the DPDK shared libraries (`libdpdk.so` / `libdpdk.dll` /
//! `libdpdk.dylib`, plus `librte_*` on some distributions) are loaded lazily
//! at runtime via `libloading`. If the library (or a required symbol) cannot
//! be found, operations degrade gracefully to
//! [`crate::HwError::NotAvailable`] rather than failing to link.
//!
//! The FFI signatures and the `rte_eth_conf` / `rte_eth_stats` layouts here
//! target the DPDK 23.x ABI. If you build against a different DPDK version,
//! regenerate the struct layouts from the installed `rte_*.h` headers.

use crate::{HwError, HwResult};
#[cfg(feature = "dpdk")]
use std::os::raw::{c_char, c_int, c_void};

// ─── DPDK opaque / layout types ────────────────────────────────────────────

/// Opaque DPDK mempool handle.
pub struct RteMempool {
    _private: [u8; 0],
}

/// Opaque DPDK mbuf handle.
pub struct RteMbuf {
    _private: [u8; 0],
}

/// Opaque DPDK ethernet device handle.
pub struct RteEthDev {
    _private: [u8; 0],
}

/// Best-effort view of the leading fields of `struct rte_mbuf` (DPDK 23.x).
///
/// The full `rte_mbuf` is large and version-specific; this captures the fields
/// the [`Mbuf`] accessors read. Regenerate from `rte_mbuf.h` if layouts drift.
#[repr(C)]
pub struct DpdkMbuf {
    /// Pointer to the segment buffer.
    pub buf_addr: *mut u8,
    /// I/O virtual address (unused by the Rust side).
    pub buf_iova: u64,
    /// Reference count.
    pub refcnt: u16,
    /// Number of segments.
    pub nb_segs: u16,
    /// Port the packet was received on.
    pub port: u16,
    /// Packet type flags.
    pub packet_type: u32,
    /// Total packet length.
    pub pkt_len: u32,
    /// Data length of this segment.
    pub data_len: u16,
    /// Data offset within the buffer.
    pub data_off: u16,
    /// VLAN TCI.
    pub vlan_tci: u16,
}

/// `struct rte_eth_conf` is large and version-specific. A zeroed buffer of
/// sufficient size behaves as "all defaults" when passed to
/// `rte_eth_dev_configure`, which is what we want for the common case.
#[repr(C)]
pub struct RteEthConf {
    _pad: [u8; 1024],
}

/// `struct rte_eth_stats` leading counters (ipackets, opackets, ibytes,
/// obytes, imissed). `rx_nombuf` is read at a best-effort offset that matches
/// DPDK 19–23 layouts.
#[repr(C)]
pub struct RteEthStats {
    pub ipackets: u64,
    pub opackets: u64,
    pub ibytes: u64,
    pub obytes: u64,
    pub imissed: u64,
    _pad: [u64; 3],
    pub rx_nombuf: u64,
}

// ─── Dynamic loader (feature `dpdk`) ───────────────────────────────────────

#[cfg(feature = "dpdk")]
#[allow(non_snake_case, dead_code)]
struct DpdkApi {
    _lib: libloading::Library,
    rte_eal_init: unsafe extern "C" fn(c_int, *mut *mut c_char) -> c_int,
    rte_eal_cleanup: unsafe extern "C" fn() -> c_int,
    rte_eth_dev_count_avail: unsafe extern "C" fn() -> u16,
    rte_pktmbuf_pool_create: unsafe extern "C" fn(
        *const c_char,
        u32,
        u32,
        u16,
        u16,
        i32,
    ) -> *mut RteMempool,
    rte_mempool_free: unsafe extern "C" fn(*mut RteMempool),
    rte_mempool_avail_count: unsafe extern "C" fn(*mut RteMempool) -> u32,
    rte_pktmbuf_alloc: unsafe extern "C" fn(*mut RteMempool) -> *mut RteMbuf,
    rte_pktmbuf_free: unsafe extern "C" fn(*mut RteMbuf),
    rte_pktmbuf_free_bulk: unsafe extern "C" fn(*mut RteMempool, *mut *mut RteMbuf, u32) -> u32,
    rte_pktmbuf_append: unsafe extern "C" fn(*mut RteMbuf, u16) -> *mut u8,
    rte_pktmbuf_prepend: unsafe extern "C" fn(*mut RteMbuf, u16) -> *mut u8,
    rte_pktmbuf_adj: unsafe extern "C" fn(*mut RteMbuf, u16) -> *mut u8,
    rte_pktmbuf_trim: unsafe extern "C" fn(*mut RteMbuf, u16) -> c_int,
    rte_eth_dev_configure:
        unsafe extern "C" fn(u16, u16, u16, *const RteEthConf) -> c_int,
    rte_eth_rx_queue_setup:
        unsafe extern "C" fn(u16, u16, u16, i32, *const c_void, *mut RteMempool) -> c_int,
    rte_eth_tx_queue_setup:
        unsafe extern "C" fn(u16, u16, u16, i32, *const c_void) -> c_int,
    rte_eth_dev_start: unsafe extern "C" fn(u16) -> c_int,
    rte_eth_dev_stop: unsafe extern "C" fn(u16) -> c_int,
    rte_eth_stats_get: unsafe extern "C" fn(u16, *mut RteEthStats) -> c_int,
    rte_eth_rx_burst:
        unsafe extern "C" fn(u16, u16, *mut *mut RteMbuf, u16) -> u16,
    rte_eth_tx_burst:
        unsafe extern "C" fn(u16, u16, *mut *mut RteMbuf, u16) -> u16,
}

#[cfg(feature = "dpdk")]
static DPDK_API: std::sync::Mutex<Option<DpdkApi>> = std::sync::Mutex::new(None);

/// Load the DPDK shared library once and cache the resolved API.
#[cfg(feature = "dpdk")]
fn api() -> HwResult<&'static DpdkApi> {
    {
        let guard = DPDK_API.lock().unwrap();
        if guard.is_some() {
            // SAFETY: just verified Some; it stays Some for the program lifetime.
            return Ok(unsafe { &*(&*guard as *const Option<DpdkApi> as *const DpdkApi) });
        }
    }

    let lib = unsafe {
        libloading::Library::new("libdpdk.so")
            .or_else(|_| libloading::Library::new("libdpdk.dll"))
            .or_else(|_| libloading::Library::new("libdpdk.dylib"))
            .or_else(|_| libloading::Library::new("libdpdk.so.23"))
            .map_err(|e| HwError::NotAvailable(format!("DPDK library not found: {}", e)))?
    };

    macro_rules! load_fn {
        ($lib:expr, $name:ident, $ty:ty) => {{
            let sym = unsafe { $lib.get::<$ty>(stringify!($name).as_bytes()) }
                .map_err(|e| {
                    HwError::NotAvailable(format!(
                        "DPDK symbol {} not found: {}",
                        stringify!($name),
                        e
                    ))
                })?;
            *sym
        }};
    }

    let api = DpdkApi {
        rte_eal_init: load_fn!(lib, rte_eal_init, unsafe extern "C" fn(c_int, *mut *mut c_char) -> c_int),
        rte_eal_cleanup: load_fn!(lib, rte_eal_cleanup, unsafe extern "C" fn() -> c_int),
        rte_eth_dev_count_avail: load_fn!(lib, rte_eth_dev_count_avail, unsafe extern "C" fn() -> u16),
        rte_pktmbuf_pool_create: load_fn!(
            lib,
            rte_pktmbuf_pool_create,
            unsafe extern "C" fn(*const c_char, u32, u32, u16, u16, i32) -> *mut RteMempool
        ),
        rte_mempool_free: load_fn!(lib, rte_mempool_free, unsafe extern "C" fn(*mut RteMempool)),
        rte_mempool_avail_count: load_fn!(
            lib,
            rte_mempool_avail_count,
            unsafe extern "C" fn(*mut RteMempool) -> u32
        ),
        rte_pktmbuf_alloc: load_fn!(
            lib,
            rte_pktmbuf_alloc,
            unsafe extern "C" fn(*mut RteMempool) -> *mut RteMbuf
        ),
        rte_pktmbuf_free: load_fn!(lib, rte_pktmbuf_free, unsafe extern "C" fn(*mut RteMbuf)),
        rte_pktmbuf_free_bulk: load_fn!(
            lib,
            rte_pktmbuf_free_bulk,
            unsafe extern "C" fn(*mut RteMempool, *mut *mut RteMbuf, u32) -> u32
        ),
        rte_pktmbuf_append: load_fn!(
            lib,
            rte_pktmbuf_append,
            unsafe extern "C" fn(*mut RteMbuf, u16) -> *mut u8
        ),
        rte_pktmbuf_prepend: load_fn!(
            lib,
            rte_pktmbuf_prepend,
            unsafe extern "C" fn(*mut RteMbuf, u16) -> *mut u8
        ),
        rte_pktmbuf_adj: load_fn!(
            lib,
            rte_pktmbuf_adj,
            unsafe extern "C" fn(*mut RteMbuf, u16) -> *mut u8
        ),
        rte_pktmbuf_trim: load_fn!(lib, rte_pktmbuf_trim, unsafe extern "C" fn(*mut RteMbuf, u16) -> c_int),
        rte_eth_dev_configure: load_fn!(
            lib,
            rte_eth_dev_configure,
            unsafe extern "C" fn(u16, u16, u16, *const RteEthConf) -> c_int
        ),
        rte_eth_rx_queue_setup: load_fn!(
            lib,
            rte_eth_rx_queue_setup,
            unsafe extern "C" fn(u16, u16, u16, i32, *const c_void, *mut RteMempool) -> c_int
        ),
        rte_eth_tx_queue_setup: load_fn!(
            lib,
            rte_eth_tx_queue_setup,
            unsafe extern "C" fn(u16, u16, u16, i32, *const c_void) -> c_int
        ),
        rte_eth_dev_start: load_fn!(lib, rte_eth_dev_start, unsafe extern "C" fn(u16) -> c_int),
        rte_eth_dev_stop: load_fn!(lib, rte_eth_dev_stop, unsafe extern "C" fn(u16) -> c_int),
        rte_eth_stats_get: load_fn!(
            lib,
            rte_eth_stats_get,
            unsafe extern "C" fn(u16, *mut RteEthStats) -> c_int
        ),
        rte_eth_rx_burst: load_fn!(
            lib,
            rte_eth_rx_burst,
            unsafe extern "C" fn(u16, u16, *mut *mut RteMbuf, u16) -> u16
        ),
        rte_eth_tx_burst: load_fn!(
            lib,
            rte_eth_tx_burst,
            unsafe extern "C" fn(u16, u16, *mut *mut RteMbuf, u16) -> u16
        ),
        _lib: lib,
    };

    let mut guard = DPDK_API.lock().unwrap();
    *guard = Some(api);
    Ok(unsafe { &*(&*guard as *const Option<DpdkApi> as *const DpdkApi) })
}

// ─── DPDK environment ──────────────────────────────────────────────────────

/// Top-level DPDK entry point.
///
/// Initialize the DPDK Environment Abstraction Layer (EAL). This must be
/// called before any other DPDK operation.
pub struct Dpdk;

impl Dpdk {
    /// Initialize the DPDK EAL.
    ///
    /// `args` are passed straight to `rte_eal_init` (e.g. `["-l", "0-3",
    /// "--proc-type=primary"]`). The first argument should be a program name.
    pub fn eal_init(args: &[&str]) -> HwResult<()> {
        #[cfg(feature = "dpdk")]
        {
            let api = api()?;
            let c_args: Vec<std::ffi::CString> = args
                .iter()
                .map(|a| {
                    std::ffi::CString::new(*a)
                        .map_err(|e| HwError::InvalidParam(format!("invalid EAL arg: {}", e)))
                })
                .collect::<Result<_, _>>()?;
            let mut ptrs: Vec<*mut c_char> = c_args
                .iter()
                .map(|c| c.as_ptr() as *mut c_char)
                .collect();
            let argc = ptrs.len() as c_int;
            let rc = unsafe { (api.rte_eal_init)(argc, ptrs.as_mut_ptr()) };
            if rc < 0 {
                return Err(HwError::InitFailed(format!("rte_eal_init returned {}", rc)));
            }
            Ok(())
        }
        #[cfg(not(feature = "dpdk"))]
        {
            let _ = args;
            Err(HwError::NotAvailable(
                "DPDK support requires the `dpdk` feature and an installed DPDK library".into(),
            ))
        }
    }

    /// Clean up the DPDK EAL.
    pub fn eal_cleanup() -> HwResult<()> {
        #[cfg(feature = "dpdk")]
        {
            let api = api()?;
            let rc = unsafe { (api.rte_eal_cleanup)() };
            if rc != 0 {
                return Err(HwError::InitFailed(format!("rte_eal_cleanup returned {}", rc)));
            }
            Ok(())
        }
        #[cfg(not(feature = "dpdk"))]
        {
            Err(HwError::NotAvailable(
                "DPDK support requires the `dpdk` feature and an installed DPDK library".into(),
            ))
        }
    }

    /// Number of available (attached) Ethernet devices.
    pub fn port_count() -> HwResult<u16> {
        #[cfg(feature = "dpdk")]
        {
            let api = api()?;
            Ok(unsafe { (api.rte_eth_dev_count_avail)() })
        }
        #[cfg(not(feature = "dpdk"))]
        {
            Err(HwError::NotAvailable(
                "DPDK support requires the `dpdk` feature and an installed DPDK library".into(),
            ))
        }
    }
}

// ─── DPDK Mempool ──────────────────────────────────────────────────────────

/// A DPDK memory pool for packet buffers (mbufs).
pub struct Mempool {
    name: String,
    count: usize,
    _pool: *mut RteMempool,
}

unsafe impl Send for Mempool {}
unsafe impl Sync for Mempool {}

impl Mempool {
    /// Create a new mempool of packet buffers (mbufs).
    ///
    /// # Arguments
    /// - `name`: Name of the mempool.
    /// - `count`: Number of mbufs in the pool.
    /// - `cache_size`: Per-core cache size (0 to disable).
    /// - `priv_size`: Private data size per mbuf.
    /// - `data_room_size`: Data buffer size per mbuf.
    pub fn new(
        name: &str,
        count: usize,
        cache_size: u32,
        priv_size: u16,
        data_room_size: u16,
    ) -> HwResult<Self> {
        #[cfg(feature = "dpdk")]
        {
            let api = api()?;
            let c_name = std::ffi::CString::new(name)
                .map_err(|e| HwError::InvalidParam(format!("invalid mempool name: {}", e)))?;
            let pool = unsafe {
                (api.rte_pktmbuf_pool_create)(
                    c_name.as_ptr(),
                    count as u32,
                    cache_size,
                    priv_size,
                    data_room_size,
                    -1, // SOCKET_ID_ANY
                )
            };
            if pool.is_null() {
                return Err(HwError::OutOfMemory);
            }
            Ok(Self {
                name: name.to_string(),
                count,
                _pool: pool,
            })
        }
        #[cfg(not(feature = "dpdk"))]
        {
            let _ = (name, count, cache_size, priv_size, data_room_size);
            Err(HwError::NotAvailable(
                "DPDK support requires the `dpdk` feature and an installed DPDK library".into(),
            ))
        }
    }

    /// Get the raw DPDK mempool pointer.
    pub fn as_ptr(&self) -> *mut RteMempool {
        self._pool
    }

    /// Get the name of the mempool.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Number of mbufs in the pool.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Allocate an mbuf from the pool.
    pub fn alloc(&self) -> Option<Mbuf> {
        #[cfg(feature = "dpdk")]
        {
            let api = api().ok()?;
            let ptr = unsafe { (api.rte_pktmbuf_alloc)(self._pool) };
            if ptr.is_null() {
                None
            } else {
                Some(Mbuf { ptr: ptr as *mut DpdkMbuf })
            }
        }
        #[cfg(not(feature = "dpdk"))]
        {
            None
        }
    }

    /// Return mbufs to the pool.
    pub fn free_bulk(&self, mbufs: &[Mbuf]) {
        #[cfg(feature = "dpdk")]
        {
            if mbufs.is_empty() {
                return;
            }
            if let Ok(api) = api() {
                let mut ptrs: Vec<*mut RteMbuf> =
                    mbufs.iter().map(|m| m.ptr as *mut RteMbuf).collect();
                unsafe {
                    (api.rte_pktmbuf_free_bulk)(
                        self._pool,
                        ptrs.as_mut_ptr(),
                        ptrs.len() as u32,
                    )
                };
            }
        }
        #[cfg(not(feature = "dpdk"))]
        {
            let _ = mbufs;
        }
    }

    /// Number of available mbufs.
    pub fn available_count(&self) -> u32 {
        #[cfg(feature = "dpdk")]
        {
            if let Ok(api) = api() {
                return unsafe { (api.rte_mempool_avail_count)(self._pool) };
            }
            0
        }
        #[cfg(not(feature = "dpdk"))]
        {
            0
        }
    }
}

impl Drop for Mempool {
    fn drop(&mut self) {
        #[cfg(feature = "dpdk")]
        {
            if let Ok(api) = api() {
                unsafe { (api.rte_mempool_free)(self._pool) };
            }
        }
    }
}

// ─── DPDK Mbuf ─────────────────────────────────────────────────────────────

/// A DPDK packet buffer (mbuf).
pub struct Mbuf {
    ptr: *mut DpdkMbuf,
}

unsafe impl Send for Mbuf {}
unsafe impl Sync for Mbuf {}

impl Mbuf {
    /// Create a new Mbuf wrapper.
    ///
    /// # Safety
    /// `ptr` must be a valid DPDK mbuf pointer.
    pub unsafe fn new(ptr: *mut DpdkMbuf) -> Self {
        Self { ptr }
    }

    /// Get the raw mbuf pointer.
    pub fn as_ptr(&self) -> *mut DpdkMbuf {
        self.ptr
    }

    /// Get the packet data pointer.
    pub fn data_ptr(&self) -> *mut u8 {
        unsafe { (*self.ptr).buf_addr.add((*self.ptr).data_off as usize) }
    }

    /// Get the packet data length.
    pub fn data_len(&self) -> u16 {
        unsafe { (*self.ptr).data_len }
    }

    /// Get the total packet length (including chained segments).
    pub fn pkt_len(&self) -> u32 {
        unsafe { (*self.ptr).pkt_len }
    }

    /// Get the number of segments.
    pub fn nb_segs(&self) -> u16 {
        unsafe { (*self.ptr).nb_segs }
    }

    /// Append data to the packet.
    pub fn append(&mut self, len: u16) -> HwResult<*mut u8> {
        #[cfg(feature = "dpdk")]
        {
            let api = api()?;
            let ptr = unsafe { (api.rte_pktmbuf_append)(self.ptr as *mut RteMbuf, len) };
            if ptr.is_null() {
                Err(HwError::OutOfMemory)
            } else {
                Ok(ptr)
            }
        }
        #[cfg(not(feature = "dpdk"))]
        {
            let _ = len;
            Err(HwError::NotAvailable(
                "DPDK support requires the `dpdk` feature and an installed DPDK library".into(),
            ))
        }
    }

    /// Prepend data to the packet.
    pub fn prepend(&mut self, len: u16) -> HwResult<*mut u8> {
        #[cfg(feature = "dpdk")]
        {
            let api = api()?;
            let ptr = unsafe { (api.rte_pktmbuf_prepend)(self.ptr as *mut RteMbuf, len) };
            if ptr.is_null() {
                Err(HwError::OutOfMemory)
            } else {
                Ok(ptr)
            }
        }
        #[cfg(not(feature = "dpdk"))]
        {
            let _ = len;
            Err(HwError::NotAvailable(
                "DPDK support requires the `dpdk` feature and an installed DPDK library".into(),
            ))
        }
    }

    /// Trim `len` bytes from the head of the packet.
    pub fn adj(&mut self, len: u16) -> HwResult<*mut u8> {
        #[cfg(feature = "dpdk")]
        {
            let api = api()?;
            let ptr = unsafe { (api.rte_pktmbuf_adj)(self.ptr as *mut RteMbuf, len) };
            if ptr.is_null() {
                Err(HwError::InvalidParam("rte_pktmbuf_adj failed".into()))
            } else {
                Ok(ptr)
            }
        }
        #[cfg(not(feature = "dpdk"))]
        {
            let _ = len;
            Err(HwError::NotAvailable(
                "DPDK support requires the `dpdk` feature and an installed DPDK library".into(),
            ))
        }
    }

    /// Trim `len` bytes from the tail of the packet.
    pub fn trim(&mut self, len: u16) -> HwResult<()> {
        #[cfg(feature = "dpdk")]
        {
            let api = api()?;
            let rc = unsafe { (api.rte_pktmbuf_trim)(self.ptr as *mut RteMbuf, len) };
            if rc != 0 {
                Err(HwError::InvalidParam("rte_pktmbuf_trim failed".into()))
            } else {
                Ok(())
            }
        }
        #[cfg(not(feature = "dpdk"))]
        {
            let _ = len;
            Err(HwError::NotAvailable(
                "DPDK support requires the `dpdk` feature and an installed DPDK library".into(),
            ))
        }
    }
}

impl Drop for Mbuf {
    fn drop(&mut self) {
        #[cfg(feature = "dpdk")]
        {
            if let Ok(api) = api() {
                unsafe { (api.rte_pktmbuf_free)(self.ptr as *mut RteMbuf) };
            }
        }
    }
}

// ─── DPDK Port ─────────────────────────────────────────────────────────────

/// Configuration for a DPDK ethernet port.
#[derive(Debug, Clone)]
pub struct EthConf {
    /// Link speed (0 = auto-negotiate).
    pub link_speed: u32,
    /// Maximum packet length.
    pub max_rx_pkt_len: u32,
    /// Number of RX queues.
    pub nb_rx_queues: u16,
    /// Number of TX queues.
    pub nb_tx_queues: u16,
    /// Offload flags.
    pub offloads: u64,
}

impl Default for EthConf {
    fn default() -> Self {
        Self {
            link_speed: 0,
            max_rx_pkt_len: 1518,
            nb_rx_queues: 1,
            nb_tx_queues: 1,
            offloads: 0,
        }
    }
}

/// A DPDK ethernet port with poll-mode I/O.
pub struct Port {
    port_id: u16,
    #[allow(dead_code)] // Used when DPDK is enabled
    conf: EthConf,
}

unsafe impl Send for Port {}
unsafe impl Sync for Port {}

impl Port {
    /// Open a DPDK ethernet port.
    ///
    /// Configures the device with `nb_rx_queues`/`nb_tx_queues`, sets up one
    /// RX/TX queue per ring (backed by `mempool` with 1024 descriptors), and
    /// leaves the port ready to be started via [`Port::start`].
    pub fn open(port_id: u16, conf: EthConf, mempool: &Mempool) -> HwResult<Self> {
        #[cfg(feature = "dpdk")]
        {
            let api = api()?;
            let eth_conf = RteEthConf { _pad: [0u8; 1024] };
            let rc = unsafe {
                (api.rte_eth_dev_configure)(
                    port_id,
                    conf.nb_rx_queues,
                    conf.nb_tx_queues,
                    &eth_conf as *const RteEthConf,
                )
            };
            if rc != 0 {
                return Err(HwError::InitFailed(format!(
                    "rte_eth_dev_configure returned {}",
                    rc
                )));
            }

            const NB_DESC: u16 = 1024;
            for q in 0..conf.nb_rx_queues {
                let rc = unsafe {
                    (api.rte_eth_rx_queue_setup)(
                        port_id,
                        q,
                        NB_DESC,
                        -1, // SOCKET_ID_ANY
                        std::ptr::null(),
                        mempool.as_ptr(),
                    )
                };
                if rc != 0 {
                    return Err(HwError::InitFailed(format!(
                        "rte_eth_rx_queue_setup(q={}) returned {}",
                        q, rc
                    )));
                }
            }
            for q in 0..conf.nb_tx_queues {
                let rc = unsafe {
                    (api.rte_eth_tx_queue_setup)(
                        port_id,
                        q,
                        NB_DESC,
                        -1,
                        std::ptr::null(),
                    )
                };
                if rc != 0 {
                    return Err(HwError::InitFailed(format!(
                        "rte_eth_tx_queue_setup(q={}) returned {}",
                        q, rc
                    )));
                }
            }

            Ok(Self { port_id, conf })
        }
        #[cfg(not(feature = "dpdk"))]
        {
            let _ = (port_id, &conf, mempool);
            Err(HwError::NotAvailable(
                "DPDK support requires the `dpdk` feature and an installed DPDK library".into(),
            ))
        }
    }

    /// Start the port.
    pub fn start(&mut self) -> HwResult<()> {
        #[cfg(feature = "dpdk")]
        {
            let api = api()?;
            let rc = unsafe { (api.rte_eth_dev_start)(self.port_id) };
            if rc != 0 {
                return Err(HwError::InitFailed(format!(
                    "rte_eth_dev_start returned {}",
                    rc
                )));
            }
            Ok(())
        }
        #[cfg(not(feature = "dpdk"))]
        {
            Err(HwError::NotAvailable(
                "DPDK support requires the `dpdk` feature and an installed DPDK library".into(),
            ))
        }
    }

    /// Stop the port.
    pub fn stop(&mut self) {
        #[cfg(feature = "dpdk")]
        {
            if let Ok(api) = api() {
                unsafe { (api.rte_eth_dev_stop)(self.port_id) };
            }
        }
    }

    /// Receive packets from the port.
    ///
    /// # Returns
    /// The received mbufs (up to `mbufs.capacity()`).
    pub fn rx_burst(&self, queue_id: u16, mbufs: &mut Vec<Mbuf>) -> HwResult<usize> {
        #[cfg(feature = "dpdk")]
        {
            let api = api()?;
            let mut ptrs: Vec<*mut RteMbuf> = vec![std::ptr::null_mut(); mbufs.capacity().max(1)];
            let n = unsafe {
                (api.rte_eth_rx_burst)(
                    self.port_id,
                    queue_id,
                    ptrs.as_mut_ptr(),
                    ptrs.len() as u16,
                )
            };
            mbufs.clear();
            for &p in ptrs.iter().take(n as usize) {
                if !p.is_null() {
                    mbufs.push(Mbuf {
                        ptr: p as *mut DpdkMbuf,
                    });
                }
            }
            Ok(n as usize)
        }
        #[cfg(not(feature = "dpdk"))]
        {
            let _ = (queue_id, mbufs);
            Err(HwError::NotAvailable(
                "DPDK support requires the `dpdk` feature and an installed DPDK library".into(),
            ))
        }
    }

    /// Transmit packets on the port.
    ///
    /// # Returns
    /// Number of packets successfully transmitted.
    pub fn tx_burst(&self, queue_id: u16, mbufs: &[Mbuf]) -> HwResult<usize> {
        #[cfg(feature = "dpdk")]
        {
            let api = api()?;
            let mut ptrs: Vec<*mut RteMbuf> =
                mbufs.iter().map(|m| m.ptr as *mut RteMbuf).collect();
            let n = unsafe {
                (api.rte_eth_tx_burst)(
                    self.port_id,
                    queue_id,
                    ptrs.as_mut_ptr(),
                    ptrs.len() as u16,
                )
            };
            Ok(n as usize)
        }
        #[cfg(not(feature = "dpdk"))]
        {
            let _ = (queue_id, mbufs);
            Err(HwError::NotAvailable(
                "DPDK support requires the `dpdk` feature and an installed DPDK library".into(),
            ))
        }
    }

    /// Get port statistics.
    pub fn stats(&self) -> HwResult<PortStats> {
        #[cfg(feature = "dpdk")]
        {
            let api = api()?;
            let mut stats = RteEthStats {
                ipackets: 0,
                opackets: 0,
                ibytes: 0,
                obytes: 0,
                imissed: 0,
                _pad: [0; 3],
                rx_nombuf: 0,
            };
            let rc = unsafe { (api.rte_eth_stats_get)(self.port_id, &mut stats as *mut RteEthStats) };
            if rc != 0 {
                return Err(HwError::InitFailed(format!(
                    "rte_eth_stats_get returned {}",
                    rc
                )));
            }
            Ok(PortStats {
                rx_packets: stats.ipackets,
                tx_packets: stats.opackets,
                rx_bytes: stats.ibytes,
                tx_bytes: stats.obytes,
                rx_errors: 0,
                tx_errors: 0,
                rx_missed: stats.imissed,
                rx_nombuf: stats.rx_nombuf,
            })
        }
        #[cfg(not(feature = "dpdk"))]
        {
            Err(HwError::NotAvailable(
                "DPDK support requires the `dpdk` feature and an installed DPDK library".into(),
            ))
        }
    }

    /// Get the port ID.
    pub fn port_id(&self) -> u16 {
        self.port_id
    }
}

/// Port statistics.
#[derive(Debug, Default, Clone)]
pub struct PortStats {
    /// Packets received.
    pub rx_packets: u64,
    /// Packets transmitted.
    pub tx_packets: u64,
    /// Bytes received.
    pub rx_bytes: u64,
    /// Bytes transmitted.
    pub tx_bytes: u64,
    /// RX errors.
    pub rx_errors: u64,
    /// TX errors.
    pub tx_errors: u64,
    /// RX missed (dropped) packets.
    pub rx_missed: u64,
    /// RX no-mbuffer errors.
    pub rx_nombuf: u64,
}
