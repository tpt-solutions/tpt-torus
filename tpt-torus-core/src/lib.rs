//! TPT Torus Core — the Virtual Torus abstraction.
//!
//! Exposes the [`Torus`] handle, and the [`Flow`] (submission) / [`Result`] (completion)
//! types that replace raw SQE/CQE across all backends.

pub mod async_api;
pub mod backend;
pub mod cgroup;
pub mod error;
pub mod flow;
pub mod lease;
pub mod operation;
pub mod raw_api;
pub mod result;
pub mod rings;
pub mod torus_panic;

pub use error::{Error, Result};
pub use flow::Flow;
pub use lease::{LeaseError, LeaseRegistry, SharedLeaseRegistry};
pub use operation::Operation;
pub use result::Result as TorusResult;
pub use rings::{CompletionRing, SubmissionRing};
pub use torus_panic::TorusPanic;

use backend::Backend;
use std::sync::{Arc, Mutex};

/// The main context object for the Virtual Torus.
///
/// `Torus` owns the virtual submission and completion rings and delegates
/// to a platform-specific [`Backend`] for actual I/O. It is thread-safe
/// and can be shared across threads via `Arc<Torus>`.
pub struct Torus {
    sq: SubmissionRing,
    cq: CompletionRing,
    backend: Mutex<Box<dyn Backend>>,
}

// SAFETY: Torus is thread-safe. The backend is behind a Mutex, and the rings
// use atomic operations for synchronization.
unsafe impl Send for Torus {}
unsafe impl Sync for Torus {}

impl Torus {
    /// Create a new Torus instance with the given ring size and backend.
    ///
    /// `ring_entries` must be a power of two (e.g. 256, 1024, 4096).
    pub fn new(ring_entries: u32, backend: Box<dyn Backend>) -> Result<Self> {
        if !ring_entries.is_power_of_two() {
            return Err(Error::InvalidParam("ring_entries must be a power of two"));
        }
        Ok(Self {
            sq: SubmissionRing::new(ring_entries),
            cq: CompletionRing::new(ring_entries),
            backend: Mutex::new(backend),
        })
    }

    /// Submit a single flow to the Virtual Torus.
    pub fn submit(&self, flow: &Flow) -> Result<()> {
        let n = self
            .backend
            .lock()
            .unwrap()
            .submit(std::slice::from_ref(flow))?;
        if n == 0 {
            Err(Error::SubmissionFull)
        } else {
            Ok(())
        }
    }

    /// Submit a batch of flows to the Virtual Torus.
    pub fn submit_batch(&self, flows: &[Flow]) -> Result<usize> {
        self.backend.lock().unwrap().submit(flows)
    }

    /// Reap all available completions.
    pub fn reap(&self, results: &mut Vec<TorusResult>) -> Result<usize> {
        self.backend.lock().unwrap().reap(results)
    }

    /// Block until at least one completion is available.
    pub fn wait(&self, timeout_us: u64) -> Result<()> {
        self.backend.lock().unwrap().wait(timeout_us)
    }

    /// Number of in-flight operations.
    pub fn in_flight(&self) -> u32 {
        self.backend.lock().unwrap().in_flight()
    }

    /// Access the virtual submission ring.
    pub fn submission_ring(&self) -> &SubmissionRing {
        &self.sq
    }

    /// Access the virtual completion ring.
    pub fn completion_ring(&self) -> &CompletionRing {
        &self.cq
    }

    /// Get raw, unguarded access to the Torus, bypassing Buffer Leasing.
    ///
    /// # Safety
    ///
    /// The returned [`RawTorus`] bypasses all buffer safety checks.
    /// The caller is responsible for ensuring buffer validity.
    pub unsafe fn raw(&self) -> raw_api::RawTorus<'_> {
        raw_api::RawTorus::new(self)
    }
}

/// Shared handle to a `Torus` instance, suitable for multi-threaded use.
pub type SharedTorus = Arc<Torus>;
