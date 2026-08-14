/// A completion from the Virtual Torus — the user-space equivalent of an io_uring CQE.
///
/// `Result` represents a finished I/O operation. The `result` field holds the
/// number of bytes transferred (on success) or a negative errno (on failure).
/// The `user_data` field is the value originally set on the submitting [`Flow`].
#[derive(Debug, Clone)]
pub struct Result {
    /// Number of bytes transferred, or a negative errno on failure.
    pub result: i64,
    /// User data from the submitting [`Flow`](crate::Flow).
    pub user_data: u64,
}

impl Result {
    /// Create a new completion result.
    pub fn new(result: i64, user_data: u64) -> Self {
        Self { result, user_data }
    }

    /// Returns `true` if the operation completed successfully.
    pub fn is_ok(&self) -> bool {
        self.result >= 0
    }

    /// Returns the number of bytes transferred, or `None` on error.
    pub fn bytes(&self) -> Option<usize> {
        if self.is_ok() {
            Some(self.result as usize)
        } else {
            None
        }
    }

    /// Returns the raw result value (bytes transferred or negative errno).
    pub fn raw(&self) -> i64 {
        self.result
    }

    /// Returns the negative errno, or `None` on success.
    pub fn error(&self) -> Option<i32> {
        if self.is_ok() {
            None
        } else {
            Some(self.result.wrapping_neg() as i32)
        }
    }
}
