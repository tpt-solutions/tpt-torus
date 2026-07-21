//! Integration tests for the C++ FFI layer.

/// Test that the FFI types have correct sizes.
#[test]
fn test_torus_operation_size() {
    // TorusOperation must match the C++ layout
    // op_type(4) + fd(4) + buf(8) + len(8) + offset(8) + user_data(8) = 40
    assert_eq!(std::mem::size_of::<tpt_torus_cxx::TorusOperation>(), 40);
}

/// Test that completion types have correct sizes.
#[test]
fn test_torus_completion_size() {
    // result(8) + user_data(8) = 16
    assert_eq!(std::mem::size_of::<tpt_torus_cxx::TorusCompletion>(), 16);
}

/// Test that the FFI functions are callable (with null handle).
#[test]
fn test_torus_in_flight_null_handle() {
    // Should return 0 for null handle
    let result = unsafe { tpt_torus_cxx::torus_in_flight(std::ptr::null()) };
    assert_eq!(result, 0);
}
