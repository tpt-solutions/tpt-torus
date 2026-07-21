//! Raw, unguarded access to the Virtual Torus, bypassing Buffer Leasing.
//!
//! This module provides an explicit opt-out path for users who need to pass
//! raw pointers directly to the I/O engine without lease registration.
//!
//! # Safety
//!
//! Using `RawTorus` bypasses all buffer safety checks. The caller is responsible for:
//! - Ensuring buffers remain valid for the entire duration of the I/O operation
//! - Ensuring buffers are not freed or reused while in-flight
//! - Ensuring buffer addresses are valid and properly aligned
//! - Handling any kernel corruption that may result from invalid pointers
//!
//! Per the **Fail-Safe Defaults** principle, buffer registration and strict
//! sandboxing are enabled by default. Users must explicitly opt out to use
//! raw, unsafe pointers.
//!
//! # Example
//!
//! ```ignore
//! use tpt_torus_core::raw_api::RawTorus;
//!
//! let torus = Torus::new(256, backend)?;
//! let raw = torus.raw();
//!
//! // SAFETY: Caller must ensure the buffer is valid and not freed during the operation
//! unsafe {
//!     raw.submit_write(fd, buf.as_ptr(), buf.len(), offset)?;
//! }
//! ```

use crate::flow::Flow;
use crate::operation::Operation;
use crate::result::Result as TorusResult;
use crate::Torus;

/// Raw, unguarded access to the Virtual Torus.
///
/// Obtained via [`Torus::raw()`]. Bypasses all buffer safety checks.
///
/// # Safety
///
/// All methods on `RawTorus` are unsafe because they bypass the Buffer Leasing
/// system. The caller must ensure all buffer safety guarantees manually.
pub struct RawTorus<'a> {
    torus: &'a Torus,
}

impl<'a> RawTorus<'a> {
    /// Create a new `RawTorus` reference.
    ///
    /// # Safety
    ///
    /// The caller must understand that using this reference bypasses all
    /// buffer safety checks.
    pub(crate) fn new(torus: &'a Torus) -> Self {
        Self { torus }
    }

    /// Submit a raw read operation without buffer registration.
    ///
    /// # Safety
    ///
    /// - `buf` must point to a valid, live memory region of at least `len` bytes
    /// - `buf` must not be freed or modified until the operation completes
    /// - `fd` must be a valid file descriptor
    pub unsafe fn submit_read(
        &self,
        fd: i32,
        buf: *mut u8,
        len: usize,
        offset: u64,
    ) -> crate::Result<()> {
        let flow = Flow::new(Operation::Read {
            fd,
            buf,
            len,
            offset,
        });
        self.torus.submit(&flow)
    }

    /// Submit a raw write operation without buffer registration.
    ///
    /// # Safety
    ///
    /// - `buf` must point to a valid, live memory region of at least `len` bytes
    /// - `buf` must not be freed until the operation completes
    /// - `fd` must be a valid file descriptor
    pub unsafe fn submit_write(
        &self,
        fd: i32,
        buf: *const u8,
        len: usize,
        offset: u64,
    ) -> crate::Result<()> {
        let flow = Flow::new(Operation::Write {
            fd,
            buf,
            len,
            offset,
        });
        self.torus.submit(&flow)
    }

    /// Submit a raw recv operation without buffer registration.
    ///
    /// # Safety
    ///
    /// - `buf` must point to a valid, live memory region of at least `len` bytes
    /// - `buf` must not be freed or modified until the operation completes
    /// - `fd` must be a valid socket file descriptor
    pub unsafe fn submit_recv(&self, fd: i32, buf: *mut u8, len: usize) -> crate::Result<()> {
        let flow = Flow::new(Operation::Recv { fd, buf, len });
        self.torus.submit(&flow)
    }

    /// Submit a raw send operation without buffer registration.
    ///
    /// # Safety
    ///
    /// - `buf` must point to a valid, live memory region of at least `len` bytes
    /// - `buf` must not be freed until the operation completes
    /// - `fd` must be a valid socket file descriptor
    pub unsafe fn submit_send(&self, fd: i32, buf: *const u8, len: usize) -> crate::Result<()> {
        let flow = Flow::new(Operation::Send { fd, buf, len });
        self.torus.submit(&flow)
    }

    /// Submit a raw close operation.
    pub fn submit_close(&self, fd: i32) -> crate::Result<()> {
        let flow = Flow::new(Operation::Close { fd });
        self.torus.submit(&flow)
    }

    /// Reap all available completions.
    pub fn reap(&self, results: &mut Vec<TorusResult>) -> crate::Result<usize> {
        self.torus.reap(results)
    }

    /// Block until at least one completion is available.
    pub fn wait(&self, timeout_us: u64) -> crate::Result<()> {
        self.torus.wait(timeout_us)
    }

    /// Access the underlying Torus instance.
    pub fn torus(&self) -> &Torus {
        self.torus
    }
}
