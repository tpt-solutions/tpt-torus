use std::fmt;

/// Errors that can occur in the Virtual Torus.
#[derive(Debug)]
pub enum Error {
    /// The submission ring is full.
    SubmissionFull,
    /// No completions available (non-blocking reap).
    NothingAvailable,
    /// The operation failed with the given errno.
    Os(i32),
    /// An invalid parameter was passed.
    InvalidParam(&'static str),
    /// The backend encountered an internal error.
    Backend(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::SubmissionFull => write!(f, "submission queue is full"),
            Error::NothingAvailable => write!(f, "no completions available"),
            Error::Os(e) => write!(f, "os error: {}", e),
            Error::InvalidParam(p) => write!(f, "invalid parameter: {}", p),
            Error::Backend(msg) => write!(f, "backend error: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

impl From<i32> for Error {
    fn from(e: i32) -> Self {
        Error::Os(e)
    }
}

/// Convenience type alias for Results.
pub type Result<T> = std::result::Result<T, Error>;
