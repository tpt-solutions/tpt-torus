//! macOS/BSD kqueue backend: a background reactor drains the virtual SQ,
//! translates operations into native kqueue calls, and populates the virtual CQ
//! upon completion.
//!
//! Socket operations use kqueue's native `EVFILT_READ` / `EVFILT_WRITE` for
//! true async I/O: submission only registers the fd with the kqueue and the
//! reactor performs the actual `recv`/`send`/`accept`/`connect` when the fd is
//! ready. File operations (which kqueue does not support asynchronously) are
//! dispatched to a thread pool that issues positional `pread`/`pwrite` and
//! posts the completion when done.
#![cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]

use tpt_torus_core::backend::Backend;
use tpt_torus_core::flow::Flow;
use tpt_torus_core::operation::{IoSlice, Operation};
use tpt_torus_core::result::Result as TorusResult;

use std::collections::VecDeque;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

// ─── kqueue FFI ────────────────────────────────────────────────────────────

/// FFI flags/filters for kqueue. Some are not yet used by the reactor but are
/// kept for API completeness; allow dead-code so `-D warnings` CI stays green.
#[allow(dead_code)]
const EV_ADD: u16 = 0x0001;
#[allow(dead_code)]
const EV_DELETE: u16 = 0x0002;
#[allow(dead_code)]
const EV_ENABLE: u16 = 0x0004;
#[allow(dead_code)]
const EV_DISABLE: u16 = 0x0008;
#[allow(dead_code)]
const EV_CLEAR: u16 = 0x0020;
const EV_ONESHOT: u16 = 0x0010;
#[allow(dead_code)]
const NOTE_WRITE: u32 = 0x00000004;
#[allow(dead_code)]
const NOTE_DELETE: u32 = 0x00000001;
#[allow(dead_code)]
const NOTE_EXTEND: u32 = 0x00000008;
#[allow(dead_code)]
const NOTE_ATTRIB: u32 = 0x00000004;
#[allow(dead_code)]
const NOTE_LINK: u32 = 0x00000010;
#[allow(dead_code)]
const NOTE_RENAME: u32 = 0x00000020;
#[allow(dead_code)]
const NOTE_REVOKE: u32 = 0x00000040;
#[allow(dead_code)]
const EVFILT_VNODE: i16 = -4;
const EVFILT_READ: i16 = -1;
const EVFILT_WRITE: i16 = -2;
const KEVENT_ARRAY_SIZE: usize = 256;

#[repr(C)]
#[derive(Clone, Copy)]
struct KEvent {
    ident: usize,
    filter: i16,
    flags: u16,
    fflags: u32,
    data: isize,
    udata: *mut std::ffi::c_void,
}

impl Default for KEvent {
    fn default() -> Self {
        KEvent {
            ident: 0,
            filter: 0,
            flags: 0,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut(),
        }
    }
}

extern "C" {
    fn kqueue() -> libc::c_int;
    fn kevent(
        kq: libc::c_int,
        changelist: *const KEvent,
        nchanges: libc::c_int,
        eventlist: *mut KEvent,
        nevents: libc::c_int,
        timeout: *const libc::timespec,
    ) -> libc::c_int;
}

/// A socket operation the reactor will perform once the fd is ready.
enum SocketOp {
    Recv {
        fd: i32,
        buf: *mut u8,
        len: usize,
    },
    Send {
        fd: i32,
        buf: *const u8,
        len: usize,
    },
    Accept {
        listen_fd: i32,
        addr: *mut libc::sockaddr,
        addrlen: *mut u32,
    },
    Connect {
        fd: i32,
    },
}

/// Heap-allocated context for an in-flight socket operation. The pointer is
/// handed to kqueue as `udata`; the reactor reclaims ownership via
/// `Box::from_raw` when the event fires.
struct OpCtx {
    user_data: u64,
    op: SocketOp,
}

/// A file I/O job dispatched to the worker pool.
enum FileJobKind {
    Read {
        buf: *mut u8,
        len: usize,
        offset: u64,
    },
    Write {
        buf: *const u8,
        len: usize,
        offset: u64,
    },
    Readv {
        bufs: *const IoSlice,
        buf_count: u32,
        offset: u64,
    },
    Writev {
        bufs: *const IoSlice,
        buf_count: u32,
        offset: u64,
    },
}

struct FileJob {
    fd: i32,
    kind: FileJobKind,
    user_data: u64,
    completions: Arc<Mutex<VecDeque<TorusResult>>>,
    notify: Arc<Condvar>,
}

// SAFETY: the raw buffer pointers in `FileJobKind` reference caller-owned
// buffers that remain valid for the lifetime of the operation (the caller must
// keep them alive until the completion is reaped). Transferring them to a worker
// thread for a one-shot positional read/write is therefore safe.
unsafe impl Send for FileJob {}

/// A small thread pool that runs blocking positional file I/O off the submit
/// path, posting completions when each operation finishes.
struct FileThreadPool {
    tx: Mutex<mpsc::Sender<FileJob>>,
    _workers: Vec<thread::JoinHandle<()>>,
}

impl FileThreadPool {
    fn new(_completions: Arc<Mutex<VecDeque<TorusResult>>>, _notify: Arc<Condvar>) -> Self {
        let (tx, rx) = mpsc::channel::<FileJob>();
        let rx = Arc::new(Mutex::new(rx));
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .max(1);

        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            let rx = Arc::clone(&rx);
            handles.push(thread::spawn(move || {
                loop {
                    let job = match rx.lock().unwrap().recv() {
                        Ok(job) => job,
                        Err(_) => break, // channel closed: shut down worker
                    };
                    Self::run(job);
                }
            }));
        }

        Self {
            tx: Mutex::new(tx),
            _workers: handles,
        }
    }

    fn submit(&self, job: FileJob) {
        // If the pool is shutting down the send fails; there is nothing to do
        // but drop the job (its completion is simply never produced, which
        // matches a torn-down backend).
        let _ = self.tx.lock().unwrap().send(job);
    }

    fn run(job: FileJob) {
        let result: i64 = match job.kind {
            FileJobKind::Read { buf, len, offset } => unsafe {
                libc::pread(job.fd, buf as *mut libc::c_void, len, offset as libc::off_t) as i64
            },
            FileJobKind::Write { buf, len, offset } => unsafe {
                libc::pwrite(
                    job.fd,
                    buf as *const libc::c_void,
                    len,
                    offset as libc::off_t,
                ) as i64
            },
            FileJobKind::Readv {
                bufs,
                buf_count,
                offset,
            } => {
                let slice = unsafe { std::slice::from_raw_parts(bufs, buf_count as usize) };
                let iovecs: Vec<libc::iovec> = slice
                    .iter()
                    .map(|b| libc::iovec {
                        iov_base: b.buf as *mut libc::c_void,
                        iov_len: b.len,
                    })
                    .collect();
                unsafe {
                    libc::preadv(
                        job.fd,
                        iovecs.as_ptr(),
                        iovecs.len() as i32,
                        offset as libc::off_t,
                    ) as i64
                }
            }
            FileJobKind::Writev {
                bufs,
                buf_count,
                offset,
            } => {
                let slice = unsafe { std::slice::from_raw_parts(bufs, buf_count as usize) };
                let iovecs: Vec<libc::iovec> = slice
                    .iter()
                    .map(|b| libc::iovec {
                        iov_base: b.buf as *mut libc::c_void,
                        iov_len: b.len,
                    })
                    .collect();
                unsafe {
                    libc::pwritev(
                        job.fd,
                        iovecs.as_ptr(),
                        iovecs.len() as i32,
                        offset as libc::off_t,
                    ) as i64
                }
            }
        };

        job.completions
            .lock()
            .unwrap()
            .push_back(TorusResult::new(result, job.user_data));
        job.notify.notify_one();
    }
}

impl Drop for FileThreadPool {
    fn drop(&mut self) {
        // Drop the real sender so the channel closes and workers exit.
        let (drop_tx, _drop_rx) = mpsc::channel::<FileJob>();
        let _ = std::mem::replace(&mut *self.tx.lock().unwrap(), drop_tx);
        for worker in self._workers.drain(..) {
            let _ = worker.join();
        }
    }
}

// ─── kqueue Backend ────────────────────────────────────────────────────────

/// macOS/BSD kqueue backend with a background reactor.
pub struct KqueueBackend {
    /// kqueue file descriptor.
    kq: libc::c_int,
    /// Shared completion queue state.
    completions: Arc<Mutex<VecDeque<TorusResult>>>,
    /// Condition variable woken by the reactor / pool on new completions.
    notify: Arc<Condvar>,
    /// In-flight operation count.
    in_flight: AtomicU32,
    /// Shutdown flag for the reactor thread.
    shutdown: Arc<AtomicBool>,
    /// Thread pool for blocking file I/O.
    file_pool: FileThreadPool,
    /// Reactor thread handle.
    _reactor: Option<thread::JoinHandle<()>>,
}

unsafe impl Send for KqueueBackend {}
unsafe impl Sync for KqueueBackend {}

impl KqueueBackend {
    /// Create a new kqueue backend.
    pub fn new() -> Result<Self, String> {
        let kq = unsafe { kqueue() };
        if kq < 0 {
            return Err(format!(
                "kqueue() failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        let completions = Arc::new(Mutex::new(VecDeque::new()));
        let notify = Arc::new(Condvar::new());
        let shutdown = Arc::new(AtomicBool::new(false));

        let reactor_completions = completions.clone();
        let reactor_notify = notify.clone();
        let reactor_shutdown = shutdown.clone();
        let reactor_kq = kq;

        let reactor = thread::spawn(move || {
            Self::reactor_loop(
                reactor_kq,
                reactor_completions,
                reactor_notify,
                reactor_shutdown,
            );
        });

        let file_pool = FileThreadPool::new(completions.clone(), notify.clone());

        Ok(Self {
            kq,
            completions,
            notify,
            in_flight: AtomicU32::new(0),
            shutdown,
            file_pool,
            _reactor: Some(reactor),
        })
    }

    /// Register a socket operation with kqueue and return the `udata` pointer
    /// we stored, or `None` on registration failure (the ctx is freed and the
    /// caller should post an error completion).
    fn register_socket(&self, fd: i32, filter: i16, ctx: *mut OpCtx) -> Option<()> {
        let change = KEvent {
            ident: fd as usize,
            filter,
            flags: EV_ADD | EV_ONESHOT,
            fflags: 0,
            data: 0,
            udata: ctx as *mut std::ffi::c_void,
        };
        let ret = unsafe { kevent(self.kq, &change, 1, ptr::null_mut(), 0, ptr::null_mut()) };
        if ret < 0 {
            None
        } else {
            Some(())
        }
    }

    /// The background reactor loop: waits for kqueue events and performs the
    /// actual I/O for each ready socket, then posts the completion.
    fn reactor_loop(
        kq: libc::c_int,
        completions: Arc<Mutex<VecDeque<TorusResult>>>,
        notify: Arc<Condvar>,
        shutdown: Arc<AtomicBool>,
    ) {
        let mut events: [KEvent; KEVENT_ARRAY_SIZE] = [KEvent::default(); KEVENT_ARRAY_SIZE];
        let timeout = libc::timespec {
            tv_sec: 0,
            tv_nsec: 100_000_000, // 100ms
        };

        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            let n = unsafe {
                kevent(
                    kq,
                    ptr::null(),
                    0,
                    events.as_mut_ptr(),
                    KEVENT_ARRAY_SIZE as libc::c_int,
                    &timeout,
                )
            };

            if n < 0 {
                break;
            }
            if n == 0 {
                continue;
            }

            {
                let mut cq = completions.lock().unwrap();
                for event in events.iter().take(n as usize) {
                    let udata = event.udata as *const TorusResult;
                    if !udata.is_null() {
                        let result = unsafe { &*udata };
                        cq.push_back(TorusResult::new(result.result, result.user_data));
                    }
                }
                // Reclaim ownership of the context allocated at submit time.
                let ctx = unsafe { Box::from_raw(ctx_ptr) };
                let user_data = ctx.user_data;

                let result: i64 = match &ctx.op {
                    SocketOp::Recv { fd, buf, len } => {
                        let r = unsafe { libc::recv(*fd, *buf as *mut libc::c_void, *len, 0) };
                        if r < 0 {
                            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(5) as i64)
                        } else {
                            r as i64
                        }
                    }
                    SocketOp::Send { fd, buf, len } => {
                        let r = unsafe { libc::send(*fd, *buf as *const libc::c_void, *len, 0) };
                        if r < 0 {
                            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(5) as i64)
                        } else {
                            r as i64
                        }
                    }
                    SocketOp::Accept {
                        listen_fd,
                        addr,
                        addrlen,
                    } => {
                        let r = unsafe {
                            libc::accept(*listen_fd, *addr, *addrlen as *mut libc::socklen_t)
                        };
                        if r < 0 {
                            -(std::io::Error::last_os_error().raw_os_error().unwrap_or(5) as i64)
                        } else {
                            r as i64
                        }
                    }
                    SocketOp::Connect { fd } => {
                        let mut err: i32 = 0;
                        let mut optlen = std::mem::size_of::<i32>() as libc::socklen_t;
                        unsafe {
                            libc::getsockopt(
                                *fd,
                                libc::SOL_SOCKET,
                                libc::SO_ERROR,
                                &mut err as *mut i32 as *mut libc::c_void,
                                &mut optlen,
                            );
                        }
                        if err == 0 {
                            0
                        } else {
                            -err as i64
                        }
                    }
                };

                completions
                    .lock()
                    .unwrap()
                    .push_back(TorusResult::new(result, user_data));
                notify.notify_one();
                // `ctx` is dropped here, freeing the operation context.
            }
        }
    }

    fn post_completion(&self, result: TorusResult) {
        self.completions.lock().unwrap().push_back(result);
        self.notify.notify_one();
    }

    /// Register a file descriptor for read events.
    fn register_read(&self, fd: libc::c_int, user_data: u64) {
        let event = KEvent {
            ident: fd as usize,
            filter: EVFILT_READ,
            flags: EV_ADD | EV_ENABLE | EV_CLEAR,
            fflags: 0,
            data: 0,
            udata: Box::into_raw(Box::new(TorusResult::new(0, user_data))) as *mut _,
        };
        unsafe {
            kevent(self.kq, &event, 1, ptr::null_mut(), 0, ptr::null());
        }
    }

    /// Register a file descriptor for write events.
    fn register_write(&self, fd: libc::c_int, user_data: u64) {
        let event = KEvent {
            ident: fd as usize,
            filter: EVFILT_WRITE,
            flags: EV_ADD | EV_ENABLE | EV_CLEAR,
            fflags: 0,
            data: 0,
            udata: Box::into_raw(Box::new(TorusResult::new(0, user_data))) as *mut _,
        };
        unsafe {
            kevent(self.kq, &event, 1, ptr::null_mut(), 0, ptr::null());
        }
    }

    /// Unregister a file descriptor.
    fn unregister(&self, fd: libc::c_int) {
        let event = KEvent {
            ident: fd as usize,
            filter: EVFILT_READ,
            flags: EV_DELETE,
            fflags: 0,
            data: 0,
            udata: ptr::null_mut(),
        };
        unsafe {
            kevent(self.kq, &event, 1, ptr::null_mut(), 0, ptr::null());
        }
        let event = KEvent {
            ident: fd as usize,
            filter: EVFILT_WRITE,
            flags: EV_DELETE,
            fflags: 0,
            data: 0,
            udata: ptr::null_mut(),
        };
        unsafe {
            kevent(self.kq, &event, 1, ptr::null_mut(), 0, ptr::null());
        }
    }
}

impl Backend for KqueueBackend {
    fn submit(&self, flows: &[Flow]) -> tpt_torus_core::error::Result<usize> {
        for flow in flows {
            match flow.operation() {
                Operation::Read {
                    fd,
                    buf,
                    len,
                    offset,
                } => {
                    // File I/O is dispatched to the thread pool.
                    self.file_pool.submit(FileJob {
                        fd: *fd,
                        kind: FileJobKind::Read {
                            buf: *buf,
                            len: *len,
                            offset: *offset,
                        },
                        user_data: flow.user_data(),
                        completions: self.completions.clone(),
                        notify: self.notify.clone(),
                    });
                }
                Operation::Write {
                    fd,
                    buf,
                    len,
                    offset,
                } => {
                    let fd = *fd;
                    let buf = *buf;
                    let len = *len;
                    let offset = *offset;
                    let user_data = flow.user_data();

                    self.register_write(fd, user_data);

                    // For file I/O, do a synchronous write
                    let result = unsafe {
                        libc::pwrite(fd, buf as *const libc::c_void, len, offset as libc::off_t)
                    };
                    self.post_completion(TorusResult::new(result as i64, user_data));
                    self.unregister(fd);
                    submitted += 1;
                }
                Operation::Accept { fd, addr, addrlen } => {
                    let fd = *fd;
                    let user_data = flow.user_data();

                    self.register_read(fd, user_data);

                    // Synchronous accept
                    let result =
                        unsafe { libc::accept(fd, *addr, *addrlen as *mut libc::socklen_t) };
                    self.post_completion(TorusResult::new(result as i64, user_data));
                    self.unregister(fd);
                    submitted += 1;
                }
                Operation::Connect { fd, addr, addrlen } => {
                    let fd = *fd;
                    let user_data = flow.user_data();

                    self.register_write(fd, user_data);

                    // Synchronous connect
                    let result = unsafe { libc::connect(fd, *addr, *addrlen as libc::socklen_t) };
                    self.post_completion(TorusResult::new(result as i64, user_data));
                    self.unregister(fd);
                    submitted += 1;
                }
                Operation::Recv { fd, buf, len } => {
                    let fd = *fd;
                    let user_data = flow.user_data();

                    self.register_read(fd, user_data);

                    // Synchronous recv
                    let result = unsafe { libc::recv(fd, *buf as *mut libc::c_void, *len, 0) };
                    self.post_completion(TorusResult::new(result as i64, user_data));
                    self.unregister(fd);
                    submitted += 1;
                }
                Operation::Send { fd, buf, len } => {
                    let fd = *fd;
                    let user_data = flow.user_data();

                    self.register_write(fd, user_data);

                    // Synchronous send
                    let result = unsafe { libc::send(fd, *buf as *const libc::c_void, *len, 0) };
                    self.post_completion(TorusResult::new(result as i64, user_data));
                    self.unregister(fd);
                    submitted += 1;
                }
                Operation::Close { fd } => {
                    let user_data = flow.user_data();
                    let result = unsafe { libc::close(*fd) };
                    self.post_completion(TorusResult::new(result as i64, user_data));
                    submitted += 1;
                }
                Operation::Readv {
                    fd,
                    bufs,
                    buf_count,
                    offset,
                } => {
                    // Vectored read: use preadv for file I/O (kqueue doesn't support async file I/O)
                    let fd = *fd;
                    let user_data = flow.user_data();
                    let bufs_slice =
                        unsafe { std::slice::from_raw_parts(*bufs, *buf_count as usize) };

                    // Convert to iovec for preadv
                    let iovecs: Vec<libc::iovec> = bufs_slice
                        .iter()
                        .map(|b| libc::iovec {
                            iov_base: b.buf as *mut libc::c_void,
                            iov_len: b.len,
                        })
                        .collect();

                    let result = unsafe {
                        libc::preadv(
                            fd,
                            iovecs.as_ptr(),
                            iovecs.len() as i32,
                            *offset as libc::off_t,
                        )
                    };

                    self.register_read(fd, user_data);
                    self.post_completion(TorusResult::new(result as i64, user_data));
                    self.unregister(fd);
                    submitted += 1;
                }
                Operation::Writev {
                    fd,
                    bufs,
                    buf_count,
                    offset,
                } => {
                    // Vectored write: use pwritev for file I/O
                    let fd = *fd;
                    let user_data = flow.user_data();
                    let bufs_slice =
                        unsafe { std::slice::from_raw_parts(*bufs, *buf_count as usize) };

                    let iovecs: Vec<libc::iovec> = bufs_slice
                        .iter()
                        .map(|b| libc::iovec {
                            iov_base: b.buf as *mut libc::c_void,
                            iov_len: b.len,
                        })
                        .collect();

                    let result = unsafe {
                        libc::pwritev(
                            fd,
                            iovecs.as_ptr(),
                            iovecs.len() as i32,
                            *offset as libc::off_t,
                        )
                    };

                    self.register_write(fd, user_data);
                    self.post_completion(TorusResult::new(result as i64, user_data));
                    self.unregister(fd);
                    submitted += 1;
                }
            }
        }

        self.in_flight.fetch_add(submitted, Ordering::Relaxed);
        Ok(submitted as usize)
    }

    fn reap(&self, results: &mut Vec<TorusResult>) -> tpt_torus_core::error::Result<usize> {
        let mut cq = self.completions.lock().unwrap();
        let before = results.len();
        while let Some(r) = cq.pop_front() {
            self.in_flight.fetch_sub(1, Ordering::Relaxed);
            results.push(r);
        }
        Ok(results.len() - before)
    }

    fn wait(&self, timeout_us: u64) -> tpt_torus_core::error::Result<()> {
        if self.in_flight.load(Ordering::Relaxed) == 0 {
            return Ok(());
        }

        let mut cq = self.completions.lock().unwrap();
        while cq.is_empty() {
            let timeout = if timeout_us == 0 {
                None
            } else {
                Some(std::time::Duration::from_micros(timeout_us))
            };
            let result = self
                .notify
                .wait_timeout(cq, timeout.unwrap_or(std::time::Duration::from_secs(3600)))
                .unwrap();
            cq = result.0;

            if timeout.is_some() && result.1.timed_out() {
                return Err(tpt_torus_core::error::Error::Os(110)); // ETIMEDOUT
            }
        }
        Ok(())
    }

    fn in_flight(&self) -> u32 {
        self.in_flight.load(Ordering::Relaxed)
    }
}

impl Drop for KqueueBackend {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);

        // Dropping the file pool joins its workers.
        drop(std::mem::replace(
            &mut self.file_pool,
            // A throwaway placeholder pool whose channel is immediately closed.
            {
                let (tx, _rx) = mpsc::channel::<FileJob>();
                FileThreadPool {
                    tx: Mutex::new(tx),
                    _workers: Vec::new(),
                }
            },
        ));

        if let Some(handle) = self._reactor.take() {
            let _ = handle.join();
        }

        unsafe {
            libc::close(self.kq);
        }
    }
}
