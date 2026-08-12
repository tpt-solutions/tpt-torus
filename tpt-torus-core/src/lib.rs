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
pub mod observability;
pub mod operation;
pub mod raw_api;
pub mod result;
pub mod rings;
pub mod torus_panic;

pub use error::{Error, Result};
pub use flow::Flow;
pub use lease::{LeaseError, LeaseRegistry, SharedLeaseRegistry};
pub use operation::{IoSlice, Operation};
pub use result::Result as TorusResult;
pub use rings::{CompletionRing, SubmissionRing};
pub use torus_panic::TorusPanic;

use backend::Backend;
use std::sync::atomic::{AtomicU32, Ordering};
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

    /// Submit a vectored read (readv) operation.
    ///
    /// Reads from `fd` at `offset` into multiple buffers described by `bufs`.
    /// Returns the total number of bytes read across all buffers.
    pub fn readv(
        &self,
        fd: i32,
        bufs: &[IoSlice],
        offset: u64,
        user_data: u64,
    ) -> Result<()> {
        let flow = Flow::with_user_data(
            Operation::Readv {
                fd,
                bufs: bufs.as_ptr(),
                buf_count: bufs.len() as u32,
                offset,
            },
            user_data,
        );
        self.submit(&flow)
    }

    /// Submit a vectored write (writev) operation.
    ///
    /// Writes to `fd` at `offset` from multiple buffers described by `bufs`.
    /// Returns the total number of bytes written across all buffers.
    pub fn writev(
        &self,
        fd: i32,
        bufs: &[IoSlice],
        offset: u64,
        user_data: u64,
    ) -> Result<()> {
        let flow = Flow::with_user_data(
            Operation::Writev {
                fd,
                bufs: bufs.as_ptr(),
                buf_count: bufs.len() as u32,
                offset,
            },
            user_data,
        );
        self.submit(&flow)
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

    /// Register all buffers currently tracked by `registry` with the OS kernel
    /// for zero-copy fixed-buffer I/O (io_uring `IORING_REGISTER_BUFFERS`).
    ///
    /// After this call, `read`/`write` operations whose buffer matches a
    /// registered region base will be issued as `IORING_OP_READ_FIXED` /
    /// `WRITE_FIXED`, skipping per-operation address translation in the kernel.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use tpt_torus_core::lease::LeaseRegistry;
    /// use tpt_torus_core::Torus;
    /// # fn make_torus() -> Torus { unimplemented!() }
    /// let torus = make_torus();
    /// let registry = LeaseRegistry::new();
    /// let mut buf = vec![0u8; 4096];
    /// unsafe { registry.register(buf.as_mut_ptr(), buf.len()) };
    /// torus.register_leases(&registry)?; // enables IORING_OP_READ/WRITE_FIXED
    /// # Ok::<(), tpt_torus_core::Error>(())
    /// ```
    ///
    /// # Platform notes
    /// - Linux (io_uring): registers the regions with the kernel immediately.
    /// - Other platforms: this is a no-op (no fixed-buffer mechanism available).
    #[cfg(unix)]
    pub fn register_leases(&self, registry: &LeaseRegistry) -> crate::error::Result<()> {
        let buffers = registry.as_register_buffers();
        if buffers.is_empty() {
            return Ok(());
        }
        self.backend.lock().unwrap().register_buffers(&buffers)
    }

    /// Register lease buffers with the kernel.
    ///
    /// No-op on platforms without a fixed-buffer mechanism. See the Unix
    /// implementation of [`Torus::register_leases`].
    #[cfg(not(unix))]
    pub fn register_leases(&self, _registry: &LeaseRegistry) -> crate::error::Result<()> {
        Ok(())
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

/// A pool of `Torus` instances that distributes I/O across multiple backends.
///
/// `TorusPool` avoids serializing all I/O through a single `Mutex<dyn Backend>`
/// by maintaining N independent `Torus` instances and distributing operations
/// across them via round-robin. Each `Torus` in the pool has its own backend
/// and ring pair, so submissions on different pool entries are fully concurrent.
///
/// # Example
///
/// ```ignore
/// use tpt_torus_core::TorusPool;
///
/// // Create a pool with 4 Torus instances (one per core)
/// let pool = TorusPool::new(4, 256, |ring_entries| {
///     Box::new(UringBackend::new(ring_entries)?)
/// })?;
///
/// // Submit operations — distributed across pool members
/// pool.submit(&flow)?;
/// ```
pub struct TorusPool {
    instances: Vec<Arc<Torus>>,
    next: AtomicU32,
}

impl TorusPool {
    /// Create a new pool with `count` Torus instances.
    ///
    /// Each instance gets `ring_entries` SQ/CQ entries. The `make_backend`
    /// closure is called once per instance to create the platform-specific backend.
    pub fn new<F>(
        count: usize,
        ring_entries: u32,
        make_backend: F,
    ) -> Result<Self>
    where
        F: Fn(u32) -> Result<Box<dyn Backend>>,
    {
        if count == 0 {
            return Err(Error::InvalidParam("pool count must be > 0"));
        }
        let mut instances = Vec::with_capacity(count);
        for _ in 0..count {
            let backend = make_backend(ring_entries)?;
            instances.push(Arc::new(Torus::new(ring_entries, backend)?));
        }
        Ok(Self {
            instances,
            next: AtomicU32::new(0),
        })
    }

    /// Submit a flow to the next available Torus instance (round-robin).
    pub fn submit(&self, flow: &Flow) -> Result<()> {
        let idx = self.next.fetch_add(1, Ordering::Relaxed) as usize % self.instances.len();
        self.instances[idx].submit(flow)
    }

    /// Submit a batch of flows, distributing them across pool instances.
    pub fn submit_batch(&self, flows: &[Flow]) -> Result<usize> {
        let mut total = 0;
        for flow in flows {
            self.submit(flow)?;
            total += 1;
        }
        Ok(total)
    }

    /// Reap completions from all pool instances.
    pub fn reap(&self, results: &mut Vec<TorusResult>) -> Result<usize> {
        let mut total = 0;
        for instance in &self.instances {
            total += instance.reap(results)?;
        }
        Ok(total)
    }

    /// The number of Torus instances in this pool.
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// Whether the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Get a reference to a specific pool instance.
    pub fn get(&self, index: usize) -> Option<&Torus> {
        self.instances.get(index).map(|arc| arc.as_ref())
    }

    /// Total in-flight operations across all pool instances.
    pub fn in_flight(&self) -> u32 {
        self.instances.iter().map(|i| i.in_flight()).sum()
    }
}
