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
    closesocket, getsockopt, setsockopt, AcceptEx, GetAcceptExSockaddrs, WSAGetLastError, WSAIoctl,
    WSARecv, WSASend, WSASocketW, FROM_PROTOCOL_INFO, INVALID_SOCKET, LPFN_CONNECTEX,
    SIO_GET_EXTENSION_FUNCTION_POINTER, SOCKADDR_STORAGE, SOCKET, SOL_SOCKET, SO_PROTOCOL_INFOW,
    SO_UPDATE_ACCEPT_CONTEXT, SO_UPDATE_CONNECT_CONTEXT, WSABUF, WSAID_CONNECTEX,
    WSAPROTOCOL_INFOW, WSA_FLAG_OVERLAPPED,
};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::IO::{
    CreateIoCompletionPort, GetOverlappedResult, GetQueuedCompletionStatusEx,
    PostQueuedCompletionStatus, OVERLAPPED, OVERLAPPED_ENTRY,
};

// ─── Constants ─────────────────────────────────────────────────────────────

const MAX_COMPLETIONS: u32 = 256;
const CP_THREADS: u32 = 1;
const ERROR_IO_PENDING: i32 = 997;

/// Size of each address slot AcceptEx requires in its output buffer: the
/// largest possible socket address plus 16 bytes of required padding.
const ACCEPT_ADDR_LEN: u32 = std::mem::size_of::<SOCKADDR_STORAGE>() as u32 + 16;

// ─── Overlapped wrapper ────────────────────────────────────────────────────

/// Per-operation state attached to the raw `OVERLAPPED` so the reactor thread
/// can finish operations that need more than "bytes transferred" on completion.
enum OverlappedKind {
    /// Read/Write/Recv/Send: the CQE result is just the byte count.
    Generic,
    /// AcceptEx: needs the listening socket (to check completion status and
    /// hand off SO_UPDATE_ACCEPT_CONTEXT), the pre-created accept socket
    /// (becomes the new connection's fd), the AcceptEx output address buffer,
    /// and the caller's `addr`/`addrlen` out-parameters to fill in.
    Accept {
        listen_socket: SOCKET,
        accept_socket: SOCKET,
        addr_buf: Box<[u8]>,
        out_addr: AcceptOutAddr,
    },
    /// ConnectEx: needs the connecting socket to check completion status and
    /// apply SO_UPDATE_CONNECT_CONTEXT.
    Connect { socket: SOCKET },
}

/// The caller-supplied `addr`/`addrlen` out-parameters from `Operation::Accept`,
/// bundled so they can be threaded through as a single argument.
struct AcceptOutAddr {
    addr: *mut libc::sockaddr,
    addrlen: *mut u32,
    capacity: u32,
}

unsafe impl Send for AcceptOutAddr {}
unsafe impl Sync for AcceptOutAddr {}

/// Custom overlapped structure that carries Torus user data.
///
/// SAFETY: This struct must be placed in a boxed allocation that outlives
/// the I/O operation. The reactor thread reads `user_data` after the kernel
/// signals completion.
#[repr(C)]
struct TorusOverlapped {
    overlapped: OVERLAPPED,
    user_data: u64,
    kind: OverlappedKind,
}

unsafe impl Send for OverlappedKind {}
unsafe impl Sync for OverlappedKind {}

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

    /// Start an `AcceptEx` operation. On any failure before the operation is
    /// successfully queued to the completion port, returns the Win32/Winsock
    /// error code to report as an immediate synchronous completion.
    fn begin_accept(
        &self,
        listen_socket: SOCKET,
        caller_addr: *mut libc::sockaddr,
        caller_addrlen: *mut u32,
        caller_addr_capacity: u32,
        user_data: u64,
    ) -> std::result::Result<(), i32> {
        unsafe {
            // AcceptEx requires a pre-created socket of the same protocol as
            // the listener; discover that protocol via SO_PROTOCOL_INFOW.
            let mut proto_info: WSAPROTOCOL_INFOW = std::mem::zeroed();
            let mut proto_info_len = std::mem::size_of::<WSAPROTOCOL_INFOW>() as i32;
            if getsockopt(
                listen_socket,
                SOL_SOCKET,
                SO_PROTOCOL_INFOW,
                &mut proto_info as *mut _ as *mut u8,
                &mut proto_info_len,
            ) != 0
            {
                return Err(WSAGetLastError());
            }

            let accept_socket = WSASocketW(
                FROM_PROTOCOL_INFO,
                FROM_PROTOCOL_INFO,
                FROM_PROTOCOL_INFO,
                &proto_info,
                0,
                WSA_FLAG_OVERLAPPED,
            );
            if accept_socket == INVALID_SOCKET {
                return Err(WSAGetLastError());
            }

            if CreateIoCompletionPort(accept_socket as HANDLE, self.iocp.0, 0, CP_THREADS).is_null()
            {
                let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(5);
                closesocket(accept_socket);
                return Err(err);
            }

            let ovl = Box::new(TorusOverlapped {
                overlapped: std::mem::zeroed(),
                user_data,
                kind: OverlappedKind::Accept {
                    listen_socket,
                    accept_socket,
                    addr_buf: vec![0u8; (ACCEPT_ADDR_LEN * 2) as usize].into_boxed_slice(),
                    out_addr: AcceptOutAddr {
                        addr: caller_addr,
                        addrlen: caller_addrlen,
                        capacity: caller_addr_capacity,
                    },
                },
            });
            let ovl_ptr = Box::into_raw(ovl);

            let addr_buf_ptr = match &mut (*ovl_ptr).kind {
                OverlappedKind::Accept { addr_buf, .. } => addr_buf.as_mut_ptr(),
                _ => unreachable!(),
            };

            let mut bytes_received: u32 = 0;
            let ret = AcceptEx(
                listen_socket,
                accept_socket,
                addr_buf_ptr as *mut core::ffi::c_void,
                0,
                ACCEPT_ADDR_LEN,
                ACCEPT_ADDR_LEN,
                &mut bytes_received,
                &mut (*ovl_ptr).overlapped,
            );

            if ret == 0 {
                let err = WSAGetLastError();
                if err != ERROR_IO_PENDING {
                    closesocket(accept_socket);
                    drop(Box::from_raw(ovl_ptr));
                    return Err(err);
                }
            }

            Ok(())
        }
    }

    /// Finish a completed `AcceptEx`: verify success, splice the local/remote
    /// addresses out of the AcceptEx output buffer into the caller's
    /// `addr`/`addrlen`, and make the accepted socket usable via
    /// `SO_UPDATE_ACCEPT_CONTEXT`.
    fn finish_accept(
        listen_socket: SOCKET,
        accept_socket: SOCKET,
        addr_buf: &[u8],
        out_addr: AcceptOutAddr,
        overlapped: &OVERLAPPED,
        user_data: u64,
    ) -> TorusResult {
        unsafe {
            let mut transferred: u32 = 0;
            let ok = GetOverlappedResult(
                listen_socket as HANDLE,
                overlapped as *const OVERLAPPED,
                &mut transferred,
                0,
            );
            if ok == 0 {
                let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(5);
                closesocket(accept_socket);
                return TorusResult::new(-err as i64, user_data);
            }

            // Required so the accepted socket behaves like a normal connected
            // socket (inherits properties from the listening socket).
            let _ = setsockopt(
                accept_socket,
                SOL_SOCKET,
                SO_UPDATE_ACCEPT_CONTEXT,
                &listen_socket as *const SOCKET as *const u8,
                std::mem::size_of::<SOCKET>() as i32,
            );

            if !out_addr.addr.is_null() && !out_addr.addrlen.is_null() && out_addr.capacity > 0 {
                let mut local_addr: *mut windows_sys::Win32::Networking::WinSock::SOCKADDR =
                    ptr::null_mut();
                let mut local_len: i32 = 0;
                let mut remote_addr: *mut windows_sys::Win32::Networking::WinSock::SOCKADDR =
                    ptr::null_mut();
                let mut remote_len: i32 = 0;

                GetAcceptExSockaddrs(
                    addr_buf.as_ptr() as *const core::ffi::c_void,
                    0,
                    ACCEPT_ADDR_LEN,
                    ACCEPT_ADDR_LEN,
                    &mut local_addr,
                    &mut local_len,
                    &mut remote_addr,
                    &mut remote_len,
                );

                if !remote_addr.is_null() && remote_len > 0 {
                    let copy_len = (remote_len as u32).min(out_addr.capacity) as usize;
                    ptr::copy_nonoverlapping(
                        remote_addr as *const u8,
                        out_addr.addr as *mut u8,
                        copy_len,
                    );
                    *out_addr.addrlen = remote_len as u32;
                }
            }

            TorusResult::new(accept_socket as i64, user_data)
        }
    }

    /// Start a `ConnectEx` operation. `ConnectEx` requires the socket to
    /// already be bound; if it isn't, bind it to a wildcard address matching
    /// the target address family first (mirrors what `connect()` does
    /// implicitly on POSIX).
    fn begin_connect(
        &self,
        socket: SOCKET,
        addr: *const libc::sockaddr,
        addrlen: u32,
        user_data: u64,
    ) -> std::result::Result<(), i32> {
        use windows_sys::Win32::Networking::WinSock::{
            bind, getsockname, AF_INET, AF_INET6, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6,
        };

        unsafe {
            let family = (*addr).sa_family;

            let mut probe: SOCKADDR_STORAGE = std::mem::zeroed();
            let mut probe_len = std::mem::size_of::<SOCKADDR_STORAGE>() as i32;
            if getsockname(
                socket,
                &mut probe as *mut _ as *mut SOCKADDR,
                &mut probe_len,
            ) != 0
            {
                match family {
                    AF_INET => {
                        let mut local: SOCKADDR_IN = std::mem::zeroed();
                        local.sin_family = AF_INET;
                        if bind(
                            socket,
                            &local as *const _ as *const SOCKADDR,
                            std::mem::size_of::<SOCKADDR_IN>() as i32,
                        ) != 0
                        {
                            return Err(WSAGetLastError());
                        }
                    }
                    AF_INET6 => {
                        let mut local: SOCKADDR_IN6 = std::mem::zeroed();
                        local.sin6_family = AF_INET6;
                        if bind(
                            socket,
                            &local as *const _ as *const SOCKADDR,
                            std::mem::size_of::<SOCKADDR_IN6>() as i32,
                        ) != 0
                        {
                            return Err(WSAGetLastError());
                        }
                    }
                    _ => return Err(windows_sys::Win32::Networking::WinSock::WSAEAFNOSUPPORT),
                }
            }

            let mut connect_ex: LPFN_CONNECTEX = None;
            let mut bytes_returned: u32 = 0;
            let guid = WSAID_CONNECTEX;
            let ret = WSAIoctl(
                socket,
                SIO_GET_EXTENSION_FUNCTION_POINTER,
                &guid as *const _ as *const core::ffi::c_void,
                std::mem::size_of_val(&guid) as u32,
                &mut connect_ex as *mut _ as *mut core::ffi::c_void,
                std::mem::size_of::<LPFN_CONNECTEX>() as u32,
                &mut bytes_returned,
                ptr::null_mut(),
                None,
            );
            if ret != 0 {
                return Err(WSAGetLastError());
            }
            let connect_ex = match connect_ex {
                Some(f) => f,
                None => return Err(windows_sys::Win32::Networking::WinSock::WSAEOPNOTSUPP),
            };

            let ovl = Box::new(TorusOverlapped {
                overlapped: std::mem::zeroed(),
                user_data,
                kind: OverlappedKind::Connect { socket },
            });
            let ovl_ptr = Box::into_raw(ovl);

            let mut bytes_sent: u32 = 0;
            let ok = connect_ex(
                socket,
                addr as *const windows_sys::Win32::Networking::WinSock::SOCKADDR,
                addrlen as i32,
                ptr::null(),
                0,
                &mut bytes_sent,
                &mut (*ovl_ptr).overlapped,
            );

            if ok == 0 {
                let err = WSAGetLastError();
                if err != ERROR_IO_PENDING {
                    drop(Box::from_raw(ovl_ptr));
                    return Err(err);
                }
            }

            Ok(())
        }
    }

    /// Finish a completed `ConnectEx`: verify success and apply
    /// `SO_UPDATE_CONNECT_CONTEXT` so the socket behaves like one created via
    /// a normal blocking `connect()`.
    fn finish_connect(socket: SOCKET, overlapped: &OVERLAPPED, user_data: u64) -> TorusResult {
        unsafe {
            let mut transferred: u32 = 0;
            let ok = GetOverlappedResult(
                socket as HANDLE,
                overlapped as *const OVERLAPPED,
                &mut transferred,
                0,
            );
            if ok == 0 {
                let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(5);
                return TorusResult::new(-err as i64, user_data);
            }

            let _ = setsockopt(
                socket,
                SOL_SOCKET,
                SO_UPDATE_CONNECT_CONTEXT,
                ptr::null(),
                0,
            );
            TorusResult::new(0, user_data)
        }
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
                    let overlapped_ptr = entry.lpOverlapped as *mut TorusOverlapped;
                    if overlapped_ptr.is_null() {
                        continue;
                    }

                    // Deallocate the overlapped box, moving its fields out.
                    let torus_ovl = unsafe { Box::from_raw(overlapped_ptr) };
                    let TorusOverlapped {
                        overlapped: ovl_copy,
                        user_data,
                        kind,
                    } = *torus_ovl;
                    let bytes = entry.dwNumberOfBytesTransferred;

                    let result = match kind {
                        OverlappedKind::Generic => {
                            if bytes == 0 {
                                TorusResult::new(
                                    -std::io::Error::last_os_error().raw_os_error().unwrap_or(5)
                                        as i64,
                                    user_data,
                                )
                            } else {
                                TorusResult::new(bytes as i64, user_data)
                            }
                        }
                        OverlappedKind::Accept {
                            listen_socket,
                            accept_socket,
                            addr_buf,
                            out_addr,
                        } => Self::finish_accept(
                            listen_socket,
                            accept_socket,
                            &addr_buf,
                            out_addr,
                            &ovl_copy,
                            user_data,
                        ),
                        OverlappedKind::Connect { socket } => {
                            Self::finish_connect(socket, &ovl_copy, user_data)
                        }
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
                        kind: OverlappedKind::Generic,
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
                        kind: OverlappedKind::Generic,
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
                        kind: OverlappedKind::Generic,
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
                        kind: OverlappedKind::Generic,
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
                Operation::Readv {
                    fd,
                    bufs,
                    buf_count,
                    offset,
                } => {
                    // Vectored read: submit individual ReadFile ops for each buffer.
                    // Total bytes = sum of individual results (reported on completion).
                    let handle = *fd as HANDLE;
                    let bufs_slice =
                        unsafe { std::slice::from_raw_parts(*bufs, *buf_count as usize) };
                    let mut current_offset = *offset;

                    for buf_desc in bufs_slice {
                        if buf_desc.len == 0 {
                            continue;
                        }
                        let ovl = Box::new(TorusOverlapped {
                            overlapped: unsafe { std::mem::zeroed() },
                            user_data: flow.user_data(),
                            kind: OverlappedKind::Generic,
                        });
                        let ovl_ptr = Box::into_raw(ovl) as *mut OVERLAPPED;

                        unsafe {
                            let ovl_ref = &mut *ovl_ptr;
                            ovl_ref.Anonymous.Anonymous.Offset = current_offset as u32;
                            ovl_ref.Anonymous.Anonymous.OffsetHigh = (current_offset >> 32) as u32;
                        }

                        let ret = unsafe {
                            ReadFile(
                                handle,
                                buf_desc.buf,
                                buf_desc.len as u32,
                                ptr::null_mut(),
                                ovl_ptr,
                            )
                        };

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
                            break;
                        }
                        current_offset += buf_desc.len as u64;
                    }
                    submitted += 1;
                }
                Operation::Writev {
                    fd,
                    bufs,
                    buf_count,
                    offset,
                } => {
                    // Vectored write: submit individual WriteFile ops for each buffer.
                    let handle = *fd as HANDLE;
                    let bufs_slice =
                        unsafe { std::slice::from_raw_parts(*bufs, *buf_count as usize) };
                    let mut current_offset = *offset;

                    for buf_desc in bufs_slice {
                        if buf_desc.len == 0 {
                            continue;
                        }
                        let ovl = Box::new(TorusOverlapped {
                            overlapped: unsafe { std::mem::zeroed() },
                            user_data: flow.user_data(),
                            kind: OverlappedKind::Generic,
                        });
                        let ovl_ptr = Box::into_raw(ovl) as *mut OVERLAPPED;

                        unsafe {
                            let ovl_ref = &mut *ovl_ptr;
                            ovl_ref.Anonymous.Anonymous.Offset = current_offset as u32;
                            ovl_ref.Anonymous.Anonymous.OffsetHigh = (current_offset >> 32) as u32;
                        }

                        let ret = unsafe {
                            WriteFile(
                                handle,
                                buf_desc.buf,
                                buf_desc.len as u32,
                                ptr::null_mut(),
                                ovl_ptr,
                            )
                        };

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
                            break;
                        }
                        current_offset += buf_desc.len as u64;
                    }
                    submitted += 1;
                }
                Operation::Accept { fd, addr, addrlen } => {
                    let listen_socket = *fd as SOCKET;
                    let user_data = flow.user_data();
                    let caller_addr = *addr;
                    let caller_addrlen = *addrlen;
                    let caller_addr_capacity = if caller_addrlen.is_null() {
                        0
                    } else {
                        unsafe { *caller_addrlen }
                    };

                    if let Err(err) = self.begin_accept(
                        listen_socket,
                        caller_addr,
                        caller_addrlen,
                        caller_addr_capacity,
                        user_data,
                    ) {
                        self.completions
                            .lock()
                            .unwrap()
                            .push_back(TorusResult::new(-err as i64, user_data));
                        self.notify.notify_one();
                    }
                    submitted += 1;
                }
                Operation::Connect { fd, addr, addrlen } => {
                    let socket = *fd as SOCKET;
                    let user_data = flow.user_data();

                    if let Err(err) = self.begin_connect(socket, *addr, *addrlen, user_data) {
                        self.completions
                            .lock()
                            .unwrap()
                            .push_back(TorusResult::new(-err as i64, user_data));
                        self.notify.notify_one();
                    }
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
