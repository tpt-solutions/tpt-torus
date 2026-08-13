//! Raw, unsafe FFI bindings to io_uring (Linux), IOCP (Windows), and kqueue (macOS/BSD).
//!
//! This crate is intentionally low-level. Application code should use `tpt-torus-core` instead.

#[cfg(target_os = "linux")]
use std::os::raw::{c_int, c_uint, c_void};

// ─── Submission Queue Entry (io_uring_sqe) ─────────────────────────────────

/// Raw io_uring submission queue entry.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct io_uring_sqe {
    pub opcode: u8,
    pub flags: u8,
    pub ioprio: u16,
    pub fd: i32,
    pub off_addr2: u64,
    pub addr_splice_off_in: u64,
    pub len: u32,
    pub op_flags: u32,
    pub user_data: u64,
    pub buf_group: u16,
    pub personality: u16,
    pub splice_fd_in: i32,
    pub __pad2: [u32; 2],
}

// ─── Completion Queue Entry (io_uring_cqe) ─────────────────────────────────

/// Raw io_uring completion queue entry.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct io_uring_cqe {
    pub user_data: u64,
    pub res: i32,
    pub flags: u32,
}

// ─── Setup Parameters ──────────────────────────────────────────────────────

/// Parameters for `io_uring_setup`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct io_uring_params {
    pub sq_entries: u32,
    pub cq_entries: u32,
    pub flags: u32,
    pub sq_thread_cpu: u32,
    pub sq_thread_idle: u32,
    pub features: u32,
    pub wq_fd: u32,
    pub resv: [u32; 3],
    pub sq_off: io_sqring_offsets,
    pub cq_off: io_cqring_offsets,
}

/// Offsets into the shared memory region for the submission ring.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct io_sqring_offsets {
    pub head: u32,
    pub tail: u32,
    pub ring_mask: u32,
    pub ring_entries: u32,
    pub flags: u32,
    pub dropped: u32,
    pub array: u32,
    pub resv1: u32,
    pub resv2: u64,
}

/// Offsets into the shared memory region for the completion ring.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct io_cqring_offsets {
    pub head: u32,
    pub tail: u32,
    pub ring_mask: u32,
    pub ring_entries: u32,
    pub overflow: u32,
    pub cqes: u32,
    pub flags: u32,
    pub resv1: u32,
    pub resv2: u64,
}

// ─── Opcodes ───────────────────────────────────────────────────────────────

/// io_uring operation opcodes.
pub mod opcodes {
    pub const IORING_OP_NOP: u8 = 0;
    pub const IORING_OP_READV: u8 = 1;
    pub const IORING_OP_WRITEV: u8 = 2;
    pub const IORING_OP_FSYNC: u8 = 3;
    pub const IORING_OP_READ_FIXED: u8 = 4;
    pub const IORING_OP_WRITE_FIXED: u8 = 5;
    pub const IORING_OP_POLL_ADD: u8 = 6;
    pub const IORING_OP_POLL_REMOVE: u8 = 7;
    pub const IORING_OP_SYNC_FILE_RANGE: u8 = 8;
    pub const IORING_OP_SENDMSG: u8 = 9;
    pub const IORING_OP_RECVMSG: u8 = 10;
    pub const IORING_OP_TIMEOUT: u8 = 11;
    pub const IORING_OP_TIMEOUT_REMOVE: u8 = 12;
    pub const IORING_OP_ACCEPT: u8 = 13;
    pub const IORING_OP_ASYNC_CANCEL: u8 = 14;
    pub const IORING_OP_LINK_TIMEOUT: u8 = 15;
    pub const IORING_OP_CONNECT: u8 = 16;
    pub const IORING_OP_FALLOCATE: u8 = 17;
    pub const IORING_OP_OPENAT: u8 = 18;
    pub const IORING_OP_CLOSE: u8 = 19;
    pub const IORING_OP_FILES_UPDATE: u8 = 20;
    pub const IORING_OP_STATX: u8 = 21;
    pub const IORING_OP_READ: u8 = 22;
    pub const IORING_OP_WRITE: u8 = 23;
    pub const IORING_OP_FADVISE: u8 = 24;
    pub const IORING_OP_MADVISE: u8 = 25;
    pub const IORING_OP_SEND: u8 = 26;
    pub const IORING_OP_RECV: u8 = 27;
    pub const IORING_OP_OPENAT2: u8 = 28;
    pub const IORING_OP_EPOLL_CTL: u8 = 29;
    pub const IORING_OP_SPLICE: u8 = 30;
    pub const IORING_OP_PROVIDE_BUFFERS: u8 = 31;
    pub const IORING_OP_REMOVE_BUFFERS: u8 = 32;
    pub const IORING_OP_TEE: u8 = 33;
    pub const IORING_OP_SHUTDOWN: u8 = 34;
    pub const IORING_OP_RENAMEAT: u8 = 35;
    pub const IORING_OP_UNLINKAT: u8 = 36;
    pub const IORING_OP_MKDIRAT: u8 = 37;
    pub const IORING_OP_SYMLINKAT: u8 = 38;
    pub const IORING_OP_LINKAT: u8 = 39;
}

// ─── SQE Flags ─────────────────────────────────────────────────────────────

/// Flags for `io_uring_sqe::flags`.
pub mod sqe_flags {
    pub const IOSQE_FIXED_FILE: u8 = 1 << 0;
    pub const IOSQE_IO_DRAIN: u8 = 1 << 1;
    pub const IOSQE_IO_LINK: u8 = 1 << 2;
    pub const IOSQE_IO_HARDLINK: u8 = 1 << 3;
    pub const IOSQE_ASYNC: u8 = 1 << 4;
    pub const IOSQE_BUFFER_SELECT: u8 = 1 << 5;
}

// ─── ioprio Flags (multishot accept/recv) ─────────────────────────────────

/// Flags OR'd into `io_uring_sqe::ioprio` for ops that support them.
///
/// Multishot mode for `IORING_OP_ACCEPT`/`IORING_OP_RECV` is armed via bits in
/// `ioprio`, not the `op_flags` union slot (which holds the normal
/// `accept4()`/`recv()` flags).
pub mod ioprio_flags {
    pub const IORING_ACCEPT_MULTISHOT: u16 = 1 << 0;
    pub const IORING_RECV_MULTISHOT: u16 = 1 << 1;
}

// ─── mmap Offsets ──────────────────────────────────────────────────────────

/// Magic offsets passed to `mmap(2)` on the io_uring fd to reach each region.
/// These are fixed kernel ABI constants, not derived from `io_uring_params`.
pub mod mmap_offsets {
    pub const IORING_OFF_SQ_RING: libc::off_t = 0;
    pub const IORING_OFF_CQ_RING: libc::off_t = 0x8000000;
    pub const IORING_OFF_SQES: libc::off_t = 0x10000000;
}

// ─── Setup Flags ───────────────────────────────────────────────────────────

/// Flags for `io_uring_params::flags`.
pub mod setup_flags {
    pub const IORING_SETUP_IOPOLL: u32 = 1 << 0;
    pub const IORING_SETUP_SQPOLL: u32 = 1 << 1;
    pub const IORING_SETUP_SQ_AFF: u32 = 1 << 2;
    pub const IORING_SETUP_CQ_NOSPACE_DROP: u32 = 1 << 3;
    pub const IORING_SETUP_REGISTERED_FD: u32 = 1 << 4;
    pub const IORING_SETUP_DEFER_DRING: u32 = 1 << 5;
}

// ─── Feature Flags ─────────────────────────────────────────────────────────

/// Feature flags returned in `io_uring_params::features`.
pub mod features {
    pub const IORING_FEAT_SINGLE_MMAP: u32 = 1 << 0;
    pub const IORING_FEAT_NODROP: u32 = 1 << 1;
    pub const IORING_FEAT_SUBMIT_STABLE: u32 = 1 << 2;
    pub const IORING_FEAT_RW_CURPOS: u32 = 1 << 3;
    pub const IORING_FEAT_CUR_PERSONALITY: u32 = 1 << 4;
    pub const IORING_FEAT_FAST_POLL: u32 = 1 << 5;
    pub const IORING_FEAT_POLL_32BITS: u32 = 1 << 6;
    pub const IORING_FEAT_SQPOLL_NONFIXED: u32 = 1 << 7;
}

// ─── CQE Flags ─────────────────────────────────────────────────────────────

/// Flags for `io_uring_cqe::flags`.
pub mod cqe_flags {
    pub const IORING_CQE_F_BUFFER: u32 = 1 << 0;
    pub const IORING_CQE_F_MORE: u32 = 1 << 1;
    pub const IORING_CQE_F_SOCK_NONEMPTY: u32 = 1 << 2;
    pub const IORING_CQE_F_NOTIF: u32 = 1 << 3;
}

// ─── io_uring_enter flags ──────────────────────────────────────────────────

/// Flags for the `io_uring_enter` syscall.
pub mod enter_flags {
    pub const IORING_ENTER_GETEVENTS: u32 = 1 << 0;
    pub const IORING_ENTER_SQ_WAIT: u32 = 1 << 1;
    pub const IORING_ENTER_SQ_TICKET: u32 = 1 << 2;
    pub const IORING_ENTER_EXT_ARG: u32 = 1 << 3;
}

// ─── Syscall Declarations (Linux only) ────────────────────────────────────
//
// io_uring has no glibc wrapper (these are raw syscalls, normally reached via
// liburing, which we don't link against), so they're issued directly through
// `libc::syscall` rather than declared as `extern "C"` — there is no such
// symbol in libc/libgcc to link against.

/// Create an io_uring instance.
///
/// `entries` is the number of SQE entries (must be power of 2, max 4096).
/// `params` is an in/out parameter for setup configuration.
/// Returns a file descriptor for the io_uring instance, or `-errno` on failure.
///
/// # Safety
///
/// `params` must be a valid, non-null pointer to an `io_uring_params` the
/// caller owns for the duration of the call.
#[cfg(target_os = "linux")]
pub unsafe fn io_uring_setup(entries: c_uint, params: *mut io_uring_params) -> c_int {
    unsafe { libc::syscall(libc::SYS_io_uring_setup, entries, params) as c_int }
}

/// Submit entries to the io_uring instance and/or wait for completions.
///
/// `fd` is the io_uring file descriptor.
/// `to_submit` is the number of SQEs to submit.
/// `min_complete` is the minimum number of CQEs to wait for (0 = non-blocking).
/// `flags` is a bitmask of `enter_flags`.
/// Returns the number of submissions accepted, or `-errno`.
///
/// # Safety
///
/// `fd` must be a valid io_uring file descriptor from `io_uring_setup`, and
/// `sig` must be null or point to a valid `sigset_t` the caller owns for the
/// duration of the call.
#[cfg(target_os = "linux")]
pub unsafe fn io_uring_enter(
    fd: c_int,
    to_submit: c_uint,
    min_complete: c_uint,
    flags: c_uint,
    sig: *const c_void,
) -> isize {
    unsafe {
        libc::syscall(
            libc::SYS_io_uring_enter,
            fd,
            to_submit,
            min_complete,
            flags,
            sig,
            0usize,
        ) as isize
    }
}

/// Register resources with the io_uring instance.
///
/// # Safety
///
/// `fd` must be a valid io_uring file descriptor, and `arg` must be null or
/// point to `nr_args` valid elements of whatever type `opcode` expects.
#[cfg(target_os = "linux")]
pub unsafe fn io_uring_register(
    fd: c_int,
    opcode: c_uint,
    arg: *const c_void,
    nr_args: c_uint,
) -> c_int {
    unsafe { libc::syscall(libc::SYS_io_uring_register, fd, opcode, arg, nr_args) as c_int }
}

/// Unregister resources with the io_uring instance.
///
/// This is `io_uring_register` with a null/empty argument, per the kernel ABI —
/// there is no separate `io_uring_unregister` syscall.
///
/// # Safety
///
/// `fd` must be a valid io_uring file descriptor.
#[cfg(target_os = "linux")]
pub unsafe fn io_uring_unregister(fd: c_int, opcode: c_uint) -> c_int {
    unsafe {
        libc::syscall(
            libc::SYS_io_uring_register,
            fd,
            opcode,
            std::ptr::null::<c_void>(),
            0u32,
        ) as c_int
    }
}

/// Enter the ring and wait for events (convenience wrapper).
///
/// This is the full 6-argument form of `io_uring_enter`, including the
/// signal-mask size the kernel syscall always takes.
///
/// # Safety
///
/// `fd` must be a valid io_uring file descriptor from `io_uring_setup`, and
/// `sig` must be null or point to a valid `sigset_t` of size `sz` that the
/// caller owns for the duration of the call.
#[cfg(target_os = "linux")]
pub unsafe fn io_uring_enter2(
    fd: c_int,
    to_submit: c_uint,
    min_complete: c_uint,
    flags: c_uint,
    sig: *const c_void,
    sz: usize,
) -> isize {
    unsafe {
        libc::syscall(
            libc::SYS_io_uring_enter,
            fd,
            to_submit,
            min_complete,
            flags,
            sig,
            sz,
        ) as isize
    }
}

// ─── Convenience helpers (Linux only) ─────────────────────────────────────

/// Initialize an io_uring instance with the given number of entries.
///
/// Returns `(fd, params)` on success, or an error string on failure.
#[cfg(target_os = "linux")]
pub fn queue_init(entries: u32, params: &mut io_uring_params) -> Result<i32, &'static str> {
    let fd = unsafe { io_uring_setup(entries, params) };
    if fd < 0 {
        let errno = -fd;
        match errno {
            12 => Err("ENOMEM: insufficient kernel resources"),
            22 => Err("EINVAL: invalid entries count or parameters"),
            27 => Err("EMFILE: too many open file descriptors"),
            28 => Err("ENOSPC: insufficient memory in the io_uring ring"),
            _ => Err("unknown error during io_uring_setup"),
        }
    } else {
        Ok(fd)
    }
}

/// Clean up an io_uring instance by closing its file descriptor.
#[cfg(target_os = "linux")]
pub fn queue_exit(fd: i32) {
    unsafe {
        libc::close(fd);
    }
}
