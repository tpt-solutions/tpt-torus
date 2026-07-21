use crate::flow::Flow;
use crate::result::Result;

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
}
