/// I/O operations that can be submitted to the Virtual Torus.
///
/// Each variant maps to a native operation on the underlying backend
/// (io_uring opcode on Linux, IOCP on Windows, kqueue on macOS).
#[derive(Debug, Clone)]
pub enum Operation {
    /// Read from a file descriptor at a given offset.
    Read {
        fd: i32,
        buf: *mut u8,
        len: usize,
        offset: u64,
    },
    /// Write to a file descriptor at a given offset.
    Write {
        fd: i32,
        buf: *const u8,
        len: usize,
        offset: u64,
    },
    /// Accept an incoming connection on a listening socket.
    Accept {
        fd: i32,
        addr: *mut libc::sockaddr,
        addrlen: *mut u32,
    },
    /// Connect a socket to a remote address.
    Connect {
        fd: i32,
        addr: *const libc::sockaddr,
        addrlen: u32,
    },
    /// Receive data from a connected socket.
    Recv { fd: i32, buf: *mut u8, len: usize },
    /// Send data to a connected socket.
    Send { fd: i32, buf: *const u8, len: usize },
    /// Close a file descriptor.
    Close { fd: i32 },
}
