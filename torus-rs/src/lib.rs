//! # torus-rs
//!
//! Ergonomic Rust bindings for [TPT Torus](https://github.com/tpt-solutions/tpt-torus),
//! a cross-platform asynchronous I/O library that unifies [`io_uring`] (Linux),
//! IOCP (Windows), and kqueue (macOS/BSD) behind one Virtual Torus API.
//!
//! This crate is a batteries-included facade over [`tpt_torus_core`] and the
//! platform backends. It re-exports the full core API and adds a
//! platform-aware [`open`] helper so you don't have to pick a backend by hand.
//!
//! ## Quick start
//!
//! ```no_run
//! use torus::{open, Flow, Operation};
//!
//! let torus = open(1024)?;
//! let flow = Flow::new(Operation::Read { fd: 0, buf: std::ptr::null_mut(), len: 0, offset: 0 });
//! torus.submit(&flow)?;
//! # Ok::<(), torus::Error>(())
//! ```
//!
//! ## Hardware bypass
//!
//! Enable the `hardware` feature (and `spdk` / `dpdk` / `gpu_direct` as
//! needed) to access the [`hw`] module for user-space NVMe/network I/O.
//!
//! [`io_uring`]: https://en.wikipedia.org/wiki/Io_uring
//! [`tpt_torus_core`]: tpt_torus_core

use tpt_torus_core::backend::Backend;
pub use tpt_torus_core::*;

/// Hardware-bypass extensions (SPDK / DPDK / GPU-Direct).
#[cfg(feature = "hardware")]
pub use tpt_torus_hw as hw;

/// Open a [`Torus`] using the platform-default backend.
///
/// Selects `io_uring` on Linux, IOCP on Windows, and kqueue on macOS/BSD.
/// `ring_entries` must be a power of two.
///
/// # Example
///
/// ```no_run
/// let torus = torus::open(1024)?;
/// # Ok::<(), torus::Error>(())
/// ```
pub fn open(ring_entries: u32) -> crate::Result<Torus> {
    let backend = default_backend(ring_entries)
        .map_err(|e| crate::Error::Backend(format!("failed to create backend: {}", e)))?;
    Torus::new(ring_entries, backend)
}

/// Build the platform-default backend as a boxed [`Backend`].
pub fn default_backend(ring_entries: u32) -> std::result::Result<Box<dyn Backend>, String> {
    #[cfg(all(
        unix,
        not(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "openbsd",
            target_os = "netbsd"
        ))
    ))]
    {
        Ok(Box::new(tpt_torus_backend_uring::UringBackend::new(
            ring_entries,
        )?))
    }

    #[cfg(target_os = "windows")]
    {
        let _ = ring_entries;
        Ok(Box::new(tpt_torus_backend_iocp::IocpBackend::new()?))
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    {
        let _ = ring_entries;
        Ok(Box::new(tpt_torus_backend_kqueue::KqueueBackend::new()?))
    }

    #[cfg(not(any(
        unix,
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    {
        let _ = ring_entries;
        Err("no supported backend for this target".to_string())
    }
}
