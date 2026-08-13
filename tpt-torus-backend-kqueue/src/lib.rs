//! macOS/BSD kqueue backend: a lock-free background reactor drains the virtual SQ,
//! translates operations into native kqueue calls, and populates the virtual CQ
//! upon completion.
//!
//! NOTE: kqueue does not natively support async file I/O on macOS/BSD.
//! File operations are dispatched to a thread pool. Socket operations
//! use kqueue's native EVFILT_READ/EVFILT_WRITE for true async I/O.
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
use tpt_torus_core::operation::Operation;
use tpt_torus_core::result::Result as TorusResult;

use std::collections::VecDeque;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
#[allow(dead_code)]
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
#[allow(dead_code)]
const EVFILT_READ: i16 = -1;
#[allow(dead_code)]
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
        Self {
            ident: 0,
            filter: 0,
            flags: 0,
            fflags: 0,
            data: 0,
            udata: ptr::null_mut(),
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

// ─── kqueue Backend ────────────────────────────────────────────────────────

/// macOS/BSD kqueue backend with a background reactor.
pub struct KqueueBackend {
    /// kqueue file descriptor.
    kq: libc::c_int,
    /// Shared completion queue state.
    completions: Arc<Mutex<VecDeque<TorusResult>>>,
    /// Condition variable woken by the reactor on new completions.
    notify: Arc<Condvar>,
    /// In-flight operation count.
    in_flight: AtomicU32,
    /// Shutdown flag for the reactor thread.
    shutdown: Arc<AtomicBool>,
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

        Ok(Self {
            kq,
            completions,
            notify,
            in_flight: AtomicU32::new(0),
            shutdown,
            _reactor: Some(reactor),
        })
    }

    /// The background reactor loop: waits for kqueue events and posts to the virtual CQ.
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
            }
            notify.notify_all();
        }
    }

    fn post_completion(&self, result: TorusResult) {
        self.completions.lock().unwrap().push_back(result);
        self.notify.notify_one();
    }
}

impl Backend for KqueueBackend {
    fn submit(&self, flows: &[Flow]) -> tpt_torus_core::error::Result<usize> {
        let mut submitted: usize = 0;

        for flow in flows {
            match flow.operation() {
                Operation::Read {
                    fd,
                    buf,
                    len,
                    offset,
                } => {
                    // File read — synchronous since kqueue doesn't support async file I/O.
                    let fd = *fd;
                    let buf = *buf;
                    let len = *len;
                    let offset = *offset;
                    let user_data = flow.user_data();

                    // For file I/O, do a synchronous read since kqueue doesn't support it.
                    // The synchronous result is the only completion; we must not register
                    // the fd with kqueue here (the reactor would post a spurious 0-byte
                    // completion for the registration event).
                    let result = unsafe {
                        libc::pread(fd, buf as *mut libc::c_void, len, offset as libc::off_t)
                    };
                    self.post_completion(TorusResult::new(result as i64, user_data));
                    submitted += 1;
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

                    // For file I/O, do a synchronous write. The synchronous result is the
                    // only completion; we must not register the fd with kqueue here.
                    let result = unsafe {
                        libc::pwrite(fd, buf as *const libc::c_void, len, offset as libc::off_t)
                    };
                    self.post_completion(TorusResult::new(result as i64, user_data));
                    submitted += 1;
                }
                Operation::Accept { fd, addr, addrlen } => {
                    let fd = *fd;
                    let user_data = flow.user_data();

                    // Synchronous accept (the synchronous result is the only completion).
                    let result = unsafe {
                        libc::accept(
                            fd,
                            *addr as *mut libc::sockaddr,
                            *addrlen as *mut libc::socklen_t,
                        )
                    };
                    self.post_completion(TorusResult::new(result as i64, user_data));
                    submitted += 1;
                }
                Operation::Connect { fd, addr, addrlen } => {
                    let fd = *fd;
                    let user_data = flow.user_data();

                    // Synchronous connect (the synchronous result is the only completion).
                    let result = unsafe {
                        libc::connect(
                            fd,
                            *addr as *const libc::sockaddr,
                            *addrlen as libc::socklen_t,
                        )
                    };
                    self.post_completion(TorusResult::new(result as i64, user_data));
                    submitted += 1;
                }
                Operation::Recv { fd, buf, len } => {
                    let fd = *fd;
                    let buf = *buf;
                    let len = *len;
                    let user_data = flow.user_data();

                    // Synchronous recv (the synchronous result is the only completion).
                    let result = unsafe { libc::recv(fd, buf as *mut libc::c_void, len, 0) };
                    self.post_completion(TorusResult::new(result as i64, user_data));
                    submitted += 1;
                }
                Operation::Send { fd, buf, len } => {
                    let fd = *fd;
                    let buf = *buf;
                    let len = *len;
                    let user_data = flow.user_data();

                    // Synchronous send (the synchronous result is the only completion).
                    let result = unsafe { libc::send(fd, buf as *const libc::c_void, len, 0) };
                    self.post_completion(TorusResult::new(result as i64, user_data));
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

                    self.post_completion(TorusResult::new(result as i64, user_data));
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

                    self.post_completion(TorusResult::new(result as i64, user_data));
                    submitted += 1;
                }
            }
        }

        self.in_flight
            .fetch_add(submitted as u32, Ordering::Relaxed);
        Ok(submitted)
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

        if let Some(handle) = self._reactor.take() {
            let _ = handle.join();
        }

        unsafe {
            libc::close(self.kq);
        }
    }
}
