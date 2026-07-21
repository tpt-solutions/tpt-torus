//! Integration tests for the IOCP backend.

#![cfg(windows)]

use tpt_torus_backend_iocp::IocpBackend;
use tpt_torus_core::backend::Backend;

#[test]
fn test_iocp_backend_creation() {
    let backend = IocpBackend::new();
    assert!(
        backend.is_ok(),
        "failed to create IOCP backend: {:?}",
        backend.err()
    );
}

#[test]
fn test_backend_drop_cleans_up() {
    {
        let _backend = IocpBackend::new().expect("failed to create IOCP backend");
    }
    // If we get here, the Drop impl ran without panicking
}

#[test]
fn test_in_flight_starts_at_zero() {
    let backend = IocpBackend::new().expect("failed to create IOCP backend");
    assert_eq!(backend.in_flight(), 0);
}
