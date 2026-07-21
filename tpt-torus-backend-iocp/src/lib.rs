//! Windows IOCP backend: a lock-free background reactor drains the virtual SQ,
//! translates operations into native IOCP calls, and populates the virtual CQ
//! upon completion.
#![cfg(windows)]

use tpt_torus_core::backend::Backend;
use tpt_torus_core::flow::Flow;
use tpt_torus_core::operation::Operation;
use tpt_torus_core::result::Result as TorusResult;

use std::collections::VecDeque;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Networking::WinSock::{
    closesocket, WSARecv, WSASend, INVALID_SOCKET, SOCKET, WSABUF,
};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatusEx, PostQueuedCompletionStatus, OVERLAPPED,
    OVERLAPPED_ENTRY,
};

// ─── Constants ─────────────────────────────────────────────────────────────

const MAX_COMPLETIONS: u32 = 256;
const CP_THREADS: u32 = 1;

// ─── Overlapped wrapper ────────────────────────────────────────────────────

/// Custom overlapped structure that carries Torus user data.
///
/// SAFETY: This struct must be placed in a boxed allocation that outlives
/// the I/O operation. The reactor thread reads `user_data` after the kernel
/// signals completion.
#[repr(C)]
struct TorusOverlapped {
    overlapped: OVERLAPPED,
    user_data: u64,
}

unsafe impl Send for TorusOverlapped {}
unsafe impl Sync for TorusOverlapped {}

// ─── Safe handle wrapper for thread safety ─────────────────────────────────

/// Wrapper around a raw HANDLE that implements Send/Sync.
/// SAFETY: The IOCP handle is thread-safe for the operations we use.
struct SafeHandle(HANDLE);
unsafe impl Send for SafeHandle {}
unsafe impl Sync for SafeHandle {}

// ─── IOCP Backend ──────────────────────────────────────────────────────────

/// Windows IOCP backend with a background reactor.
pub struct IocpBackend {
    /// IOCP port handle.
    iocp: SafeHandle,
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

unsafe impl Send for IocpBackend {}
unsafe impl Sync for IocpBackend {}

impl IocpBackend {
    /// Create a new IOCP backend.
    pub fn new() -> Result<Self, String> {
        // Create IOCP with 1 worker thread (we'll do completions in the reactor)
        let iocp =
            unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, ptr::null_mut(), 0, CP_THREADS) };
        if iocp.is_null() {
            return Err(format!(
                "CreateIoCompletionPort failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        let completions = Arc::new(Mutex::new(VecDeque::new()));
        let notify = Arc::new(Condvar::new());
        let shutdown = Arc::new(AtomicBool::new(false));

        let reactor_completions = completions.clone();
        let reactor_notify = notify.clone();
        let reactor_shutdown = shutdown.clone();
        let reactor_iocp = SafeHandle(iocp);

        let reactor = thread::spawn(move || {
            Self::reactor_loop(
                reactor_iocp,
                reactor_completions,
                reactor_notify,
                reactor_shutdown,
            );
        });

        Ok(Self {
            iocp: SafeHandle(iocp),
            completions,
            notify,
            in_flight: AtomicU32::new(0),
            shutdown,
            _reactor: Some(reactor),
        })
    }

    /// # Safety
    ///
    /// `handle` must be a valid Win32 HANDLE.
    pub unsafe fn associate(&self, handle: HANDLE) -> bool {
        !CreateIoCompletionPort(handle, self.iocp.0, 0, CP_THREADS).is_null()
    }

    /// The background reactor loop: waits for IOCP completions and posts to the virtual CQ.
    fn reactor_loop(
        iocp: SafeHandle,
        completions: Arc<Mutex<VecDeque<TorusResult>>>,
        notify: Arc<Condvar>,
        shutdown: Arc<AtomicBool>,
    ) {
        let mut entries: [OVERLAPPED_ENTRY; MAX_COMPLETIONS as usize] =
            unsafe { std::mem::zeroed() };

        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            let mut num_entries = 0u32;
            let ret = unsafe {
                GetQueuedCompletionStatusEx(
                    iocp.0,
                    entries.as_mut_ptr(),
                    MAX_COMPLETIONS,
                    &mut num_entries,
                    100, // 100ms timeout to check shutdown flag
                    0,
                )
            };

            if ret == 0 {
                // Could be timeout (ERROR_TIMEOUT = 258) or real error
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(258) {
                    continue; // Timeout — check shutdown
                }
                break; // Real error
            }

            if num_entries == 0 {
                continue;
            }

            // Process completions
            {
                let mut cq = completions.lock().unwrap();
                for entry in entries.iter().take(num_entries as usize) {
                    let overlapped = entry.lpOverlapped as *const TorusOverlapped;
                    if overlapped.is_null() {
                        continue;
                    }

                    let torus_ovl = unsafe { &*overlapped };
                    let bytes = entry.dwNumberOfBytesTransferred;
                    let user_data = torus_ovl.user_data;

                    // Deallocate the overlapped box
                    unsafe {
                        drop(Box::from_raw(overlapped as *mut TorusOverlapped));
                    }

                    let result = if bytes == 0 {
                        TorusResult::new(
                            -std::io::Error::last_os_error().raw_os_error().unwrap_or(5) as i64,
                            user_data,
                        )
                    } else {
                        TorusResult::new(bytes as i64, user_data)
                    };

                    cq.push_back(result);
                }
            }
            notify.notify_all();
        }
    }
}

impl Backend for IocpBackend {
    fn submit(&self, flows: &[Flow]) -> tpt_torus_core::error::Result<usize> {
        let mut submitted = 0usize;

        for flow in flows {
            match flow.operation() {
                Operation::Read {
                    fd,
                    buf,
                    len,
                    offset,
                } => {
                    let handle = *fd as HANDLE;

                    let ovl = Box::new(TorusOverlapped {
                        overlapped: unsafe { std::mem::zeroed() },
                        user_data: flow.user_data(),
                    });
                    let ovl_ptr = Box::into_raw(ovl) as *mut OVERLAPPED;

                    unsafe {
                        let ovl_ref = &mut *ovl_ptr;
                        // Set the offset in the overlapped structure
                        let off = *offset;
                        let low = off as u32;
                        let high = (off >> 32) as u32;
                        ovl_ref.Anonymous.Anonymous.Offset = low;
                        ovl_ref.Anonymous.Anonymous.OffsetHigh = high;
                    }

                    let ret =
                        unsafe { ReadFile(handle, *buf, *len as u32, ptr::null_mut(), ovl_ptr) };

                    if ret == 0 {
                        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(5);
                        // Deallocate the overlapped since it won't complete
                        unsafe {
                            drop(Box::from_raw(ovl_ptr));
                        }
                        self.completions
                            .lock()
                            .unwrap()
                            .push_back(TorusResult::new(-err as i64, flow.user_data()));
                        self.notify.notify_one();
                    }
                    submitted += 1;
                }
                Operation::Write {
                    fd,
                    buf,
                    len,
                    offset,
                } => {
                    let handle = *fd as HANDLE;

                    let ovl = Box::new(TorusOverlapped {
                        overlapped: unsafe { std::mem::zeroed() },
                        user_data: flow.user_data(),
                    });
                    let ovl_ptr = Box::into_raw(ovl) as *mut OVERLAPPED;

                    unsafe {
                        let ovl_ref = &mut *ovl_ptr;
                        let off = *offset;
                        ovl_ref.Anonymous.Anonymous.Offset = off as u32;
                        ovl_ref.Anonymous.Anonymous.OffsetHigh = (off >> 32) as u32;
                    }

                    let ret =
                        unsafe { WriteFile(handle, *buf, *len as u32, ptr::null_mut(), ovl_ptr) };

                    if ret == 0 {
                        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(5);
                        unsafe {
                            drop(Box::from_raw(ovl_ptr));
                        }
                        self.completions
                            .lock()
                            .unwrap()
                            .push_back(TorusResult::new(-err as i64, flow.user_data()));
                        self.notify.notify_one();
                    }
                    submitted += 1;
                }
                Operation::Recv { fd, buf, len } => {
                    let socket = *fd as SOCKET;

                    let ovl = Box::new(TorusOverlapped {
                        overlapped: unsafe { std::mem::zeroed() },
                        user_data: flow.user_data(),
                    });
                    let ovl_ptr = Box::into_raw(ovl);

                    let mut flags: u32 = 0;
                    let mut bytes_read: u32 = 0;
                    let wsa_buf = WSABUF {
                        len: *len as u32,
                        buf: *buf,
                    };

                    let ret = unsafe {
                        WSARecv(
                            socket,
                            &wsa_buf,
                            1,
                            &mut bytes_read,
                            &mut flags,
                            &mut (*ovl_ptr).overlapped,
                            None,
                        )
                    };

                    if ret != 0 {
                        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(5);
                        unsafe {
                            drop(Box::from_raw(ovl_ptr));
                        }
                        self.completions
                            .lock()
                            .unwrap()
                            .push_back(TorusResult::new(-err as i64, flow.user_data()));
                        self.notify.notify_one();
                    }
                    submitted += 1;
                }
                Operation::Send { fd, buf, len } => {
                    let socket = *fd as SOCKET;

                    let ovl = Box::new(TorusOverlapped {
                        overlapped: unsafe { std::mem::zeroed() },
                        user_data: flow.user_data(),
                    });
                    let ovl_ptr = Box::into_raw(ovl);

                    let mut bytes_sent: u32 = 0;
                    let wsa_buf = WSABUF {
                        len: *len as u32,
                        buf: *buf as *mut u8,
                    };

                    let ret = unsafe {
                        WSASend(
                            socket,
                            &wsa_buf,
                            1,
                            &mut bytes_sent,
                            0,
                            &mut (*ovl_ptr).overlapped,
                            None,
                        )
                    };

                    if ret != 0 {
                        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(5);
                        unsafe {
                            drop(Box::from_raw(ovl_ptr));
                        }
                        self.completions
                            .lock()
                            .unwrap()
                            .push_back(TorusResult::new(-err as i64, flow.user_data()));
                        self.notify.notify_one();
                    }
                    submitted += 1;
                }
                Operation::Accept { .. } => {
                    // Accept on Windows requires AcceptEx — simplified synchronous fallback
                    let user_data = flow.user_data();
                    self.completions.lock().unwrap().push_back(
                        TorusResult::new(-120, user_data), // ENOSYS — not yet implemented
                    );
                    self.notify.notify_one();
                    submitted += 1;
                }
                Operation::Connect { .. } => {
                    // Connect on Windows requires ConnectEx — simplified synchronous fallback
                    let user_data = flow.user_data();
                    self.completions.lock().unwrap().push_back(
                        TorusResult::new(-120, user_data), // ENOSYS — not yet implemented
                    );
                    self.notify.notify_one();
                    submitted += 1;
                }
                Operation::Close { fd } => {
                    let handle = *fd as HANDLE;
                    if *fd as SOCKET == INVALID_SOCKET {
                        unsafe {
                            CloseHandle(handle);
                        }
                    } else {
                        unsafe {
                            closesocket(*fd as SOCKET);
                        }
                    }
                    self.completions
                        .lock()
                        .unwrap()
                        .push_back(TorusResult::new(0, flow.user_data()));
                    self.notify.notify_one();
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

        let deadline = std::time::Instant::now()
            .checked_add(std::time::Duration::from_micros(timeout_us))
            .unwrap_or_else(std::time::Instant::now);

        loop {
            {
                let cq = self.completions.lock().unwrap();
                if !cq.is_empty() {
                    return Ok(());
                }
            }

            if std::time::Instant::now() >= deadline {
                return Ok(());
            }

            std::thread::yield_now();
        }
    }

    fn in_flight(&self) -> u32 {
        self.in_flight.load(Ordering::Relaxed)
    }
}

impl Drop for IocpBackend {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);

        // Post a dummy completion to wake the reactor
        unsafe {
            PostQueuedCompletionStatus(self.iocp.0, 0, 0, ptr::null_mut());
        }

        if let Some(handle) = self._reactor.take() {
            let _ = handle.join();
        }

        unsafe {
            CloseHandle(self.iocp.0);
        }
    }
}
