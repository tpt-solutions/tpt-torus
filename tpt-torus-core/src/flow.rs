use crate::operation::Operation;

/// A submission to the Virtual Torus — the user-space equivalent of an io_uring SQE.
///
/// `Flow` wraps an I/O [`Operation`] along with user-provided data (`user_data`)
/// that is returned verbatim in the corresponding [`Result`](crate::Result) when
/// the operation completes. Applications submit `Flow`s and later reap the
/// matching [`Result`](crate::Result)s, correlating them via `user_data`.
///
/// # Example
///
/// ```
/// use tpt_torus_core::flow::Flow;
/// use tpt_torus_core::operation::Operation;
///
/// let flow = Flow::new(Operation::Read {
///     fd: 0,
///     buf: std::ptr::null_mut(),
///     len: 0,
///     offset: 0,
/// });
/// assert_eq!(flow.user_data(), 0);
/// ```
pub struct Flow {
    /// The I/O operation this flow submits to the backend.
    pub(crate) op: Operation,
    /// Opaque caller data echoed back on completion (via [`Result::user_data`](crate::Result::user_data)).
    pub(crate) user_data: u64,
}

impl Flow {
    /// Create a new `Flow` with the given operation and zero user data.
    pub fn new(op: Operation) -> Self {
        Self { op, user_data: 0 }
    }

    /// Create a new `Flow` with user-provided data returned on completion.
    ///
    /// The `user_data` value is opaque to the framework; it is stored with the
    /// flow and handed back unchanged in the corresponding
    /// [`Result`](crate::Result) so callers can correlate submissions with
    /// completions (e.g. as a request id or a pointer to a context struct).
    pub fn with_user_data(op: Operation, user_data: u64) -> Self {
        Self { op, user_data }
    }

    /// Attach arbitrary user data to this flow.
    ///
    /// The value is returned unchanged in the corresponding [`Result`](crate::Result).
    ///
    /// Returns `&mut Self` for method chaining.
    pub fn set_user_data(&mut self, data: u64) -> &mut Self {
        self.user_data = data;
        self
    }

    /// Get the user data associated with this flow.
    pub fn user_data(&self) -> u64 {
        self.user_data
    }

    /// Access the inner operation.
    pub fn operation(&self) -> &Operation {
        &self.op
    }

    /// Consume the flow, returning the inner operation.
    pub fn into_operation(self) -> Operation {
        self.op
    }
}
