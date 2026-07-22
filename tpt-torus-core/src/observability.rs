//! Tracing and observability hooks for TPT Torus.
//!
//! This module provides optional tracing spans and metrics for I/O operations.
//! Enable the `tracing` feature to activate instrumentation.
//!
//! # Example
//!
//! ```toml
//! [dependencies]
//! tpt-torus-core = { version = "0.1.0", features = ["tracing"] }
//! ```
//!
//! Each `Flow` submission creates a tracing span that follows the operation
//! through submit → wait → reap, recording:
//! - Operation type (Read, Write, Accept, Connect, Recv, Send, Close)
//! - File descriptor
//! - Buffer size and offset
//! - Completion status and latency

use crate::flow::Flow;
#[cfg(feature = "tracing")]
use crate::operation::Operation;
use std::time::Instant;

/// Guard that records timing and emits a tracing event on drop.
///
/// Created when a Flow is submitted; dropped when the completion is reaped.
pub struct FlowSpan {
    start: Instant,
    user_data: u64,
    #[cfg(feature = "tracing")]
    span: tracing::Span,
}

impl FlowSpan {
    /// Create a new span for the given flow.
    pub fn new(flow: &Flow) -> Self {
        let start = Instant::now();
        let user_data = flow.user_data;

        #[cfg(feature = "tracing")]
        let span = {
            let op_name = match &flow.operation {
                Operation::Read { fd, len, offset, .. } => {
                    tracing::info_span!("torus_io", op = "read", fd = fd, len = len, offset = offset)
                }
                Operation::Write { fd, len, offset, .. } => {
                    tracing::info_span!("torus_io", op = "write", fd = fd, len = len, offset = offset)
                }
                Operation::Accept { fd, .. } => {
                    tracing::info_span!("torus_io", op = "accept", fd = fd)
                }
                Operation::Connect { fd, .. } => {
                    tracing::info_span!("torus_io", op = "connect", fd = fd)
                }
                Operation::Recv { fd, len, .. } => {
                    tracing::info_span!("torus_io", op = "recv", fd = fd, len = len)
                }
                Operation::Send { fd, len, .. } => {
                    tracing::info_span!("torus_io", op = "send", fd = fd, len = len)
                }
                Operation::Close { fd } => {
                    tracing::info_span!("torus_io", op = "close", fd = fd)
                }
            };
            span
        };

        Self {
            start,
            user_data,
            #[cfg(feature = "tracing")]
            span,
        }
    }

    /// Record the completion of this operation.
    pub fn complete(self, result: i64) {
        let elapsed = self.start.elapsed();

        #[cfg(feature = "tracing")]
        {
            let _enter = self.span.enter();
            if result >= 0 {
                tracing::info!(
                    user_data = self.user_data,
                    bytes = result,
                    latency_us = elapsed.as_micros() as u64,
                    "torus_io_complete"
                );
            } else {
                tracing::warn!(
                    user_data = self.user_data,
                    errno = result,
                    latency_us = elapsed.as_micros() as u64,
                    "torus_io_error"
                );
            }
        }

        let _ = (elapsed, self.user_data, result);
    }
}
