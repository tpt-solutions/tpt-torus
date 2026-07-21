//! High-level async/await wrapper API for TPT Torus.
//!
//! This module provides ergonomic async operations that cover ~90% of use cases.
//! For advanced use cases (manual batching, linked operations), use the raw
//! `Torus` + `Flow` API directly.
//!
//! # Example
//!
//! ```ignore
//! use tpt_torus_core::async_api::*;
//!
//! let torus = TorusAsync::new(256, backend)?;
//!
//! // Read from a file
//! let bytes = torus.read(fd, &mut buf, offset).await?;
//!
//! // Write to a file
//! torus.write(fd, &data, offset).await?;
//!
//! // Receive from a socket
//! let n = torus.recv(fd, &mut buf).await?;
//!
//! // Send to a socket
//! torus.send(fd, &data).await?;
//! ```

use crate::backend::Backend;
use crate::flow::Flow;
use crate::operation::Operation;
use crate::Torus;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// High-level async wrapper around the Virtual Torus.
///
/// `TorusAsync` provides ergonomic async/await operations for common I/O tasks.
/// It wraps a `Torus` instance and handles the submit/wait/reap cycle internally.
pub struct TorusAsync {
    torus: Arc<Torus>,
}

impl TorusAsync {
    /// Create a new async Torus instance.
    pub fn new(ring_entries: u32, backend: Box<dyn Backend>) -> crate::Result<Self> {
        let torus = Torus::new(ring_entries, backend)?;
        Ok(Self {
            torus: Arc::new(torus),
        })
    }

    /// Create from an existing Torus instance.
    pub fn from_torus(torus: Arc<Torus>) -> Self {
        Self { torus }
    }

    /// Get a reference to the underlying Torus.
    pub fn torus(&self) -> &Torus {
        &self.torus
    }

    /// Read from a file descriptor at the given offset.
    ///
    /// Returns the number of bytes read.
    pub fn read<'a>(&'a self, fd: i32, buf: &'a mut [u8], offset: u64) -> ReadFuture<'a> {
        ReadFuture {
            torus: &self.torus,
            fd,
            buf,
            offset,
            submitted: false,
            user_data: 0,
        }
    }

    /// Write to a file descriptor at the given offset.
    ///
    /// Returns the number of bytes written.
    pub fn write<'a>(&'a self, fd: i32, buf: &'a [u8], offset: u64) -> WriteFuture<'a> {
        WriteFuture {
            torus: &self.torus,
            fd,
            buf,
            offset,
            submitted: false,
            user_data: 0,
        }
    }

    /// Receive data from a connected socket.
    ///
    /// Returns the number of bytes received.
    pub fn recv<'a>(&'a self, fd: i32, buf: &'a mut [u8]) -> RecvFuture<'a> {
        RecvFuture {
            torus: &self.torus,
            fd,
            buf,
            submitted: false,
            user_data: 0,
        }
    }

    /// Send data to a connected socket.
    ///
    /// Returns the number of bytes sent.
    pub fn send<'a>(&'a self, fd: i32, buf: &'a [u8]) -> SendFuture<'a> {
        SendFuture {
            torus: &self.torus,
            fd,
            buf,
            submitted: false,
            user_data: 0,
        }
    }

    /// Accept an incoming connection.
    ///
    /// Returns the new socket file descriptor.
    pub fn accept(
        &self,
        fd: i32,
        addr: *mut libc::sockaddr,
        addrlen: *mut u32,
    ) -> AcceptFuture<'_> {
        AcceptFuture {
            torus: &self.torus,
            fd,
            addr,
            addrlen,
            submitted: false,
            user_data: 0,
        }
    }

    /// Connect to a remote address.
    pub fn connect(&self, fd: i32, addr: *const libc::sockaddr, addrlen: u32) -> ConnectFuture<'_> {
        ConnectFuture {
            torus: &self.torus,
            fd,
            addr,
            addrlen,
            submitted: false,
            user_data: 0,
        }
    }

    /// Close a file descriptor.
    pub fn close(&self, fd: i32) -> CloseFuture<'_> {
        CloseFuture {
            torus: &self.torus,
            fd,
            submitted: false,
            user_data: 0,
        }
    }
}

// ─── Future implementations ────────────────────────────────────────────────

/// Future for read operations.
pub struct ReadFuture<'a> {
    torus: &'a Torus,
    fd: i32,
    buf: &'a mut [u8],
    offset: u64,
    submitted: bool,
    user_data: u64,
}

impl<'a> Future for ReadFuture<'a> {
    type Output = crate::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        if !this.submitted {
            let flow = Flow::with_user_data(
                Operation::Read {
                    fd: this.fd,
                    buf: this.buf.as_mut_ptr(),
                    len: this.buf.len(),
                    offset: this.offset,
                },
                this.user_data,
            );

            match this.torus.submit(&flow) {
                Ok(()) => {
                    this.submitted = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        } else {
            let mut results = Vec::new();
            this.torus.reap(&mut results)?;

            if results.is_empty() {
                // Not ready yet — register waker and return pending
                // In a real implementation, the backend would wake the task
                Poll::Pending
            } else {
                let result = &results[0];
                if result.is_ok() {
                    Poll::Ready(Ok(result.bytes().unwrap_or(0)))
                } else {
                    Poll::Ready(Err(crate::Error::Os(result.error().unwrap_or(5))))
                }
            }
        }
    }
}

/// Future for write operations.
pub struct WriteFuture<'a> {
    torus: &'a Torus,
    fd: i32,
    buf: &'a [u8],
    offset: u64,
    submitted: bool,
    user_data: u64,
}

impl<'a> Future for WriteFuture<'a> {
    type Output = crate::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        if !this.submitted {
            let flow = Flow::with_user_data(
                Operation::Write {
                    fd: this.fd,
                    buf: this.buf.as_ptr(),
                    len: this.buf.len(),
                    offset: this.offset,
                },
                this.user_data,
            );

            match this.torus.submit(&flow) {
                Ok(()) => {
                    this.submitted = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        } else {
            let mut results = Vec::new();
            this.torus.reap(&mut results)?;

            if results.is_empty() {
                Poll::Pending
            } else {
                let result = &results[0];
                if result.is_ok() {
                    Poll::Ready(Ok(result.bytes().unwrap_or(0)))
                } else {
                    Poll::Ready(Err(crate::Error::Os(result.error().unwrap_or(5))))
                }
            }
        }
    }
}

/// Future for recv operations.
pub struct RecvFuture<'a> {
    torus: &'a Torus,
    fd: i32,
    buf: &'a mut [u8],
    submitted: bool,
    user_data: u64,
}

impl<'a> Future for RecvFuture<'a> {
    type Output = crate::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        if !this.submitted {
            let flow = Flow::with_user_data(
                Operation::Recv {
                    fd: this.fd,
                    buf: this.buf.as_mut_ptr(),
                    len: this.buf.len(),
                },
                this.user_data,
            );

            match this.torus.submit(&flow) {
                Ok(()) => {
                    this.submitted = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        } else {
            let mut results = Vec::new();
            this.torus.reap(&mut results)?;

            if results.is_empty() {
                Poll::Pending
            } else {
                let result = &results[0];
                if result.is_ok() {
                    Poll::Ready(Ok(result.bytes().unwrap_or(0)))
                } else {
                    Poll::Ready(Err(crate::Error::Os(result.error().unwrap_or(5))))
                }
            }
        }
    }
}

/// Future for send operations.
pub struct SendFuture<'a> {
    torus: &'a Torus,
    fd: i32,
    buf: &'a [u8],
    submitted: bool,
    user_data: u64,
}

impl<'a> Future for SendFuture<'a> {
    type Output = crate::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        if !this.submitted {
            let flow = Flow::with_user_data(
                Operation::Send {
                    fd: this.fd,
                    buf: this.buf.as_ptr(),
                    len: this.buf.len(),
                },
                this.user_data,
            );

            match this.torus.submit(&flow) {
                Ok(()) => {
                    this.submitted = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        } else {
            let mut results = Vec::new();
            this.torus.reap(&mut results)?;

            if results.is_empty() {
                Poll::Pending
            } else {
                let result = &results[0];
                if result.is_ok() {
                    Poll::Ready(Ok(result.bytes().unwrap_or(0)))
                } else {
                    Poll::Ready(Err(crate::Error::Os(result.error().unwrap_or(5))))
                }
            }
        }
    }
}

/// Future for accept operations.
pub struct AcceptFuture<'a> {
    torus: &'a Torus,
    fd: i32,
    addr: *mut libc::sockaddr,
    addrlen: *mut u32,
    submitted: bool,
    user_data: u64,
}

impl<'a> Future for AcceptFuture<'a> {
    type Output = crate::Result<i32>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        if !this.submitted {
            let flow = Flow::with_user_data(
                Operation::Accept {
                    fd: this.fd,
                    addr: this.addr,
                    addrlen: this.addrlen,
                },
                this.user_data,
            );

            match this.torus.submit(&flow) {
                Ok(()) => {
                    this.submitted = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        } else {
            let mut results = Vec::new();
            this.torus.reap(&mut results)?;

            if results.is_empty() {
                Poll::Pending
            } else {
                let result = &results[0];
                if result.is_ok() {
                    Poll::Ready(Ok(result.raw() as i32))
                } else {
                    Poll::Ready(Err(crate::Error::Os(result.error().unwrap_or(5))))
                }
            }
        }
    }
}

/// Future for connect operations.
pub struct ConnectFuture<'a> {
    torus: &'a Torus,
    fd: i32,
    addr: *const libc::sockaddr,
    addrlen: u32,
    submitted: bool,
    user_data: u64,
}

impl<'a> Future for ConnectFuture<'a> {
    type Output = crate::Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        if !this.submitted {
            let flow = Flow::with_user_data(
                Operation::Connect {
                    fd: this.fd,
                    addr: this.addr,
                    addrlen: this.addrlen,
                },
                this.user_data,
            );

            match this.torus.submit(&flow) {
                Ok(()) => {
                    this.submitted = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        } else {
            let mut results = Vec::new();
            this.torus.reap(&mut results)?;

            if results.is_empty() {
                Poll::Pending
            } else {
                let result = &results[0];
                if result.is_ok() {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Ready(Err(crate::Error::Os(result.error().unwrap_or(5))))
                }
            }
        }
    }
}

/// Future for close operations.
pub struct CloseFuture<'a> {
    torus: &'a Torus,
    fd: i32,
    submitted: bool,
    user_data: u64,
}

impl<'a> Future for CloseFuture<'a> {
    type Output = crate::Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        if !this.submitted {
            let flow = Flow::with_user_data(Operation::Close { fd: this.fd }, this.user_data);

            match this.torus.submit(&flow) {
                Ok(()) => {
                    this.submitted = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        } else {
            let mut results = Vec::new();
            this.torus.reap(&mut results)?;

            if results.is_empty() {
                Poll::Pending
            } else {
                let result = &results[0];
                if result.is_ok() {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Ready(Err(crate::Error::Os(result.error().unwrap_or(5))))
                }
            }
        }
    }
}
