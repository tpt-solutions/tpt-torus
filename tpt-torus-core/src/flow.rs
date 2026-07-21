use crate::operation::Operation;

/// A submission to the Virtual Torus — the user-space equivalent of an io_uring SQE.
///
/// `Flow` wraps an I/O operation along with user-provided data that will be
/// returned verbatim in the corresponding [`Result`](crate::Result).
pub struct Flow {
    pub(crate) op: Operation,
    pub(crate) user_data: u64,
}

impl Flow {
    /// Create a new `Flow` with the given operation and zero user data.
    pub fn new(op: Operation) -> Self {
        Self { op, user_data: 0 }
    }

    /// Create a new `Flow` with user-provided data returned on completion.
    pub fn with_user_data(op: Operation, user_data: u64) -> Self {
        Self { op, user_data }
    }

    /// Attach arbitrary user data to this flow.
    ///
    /// The value is returned unchanged in the corresponding [`Result`].
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
