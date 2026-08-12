//! C-compatible FFI layer for TPT Torus.
//!
//! This crate exposes a C API that can be called from C, C++, or any FFI-compatible language.
//! It wraps the Rust `Torus` handle and provides callback-based async operations.

use std::sync::Arc;
use tpt_torus_core::backend::Backend;
use tpt_torus_core::flow::Flow;
use tpt_torus_core::operation::Operation;
use tpt_torus_core::Torus;

/// Opaque handle to a Torus instance.
pub struct TorusHandle {
    torus: Arc<Torus>,
}

/// Callback type for async operation completion.
///
/// # Arguments
/// - `result`: Number of bytes transferred, or negative errno on failure.
/// - `user_data`: User-provided data passed through from the submission.
/// - `context`: User-provided context pointer.
pub type CompletionCallback =
    extern "C" fn(result: i64, user_data: u64, context: *mut std::ffi::c_void);

/// Construct the platform-default backend for `torus_create`.
fn make_backend(ring_entries: u32) -> Option<Box<dyn Backend>> {
    #[cfg(all(
        unix,
        not(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "openbsd",
            target_os = "netbsd"
        ))
    ))]
    {
        Some(Box::new(
            tpt_torus_backend_uring::UringBackend::new(ring_entries).ok()?,
        ))
    }

    #[cfg(target_os = "windows")]
    {
        let _ = ring_entries;
        Some(Box::new(tpt_torus_backend_iocp::IocpBackend::new().ok()?))
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    {
        let _ = ring_entries;
        Some(Box::new(
            tpt_torus_backend_kqueue::KqueueBackend::new().ok()?,
        ))
    }

    #[cfg(not(any(
        unix,
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    {
        let _ = ring_entries;
        None
    }
}

/// Create a new Torus instance.
///
/// # Arguments
/// - `ring_entries`: Number of SQ/CQ entries (must be power of 2).
/// - `backend`: Platform-specific backend. Pass 0 to use the default for the current OS.
///
/// # Returns
/// Pointer to the Torus handle, or NULL on failure.
///
/// # Safety
/// The returned pointer must be freed with [`torus_destroy`].
#[no_mangle]
pub extern "C" fn torus_create(ring_entries: u32, _backend: i32) -> *mut TorusHandle {
    let backend = match make_backend(ring_entries) {
        Some(b) => b,
        None => return std::ptr::null_mut(),
    };

    match Torus::new(ring_entries, backend) {
        Ok(torus) => Box::into_raw(Box::new(TorusHandle {
            torus: Arc::new(torus),
        })),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Create a new Torus instance with a pre-configured backend.
///
/// This is the primary creation function for FFI use.
///
/// # Arguments
/// - `handle`: Pointer to receive the new Torus handle.
/// - `ring_entries`: Number of SQ/CQ entries (must be power of 2).
///
/// # Returns
/// 0 on success, negative errno on failure.
///
/// # Safety
/// The caller must ensure `handle` is a valid pointer to a `*mut TorusHandle`.
#[no_mangle]
pub unsafe extern "C" fn torus_create_with_backend(
    handle: *mut *mut TorusHandle,
    ring_entries: u32,
) -> i32 {
    if handle.is_null() {
        return -22; // EINVAL
    }

    // NOTE: This function requires a backend to be created externally.
    // In practice, the C++ wrapper would create the backend and pass it.
    // For now, return an error indicating this function needs a backend.
    let _ = ring_entries;
    *handle = std::ptr::null_mut();
    -38 // ENOSYS - function not implemented (needs backend)
}

/// Destroy a Torus instance.
///
/// # Safety
/// `handle` must be a valid pointer returned by `torus_create` or `torus_create_with_backend`.
#[no_mangle]
pub unsafe extern "C" fn torus_destroy(handle: *mut TorusHandle) {
    if !handle.is_null() {
        drop(Box::from_raw(handle));
    }
}

/// Submit a read operation.
///
/// # Arguments
/// - `handle`: Torus handle.
/// - `fd`: File descriptor to read from.
/// - `buf`: Buffer to read into.
/// - `len`: Number of bytes to read.
/// - `offset`: File offset.
/// - `user_data`: User data passed to the callback.
/// - `callback`: Completion callback.
/// - `context`: Context pointer passed to the callback.
///
/// # Returns
/// 0 on success, negative errno on failure.
///
/// # Safety
/// All pointer arguments must be valid. The buffer must remain valid until
/// the callback is invoked.
#[no_mangle]
pub unsafe extern "C" fn torus_read(
    handle: *const TorusHandle,
    fd: i32,
    buf: *mut u8,
    len: usize,
    offset: u64,
    user_data: u64,
    callback: CompletionCallback,
    context: *mut std::ffi::c_void,
) -> i32 {
    if handle.is_null() || buf.is_null() || callback as usize == 0 {
        return -22; // EINVAL
    }

    let torus = &(*handle).torus;
    let flow = Flow::with_user_data(
        Operation::Read {
            fd,
            buf,
            len,
            offset,
        },
        user_data,
    );

    match torus.submit(&flow) {
        Ok(()) => {
            // In a real implementation, we'd register the callback and context
            // to be invoked when the operation completes. For now, we use a
            // synchronous completion path.
            //
            // A proper implementation would:
            // 1. Store the callback/context in a pending operations table
            // 2. Spawn a reactor thread that calls the callbacks on completion
            // 3. The reactor would reap completions and invoke callbacks

            // For the alpha, we do a synchronous wait and callback
            let _ = torus.wait(1_000_000);
            let mut results = Vec::new();
            let _ = torus.reap(&mut results);

            if let Some(result) = results.first() {
                callback(result.result, result.user_data, context);
            } else {
                callback(-110, user_data, context); // ETIMEDOUT
            }
            0
        }
        Err(e) => {
            let errno = match e {
                tpt_torus_core::Error::SubmissionFull => -11, // EAGAIN
                tpt_torus_core::Error::Os(code) => code,
                _ => -22, // EINVAL
            };
            callback(errno as i64, user_data, context);
            errno
        }
    }
}

/// Submit a write operation.
///
/// # Safety
/// All pointer arguments must be valid. The buffer must remain valid until
/// the callback is invoked.
#[no_mangle]
pub unsafe extern "C" fn torus_write(
    handle: *const TorusHandle,
    fd: i32,
    buf: *const u8,
    len: usize,
    offset: u64,
    user_data: u64,
    callback: CompletionCallback,
    context: *mut std::ffi::c_void,
) -> i32 {
    if handle.is_null() || buf.is_null() || callback as usize == 0 {
        return -22;
    }

    let torus = &(*handle).torus;
    let flow = Flow::with_user_data(
        Operation::Write {
            fd,
            buf,
            len,
            offset,
        },
        user_data,
    );

    match torus.submit(&flow) {
        Ok(()) => {
            let _ = torus.wait(1_000_000);
            let mut results = Vec::new();
            let _ = torus.reap(&mut results);

            if let Some(result) = results.first() {
                callback(result.result, result.user_data, context);
            } else {
                callback(-110, user_data, context);
            }
            0
        }
        Err(e) => {
            let errno = match e {
                tpt_torus_core::Error::SubmissionFull => -11,
                tpt_torus_core::Error::Os(code) => code,
                _ => -22,
            };
            callback(errno as i64, user_data, context);
            errno
        }
    }
}

/// Submit a recv operation.
///
/// # Safety
/// All pointer arguments must be valid. The buffer must remain valid until
/// the callback is invoked.
#[no_mangle]
pub unsafe extern "C" fn torus_recv(
    handle: *const TorusHandle,
    fd: i32,
    buf: *mut u8,
    len: usize,
    user_data: u64,
    callback: CompletionCallback,
    context: *mut std::ffi::c_void,
) -> i32 {
    if handle.is_null() || buf.is_null() || callback as usize == 0 {
        return -22;
    }

    let torus = &(*handle).torus;
    let flow = Flow::with_user_data(Operation::Recv { fd, buf, len }, user_data);

    match torus.submit(&flow) {
        Ok(()) => {
            let _ = torus.wait(1_000_000);
            let mut results = Vec::new();
            let _ = torus.reap(&mut results);

            if let Some(result) = results.first() {
                callback(result.result, result.user_data, context);
            } else {
                callback(-110, user_data, context);
            }
            0
        }
        Err(e) => {
            let errno = match e {
                tpt_torus_core::Error::SubmissionFull => -11,
                tpt_torus_core::Error::Os(code) => code,
                _ => -22,
            };
            callback(errno as i64, user_data, context);
            errno
        }
    }
}

/// Submit a send operation.
///
/// # Safety
/// All pointer arguments must be valid. The buffer must remain valid until
/// the callback is invoked.
#[no_mangle]
pub unsafe extern "C" fn torus_send(
    handle: *const TorusHandle,
    fd: i32,
    buf: *const u8,
    len: usize,
    user_data: u64,
    callback: CompletionCallback,
    context: *mut std::ffi::c_void,
) -> i32 {
    if handle.is_null() || buf.is_null() || callback as usize == 0 {
        return -22;
    }

    let torus = &(*handle).torus;
    let flow = Flow::with_user_data(Operation::Send { fd, buf, len }, user_data);

    match torus.submit(&flow) {
        Ok(()) => {
            let _ = torus.wait(1_000_000);
            let mut results = Vec::new();
            let _ = torus.reap(&mut results);

            if let Some(result) = results.first() {
                callback(result.result, result.user_data, context);
            } else {
                callback(-110, user_data, context);
            }
            0
        }
        Err(e) => {
            let errno = match e {
                tpt_torus_core::Error::SubmissionFull => -11,
                tpt_torus_core::Error::Os(code) => code,
                _ => -22,
            };
            callback(errno as i64, user_data, context);
            errno
        }
    }
}

/// Submit a close operation.
///
/// # Safety
/// The file descriptor must be valid.
#[no_mangle]
pub unsafe extern "C" fn torus_close(
    handle: *const TorusHandle,
    fd: i32,
    user_data: u64,
    callback: CompletionCallback,
    context: *mut std::ffi::c_void,
) -> i32 {
    if handle.is_null() || callback as usize == 0 {
        return -22;
    }

    let torus = &(*handle).torus;
    let flow = Flow::with_user_data(Operation::Close { fd }, user_data);

    match torus.submit(&flow) {
        Ok(()) => {
            let _ = torus.wait(1_000_000);
            let mut results = Vec::new();
            let _ = torus.reap(&mut results);

            if let Some(result) = results.first() {
                callback(result.result, result.user_data, context);
            } else {
                callback(0, user_data, context);
            }
            0
        }
        Err(e) => {
            let errno = match e {
                tpt_torus_core::Error::SubmissionFull => -11,
                tpt_torus_core::Error::Os(code) => code,
                _ => -22,
            };
            callback(errno as i64, user_data, context);
            errno
        }
    }
}

/// Submit a batch of operations.
///
/// # Arguments
/// - `handle`: Torus handle.
/// - `ops`: Array of operation descriptors.
/// - `count`: Number of operations.
///
/// # Returns
/// Number of operations successfully submitted, or negative errno.
///
/// # Safety
/// All pointer arguments must be valid.
#[no_mangle]
pub unsafe extern "C" fn torus_submit_batch(
    handle: *const TorusHandle,
    ops: *const TorusOperation,
    count: u32,
) -> i32 {
    if handle.is_null() || ops.is_null() || count == 0 {
        return -22;
    }

    let torus = &(*handle).torus;
    let ops_slice = std::slice::from_raw_parts(ops, count as usize);

    let mut flows: Vec<Flow> = Vec::with_capacity(count as usize);
    for op in ops_slice {
        let flow = match op.op_type {
            0 => Flow::with_user_data(
                Operation::Read {
                    fd: op.fd,
                    buf: op.buf,
                    len: op.len,
                    offset: op.offset,
                },
                op.user_data,
            ),
            1 => Flow::with_user_data(
                Operation::Write {
                    fd: op.fd,
                    buf: op.buf as *const u8,
                    len: op.len,
                    offset: op.offset,
                },
                op.user_data,
            ),
            2 => Flow::with_user_data(
                Operation::Recv {
                    fd: op.fd,
                    buf: op.buf,
                    len: op.len,
                },
                op.user_data,
            ),
            3 => Flow::with_user_data(
                Operation::Send {
                    fd: op.fd,
                    buf: op.buf as *const u8,
                    len: op.len,
                },
                op.user_data,
            ),
            4 => Flow::with_user_data(Operation::Close { fd: op.fd }, op.user_data),
            _ => return -22,
        };
        flows.push(flow);
    }

    match torus.submit_batch(&flows) {
        Ok(n) => n as i32,
        Err(e) => match e {
            tpt_torus_core::Error::SubmissionFull => -11,
            tpt_torus_core::Error::Os(code) => code,
            _ => -22,
        },
    }
}

/// Wait for completions.
///
/// # Arguments
/// - `handle`: Torus handle.
/// - `timeout_us`: Timeout in microseconds (0 = infinite).
///
/// # Returns
/// 0 on success, negative errno on failure.
///
/// # Safety
/// `handle` must be a valid pointer returned by `torus_create` or `torus_create_with_backend`.
#[no_mangle]
pub unsafe extern "C" fn torus_wait(handle: *const TorusHandle, timeout_us: u64) -> i32 {
    if handle.is_null() {
        return -22;
    }

    let torus = &(*handle).torus;
    match torus.wait(timeout_us) {
        Ok(()) => 0,
        Err(e) => match e {
            tpt_torus_core::Error::Os(code) => code,
            _ => -22,
        },
    }
}

/// Reap completions.
///
/// # Arguments
/// - `handle`: Torus handle.
/// - `results`: Array to store results.
/// - `max_results`: Maximum number of results to store.
///
/// # Returns
/// Number of completions reaped.
///
/// # Safety
/// `results` must be a valid array of at least `max_results` elements.
#[no_mangle]
pub unsafe extern "C" fn torus_reap(
    handle: *const TorusHandle,
    results: *mut TorusCompletion,
    max_results: u32,
) -> i32 {
    if handle.is_null() || results.is_null() || max_results == 0 {
        return -22;
    }

    let torus = &(*handle).torus;
    let mut rust_results = Vec::with_capacity(max_results as usize);

    match torus.reap(&mut rust_results) {
        Ok(count) => {
            let out = std::slice::from_raw_parts_mut(results, max_results as usize);
            for (i, r) in rust_results.iter().enumerate().take(max_results as usize) {
                out[i] = TorusCompletion {
                    result: r.result,
                    user_data: r.user_data,
                };
            }
            count as i32
        }
        Err(e) => match e {
            tpt_torus_core::Error::Os(code) => code,
            _ => -22,
        },
    }
}

/// Get the number of in-flight operations.
///
/// # Safety
/// `handle` must be a valid pointer returned by `torus_create` or `torus_create_with_backend`.
#[no_mangle]
pub unsafe extern "C" fn torus_in_flight(handle: *const TorusHandle) -> u32 {
    if handle.is_null() {
        return 0;
    }
    (*handle).torus.in_flight()
}

/// C-compatible operation descriptor for batch submission.
#[repr(C)]
pub struct TorusOperation {
    /// Operation type: 0=Read, 1=Write, 2=Recv, 3=Send, 4=Close.
    pub op_type: u32,
    /// File descriptor.
    pub fd: i32,
    /// Buffer pointer (read/recv: *mut, write/send: *const).
    pub buf: *mut u8,
    /// Buffer length.
    pub len: usize,
    /// File offset (for read/write).
    pub offset: u64,
    /// User data passed to the callback.
    pub user_data: u64,
}

/// C-compatible completion result.
#[repr(C)]
pub struct TorusCompletion {
    /// Number of bytes transferred, or negative errno on failure.
    pub result: i64,
    /// User data from the submission.
    pub user_data: u64,
}
