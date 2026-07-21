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

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::IO::{
    CancelIoEx, CreateIoCompletionPort, GetQueuedCompletionStatusEx, OVERLAPPED,
    OVERLAPPED_ENTRY, PostQueuedCompletionStatus,
};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::Networking::Sockets::{
    closesocket, recv, send, SOCKET, WSASend, WSARecv, WSADATA, WSAStartup,
};

// ─── Constants ─────────────────────────────────────────────────────────────

const MAX_COMPLETIONS: u32 = 256;
const CP_THREADS: u32 = 1;
const STATUS_BUFFER: u64 = 0x80000000; // Custom status to distinguish completions

// ─── Overlapped wrapper ────────────────────────────────────────────────────

#[repr(C)]
struct TorusOverlapped {
    overlapped: OVERLAPPED,
    user_data: u64,
    op_tag: u32,
}

unsafe impl Send for TorusOverlapped {}
unsafe impl Sync for TorusOverlapped {}

// ─── IOCP Backend ──────────────────────────────────────────────────────────

/// Windows IOCP backend with a background reactor.
pub struct IocpBackend {
    /// IOCP port handle.
    iocp: HANDLE,
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
        // Initialize Winsock
        let mut wsa: WSADATA = unsafe { std::mem::zeroed() };
        let err = unsafe { WSAStartup(0x0202, &mut wsa) };
        if err != 0 {
            return Err(format!("WSAStartup failed: {}", err));
        }

        // Create IOCP
        let iocp = unsafe {
            CreateIoCompletionPort(INVALID_HANDLE_VALUE, 0, 0, CP_THREADS)
        };
        if iocp == 0 {
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
        let reactor_iocp = iocp;

        let reactor = thread::spawn(move || {
            Self::reactor_loop(reactor_iocp, reactor_completions, reactor_notify, reactor_shutdown);
        });

        Ok(Self {
            iocp,
            completions,
            notify,
            in_flight: AtomicU32::new(0),
            shutdown,
            _reactor: Some(reactor),
        })
    }

    /// The background reactor loop: waits for IOCP completions and posts to the virtual CQ.
    fn reactor_loop(
        iocp: HANDLE,
        completions: Arc<Mutex<VecDeque<TorusResult>>>,
        notify: Arc<Condvar>,
        shutdown: Arc<AtomicBool>,
    ) {
        let mut entries: [OVERLAPPED_ENTRY; MAX_COMPLETIONS as usize] =
            unsafe { std::mem::zeroed() };
        let mut timeout_ms: u32 = 100; // Wake periodically to check shutdown

        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            let mut num_entries = 0u32;
            let ret = unsafe {
                GetQueuedCompletionStatusEx(
                    iocp,
                    entries.as_mut_ptr(),
                    MAX_COMPLETIONS,
                    &mut num_entries,
                    timeout_ms,
                    0, // alertable
                )
            };

            if ret == 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(258) {
                    // WAIT_TIMEOUT — loop back to check shutdown
                    continue;
                }
                // Real error — break
                break;
            }

            if num_entries == 0 {
                continue;
            }

            // Process completions
            {
                let mut cq = completions.lock().unwrap();
                for i in 0..num_entries as usize {
                    let entry = &entries[i];
                    let overlapped = entry.lpOverlapped as *const TorusOverlapped;
                    if overlapped.is_null() {
                        continue;
                    }

                    let torus_ovl = unsafe { &*overlapped };
                    let bytes = entry.dwNumberOfBytesTransferred;
                    let user_data = torus_ovl.user_data;

                    let result = if bytes == 0 && torus_ovl.op_tag != 0 {
                        // Error or special status
                        TorusResult::new(-std::io::Error::last_os_error().raw_os_error().unwrap_or(5) as i64, user_data)
                    } else {
                        TorusResult::new(bytes as i64, user_data)
                    };

                    cq.push_back(result);
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

impl Backend for IocpBackend {
    fn submit(&self, flows: &[Flow]) -> tpt_torus_core::error::Result<usize> {
        let mut submitted = 0;

        for flow in flows {
            match flow.operation() {
                Operation::Read { fd, buf, len, offset } => {
                    let handle = *fd as HANDLE;
                    let mut overlapped: TorusOverlapped = unsafe { std::mem::zeroed() };
                    overlapped.user_data = flow.user_data();
                    overlapped.op_tag = 1;

                    unsafe {
                        (*overlapped.overlapped.Anonymous.Anonymous.OffsetHigh as *mut u32)
                            .write((*offset >> 32) as u32);
                        (*overlapped.overlapped.Anonymous.Anonymous.Offset as *mut u32)
                            .write(*offset as u32);
                    }

                    let ret = unsafe {
                        ReadFile(
                            handle,
                            *buf as *const _,
                            *len as u32,
                            ptr::null_mut(),
                            &mut overlapped.overlapped,
                        )
                    };

                    if ret == 0 {
                        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(5);
                        self.post_completion(TorusResult::new(-err as i64, flow.user_data()));
                    } else {
                        // Operation submitted — overlapped will complete asynchronously
                    }
                    submitted += 1;
                }
                Operation::Write { fd, buf, len, offset } => {
                    let handle = *fd as HANDLE;
                    let mut overlapped: TorusOverlapped = unsafe { std::mem::zeroed() };
                    overlapped.user_data = flow.user_data();
                    overlapped.op_tag = 2;

                    unsafe {
                        (*overlapped.overlapped.Anonymous.Anonymous.OffsetHigh as *mut u32)
                            .write((*offset >> 32) as u32);
                        (*overlapped.overlapped.Anonymous.Anonymous.Offset as *mut u32)
                            .write(*offset as u32);
                    }

                    let ret = unsafe {
                        WriteFile(
                            handle,
                            *buf as *const _,
                            *len as u32,
                            ptr::null_mut(),
                            &mut overlapped.overlapped,
                        )
                    };

                    if ret == 0 {
                        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(5);
                        self.post_completion(TorusResult::new(-err as i64, flow.user_data()));
                    }
                    submitted += 1;
                }
                Operation::Recv { fd, buf, len } => {
                    let socket = *fd as SOCKET;
                    let mut overlapped: TorusOverlapped = unsafe { std::mem::zeroed() };
                    overlapped.user_data = flow.user_data();
                    overlapped.op_tag = 3;

                    let mut flags: u32 = 0;
                    let mut bytes_read: u32 = 0;

                    let ret = unsafe {
                        WSARecv(
                            socket,
                            [windows_sys::Win32::Networking::Sockets::WSABUF {
                                len: *len as u32,
                                buf: *buf as *mut _,
                            }]
                            .as_ptr(),
                            1,
                            &mut bytes_read,
                            &mut flags,
                            &mut overlapped.overlapped,
                            None,
                        )
                    };

                    if ret != 0 {
                        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(5);
                        self.post_completion(TorusResult::new(-err as i64, flow.user_data()));
                    }
                    submitted += 1;
                }
                Operation::Send { fd, buf, len } => {
                    let socket = *fd as SOCKET;
                    let mut overlapped: TorusOverlapped = unsafe { std::mem::zeroed() };
                    overlapped.user_data = flow.user_data();
                    overlapped.op_tag = 4;

                    let mut bytes_sent: u32 = 0;

                    let ret = unsafe {
                        WSASend(
                            socket,
                            [windows_sys::Win32::Networking::Sockets::WSABUF {
                                len: *len as u32,
                                buf: *buf as *const _ as *mut _,
                            }]
                            .as_ptr(),
                            1,
                            &mut bytes_sent,
                            0,
                            &mut overlapped.overlapped,
                            None,
                        )
                    };

                    if ret != 0 {
                        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(5);
                        self.post_completion(TorusResult::new(-err as i64, flow.user_data()));
                    }
                    submitted += 1;
                }
                Operation::Accept { fd, .. } => {
                    // Accept on Windows uses AcceptEx — simplified here
                    let _ = fd;
                    submitted += 1;
                }
                Operation::Connect { fd, .. } => {
                    // Connect on Windows uses ConnectEx — simplified here
                    let _ = fd;
                    submitted += 1;
                }
                Operation::Close { fd } => {
                    // Close is synchronous
                    let handle = *fd as HANDLE;
                    if *fd as SOCKET != INVALID_SOCKET as _ {
                        unsafe { closesocket(*fd as SOCKET) };
                    } else {
                        unsafe { CloseHandle(handle) };
                    }
                    self.post_completion(TorusResult::new(0, flow.user_data()));
                    submitted += 1;
                }
            }
        }

        self.in_flight.fetch_add(submitted, Ordering::Relaxed);
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
            let result = self.notify.wait_timeout(cq, timeout.unwrap_or(std::time::Duration::from_secs(3600))).unwrap();
            cq = result.0;

            if timeout.is_some() && result.1.timed_out() {
                return Err(tpt_torus_core::error::Error::Os(258)); // WAIT_TIMEOUT
            }
        }
        Ok(())
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
            PostQueuedCompletionStatus(self.iocp, 0, 0, ptr::null());
        }

        if let Some(handle) = self._reactor.take() {
            let _ = handle.join();
        }

        unsafe {
            CloseHandle(self.iocp);
        }
    }
}
