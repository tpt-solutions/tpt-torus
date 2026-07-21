//! Tests for the Buffer Leasing system.

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
