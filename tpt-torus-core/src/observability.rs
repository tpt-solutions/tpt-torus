//! Tracing and observability hooks for TPT Torus.
//!
//! This module provides optional tracing spans and always-available metrics for
//! I/O operations. Enable the `tracing` feature to also emit `tracing` spans and
//! events; the latency/throughput metrics are recorded regardless of that feature
//! so they can be surfaced by any metrics backend.
//!
//! # Example
//!
//! ```toml
//! [dependencies]
//! tpt-torus-core = { version = "0.1.0", features = ["tracing"] }
//! ```
//!
//! Each `Flow` submission creates a [`FlowSpan`] that follows the operation
//! through submit → wait → reap, recording:
//! - Operation type (`OpKind`)
//! - Completion status, byte count, and latency
//!
//! The global [`metrics()`] recorder aggregates per-`OpKind` latency histograms
//! and success/error counters.

use crate::flow::Flow;
use crate::operation::Operation;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// The high-level operation categories TPT Torus instruments.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum OpKind {
    /// `Operation::Read` (file, positional).
    Read,
    /// `Operation::Write` (file, positional).
    Write,
    /// `Operation::Readv`.
    Readv,
    /// `Operation::Writev`.
    Writev,
    /// `Operation::Recv` (socket).
    Recv,
    /// `Operation::Send` (socket).
    Send,
    /// `Operation::Accept`.
    Accept,
    /// `Operation::Connect`.
    Connect,
    /// `Operation::Close`.
    Close,
}

impl OpKind {
    /// Map a concrete [`Operation`] to its [`OpKind`].
    pub fn from_op(op: &Operation) -> Self {
        match op {
            Operation::Read { .. } => OpKind::Read,
            Operation::Write { .. } => OpKind::Write,
            Operation::Readv { .. } => OpKind::Readv,
            Operation::Writev { .. } => OpKind::Writev,
            Operation::Recv { .. } => OpKind::Recv,
            Operation::Send { .. } => OpKind::Send,
            Operation::Accept { .. } => OpKind::Accept,
            Operation::Connect { .. } => OpKind::Connect,
            Operation::Close { .. } => OpKind::Close,
        }
    }

    /// Stable, ordered index used for the per-op metrics arrays.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The total number of operation kinds.
    pub const COUNT: usize = 9;
}

/// Latency histogram backed by atomic counters.
///
/// Buckets are defined by ascending microsecond boundaries; the final bucket
/// captures everything at or above the last boundary.
pub struct Histogram {
    boundaries_us: &'static [u64],
    counts: Vec<AtomicU64>,
}

impl Histogram {
    fn new(boundaries_us: &'static [u64]) -> Self {
        let mut counts = Vec::with_capacity(boundaries_us.len() + 1);
        for _ in 0..=boundaries_us.len() {
            counts.push(AtomicU64::new(0));
        }
        Self {
            boundaries_us,
            counts,
        }
    }

    fn record(&self, micros: u64) {
        let mut bucket = self.boundaries_us.len(); // last = "infinity" bucket
        for (i, &b) in self.boundaries_us.iter().enumerate() {
            if micros < b {
                bucket = i;
                break;
            }
        }
        self.counts[bucket].fetch_add(1, Ordering::Relaxed);
    }

    /// Snapshot of the current bucket counts (parallel to `boundaries_us` + overflow).
    pub fn snapshot(&self) -> Vec<u64> {
        self.counts
            .iter()
            .map(|c| c.load(Ordering::Relaxed))
            .collect()
    }

    /// The bucket boundaries in microseconds.
    pub fn boundaries(&self) -> &'static [u64] {
        self.boundaries_us
    }
}

/// Aggregate metrics for the whole process: a latency histogram per [`OpKind`]
/// plus total and error counters.
pub struct Metrics {
    histograms: Vec<Histogram>,
    total: AtomicU64,
    errors: AtomicU64,
}

impl Metrics {
    fn new() -> Self {
        // Latency buckets (microseconds): <1, <10, <100, <1ms, <10ms, <100ms, <1s, else.
        const BOUNDS: &[u64] = &[1, 10, 100, 1_000, 10_000, 100_000, 1_000_000];
        let mut histograms = Vec::with_capacity(OpKind::COUNT);
        for _ in 0..OpKind::COUNT {
            histograms.push(Histogram::new(BOUNDS));
        }
        Self {
            histograms,
            total: AtomicU64::new(0),
            errors: AtomicU64::new(0),
        }
    }

    /// Record a completed operation.
    pub fn record(&self, op: OpKind, latency: Duration, ok: bool) {
        let micros = latency.as_micros().min(u64::MAX as u128) as u64;
        self.histograms[op.index()].record(micros);
        self.total.fetch_add(1, Ordering::Relaxed);
        if !ok {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Read the latency histogram for a specific operation kind.
    pub fn histogram(&self, op: OpKind) -> &Histogram {
        &self.histograms[op.index()]
    }

    /// Total completed operations recorded.
    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// Total errored operations recorded.
    pub fn errors(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }
}

static METRICS: OnceLock<Metrics> = OnceLock::new();

/// Access the process-wide metrics recorder.
pub fn metrics() -> &'static Metrics {
    METRICS.get_or_init(Metrics::new)
}

/// Guard that records timing and emits a tracing event on drop.
///
/// Created when a Flow is submitted; dropped (via [`FlowSpan::complete`]) when the
/// completion is reaped. When the `tracing` feature is enabled, the submission
/// and completion are also wrapped in a `torus_io` span.
pub struct FlowSpan {
    start: Instant,
    user_data: u64,
    op: OpKind,
    #[cfg(feature = "tracing")]
    span: tracing::Span,
}

impl FlowSpan {
    /// Create a new span for the given flow.
    pub fn new(flow: &Flow) -> Self {
        let op = OpKind::from_op(flow.operation());
        let user_data = flow.user_data;

        #[cfg(feature = "tracing")]
        let span = {
            let span = match &flow.operation() {
                Operation::Read {
                    fd, len, offset, ..
                } => {
                    tracing::info_span!(
                        "torus_io",
                        op = "read",
                        fd = fd,
                        len = len,
                        offset = offset
                    )
                }
                Operation::Write {
                    fd, len, offset, ..
                } => {
                    tracing::info_span!(
                        "torus_io",
                        op = "write",
                        fd = fd,
                        len = len,
                        offset = offset
                    )
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
                Operation::Readv { fd, .. } => {
                    tracing::info_span!("torus_io", op = "readv", fd = fd)
                }
                Operation::Writev { fd, .. } => {
                    tracing::info_span!("torus_io", op = "writev", fd = fd)
                }
            };
            span
        };

        Self {
            start: Instant::now(),
            user_data,
            op,
            #[cfg(feature = "tracing")]
            span,
        }
    }

    /// Record the completion of this operation.
    ///
    /// `result` is the raw completion value: a non-negative byte count on success
    /// or a negative errno on failure.
    pub fn complete(self, result: i64) {
        let elapsed = self.start.elapsed();
        let ok = result >= 0;
        metrics().record(self.op, elapsed, ok);

        #[cfg(feature = "tracing")]
        {
            let _enter = self.span.enter();
            if ok {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::Flow;
    use crate::operation::Operation;

    #[test]
    fn metrics_record_and_snapshot() {
        let m = metrics();
        let before_total = m.total();
        let before_errors = m.errors();

        let read = Flow::with_user_data(
            Operation::Read {
                fd: 7,
                buf: std::ptr::null_mut(),
                len: 0,
                offset: 0,
            },
            1,
        );
        FlowSpan::new(&read).complete(4096); // success

        let write = Flow::with_user_data(
            Operation::Write {
                fd: 7,
                buf: std::ptr::null(),
                len: 0,
                offset: 0,
            },
            2,
        );
        FlowSpan::new(&write).complete(-5); // error (negative errno)

        assert_eq!(m.total(), before_total + 2);
        assert_eq!(m.errors(), before_errors + 1);

        // The per-op histograms should reflect the recorded ops.
        let hist = m.histogram(OpKind::Read);
        let snap = hist.snapshot();
        assert_eq!(snap.iter().sum::<u64>(), 1);
        assert!(!hist.boundaries().is_empty());
    }

    #[test]
    fn op_kind_index_in_range() {
        for op in [
            OpKind::Read,
            OpKind::Write,
            OpKind::Readv,
            OpKind::Writev,
            OpKind::Recv,
            OpKind::Send,
            OpKind::Accept,
            OpKind::Connect,
            OpKind::Close,
        ] {
            assert!(op.index() < OpKind::COUNT);
        }
    }
}
