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
//! Spdk ──────► Controller ──────► NvmeNamespace
//!     │                │                  │
//!     │                ▼                  ▼
//!     │         IO Qpair ◄──► NVMe Device (user-space)
//! ```
//!
//! # Feature Flag
//!
//! This module requires the `spdk` feature flag to be enabled. When the
//! feature is disabled, every operation returns [`crate::HwError::NotAvailable`].
//! When enabled, the SPDK shared library (`libspdk.so` / `libspdk.dll` /
//! `libspdk.dylib`) is loaded lazily at runtime via `libloading`. If the
//! library (or a required symbol) cannot be found, operations degrade
//! gracefully to [`crate::HwError::NotAvailable`] rather than failing to link.
//!
//! The FFI signatures here target the SPDK 24.x public ABI. If you build
//! against a different SPDK version, regenerate the struct layouts from the
//! installed `spdk/nvme.h` / `spdk/env.h` headers.

use crate::{HwError, HwResult};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int, c_void};

// ─── SPDK types (ABI mirrors for the public headers) ───────────────────────

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

/// SPDK NVMe transport ID (used to address a controller during probe).
///
/// Layout mirrors `struct spdk_nvme_transport_id` (SPDK 24.x).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SpdkNvmeTransportId {
    /// Transport string (e.g. "trtype:PCIe traddr:0000:01:00.0").
    pub trstring: [c_char; 256],
    /// Transport address.
    pub traddr: [c_char; 256],
    /// Transport service ID.
    pub trsvcid: [c_char; 32],
    /// Subsystem NQN.
    pub subnqn: [c_char; 256],
    /// Host NQN.
    pub hostnqn: [c_char; 256],
    /// Host address.
    pub hostaddr: [c_char; 256],
    /// Host service ID.
    pub hostsvcid: [c_char; 32],
    /// Source subsystem NQN.
    pub ssrcid: [c_char; 32],
    /// Secure channel.
    pub secure_channel: [c_char; 16],
}

impl Default for SpdkNvmeTransportId {
    fn default() -> Self {
        // SAFETY: an all-zero buffer is a valid (zeroed) representation.
        unsafe { std::mem::zeroed() }
    }
}

/// SPDK environment options.
///
/// The real struct is version-dependent and large; we over-allocate a fixed
/// buffer and let `spdk_env_opts_init` populate the real fields.
#[repr(C)]
pub struct SpdkEnvOpts {
    _pad: [u8; 8192],
}

// ─── Dynamic loader (feature `spdk`) ───────────────────────────────────────

#[cfg(feature = "spdk")]
#[allow(non_snake_case, dead_code)]
struct SpdkApi {
    _lib: libloading::Library,
    spdk_env_opts_init: unsafe extern "C" fn(*mut SpdkEnvOpts),
    spdk_env_init: unsafe extern "C" fn(*const SpdkEnvOpts) -> c_int,
    spdk_env_fini: unsafe extern "C" fn(),
    spdk_nvme_transport_id_populate_trstring:
        unsafe extern "C" fn(*mut SpdkNvmeTransportId, *const c_char) -> c_int,
    spdk_nvme_probe_ctx_create: unsafe extern "C" fn() -> *mut c_void,
    spdk_nvme_probe_ctx_add_transport_id:
        unsafe extern "C" fn(*mut c_void, *const SpdkNvmeTransportId) -> c_int,
    spdk_nvme_probe: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        SpdkProbeCb,
        SpdkAttachCb,
        SpdkRemoveCb,
    ) -> c_int,
    spdk_nvme_ctrlr_get_num_ns: unsafe extern "C" fn(*mut c_void) -> u32,
    spdk_nvme_ctrlr_get_ns: unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void,
    spdk_nvme_ctrlr_alloc_io_qpair:
        unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> *mut c_void,
    spdk_nvme_ctrlr_free_io_qpair: unsafe extern "C" fn(*mut c_void) -> c_int,
    spdk_nvme_ns_get_size: unsafe extern "C" fn(*mut c_void) -> u64,
    spdk_nvme_ns_get_sector_size: unsafe extern "C" fn(*mut c_void) -> u32,
    spdk_nvme_ns_get_num_sectors: unsafe extern "C" fn(*mut c_void) -> u64,
    spdk_nvme_ns_cmd_read: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut u8,
        u32,
        SpdkCmdCb,
        *mut c_void,
        u64,
        u32,
        u32,
        *mut c_void,
    ) -> c_int,
    spdk_nvme_ns_cmd_write: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *const u8,
        u32,
        SpdkCmdCb,
        *mut c_void,
        u64,
        u32,
        u32,
        *mut c_void,
    ) -> c_int,
    spdk_nvme_ns_cmd_flush:
        unsafe extern "C" fn(*mut c_void, *mut c_void, SpdkCmdCb, *mut c_void) -> c_int,
    spdk_nvme_qpair_process_completions: unsafe extern "C" fn(*mut c_void, u32) -> u32,
    spdk_nvme_detach: unsafe extern "C" fn(*mut c_void) -> c_int,
    spdk_dma_malloc: unsafe extern "C" fn(usize, usize, *mut u64) -> *mut c_void,
    spdk_dma_free: unsafe extern "C" fn(*mut c_void),
}

#[cfg(feature = "spdk")]
type SpdkProbeCb =
    unsafe extern "C" fn(*mut c_void, *const SpdkNvmeTransportId, *mut c_void) -> c_int;
#[cfg(feature = "spdk")]
type SpdkAttachCb =
    unsafe extern "C" fn(*mut c_void, *const SpdkNvmeTransportId, *mut c_void, *const c_void);
#[cfg(feature = "spdk")]
type SpdkRemoveCb = unsafe extern "C" fn(*mut c_void, *mut c_void);
#[cfg(feature = "spdk")]
type SpdkCmdCb = unsafe extern "C" fn(*mut c_void, *const SpdkNvmeCpl);

#[cfg(feature = "spdk")]
static SPDK_API: std::sync::Mutex<Option<SpdkApi>> = std::sync::Mutex::new(None);

/// Load the SPDK shared library once and cache the resolved API.
#[cfg(feature = "spdk")]
fn api() -> HwResult<&'static SpdkApi> {
    {
        let guard = SPDK_API.lock().unwrap();
        if guard.is_some() {
            // SAFETY: just verified Some; it stays Some for the program lifetime.
            return Ok(unsafe { &*(&*guard as *const Option<SpdkApi> as *const SpdkApi) });
        }
    }

    let lib = unsafe {
        libloading::Library::new("libspdk.so")
            .or_else(|_| libloading::Library::new("libspdk_nvme.so"))
            .or_else(|_| libloading::Library::new("libspdk.dll"))
            .or_else(|_| libloading::Library::new("libspdk.dylib"))
            .map_err(|e| HwError::NotAvailable(format!("SPDK library not found: {}", e)))?
    };

    macro_rules! load_fn {
        ($lib:expr, $name:ident, $ty:ty) => {{
            let sym = unsafe { $lib.get::<$ty>(stringify!($name).as_bytes()) }.map_err(|e| {
                HwError::NotAvailable(format!(
                    "SPDK symbol {} not found: {}",
                    stringify!($name),
                    e
                ))
            })?;
            *sym
        }};
    }

    let api = SpdkApi {
        spdk_env_opts_init: load_fn!(
            lib,
            spdk_env_opts_init,
            unsafe extern "C" fn(*mut SpdkEnvOpts)
        ),
        spdk_env_init: load_fn!(
            lib,
            spdk_env_init,
            unsafe extern "C" fn(*const SpdkEnvOpts) -> c_int
        ),
        spdk_env_fini: load_fn!(lib, spdk_env_fini, unsafe extern "C" fn()),
        spdk_nvme_transport_id_populate_trstring: load_fn!(
            lib,
            spdk_nvme_transport_id_populate_trstring,
            unsafe extern "C" fn(*mut SpdkNvmeTransportId, *const c_char) -> c_int
        ),
        spdk_nvme_probe_ctx_create: load_fn!(
            lib,
            spdk_nvme_probe_ctx_create,
            unsafe extern "C" fn() -> *mut c_void
        ),
        spdk_nvme_probe_ctx_add_transport_id: load_fn!(
            lib,
            spdk_nvme_probe_ctx_add_transport_id,
            unsafe extern "C" fn(*mut c_void, *const SpdkNvmeTransportId) -> c_int
        ),
        spdk_nvme_probe: load_fn!(
            lib,
            spdk_nvme_probe,
            unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                SpdkProbeCb,
                SpdkAttachCb,
                SpdkRemoveCb,
            ) -> c_int
        ),
        spdk_nvme_ctrlr_get_num_ns: load_fn!(
            lib,
            spdk_nvme_ctrlr_get_num_ns,
            unsafe extern "C" fn(*mut c_void) -> u32
        ),
        spdk_nvme_ctrlr_get_ns: load_fn!(
            lib,
            spdk_nvme_ctrlr_get_ns,
            unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void
        ),
        spdk_nvme_ctrlr_alloc_io_qpair: load_fn!(
            lib,
            spdk_nvme_ctrlr_alloc_io_qpair,
            unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> *mut c_void
        ),
        spdk_nvme_ctrlr_free_io_qpair: load_fn!(
            lib,
            spdk_nvme_ctrlr_free_io_qpair,
            unsafe extern "C" fn(*mut c_void) -> c_int
        ),
        spdk_nvme_ns_get_size: load_fn!(
            lib,
            spdk_nvme_ns_get_size,
            unsafe extern "C" fn(*mut c_void) -> u64
        ),
        spdk_nvme_ns_get_sector_size: load_fn!(
            lib,
            spdk_nvme_ns_get_sector_size,
            unsafe extern "C" fn(*mut c_void) -> u32
        ),
        spdk_nvme_ns_get_num_sectors: load_fn!(
            lib,
            spdk_nvme_ns_get_num_sectors,
            unsafe extern "C" fn(*mut c_void) -> u64
        ),
        spdk_nvme_ns_cmd_read: load_fn!(
            lib,
            spdk_nvme_ns_cmd_read,
            unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut u8,
                u32,
                SpdkCmdCb,
                *mut c_void,
                u64,
                u32,
                u32,
                *mut c_void,
            ) -> c_int
        ),
        spdk_nvme_ns_cmd_write: load_fn!(
            lib,
            spdk_nvme_ns_cmd_write,
            unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *const u8,
                u32,
                SpdkCmdCb,
                *mut c_void,
                u64,
                u32,
                u32,
                *mut c_void,
            ) -> c_int
        ),
        spdk_nvme_ns_cmd_flush: load_fn!(
            lib,
            spdk_nvme_ns_cmd_flush,
            unsafe extern "C" fn(*mut c_void, *mut c_void, SpdkCmdCb, *mut c_void) -> c_int
        ),
        spdk_nvme_qpair_process_completions: load_fn!(
            lib,
            spdk_nvme_qpair_process_completions,
            unsafe extern "C" fn(*mut c_void, u32) -> u32
        ),
        spdk_nvme_detach: load_fn!(
            lib,
            spdk_nvme_detach,
            unsafe extern "C" fn(*mut c_void) -> c_int
        ),
        spdk_dma_malloc: load_fn!(
            lib,
            spdk_dma_malloc,
            unsafe extern "C" fn(usize, usize, *mut u64) -> *mut c_void
        ),
        spdk_dma_free: load_fn!(lib, spdk_dma_free, unsafe extern "C" fn(*mut c_void)),
        _lib: lib,
    };

    let mut guard = SPDK_API.lock().unwrap();
    *guard = Some(api);
    Ok(unsafe { &*(&*guard as *const Option<SpdkApi> as *const SpdkApi) })
}

/// State passed through SPDK's completion callback.
#[cfg(feature = "spdk")]
struct CompletionState {
    cpl: Option<SpdkNvmeCpl>,
    done: bool,
}

#[cfg(feature = "spdk")]
unsafe extern "C" fn on_complete(arg: *mut c_void, cpl: *const SpdkNvmeCpl) {
    let st = &mut *(arg as *mut CompletionState);
    if !cpl.is_null() {
        st.cpl = Some(*cpl);
    }
    st.done = true;
}

/// Parameters collected from a probe across the attach callback.
#[cfg(feature = "spdk")]
struct ProbeCtx {
    controllers: Vec<*mut c_void>,
}

/// Callback invoked during probe for each discovered controller.
#[cfg(feature = "spdk")]
unsafe extern "C" fn probe_cb(
    cb_ctx: *mut c_void,
    _trid: *const SpdkNvmeTransportId,
    _opts: *mut c_void,
) -> c_int {
    let _ = cb_ctx;
    1 // 1 = attach this controller
}

/// Callback invoked when a controller is attached during probe.
#[cfg(feature = "spdk")]
unsafe extern "C" fn attach_cb(
    cb_ctx: *mut c_void,
    _trid: *const SpdkNvmeTransportId,
    ctrlr: *mut c_void,
    _opts: *const c_void,
) {
    if !ctrlr.is_null() {
        let ctx = &mut *(cb_ctx as *mut ProbeCtx);
        ctx.controllers.push(ctrlr);
    }
}

/// Callback invoked when a controller is removed during probe.
#[cfg(feature = "spdk")]
unsafe extern "C" fn remove_cb(cb_ctx: *mut c_void, ctrlr: *mut c_void) {
    let ctx = &mut *(cb_ctx as *mut ProbeCtx);
    ctx.controllers.retain(|&c| c != ctrlr);
}

// ─── Public API ────────────────────────────────────────────────────────────

/// Top-level SPDK entry point.
///
/// Loading SPDK initializes its environment (hugepages, thread model, etc.).
/// Calling [`Spdk::init`] is required before [`Spdk::probe`].
pub struct Spdk;

impl Spdk {
    /// Initialize the SPDK environment.
    ///
    /// This calls `spdk_env_init` with default options. Safe to call once.
    pub fn init() -> HwResult<()> {
        #[cfg(feature = "spdk")]
        {
            let api = api()?;
            let mut opts = SpdkEnvOpts { _pad: [0u8; 8192] };
            unsafe { (api.spdk_env_opts_init)(&mut opts as *mut SpdkEnvOpts) };
            let rc = unsafe { (api.spdk_env_init)(&opts as *const SpdkEnvOpts) };
            if rc != 0 {
                return Err(HwError::InitFailed(format!(
                    "spdk_env_init returned {}",
                    rc
                )));
            }
            Ok(())
        }
        #[cfg(not(feature = "spdk"))]
        {
            Err(HwError::NotAvailable(
                "SPDK support requires the `spdk` feature and an installed SPDK library".into(),
            ))
        }
    }

    /// Finalize the SPDK environment.
    pub fn fini() {
        #[cfg(feature = "spdk")]
        {
            if let Ok(api) = api() {
                unsafe { (api.spdk_env_fini)() };
            }
        }
    }

    /// Probe for NVMe controllers reachable via the given transport string.
    ///
    /// The `trid` follows SPDK's transport-string syntax, e.g.
    /// `"trtype:PCIe traddr:0000:01:00.0"`. An empty string probes the
    /// default (PCIe) transport.
    pub fn probe(trid: &str) -> HwResult<Vec<Controller>> {
        #[cfg(feature = "spdk")]
        {
            let api = api()?;
            let ctx = unsafe { (api.spdk_nvme_probe_ctx_create)() };
            if ctx.is_null() {
                return Err(HwError::InitFailed(
                    "spdk_nvme_probe_ctx_create failed".into(),
                ));
            }

            let c_trid = std::ffi::CString::new(trid)
                .map_err(|e| HwError::InvalidParam(format!("invalid trid: {}", e)))?;
            let mut tid = SpdkNvmeTransportId::default();
            let rc = unsafe {
                (api.spdk_nvme_transport_id_populate_trstring)(
                    &mut tid as *mut SpdkNvmeTransportId,
                    c_trid.as_ptr(),
                )
            };
            if rc != 0 {
                return Err(HwError::InvalidParam(format!(
                    "spdk_nvme_transport_id_populate_trstring returned {}",
                    rc
                )));
            }
            let rc = unsafe {
                (api.spdk_nvme_probe_ctx_add_transport_id)(ctx, &tid as *const SpdkNvmeTransportId)
            };
            if rc != 0 {
                return Err(HwError::InvalidParam(format!(
                    "spdk_nvme_probe_ctx_add_transport_id returned {}",
                    rc
                )));
            }

            let mut probe_ctx = ProbeCtx {
                controllers: Vec::new(),
            };
            let rc = unsafe {
                (api.spdk_nvme_probe)(
                    ctx,
                    &mut probe_ctx as *mut ProbeCtx as *mut c_void,
                    probe_cb,
                    attach_cb,
                    remove_cb,
                )
            };
            if rc != 0 {
                return Err(HwError::InitFailed(format!(
                    "spdk_nvme_probe returned {}",
                    rc
                )));
            }

            Ok(probe_ctx
                .controllers
                .into_iter()
                .map(|ctrlr| Controller { ctrlr })
                .collect())
        }
        #[cfg(not(feature = "spdk"))]
        {
            let _ = trid;
            Err(HwError::NotAvailable(
                "SPDK support requires the `spdk` feature and an installed SPDK library".into(),
            ))
        }
    }
}

/// An attached SPDK NVMe controller.
pub struct Controller {
    ctrlr: *mut c_void,
}

impl Controller {
    /// Number of namespaces exposed by this controller.
    pub fn num_namespaces(&self) -> HwResult<u32> {
        #[cfg(feature = "spdk")]
        {
            let api = api()?;
            Ok(unsafe { (api.spdk_nvme_ctrlr_get_num_ns)(self.ctrlr) })
        }
        #[cfg(not(feature = "spdk"))]
        {
            Err(HwError::NotAvailable(
                "SPDK support requires the `spdk` feature and an installed SPDK library".into(),
            ))
        }
    }

    /// Open a namespace by ID (1-based; 0 returns the first namespace).
    pub fn namespace(&self, ns_id: u32) -> HwResult<NvmeNamespace> {
        #[cfg(feature = "spdk")]
        {
            let api = api()?;
            let id = if ns_id == 0 { 1 } else { ns_id };
            let ns = unsafe { (api.spdk_nvme_ctrlr_get_ns)(self.ctrlr, id) };
            if ns.is_null() {
                return Err(HwError::InvalidParam(format!(
                    "namespace {} not found on controller",
                    id
                )));
            }
            let block_size = unsafe { (api.spdk_nvme_ns_get_sector_size)(ns) };
            let total_blocks = unsafe { (api.spdk_nvme_ns_get_num_sectors)(ns) };
            Ok(NvmeNamespace {
                ctrlr: self.ctrlr,
                ns,
                ns_id: id,
                block_size,
                total_blocks,
            })
        }
        #[cfg(not(feature = "spdk"))]
        {
            let _ = ns_id;
            Err(HwError::NotAvailable(
                "SPDK support requires the `spdk` feature and an installed SPDK library".into(),
            ))
        }
    }

    /// Get the raw controller handle.
    pub fn as_ptr(&self) -> *mut c_void {
        self.ctrlr
    }

    /// Detach this controller, freeing SPDK resources.
    pub fn detach(self) -> HwResult<()> {
        #[cfg(feature = "spdk")]
        {
            let api = api()?;
            let rc = unsafe { (api.spdk_nvme_detach)(self.ctrlr) };
            if rc != 0 {
                return Err(HwError::InitFailed(format!(
                    "spdk_nvme_detach returned {}",
                    rc
                )));
            }
            Ok(())
        }
        #[cfg(not(feature = "spdk"))]
        {
            Err(HwError::NotAvailable(
                "SPDK support requires the `spdk` feature and an installed SPDK library".into(),
            ))
        }
    }
}

/// An NVMe namespace accessed via SPDK.
///
/// Provides user-space read/write/flush operations that bypass the kernel.
/// A namespace keeps a handle to its owning controller so it can allocate an
/// I/O qpair per operation.
#[allow(dead_code)]
pub struct NvmeNamespace {
    ctrlr: *mut c_void,
    ns: *mut c_void,
    ns_id: u32,
    block_size: u32,
    total_blocks: u64,
}

unsafe impl Send for NvmeNamespace {}
unsafe impl Sync for NvmeNamespace {}

impl NvmeNamespace {
    /// Get the namespace ID.
    pub fn ns_id(&self) -> u32 {
        self.ns_id
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

    /// Allocate a DMA-capable buffer via `spdk_dma_malloc`.
    ///
    /// # Safety
    /// The returned pointer must be freed with [`NvmeNamespace::dma_free`].
    pub unsafe fn dma_alloc(&self, size: usize, align: usize) -> HwResult<*mut u8> {
        #[cfg(feature = "spdk")]
        {
            let api = api()?;
            let ptr = unsafe { (api.spdk_dma_malloc)(size, align, std::ptr::null_mut()) };
            if ptr.is_null() {
                return Err(HwError::OutOfMemory);
            }
            Ok(ptr as *mut u8)
        }
        #[cfg(not(feature = "spdk"))]
        {
            let _ = (size, align);
            Err(HwError::NotAvailable(
                "SPDK support requires the `spdk` feature and an installed SPDK library".into(),
            ))
        }
    }

    /// Free a DMA buffer allocated by [`NvmeNamespace::dma_alloc`].
    ///
    /// # Safety
    /// `ptr` must have been returned by [`NvmeNamespace::dma_alloc`].
    pub unsafe fn dma_free(&self, ptr: *mut u8) {
        #[cfg(feature = "spdk")]
        {
            if let Ok(api) = api() {
                unsafe { (api.spdk_dma_free)(ptr as *mut c_void) };
            }
        }
        #[cfg(not(feature = "spdk"))]
        {
            let _ = ptr;
        }
    }

    /// Submit a read command and wait for completion.
    ///
    /// # Arguments
    /// - `lba`: Starting logical block address.
    /// - `num_blocks`: Number of blocks to read.
    /// - `buf`: DMA-capable buffer to read into.
    /// - `cb`: Optional completion callback.
    /// - `cb_arg`: Callback argument.
    ///
    /// # Safety
    /// - `buf` must be a valid DMA-capable buffer of at least
    ///   `num_blocks * block_size` bytes.
    /// - The buffer must remain valid until the operation completes.
    pub unsafe fn read(
        &self,
        lba: u64,
        num_blocks: u32,
        buf: *mut u8,
        cb: Option<unsafe extern "C" fn(*mut c_void, *const SpdkNvmeCpl)>,
        cb_arg: *mut c_void,
    ) -> HwResult<()> {
        #[cfg(feature = "spdk")]
        {
            let qpair = alloc_qpair(self.ctrlr)?;
            let api = api()?;
            let mut state = CompletionState {
                cpl: None,
                done: false,
            };
            let rc = unsafe {
                (api.spdk_nvme_ns_cmd_read)(
                    self.ns,
                    qpair,
                    buf,
                    num_blocks * self.block_size,
                    on_complete,
                    &mut state as *mut CompletionState as *mut c_void,
                    lba,
                    num_blocks,
                    0,
                    std::ptr::null_mut(),
                )
            };
            if rc != 0 {
                unsafe { (api.spdk_nvme_ctrlr_free_io_qpair)(qpair) };
                return Err(HwError::InitFailed(format!(
                    "spdk_nvme_ns_cmd_read returned {}",
                    rc
                )));
            }
            let res = poll(qpair, &mut state);
            unsafe { (api.spdk_nvme_ctrlr_free_io_qpair)(qpair) };
            res?;
            finish(state, cb, cb_arg)
        }
        #[cfg(not(feature = "spdk"))]
        {
            let _ = (lba, num_blocks, buf, cb, cb_arg);
            Err(HwError::NotAvailable(
                "SPDK support requires the `spdk` feature and an installed SPDK library".into(),
            ))
        }
    }

    /// Submit a write command and wait for completion.
    ///
    /// # Safety
    /// - `buf` must be a valid DMA-capable buffer of at least
    ///   `num_blocks * block_size` bytes.
    /// - The buffer must remain valid until the operation completes.
    pub unsafe fn write(
        &self,
        lba: u64,
        num_blocks: u32,
        buf: *const u8,
        cb: Option<unsafe extern "C" fn(*mut c_void, *const SpdkNvmeCpl)>,
        cb_arg: *mut c_void,
    ) -> HwResult<()> {
        #[cfg(feature = "spdk")]
        {
            let qpair = alloc_qpair(self.ctrlr)?;
            let api = api()?;
            let mut state = CompletionState {
                cpl: None,
                done: false,
            };
            let rc = unsafe {
                (api.spdk_nvme_ns_cmd_write)(
                    self.ns,
                    qpair,
                    buf,
                    num_blocks * self.block_size,
                    on_complete,
                    &mut state as *mut CompletionState as *mut c_void,
                    lba,
                    num_blocks,
                    0,
                    std::ptr::null_mut(),
                )
            };
            if rc != 0 {
                unsafe { (api.spdk_nvme_ctrlr_free_io_qpair)(qpair) };
                return Err(HwError::InitFailed(format!(
                    "spdk_nvme_ns_cmd_write returned {}",
                    rc
                )));
            }
            let res = poll(qpair, &mut state);
            unsafe { (api.spdk_nvme_ctrlr_free_io_qpair)(qpair) };
            res?;
            finish(state, cb, cb_arg)
        }
        #[cfg(not(feature = "spdk"))]
        {
            let _ = (lba, num_blocks, buf, cb, cb_arg);
            Err(HwError::NotAvailable(
                "SPDK support requires the `spdk` feature and an installed SPDK library".into(),
            ))
        }
    }

    /// Submit a flush command and wait for completion.
    ///
    /// # Safety
    /// The callback must be a valid function pointer if provided.
    pub unsafe fn flush(
        &self,
        cb: Option<unsafe extern "C" fn(*mut c_void, *const SpdkNvmeCpl)>,
        cb_arg: *mut c_void,
    ) -> HwResult<()> {
        #[cfg(feature = "spdk")]
        {
            let qpair = alloc_qpair(self.ctrlr)?;
            let api = api()?;
            let mut state = CompletionState {
                cpl: None,
                done: false,
            };
            let rc = unsafe {
                (api.spdk_nvme_ns_cmd_flush)(
                    qpair,
                    self.ns,
                    on_complete,
                    &mut state as *mut CompletionState as *mut c_void,
                )
            };
            if rc != 0 {
                unsafe { (api.spdk_nvme_ctrlr_free_io_qpair)(qpair) };
                return Err(HwError::InitFailed(format!(
                    "spdk_nvme_ns_cmd_flush returned {}",
                    rc
                )));
            }
            let res = poll(qpair, &mut state);
            unsafe { (api.spdk_nvme_ctrlr_free_io_qpair)(qpair) };
            res?;
            finish(state, cb, cb_arg)
        }
        #[cfg(not(feature = "spdk"))]
        {
            let _ = (cb, cb_arg);
            Err(HwError::NotAvailable(
                "SPDK support requires the `spdk` feature and an installed SPDK library".into(),
            ))
        }
    }
}

#[cfg(feature = "spdk")]
unsafe fn alloc_qpair(ctrlr: *mut c_void) -> HwResult<*mut c_void> {
    let api = api()?;
    let qpair = unsafe { (api.spdk_nvme_ctrlr_alloc_io_qpair)(ctrlr, std::ptr::null(), 0) };
    if qpair.is_null() {
        return Err(HwError::OutOfMemory);
    }
    Ok(qpair)
}

#[cfg(feature = "spdk")]
unsafe fn poll(qpair: *mut c_void, state: &mut CompletionState) -> HwResult<()> {
    let api = api()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        unsafe { (api.spdk_nvme_qpair_process_completions)(qpair, 0) };
        if state.done {
            break;
        }
        if std::time::Instant::now() > deadline {
            return Err(HwError::Timeout);
        }
        std::thread::sleep(std::time::Duration::from_micros(10));
    }
    Ok(())
}

#[cfg(feature = "spdk")]
fn finish(
    state: CompletionState,
    cb: Option<unsafe extern "C" fn(*mut c_void, *const SpdkNvmeCpl)>,
    cb_arg: *mut c_void,
) -> HwResult<()> {
    match state.cpl {
        Some(cpl) if cpl.is_success() => {
            if let Some(cb) = cb {
                unsafe { cb(cb_arg, &cpl as *const SpdkNvmeCpl) };
            }
            Ok(())
        }
        Some(cpl) => Err(HwError::InvalidParam(format!(
            "NVMe command failed: status {:#x}",
            cpl.status_code()
        ))),
        None => Err(HwError::InitFailed(
            "NVMe command completed without status".into(),
        )),
    }
}

// ─── SPDK NVMe Command Builder ─────────────────────────────────────────────

/// Builder for SPDK NVMe commands.
#[derive(Debug, Clone, Copy)]
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
        // DMA memory is allocated externally (hugepages / spdk_dma_malloc);
        // nothing to free here.
    }
}
