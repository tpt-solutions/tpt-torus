//! Tokio-compatible `AsyncRead` / `AsyncWrite` shim over [`TorusAsync`].
//!
//! Enable the `tokio` feature to use TPT Torus file descriptors with tokio's
//! async I/O traits (`tokio::io::AsyncRead` / `AsyncWrite`) and the wider tokio
//! ecosystem (e.g. `tokio::io::copy`, framing codecs).
//!
//! Each wrapper owns an [`Arc<TorusAsync>`] plus the file descriptor, and drives
//! the underlying operation through [`TorusAsync::poll_read_op`] /
//! [`TorusAsync::poll_write_op`] so the operation is submitted exactly once and
//! completed via the normal reaper-thread waker path.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use tpt_torus_core::async_api::TorusAsync;
//! use tpt_torus_core::async_tokio::TorusAsyncReader;
//! # fn make_torus() -> Arc<TorusAsync> { unimplemented!() }
//! # fn main() -> std::io::Result<()> {
//! let torus = make_torus();
//! let reader = TorusAsyncReader::new(torus, 3 /* fd */);
//! // `reader` now implements `tokio::io::AsyncRead`.
//! # Ok(())
//! # }
//! ```

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::async_api::TorusAsync;

fn to_io_err(e: crate::Error) -> io::Error {
    match e {
        crate::Error::Os(code) => io::Error::from_raw_os_error(code),
        other => io::Error::other(other),
    }
}

/// An [`AsyncRead`] adapter over a TPT Torus file descriptor.
///
/// Reads advance an internal offset starting at 0; use [`TorusAsyncReader::new`]
/// for sequential file reads. The wrapper reads into an internal scratch buffer
/// and copies into the caller's `ReadBuf` so the in-flight operation is owned
/// for its whole lifetime by the reaper.
pub struct TorusAsyncReader {
    torus: Arc<TorusAsync>,
    fd: i32,
    offset: u64,
    scratch: Vec<u8>,
    user_data: u64,
    submitted: bool,
}

impl TorusAsyncReader {
    /// Create a new async reader for `fd` starting at offset 0.
    pub fn new(torus: Arc<TorusAsync>, fd: i32) -> Self {
        Self {
            torus,
            fd,
            offset: 0,
            scratch: Vec::new(),
            user_data: 0,
            submitted: false,
        }
    }

    /// Create a new async reader for `fd` starting at the given `offset`.
    pub fn with_offset(torus: Arc<TorusAsync>, fd: i32, offset: u64) -> Self {
        Self {
            torus,
            fd,
            offset,
            scratch: Vec::new(),
            user_data: 0,
            submitted: false,
        }
    }
}

impl AsyncRead for TorusAsyncReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let needed = buf.remaining();
        if needed == 0 {
            return Poll::Ready(Ok(()));
        }
        if this.scratch.len() < needed {
            this.scratch.resize(needed, 0);
        }

        match this.torus.poll_read_op(
            this.fd,
            &mut this.scratch[..needed],
            this.offset,
            &mut this.user_data,
            &mut this.submitted,
            cx,
        ) {
            Poll::Ready(Ok(bytes)) => {
                buf.put_slice(&this.scratch[..bytes]);
                this.offset += bytes as u64;
                this.submitted = false;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(to_io_err(e))),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// An [`AsyncWrite`] adapter over a TPT Torus file descriptor.
///
/// Each `poll_write` issues the entire provided buffer as a single positional
/// write at the current offset, then advances the offset by the bytes written.
pub struct TorusAsyncWriter {
    torus: Arc<TorusAsync>,
    fd: i32,
    offset: u64,
    user_data: u64,
    submitted: bool,
}

impl TorusAsyncWriter {
    /// Create a new async writer for `fd` starting at offset 0.
    pub fn new(torus: Arc<TorusAsync>, fd: i32) -> Self {
        Self {
            torus,
            fd,
            offset: 0,
            user_data: 0,
            submitted: false,
        }
    }

    /// Create a new async writer for `fd` starting at the given `offset`.
    pub fn with_offset(torus: Arc<TorusAsync>, fd: i32, offset: u64) -> Self {
        Self {
            torus,
            fd,
            offset,
            user_data: 0,
            submitted: false,
        }
    }
}

impl AsyncWrite for TorusAsyncWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        match this.torus.poll_write_op(
            this.fd,
            buf,
            this.offset,
            &mut this.user_data,
            &mut this.submitted,
            cx,
        ) {
            Poll::Ready(Ok(bytes)) => {
                this.offset += bytes as u64;
                this.submitted = false;
                Poll::Ready(Ok(bytes))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(to_io_err(e))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
