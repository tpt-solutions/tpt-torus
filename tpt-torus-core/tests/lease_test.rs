//! Tests for the Buffer Leasing system.

use std::sync::Arc;
use std::thread;

use tpt_torus_core::lease::{LeaseError, LeaseRegistry};

#[test]
fn test_register_and_unregister() {
    let registry = LeaseRegistry::new();
    let mut buf = vec![0u8; 1024];
    let ptr = buf.as_mut_ptr();

    unsafe {
        registry.register_mut(ptr, 1024).unwrap();
        assert_eq!(registry.region_count(), 1);

        registry.unregister(ptr).unwrap();
        assert_eq!(registry.region_count(), 0);
    }
}

#[test]
fn test_register_overlap_detection() {
    let registry = LeaseRegistry::new();
    let mut buf1 = vec![0u8; 1024];

    unsafe {
        registry.register_mut(buf1.as_mut_ptr(), 1024).unwrap();

        // Overlapping region should fail
        let overlapping = buf1.as_mut_ptr().add(512);
        let result = registry.register(overlapping as *const u8, 1024);
        assert!(result.is_err());
        match result {
            Err(LeaseError::Overlap { .. }) => {}
            _ => panic!("expected Overlap error"),
        }
    }
}

#[test]
fn test_checkout_and_checkin() {
    let registry = LeaseRegistry::new();
    let mut buf = vec![0u8; 1024];
    let ptr = buf.as_mut_ptr() as usize;

    unsafe {
        registry.register_mut(buf.as_mut_ptr(), 1024).unwrap();
    }

    // Checkout the buffer
    registry.checkout(ptr, 100).unwrap();
    assert!(registry.has_in_flight());

    // Checkin the buffer
    registry.checkin(ptr);
    assert!(!registry.has_in_flight());
}

#[test]
fn test_checkout_out_of_bounds() {
    let registry = LeaseRegistry::new();
    let mut buf = vec![0u8; 100];
    let ptr = buf.as_mut_ptr() as usize;

    unsafe {
        registry.register_mut(buf.as_mut_ptr(), 100).unwrap();
    }

    // Checkout with length exceeding the region
    let result = registry.checkout(ptr, 200);
    assert!(result.is_err());
    match result {
        Err(LeaseError::OutOfBounds { .. }) => {}
        _ => panic!("expected OutOfBounds error"),
    }
}

#[test]
fn test_verify() {
    let registry = LeaseRegistry::new();
    let mut buf = vec![0u8; 1024];
    let ptr = buf.as_mut_ptr() as usize;

    unsafe {
        registry.register_mut(buf.as_mut_ptr(), 1024).unwrap();
    }

    // Not in-flight yet
    assert!(!registry.verify(ptr, 100).unwrap());

    // Checkout
    registry.checkout(ptr, 100).unwrap();
    assert!(registry.verify(ptr, 100).unwrap());

    // Checkin
    registry.checkin(ptr);
    assert!(!registry.verify(ptr, 100).unwrap());
}

#[test]
fn test_unregister_in_flight() {
    let registry = LeaseRegistry::new();
    let mut buf = vec![0u8; 1024];
    let ptr = buf.as_mut_ptr();

    unsafe {
        registry.register_mut(ptr, 1024).unwrap();
    }

    registry.checkout(ptr as usize, 100).unwrap();

    // Can't unregister while in-flight
    unsafe {
        let result = registry.unregister(ptr as *const u8);
        assert!(result.is_err());
        match result {
            Err(LeaseError::InFlight { count }) => assert_eq!(count, 1),
            _ => panic!("expected InFlight error"),
        }
    }

    // Checkin and try again
    registry.checkin(ptr as usize);
    unsafe {
        registry.unregister(ptr as *const u8).unwrap();
    }
    assert_eq!(registry.region_count(), 0);
}

#[test]
fn test_unregister_not_registered() {
    let registry = LeaseRegistry::new();
    let buf = [0u8; 1024];
    let ptr = buf.as_ptr();

    unsafe {
        let result = registry.unregister(ptr);
        assert!(result.is_err());
        match result {
            Err(LeaseError::NotRegistered) => {}
            _ => panic!("expected NotRegistered error"),
        }
    }
}

#[test]
fn test_concurrent_register_no_overlap() {
    // Verify that concurrent registrations of overlapping regions
    // all fail — the TOCTOU fix ensures check+insert is atomic.
    let registry = Arc::new(LeaseRegistry::new());

    // Allocate a shared buffer region
    let mut buf = vec![0u8; 4096];
    let base = buf.as_mut_ptr() as usize;

    unsafe {
        registry.register_mut(buf.as_mut_ptr(), 4096).unwrap();
    }

    let mut handles = vec![];

    // Spawn 8 threads, each trying to register the same overlapping sub-region
    for _ in 0..8 {
        let registry = registry.clone();
        handles.push(thread::spawn(move || {
            // All threads try to register the same overlapping region
            let ptr = (base + 2048) as *const u8;
            let result = unsafe { registry.register(ptr, 64) };
            // ALL should fail with Overlap since the new region is inside the original
            match result {
                Ok(()) => panic!("should not succeed — region overlaps with existing"),
                Err(LeaseError::Overlap { .. }) => {}
                Err(e) => panic!("unexpected error: {:?}", e),
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // Final state: only the original region exists
    assert_eq!(registry.region_count(), 1);
}

#[test]
fn test_concurrent_register_adjacent_no_overlap() {
    // Verify that concurrent registrations of adjacent (non-overlapping) regions
    // all succeed when they don't overlap with existing regions.
    let registry = Arc::new(LeaseRegistry::new());

    let mut handles = vec![];

    // Each thread registers a unique, non-overlapping 64-byte region
    for _ in 0..8 {
        let registry = registry.clone();
        handles.push(thread::spawn(move || {
            let mut region_buf = vec![0u8; 64];
            let ptr = region_buf.as_mut_ptr();
            unsafe {
                registry.register_mut(ptr, 64).unwrap();
            }
            // Verify we can checkout our region
            registry.checkout(ptr as usize, 64).unwrap();
            registry.checkin(ptr as usize);
            // Keep region_buf alive so the memory stays valid
            std::mem::forget(region_buf);
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(registry.region_count(), 8);
}

#[test]
fn test_concurrent_checkout_checkin() {
    // Verify that concurrent checkout/checkin on the same region
    // maintains a consistent in_flight counter.
    let registry = Arc::new(LeaseRegistry::new());
    let mut buf = vec![0u8; 4096];
    let ptr = buf.as_mut_ptr();

    unsafe {
        registry.register_mut(ptr, 4096).unwrap();
    }

    let addr = ptr as usize;
    let mut handles = vec![];

    for _ in 0..8 {
        let registry = registry.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                registry.checkout(addr, 64).unwrap();
                // Simulate some work
                std::hint::black_box(42);
                registry.checkin(addr);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // All checkins should have completed
    assert!(!registry.has_in_flight());
}
