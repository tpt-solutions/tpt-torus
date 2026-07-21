//! CUDA Driver API FFI bindings for GPU-Direct integration.
//!
//! These bindings use dynamic loading (`libloading`) to load the CUDA Driver
//! API at runtime. This means CUDA doesn't need to be present at build time,
//! only at runtime when the `gpu_direct` feature is used.

use std::os::raw::{c_int, c_uint, c_void};
use std::sync::Mutex;

// ─── CUDA Types ────────────────────────────────────────────────────────────

pub type CUdevice = c_int;
pub type CUcontext = *mut c_void;
pub type CUdeviceptr = u64;
pub type CUstream = *mut c_void;
pub type CUevent = *mut c_void;

/// CUDA result code.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CUresult {
    Success = 0,
    ErrorInvalidValue = 1,
    ErrorOutOfMemory = 2,
    ErrorNotInitialized = 3,
    ErrorDeinitialized = 4,
    ErrorNoDevice = 100,
    ErrorInvalidDevice = 101,
    ErrorInvalidContext = 201,
    ErrorNotReady = 600,
    ErrorLaunchFailed = 719,
    Unknown = 999,
}

impl CUresult {
    pub fn is_ok(self) -> bool {
        self == CUresult::Success
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            CUresult::Success => "success",
            CUresult::ErrorInvalidValue => "invalid value",
            CUresult::ErrorOutOfMemory => "out of memory",
            CUresult::ErrorNotInitialized => "not initialized",
            CUresult::ErrorDeinitialized => "deinitialized",
            CUresult::ErrorNoDevice => "no CUDA device",
            CUresult::ErrorInvalidDevice => "invalid device",
            CUresult::ErrorInvalidContext => "invalid context",
            CUresult::ErrorNotReady => "not ready",
            CUresult::ErrorLaunchFailed => "launch failed",
            CUresult::Unknown => "unknown error",
        }
    }
}

impl std::fmt::Display for CUresult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::error::Error for CUresult {}

// ─── Dynamic CUDA Library ──────────────────────────────────────────────────

#[allow(non_snake_case)]
struct CudaApi {
    _lib: libloading::Library,
    cuInit: unsafe extern "C" fn(c_uint) -> CUresult,
    cuDeviceGetCount: unsafe extern "C" fn(*mut c_int) -> CUresult,
    cuDeviceGet: unsafe extern "C" fn(*mut CUdevice, c_int) -> CUresult,
    cuDeviceGetName: unsafe extern "C" fn(*mut i8, c_int, CUdevice) -> CUresult,
    cuDeviceTotalMem: unsafe extern "C" fn(*mut usize, CUdevice) -> CUresult,
    cuCtxCreate: unsafe extern "C" fn(*mut CUcontext, c_uint, CUdevice) -> CUresult,
    cuCtxDestroy: unsafe extern "C" fn(CUcontext) -> CUresult,
    cuCtxSetCurrent: unsafe extern "C" fn(CUcontext) -> CUresult,
    cuCtxSynchronize: unsafe extern "C" fn() -> CUresult,
    cuMemAlloc: unsafe extern "C" fn(*mut CUdeviceptr, usize) -> CUresult,
    cuMemFree: unsafe extern "C" fn(CUdeviceptr) -> CUresult,
    cuMemHostAlloc: unsafe extern "C" fn(*mut *mut c_void, usize, c_uint) -> CUresult,
    cuMemFreeHost: unsafe extern "C" fn(*mut c_void) -> CUresult,
    cuMemcpyHtoD: unsafe extern "C" fn(CUdeviceptr, *const c_void, usize) -> CUresult,
    cuMemcpyDtoH: unsafe extern "C" fn(*mut c_void, CUdeviceptr, usize) -> CUresult,
    cuMemcpyDtoD: unsafe extern "C" fn(CUdeviceptr, CUdeviceptr, usize) -> CUresult,
    cuMemcpyHtoDAsync: unsafe extern "C" fn(CUdeviceptr, *const c_void, usize, CUstream) -> CUresult,
    cuMemcpyDtoHAsync: unsafe extern "C" fn(*mut c_void, CUdeviceptr, usize, CUstream) -> CUresult,
    cuMemcpyDtoDAsync: unsafe extern "C" fn(CUdeviceptr, CUdeviceptr, usize, CUstream) -> CUresult,
    cuStreamCreate: unsafe extern "C" fn(*mut CUstream, c_uint) -> CUresult,
    cuStreamDestroy: unsafe extern "C" fn(CUstream) -> CUresult,
    cuStreamSynchronize: unsafe extern "C" fn(CUstream) -> CUresult,
    cuEventCreate: unsafe extern "C" fn(*mut CUevent, c_uint) -> CUresult,
    cuEventDestroy: unsafe extern "C" fn(CUevent) -> CUresult,
    cuEventRecord: unsafe extern "C" fn(CUevent, CUstream) -> CUresult,
    cuEventSynchronize: unsafe extern "C" fn(CUevent) -> CUresult,
    cuEventElapsedTime: unsafe extern "C" fn(*mut f32, CUevent, CUevent) -> CUresult,
}

static CUDA_API: Mutex<Option<CudaApi>> = Mutex::new(None);

fn load_cuda() -> Result<&'static CudaApi, crate::HwError> {
    {
        let guard = CUDA_API.lock().unwrap();
        if guard.is_some() {
            // Safety: We just checked it's Some, and it stays Some forever
            return Ok(unsafe { &*(&*guard as *const Option<CudaApi> as *const CudaApi) });
        }
    }

    let lib = unsafe {
        libloading::Library::new("libcuda.so.1")
            .or_else(|_| libloading::Library::new("nvcuda.dll"))
            .or_else(|_| libloading::Library::new("libcuda.dylib"))
            .map_err(|e| crate::HwError::NotAvailable(format!("CUDA library not found: {}", e)))?
    };

    macro_rules! load_fn {
        ($lib:expr, $name:ident, $ty:ty) => {{
            let sym = unsafe {
                $lib.get::<$ty>(stringify!($name).as_bytes())
            }
            .map_err(|e| {
                crate::HwError::NotAvailable(format!(
                    "CUDA symbol {} not found: {}",
                    stringify!($name),
                    e
                ))
            })?;
            *sym
        }};
    }

    let api = CudaApi {
        cuInit: load_fn!(lib, cuInit, unsafe extern "C" fn(c_uint) -> CUresult),
        cuDeviceGetCount: load_fn!(lib, cuDeviceGetCount, unsafe extern "C" fn(*mut c_int) -> CUresult),
        cuDeviceGet: load_fn!(lib, cuDeviceGet, unsafe extern "C" fn(*mut CUdevice, c_int) -> CUresult),
        cuDeviceGetName: load_fn!(lib, cuDeviceGetName, unsafe extern "C" fn(*mut i8, c_int, CUdevice) -> CUresult),
        cuDeviceTotalMem: load_fn!(lib, cuDeviceTotalMem, unsafe extern "C" fn(*mut usize, CUdevice) -> CUresult),
        cuCtxCreate: load_fn!(lib, cuCtxCreate, unsafe extern "C" fn(*mut CUcontext, c_uint, CUdevice) -> CUresult),
        cuCtxDestroy: load_fn!(lib, cuCtxDestroy, unsafe extern "C" fn(CUcontext) -> CUresult),
        cuCtxSetCurrent: load_fn!(lib, cuCtxSetCurrent, unsafe extern "C" fn(CUcontext) -> CUresult),
        cuCtxSynchronize: load_fn!(lib, cuCtxSynchronize, unsafe extern "C" fn() -> CUresult),
        cuMemAlloc: load_fn!(lib, cuMemAlloc, unsafe extern "C" fn(*mut CUdeviceptr, usize) -> CUresult),
        cuMemFree: load_fn!(lib, cuMemFree, unsafe extern "C" fn(CUdeviceptr) -> CUresult),
        cuMemHostAlloc: load_fn!(lib, cuMemHostAlloc, unsafe extern "C" fn(*mut *mut c_void, usize, c_uint) -> CUresult),
        cuMemFreeHost: load_fn!(lib, cuMemFreeHost, unsafe extern "C" fn(*mut c_void) -> CUresult),
        cuMemcpyHtoD: load_fn!(lib, cuMemcpyHtoD, unsafe extern "C" fn(CUdeviceptr, *const c_void, usize) -> CUresult),
        cuMemcpyDtoH: load_fn!(lib, cuMemcpyDtoH, unsafe extern "C" fn(*mut c_void, CUdeviceptr, usize) -> CUresult),
        cuMemcpyDtoD: load_fn!(lib, cuMemcpyDtoD, unsafe extern "C" fn(CUdeviceptr, CUdeviceptr, usize) -> CUresult),
        cuMemcpyHtoDAsync: load_fn!(lib, cuMemcpyHtoDAsync, unsafe extern "C" fn(CUdeviceptr, *const c_void, usize, CUstream) -> CUresult),
        cuMemcpyDtoHAsync: load_fn!(lib, cuMemcpyDtoHAsync, unsafe extern "C" fn(*mut c_void, CUdeviceptr, usize, CUstream) -> CUresult),
        cuMemcpyDtoDAsync: load_fn!(lib, cuMemcpyDtoDAsync, unsafe extern "C" fn(CUdeviceptr, CUdeviceptr, usize, CUstream) -> CUresult),
        cuStreamCreate: load_fn!(lib, cuStreamCreate, unsafe extern "C" fn(*mut CUstream, c_uint) -> CUresult),
        cuStreamDestroy: load_fn!(lib, cuStreamDestroy, unsafe extern "C" fn(CUstream) -> CUresult),
        cuStreamSynchronize: load_fn!(lib, cuStreamSynchronize, unsafe extern "C" fn(CUstream) -> CUresult),
        cuEventCreate: load_fn!(lib, cuEventCreate, unsafe extern "C" fn(*mut CUevent, c_uint) -> CUresult),
        cuEventDestroy: load_fn!(lib, cuEventDestroy, unsafe extern "C" fn(CUevent) -> CUresult),
        cuEventRecord: load_fn!(lib, cuEventRecord, unsafe extern "C" fn(CUevent, CUstream) -> CUresult),
        cuEventSynchronize: load_fn!(lib, cuEventSynchronize, unsafe extern "C" fn(CUevent) -> CUresult),
        cuEventElapsedTime: load_fn!(lib, cuEventElapsedTime, unsafe extern "C" fn(*mut f32, CUevent, CUevent) -> CUresult),
        _lib: lib,
    };

    let mut guard = CUDA_API.lock().unwrap();
    *guard = Some(api);

    // Safety: We just inserted Some, and it stays Some forever
    Ok(unsafe { &*(&*guard as *const Option<CudaApi> as *const CudaApi) })
}

// ─── Public API ────────────────────────────────────────────────────────────

/// Initialize the CUDA driver.
pub fn init() -> Result<(), crate::HwError> {
    let api = load_cuda()?;
    let result = unsafe { (api.cuInit)(0) };
    if result.is_ok() {
        Ok(())
    } else {
        Err(crate::HwError::InitFailed(format!("cuInit: {}", result)))
    }
}

/// Get the number of CUDA devices.
pub fn device_count() -> Result<i32, crate::HwError> {
    let api = load_cuda()?;
    let mut count = 0;
    let result = unsafe { (api.cuDeviceGetCount)(&mut count) };
    if result.is_ok() {
        Ok(count)
    } else {
        Err(crate::HwError::InitFailed(format!("cuDeviceGetCount: {}", result)))
    }
}

/// Get a CUDA device handle.
pub fn device_get(ordinal: i32) -> Result<CUdevice, crate::HwError> {
    let api = load_cuda()?;
    let mut device = 0;
    let result = unsafe { (api.cuDeviceGet)(&mut device, ordinal) };
    if result.is_ok() {
        Ok(device)
    } else {
        Err(crate::HwError::InitFailed(format!("cuDeviceGet: {}", result)))
    }
}

/// Get the name of a CUDA device.
pub fn device_name(device: CUdevice) -> Result<String, crate::HwError> {
    let api = load_cuda()?;
    let mut name = [0i8; 256];
    let result = unsafe { (api.cuDeviceGetName)(name.as_mut_ptr(), 256, device) };
    if result.is_ok() {
        let name = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) };
        Ok(name.to_string_lossy().into_owned())
    } else {
        Err(crate::HwError::InitFailed(format!("cuDeviceGetName: {}", result)))
    }
}

/// Get the total memory of a CUDA device.
pub fn device_total_mem(device: CUdevice) -> Result<usize, crate::HwError> {
    let api = load_cuda()?;
    let mut bytes = 0;
    let result = unsafe { (api.cuDeviceTotalMem)(&mut bytes, device) };
    if result.is_ok() {
        Ok(bytes)
    } else {
        Err(crate::HwError::InitFailed(format!("cuDeviceTotalMem: {}", result)))
    }
}

/// Create a CUDA context.
pub fn ctx_create(device: CUdevice) -> Result<CUcontext, crate::HwError> {
    let api = load_cuda()?;
    let mut ctx = std::ptr::null_mut();
    let result = unsafe { (api.cuCtxCreate)(&mut ctx, 0, device) };
    if result.is_ok() {
        Ok(ctx)
    } else {
        Err(crate::HwError::InitFailed(format!("cuCtxCreate: {}", result)))
    }
}

/// Allocate device memory.
pub fn mem_alloc(size: usize) -> Result<CUdeviceptr, crate::HwError> {
    let api = load_cuda()?;
    let mut dptr = 0;
    let result = unsafe { (api.cuMemAlloc)(&mut dptr, size) };
    if result.is_ok() {
        Ok(dptr)
    } else {
        Err(crate::HwError::InitFailed(format!("cuMemAlloc: {}", result)))
    }
}

/// Free device memory.
pub fn mem_free(dptr: CUdeviceptr) -> Result<(), crate::HwError> {
    let api = load_cuda()?;
    let result = unsafe { (api.cuMemFree)(dptr) };
    if result.is_ok() {
        Ok(())
    } else {
        Err(crate::HwError::InitFailed(format!("cuMemFree: {}", result)))
    }
}

/// Copy data from host to device (synchronous).
pub fn memcpy_h2d(dst: CUdeviceptr, src: *const u8, size: usize) -> Result<(), crate::HwError> {
    let api = load_cuda()?;
    let result = unsafe { (api.cuMemcpyHtoD)(dst, src as *const c_void, size) };
    if result.is_ok() {
        Ok(())
    } else {
        Err(crate::HwError::InitFailed(format!("cuMemcpyHtoD: {}", result)))
    }
}

/// Copy data from device to host (synchronous).
pub fn memcpy_d2h(dst: *mut u8, src: CUdeviceptr, size: usize) -> Result<(), crate::HwError> {
    let api = load_cuda()?;
    let result = unsafe { (api.cuMemcpyDtoH)(dst as *mut c_void, src, size) };
    if result.is_ok() {
        Ok(())
    } else {
        Err(crate::HwError::InitFailed(format!("cuMemcpyDtoH: {}", result)))
    }
}

/// Copy data from device to device (synchronous).
pub fn memcpy_d2d(dst: CUdeviceptr, src: CUdeviceptr, size: usize) -> Result<(), crate::HwError> {
    let api = load_cuda()?;
    let result = unsafe { (api.cuMemcpyDtoD)(dst, src, size) };
    if result.is_ok() {
        Ok(())
    } else {
        Err(crate::HwError::InitFailed(format!("cuMemcpyDtoD: {}", result)))
    }
}

/// Allocate pinned (page-locked) host memory.
pub fn mem_host_alloc(size: usize) -> Result<*mut u8, crate::HwError> {
    let api = load_cuda()?;
    let mut ptr: *mut u8 = std::ptr::null_mut();
    let result = unsafe {
        (api.cuMemHostAlloc)(&mut ptr as *mut *mut u8 as *mut *mut c_void, size, 0)
    };
    if result.is_ok() {
        Ok(ptr)
    } else {
        Err(crate::HwError::InitFailed(format!("cuMemHostAlloc: {}", result)))
    }
}

/// Free pinned (page-locked) host memory.
pub fn mem_free_host(ptr: *mut u8, _size: usize) -> Result<(), crate::HwError> {
    let api = load_cuda()?;
    let result = unsafe { (api.cuMemFreeHost)(ptr as *mut c_void) };
    if result.is_ok() {
        Ok(())
    } else {
        Err(crate::HwError::InitFailed(format!("cuMemFreeHost: {}", result)))
    }
}

/// Copy data from host to device (async).
pub fn memcpy_h2d_async(
    dst: CUdeviceptr,
    src: *const u8,
    size: usize,
    stream: CUstream,
) -> Result<(), crate::HwError> {
    let api = load_cuda()?;
    let result = unsafe { (api.cuMemcpyHtoDAsync)(dst, src as *const c_void, size, stream) };
    if result.is_ok() {
        Ok(())
    } else {
        Err(crate::HwError::InitFailed(format!("cuMemcpyHtoDAsync: {}", result)))
    }
}

/// Copy data from device to host (async).
pub fn memcpy_d2h_async(
    dst: *mut u8,
    src: CUdeviceptr,
    size: usize,
    stream: CUstream,
) -> Result<(), crate::HwError> {
    let api = load_cuda()?;
    let result = unsafe { (api.cuMemcpyDtoHAsync)(dst as *mut c_void, src, size, stream) };
    if result.is_ok() {
        Ok(())
    } else {
        Err(crate::HwError::InitFailed(format!("cuMemcpyDtoHAsync: {}", result)))
    }
}

/// Copy data from device to device (async).
pub fn memcpy_d2d_async(
    dst: CUdeviceptr,
    src: CUdeviceptr,
    size: usize,
    stream: CUstream,
) -> Result<(), crate::HwError> {
    let api = load_cuda()?;
    let result = unsafe { (api.cuMemcpyDtoDAsync)(dst, src, size, stream) };
    if result.is_ok() {
        Ok(())
    } else {
        Err(crate::HwError::InitFailed(format!("cuMemcpyDtoDAsync: {}", result)))
    }
}

/// Create a CUDA stream.
pub fn stream_create() -> Result<CUstream, crate::HwError> {
    let api = load_cuda()?;
    let mut stream = std::ptr::null_mut();
    let result = unsafe { (api.cuStreamCreate)(&mut stream, 0) };
    if result.is_ok() {
        Ok(stream)
    } else {
        Err(crate::HwError::InitFailed(format!("cuStreamCreate: {}", result)))
    }
}

/// Destroy a CUDA stream.
pub fn stream_destroy(stream: CUstream) -> Result<(), crate::HwError> {
    let api = load_cuda()?;
    let result = unsafe { (api.cuStreamDestroy)(stream) };
    if result.is_ok() {
        Ok(())
    } else {
        Err(crate::HwError::InitFailed(format!("cuStreamDestroy: {}", result)))
    }
}

/// Synchronize a CUDA stream.
pub fn stream_synchronize(stream: CUstream) -> Result<(), crate::HwError> {
    let api = load_cuda()?;
    let result = unsafe { (api.cuStreamSynchronize)(stream) };
    if result.is_ok() {
        Ok(())
    } else {
        Err(crate::HwError::InitFailed(format!("cuStreamSynchronize: {}", result)))
    }
}

/// Create a CUDA event.
pub fn event_create() -> Result<CUevent, crate::HwError> {
    let api = load_cuda()?;
    let mut event = std::ptr::null_mut();
    let result = unsafe { (api.cuEventCreate)(&mut event, 0) };
    if result.is_ok() {
        Ok(event)
    } else {
        Err(crate::HwError::InitFailed(format!("cuEventCreate: {}", result)))
    }
}

/// Destroy a CUDA event.
pub fn event_destroy(event: CUevent) -> Result<(), crate::HwError> {
    let api = load_cuda()?;
    let result = unsafe { (api.cuEventDestroy)(event) };
    if result.is_ok() {
        Ok(())
    } else {
        Err(crate::HwError::InitFailed(format!("cuEventDestroy: {}", result)))
    }
}

/// Record an event on a stream.
pub fn event_record(event: CUevent, stream: CUstream) -> Result<(), crate::HwError> {
    let api = load_cuda()?;
    let result = unsafe { (api.cuEventRecord)(event, stream) };
    if result.is_ok() {
        Ok(())
    } else {
        Err(crate::HwError::InitFailed(format!("cuEventRecord: {}", result)))
    }
}

/// Synchronize on an event.
pub fn event_synchronize(event: CUevent) -> Result<(), crate::HwError> {
    let api = load_cuda()?;
    let result = unsafe { (api.cuEventSynchronize)(event) };
    if result.is_ok() {
        Ok(())
    } else {
        Err(crate::HwError::InitFailed(format!("cuEventSynchronize: {}", result)))
    }
}

/// Get elapsed time between two events (in milliseconds).
pub fn event_elapsed(start: CUevent, end: CUevent) -> Result<f32, crate::HwError> {
    let api = load_cuda()?;
    let mut ms = 0.0;
    let result = unsafe { (api.cuEventElapsedTime)(&mut ms, start, end) };
    if result.is_ok() {
        Ok(ms)
    } else {
        Err(crate::HwError::InitFailed(format!("cuEventElapsedTime: {}", result)))
    }
}
