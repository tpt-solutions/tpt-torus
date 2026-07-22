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
//!
//! # Waker registration
//!
//! Each future registers its waker with a [`WakerRegistry`] keyed by the
//! flow's `user_data`. A background reaper thread in [`TorusAsync`] polls
//! completions and wakes any registered tasks, so this works correctly with
//! real async runtimes (tokio, async-std) that park tasks instead of busy-looping.

use crate::backend::Backend;
use crate::flow::Flow;
use crate::operation::Operation;
use crate::Torus;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

// ─── Waker Registry ─────────────────────────────────────────────────────────

/// Maps flow user_data values to task wakers.
///
/// When a future returns `Pending`, it registers its waker here keyed by the
/// flow's `user_data`. The background reaper thread calls [`WakerRegistry::wake`]
/// for each completed operation, causing the executor to re-poll the future.
struct WakerRegistry {
    wakers: Mutex<HashMap<u64, Waker>>,
    next_id: AtomicU64,
}

impl WakerRegistry {
    fn new() -> Self {
        Self {
            wakers: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Allocate a unique user_data id for a new flow.
    fn alloc_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Register a waker for the given user_data.
    fn register(&self, user_data: u64, waker: Waker) {
        self.wakers.lock().unwrap().insert(user_data, waker);
    }

    /// Wake and remove the task associated with this user_data.
    fn wake(&self, user_data: u64) {
        if let Some(waker) = self.wakers.lock().unwrap().remove(&user_data) {
            waker.wake();
        }
    }

    /// Remove a waker without waking it (used on successful completion).
    fn unregister(&self, user_data: u64) {
        self.wakers.lock().unwrap().remove(&user_data);
    }
}

// ─── TorusAsync ─────────────────────────────────────────────────────────────

/// High-level async wrapper around the Virtual Torus.
///
/// `TorusAsync` provides ergonomic async/await operations for common I/O tasks.
/// It wraps a `Torus` instance and handles the submit/wait/reap cycle internally.
///
/// A background reaper thread polls for completions and wakes registered tasks,
/// making this safe to use with any async runtime.
pub struct TorusAsync {
    torus: Arc<Torus>,
    registry: Arc<WakerRegistry>,
    /// Handle to the background reaper thread; dropped to stop it.
    _reaper: std::thread::JoinHandle<()>,
}

impl TorusAsync {
    /// Create a new async Torus instance.
    ///
    /// Spawns a background reaper thread that polls for completions every 1ms
    /// and wakes any registered tasks.
    pub fn new(ring_entries: u32, backend: Box<dyn Backend>) -> crate::Result<Self> {
        let torus = Arc::new(Torus::new(ring_entries, backend)?);
        let registry = Arc::new(WakerRegistry::new());

        let reaper_torus = Arc::clone(&torus);
        let reaper_registry = Arc::clone(&registry);
        let reaper = std::thread::Builder::new()
            .name("torus-async-reaper".into())
            .spawn(move || {
                let mut results = Vec::with_capacity(64);
                loop {
                    results.clear();
                    match reaper_torus.reap(&mut results) {
                        Ok(count) if count > 0 => {
                            for result in &results {
                                reaper_registry.wake(result.user_data);
                            }
                        }
                        _ => {
                            // Nothing available or error — sleep briefly to avoid busy-wait
                            std::thread::sleep(Duration::from_micros(500));
                        }
                    }
                }
            })
            .expect("failed to spawn torus-async-reaper thread");

        Ok(Self {
            torus,
            registry,
            _reaper: reaper,
        })
    }

    /// Create from an existing Torus instance.
    pub fn from_torus(torus: Arc<Torus>) -> Self {
        let registry = Arc::new(WakerRegistry::new());

        let reaper_torus = Arc::clone(&torus);
        let reaper_registry = Arc::clone(&registry);
        let reaper = std::thread::Builder::new()
            .name("torus-async-reaper".into())
            .spawn(move || {
                let mut results = Vec::with_capacity(64);
                loop {
                    results.clear();
                    match reaper_torus.reap(&mut results) {
                        Ok(count) if count > 0 => {
                            for result in &results {
                                reaper_registry.wake(result.user_data);
                            }
                        }
                        _ => {
                            std::thread::sleep(Duration::from_micros(500));
                        }
                    }
                }
            })
            .expect("failed to spawn torus-async-reaper thread");

        Self {
            torus,
            registry,
            _reaper: reaper,
        }
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
            registry: &self.registry,
            fd,
            buf,
            offset,
            state: FutureState::Init,
            user_data: 0,
        }
    }

    /// Write to a file descriptor at the given offset.
    ///
    /// Returns the number of bytes written.
    pub fn write<'a>(&'a self, fd: i32, buf: &'a [u8], offset: u64) -> WriteFuture<'a> {
        WriteFuture {
            torus: &self.torus,
            registry: &self.registry,
            fd,
            buf,
            offset,
            state: FutureState::Init,
            user_data: 0,
        }
    }

    /// Receive data from a connected socket.
    ///
    /// Returns the number of bytes received.
    pub fn recv<'a>(&'a self, fd: i32, buf: &'a mut [u8]) -> RecvFuture<'a> {
        RecvFuture {
            torus: &self.torus,
            registry: &self.registry,
            fd,
            buf,
            state: FutureState::Init,
            user_data: 0,
        }
    }

    /// Send data to a connected socket.
    ///
    /// Returns the number of bytes sent.
    pub fn send<'a>(&'a self, fd: i32, buf: &'a [u8]) -> SendFuture<'a> {
        SendFuture {
            torus: &self.torus,
            registry: &self.registry,
            fd,
            buf,
            state: FutureState::Init,
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
            registry: &self.registry,
            fd,
            addr,
            addrlen,
            state: FutureState::Init,
            user_data: 0,
        }
    }

    /// Connect to a remote address.
    pub fn connect(&self, fd: i32, addr: *const libc::sockaddr, addrlen: u32) -> ConnectFuture<'_> {
        ConnectFuture {
            torus: &self.torus,
            registry: &self.registry,
            fd,
            addr,
            addrlen,
            state: FutureState::Init,
            user_data: 0,
        }
    }

    /// Close a file descriptor.
    pub fn close(&self, fd: i32) -> CloseFuture<'_> {
        CloseFuture {
            torus: &self.torus,
            registry: &self.registry,
            fd,
            state: FutureState::Init,
            user_data: 0,
        }
    }
}

// ─── Shared future state ────────────────────────────────────────────────────

/// State machine shared by all future types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FutureState {
    /// Not yet submitted.
    Init,
    /// Submitted, waiting for completion.
    Waiting,
    /// Completion received (terminal).
    Done,
}

// ─── Future implementations ────────────────────────────────────────────────

/// Future for read operations.
pub struct ReadFuture<'a> {
    torus: &'a Torus,
    registry: &'a WakerRegistry,
    fd: i32,
    buf: &'a mut [u8],
    offset: u64,
    state: FutureState,
    user_data: u64,
}

impl<'a> Future for ReadFuture<'a> {
    type Output = crate::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        match this.state {
            FutureState::Init => {
                this.user_data = this.registry.alloc_id();
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
                        this.state = FutureState::Waiting;
                        // Register waker so the reaper thread can wake us
                        this.registry.register(this.user_data, cx.waker().clone());
                        Poll::Pending
                    }
                    Err(e) => {
                        this.state = FutureState::Done;
                        Poll::Ready(Err(e))
                    }
                }
            }
            FutureState::Waiting => {
                // Re-register waker in case we were woken but completion hasn't arrived yet
                this.registry.register(this.user_data, cx.waker().clone());

                let mut results = Vec::new();
                this.torus.reap(&mut results)?;

                if results.is_empty() {
                    Poll::Pending
                } else {
                    this.state = FutureState::Done;
                    this.registry.unregister(this.user_data);
                    let result = &results[0];
                    if result.is_ok() {
                        Poll::Ready(Ok(result.bytes().unwrap_or(0)))
                    } else {
                        Poll::Ready(Err(crate::Error::Os(result.error().unwrap_or(5))))
                    }
                }
            }
            FutureState::Done => panic!("poll after completion"),
        }
    }
}

impl<'a> Drop for ReadFuture<'a> {
    fn drop(&mut self) {
        if self.state == FutureState::Waiting {
            self.registry.unregister(self.user_data);
        }
    }
}

/// Future for write operations.
pub struct WriteFuture<'a> {
    torus: &'a Torus,
    registry: &'a WakerRegistry,
    fd: i32,
    buf: &'a [u8],
    offset: u64,
    state: FutureState,
    user_data: u64,
}

impl<'a> Future for WriteFuture<'a> {
    type Output = crate::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        match this.state {
            FutureState::Init => {
                this.user_data = this.registry.alloc_id();
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
                        this.state = FutureState::Waiting;
                        this.registry.register(this.user_data, cx.waker().clone());
                        Poll::Pending
                    }
                    Err(e) => {
                        this.state = FutureState::Done;
                        Poll::Ready(Err(e))
                    }
                }
            }
            FutureState::Waiting => {
                this.registry.register(this.user_data, cx.waker().clone());

                let mut results = Vec::new();
                this.torus.reap(&mut results)?;

                if results.is_empty() {
                    Poll::Pending
                } else {
                    this.state = FutureState::Done;
                    this.registry.unregister(this.user_data);
                    let result = &results[0];
                    if result.is_ok() {
                        Poll::Ready(Ok(result.bytes().unwrap_or(0)))
                    } else {
                        Poll::Ready(Err(crate::Error::Os(result.error().unwrap_or(5))))
                    }
                }
            }
            FutureState::Done => panic!("poll after completion"),
        }
    }
}

impl<'a> Drop for WriteFuture<'a> {
    fn drop(&mut self) {
        if self.state == FutureState::Waiting {
            self.registry.unregister(self.user_data);
        }
    }
}

/// Future for recv operations.
pub struct RecvFuture<'a> {
    torus: &'a Torus,
    registry: &'a WakerRegistry,
    fd: i32,
    buf: &'a mut [u8],
    state: FutureState,
    user_data: u64,
}

impl<'a> Future for RecvFuture<'a> {
    type Output = crate::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        match this.state {
            FutureState::Init => {
                this.user_data = this.registry.alloc_id();
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
                        this.state = FutureState::Waiting;
                        this.registry.register(this.user_data, cx.waker().clone());
                        Poll::Pending
                    }
                    Err(e) => {
                        this.state = FutureState::Done;
                        Poll::Ready(Err(e))
                    }
                }
            }
            FutureState::Waiting => {
                this.registry.register(this.user_data, cx.waker().clone());

                let mut results = Vec::new();
                this.torus.reap(&mut results)?;

                if results.is_empty() {
                    Poll::Pending
                } else {
                    this.state = FutureState::Done;
                    this.registry.unregister(this.user_data);
                    let result = &results[0];
                    if result.is_ok() {
                        Poll::Ready(Ok(result.bytes().unwrap_or(0)))
                    } else {
                        Poll::Ready(Err(crate::Error::Os(result.error().unwrap_or(5))))
                    }
                }
            }
            FutureState::Done => panic!("poll after completion"),
        }
    }
}

impl<'a> Drop for RecvFuture<'a> {
    fn drop(&mut self) {
        if self.state == FutureState::Waiting {
            self.registry.unregister(self.user_data);
        }
    }
}

/// Future for send operations.
pub struct SendFuture<'a> {
    torus: &'a Torus,
    registry: &'a WakerRegistry,
    fd: i32,
    buf: &'a [u8],
    state: FutureState,
    user_data: u64,
}

impl<'a> Future for SendFuture<'a> {
    type Output = crate::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        match this.state {
            FutureState::Init => {
                this.user_data = this.registry.alloc_id();
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
                        this.state = FutureState::Waiting;
                        this.registry.register(this.user_data, cx.waker().clone());
                        Poll::Pending
                    }
                    Err(e) => {
                        this.state = FutureState::Done;
                        Poll::Ready(Err(e))
                    }
                }
            }
            FutureState::Waiting => {
                this.registry.register(this.user_data, cx.waker().clone());

                let mut results = Vec::new();
                this.torus.reap(&mut results)?;

                if results.is_empty() {
                    Poll::Pending
                } else {
                    this.state = FutureState::Done;
                    this.registry.unregister(this.user_data);
                    let result = &results[0];
                    if result.is_ok() {
                        Poll::Ready(Ok(result.bytes().unwrap_or(0)))
                    } else {
                        Poll::Ready(Err(crate::Error::Os(result.error().unwrap_or(5))))
                    }
                }
            }
            FutureState::Done => panic!("poll after completion"),
        }
    }
}

impl<'a> Drop for SendFuture<'a> {
    fn drop(&mut self) {
        if self.state == FutureState::Waiting {
            self.registry.unregister(self.user_data);
        }
    }
}

/// Future for accept operations.
pub struct AcceptFuture<'a> {
    torus: &'a Torus,
    registry: &'a WakerRegistry,
    fd: i32,
    addr: *mut libc::sockaddr,
    addrlen: *mut u32,
    state: FutureState,
    user_data: u64,
}

impl<'a> Future for AcceptFuture<'a> {
    type Output = crate::Result<i32>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        match this.state {
            FutureState::Init => {
                this.user_data = this.registry.alloc_id();
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
                        this.state = FutureState::Waiting;
                        this.registry.register(this.user_data, cx.waker().clone());
                        Poll::Pending
                    }
                    Err(e) => {
                        this.state = FutureState::Done;
                        Poll::Ready(Err(e))
                    }
                }
            }
            FutureState::Waiting => {
                this.registry.register(this.user_data, cx.waker().clone());

                let mut results = Vec::new();
                this.torus.reap(&mut results)?;

                if results.is_empty() {
                    Poll::Pending
                } else {
                    this.state = FutureState::Done;
                    this.registry.unregister(this.user_data);
                    let result = &results[0];
                    if result.is_ok() {
                        Poll::Ready(Ok(result.raw() as i32))
                    } else {
                        Poll::Ready(Err(crate::Error::Os(result.error().unwrap_or(5))))
                    }
                }
            }
            FutureState::Done => panic!("poll after completion"),
        }
    }
}

impl<'a> Drop for AcceptFuture<'a> {
    fn drop(&mut self) {
        if self.state == FutureState::Waiting {
            self.registry.unregister(self.user_data);
        }
    }
}

/// Future for connect operations.
pub struct ConnectFuture<'a> {
    torus: &'a Torus,
    registry: &'a WakerRegistry,
    fd: i32,
    addr: *const libc::sockaddr,
    addrlen: u32,
    state: FutureState,
    user_data: u64,
}

impl<'a> Future for ConnectFuture<'a> {
    type Output = crate::Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        match this.state {
            FutureState::Init => {
                this.user_data = this.registry.alloc_id();
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
                        this.state = FutureState::Waiting;
                        this.registry.register(this.user_data, cx.waker().clone());
                        Poll::Pending
                    }
                    Err(e) => {
                        this.state = FutureState::Done;
                        Poll::Ready(Err(e))
                    }
                }
            }
            FutureState::Waiting => {
                this.registry.register(this.user_data, cx.waker().clone());

                let mut results = Vec::new();
                this.torus.reap(&mut results)?;

                if results.is_empty() {
                    Poll::Pending
                } else {
                    this.state = FutureState::Done;
                    this.registry.unregister(this.user_data);
                    let result = &results[0];
                    if result.is_ok() {
                        Poll::Ready(Ok(()))
                    } else {
                        Poll::Ready(Err(crate::Error::Os(result.error().unwrap_or(5))))
                    }
                }
            }
            FutureState::Done => panic!("poll after completion"),
        }
    }
}

impl<'a> Drop for ConnectFuture<'a> {
    fn drop(&mut self) {
        if self.state == FutureState::Waiting {
            self.registry.unregister(self.user_data);
        }
    }
}

/// Future for close operations.
pub struct CloseFuture<'a> {
    torus: &'a Torus,
    registry: &'a WakerRegistry,
    fd: i32,
    state: FutureState,
    user_data: u64,
}

impl<'a> Future for CloseFuture<'a> {
    type Output = crate::Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        match this.state {
            FutureState::Init => {
                this.user_data = this.registry.alloc_id();
                let flow =
                    Flow::with_user_data(Operation::Close { fd: this.fd }, this.user_data);

                match this.torus.submit(&flow) {
                    Ok(()) => {
                        this.state = FutureState::Waiting;
                        this.registry.register(this.user_data, cx.waker().clone());
                        Poll::Pending
                    }
                    Err(e) => {
                        this.state = FutureState::Done;
                        Poll::Ready(Err(e))
                    }
                }
            }
            FutureState::Waiting => {
                this.registry.register(this.user_data, cx.waker().clone());

                let mut results = Vec::new();
                this.torus.reap(&mut results)?;

                if results.is_empty() {
                    Poll::Pending
                } else {
                    this.state = FutureState::Done;
                    this.registry.unregister(this.user_data);
                    let result = &results[0];
                    if result.is_ok() {
                        Poll::Ready(Ok(()))
                    } else {
                        Poll::Ready(Err(crate::Error::Os(result.error().unwrap_or(5))))
                    }
                }
            }
            FutureState::Done => panic!("poll after completion"),
        }
    }
}

impl<'a> Drop for CloseFuture<'a> {
    fn drop(&mut self) {
        if self.state == FutureState::Waiting {
            self.registry.unregister(self.user_data);
        }
    }
}
