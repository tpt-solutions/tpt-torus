use crate::flow::Flow;
use crate::result::Result;

/// A memory region to register with a backend for zero-copy I/O.
///
/// Platform-neutral description of a buffer (base + length); backends translate
/// it into their native registration representation (e.g. `struct iovec` for
/// io_uring). Using a neutral type here keeps the `Backend` trait compilable on
/// platforms where `libc::iovec` is unavailable.
#[derive(Clone, Copy)]
pub struct RegisterBuffer {
    /// Base address of the region.
    pub ptr: *const u8,
    /// Length in bytes.
    pub len: usize,
}

/// The interface that platform-specific backends must implement.
///
/// On Linux the backend maps the virtual SQ/CQ directly to io_uring kernel shared memory.
/// On Windows/macOS a lock-free background reactor drains the virtual SQ,
/// translates ops into native calls, and populates the virtual CQ.
pub trait Backend {
    /// Submit a batch of flows to the backend.
    ///
    /// Returns the number of flows that were successfully enqueued.
    fn submit(&self, flows: &[Flow]) -> crate::error::Result<usize>;

    /// Reap all available completions from the backend.
    ///
    /// Returns completions in completion order (FIFO).
    fn reap(&self, results: &mut Vec<Result>) -> crate::error::Result<usize>;

    /// Block until at least one completion is available, up to `timeout_us` microseconds.
    ///
    /// Pass `0` for an indefinite wait.
    fn wait(&self, timeout_us: u64) -> crate::error::Result<()>;

    /// The number of in-flight (submitted but not yet completed) operations.
    fn in_flight(&self) -> u32;

    /// Register a set of memory regions with the kernel for zero-copy
    /// fixed-buffer I/O (e.g. io_uring `IORING_REGISTER_BUFFERS`).
    ///
    /// Each region's base address is recorded so that subsequent `read`/`write`
    /// operations whose buffer matches a registered base can use the fixed
    /// (`*_FIXED`) opcodes and skip per-operation address translation.
    ///
    /// The default implementation is a no-op for backends that don't support
    /// registered buffers (Windows IOCP / macOS kqueue).
    fn register_buffers(&self, _buffers: &[RegisterBuffer]) -> crate::error::Result<()> {
        Ok(())
    }

    /// Unregister all previously registered buffers.
    fn unregister_buffers(&self) -> crate::error::Result<()> {
        Ok(())
    }
}
