//! Integration tests for the C++ FFI layer.

use tpt_torus_cxx::TorusHandle;
use tpt_torus_cxx::TorusOperation;
use tpt_torus_cxx::TorusCompletion;

#[test]
fn test_torus_handle_guard_create_destroy() {
    // We can't create a real backend from C, so we test the guard with null
    let guard = tpt_torus_cxx::TorusHandleGuard(std::ptr::null_mut());
    assert!(guard.get().is_null());
    assert!(guard.release().is_null());
}

#[test]
fn test_torus_handle_guard_move() {
    let guard1 = tpt_torus_cxx::TorusHandleGuard(std::ptr::null_mut());
    let guard2 = guard1;
    assert!(guard2.get().is_null());
}

#[test]
fn test_result_ok() {
    let result = tpt_torus_cxx::Result::new(42);
    assert!(result.ok());
    assert_eq!(result.bytes(), 42);
    assert_eq!(result.error(), 0);
}

#[test]
fn test_result_error() {
    let result = tpt_torus_cxx::Result::new(-22);
    assert!(!result.ok());
    assert_eq!(result.error(), 22);
    assert_eq!(result.error_message(), "Invalid argument");
}

#[test]
fn test_torus_operation_layout() {
    // Verify the C-compatible struct has the expected layout
    assert_eq!(std::mem::size_of::<TorusOperation>(), 40);
    assert_eq!(std::mem::align_of::<TorusOperation>(), 8);
}

#[test]
fn test_torus_completion_layout() {
    assert_eq!(std::mem::size_of::<TorusCompletion>(), 16);
    assert_eq!(std::mem::align_of::<TorusCompletion>(), 8);
}

#[test]
fn test_buffer_view() {
    let data = [1u8, 2, 3, 4, 5];
    let view = tpt_torus_cxx::BufferView::new(data.as_ptr() as *mut u8, data.len());
    assert_eq!(view.size, 5);
}

#[test]
fn test_const_buffer_view() {
    let data = b"hello";
    let view = tpt_torus_cxx::ConstBufferView::new(data.as_ptr(), data.len());
    assert_eq!(view.size, 5);
}
