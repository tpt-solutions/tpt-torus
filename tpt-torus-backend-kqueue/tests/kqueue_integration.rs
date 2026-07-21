//! Integration tests for the kqueue backend.
//!
//! These tests require macOS or BSD with kqueue support.

#![cfg(unix)]

use tpt_torus_backend_kqueue::KqueueBackend;
use tpt_torus_core::backend::Backend;
use tpt_torus_core::flow::Flow;
use tpt_torus_core::operation::Operation;

#[test]
fn test_kqueue_backend_creation() {
    let backend = KqueueBackend::new();
    assert!(
        backend.is_ok(),
        "failed to create kqueue backend: {:?}",
        backend.err()
    );
}

#[test]
fn test_file_write_and_read() {
    let backend = KqueueBackend::new().expect("failed to create kqueue backend");

    let test_data = b"hello from torus kqueue!";
    let mut read_buf = vec![0u8; test_data.len()];

    // Create a temp file
    let tmpfile = std::env::temp_dir().join("torus_kqueue_test.txt");
    let write_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmpfile)
        .expect("failed to open temp file");
    let write_fd = std::os::unix::io::AsRawFd::as_raw_fd(&write_file);

    // Submit a write
    let write_flow = Flow::with_user_data(
        Operation::Write {
            fd: write_fd,
            buf: test_data.as_ptr(),
            len: test_data.len(),
            offset: 0,
        },
        1,
    );

    let submitted = backend.submit(&[write_flow]).expect("submit failed");
    assert_eq!(submitted, 1);

    backend.wait(1_000_000).expect("wait failed");

    let mut results = Vec::new();
    backend.reap(&mut results).expect("reap failed");
    assert!(!results.is_empty(), "no completions");
    assert!(results[0].is_ok(), "write failed: {}", results[0].raw());
    assert_eq!(results[0].bytes(), Some(test_data.len()));
    assert_eq!(results[0].user_data, 1);

    drop(write_file);

    // Read back
    let read_file = std::fs::OpenOptions::new()
        .read(true)
        .open(&tmpfile)
        .expect("failed to open temp file for read");
    let read_fd = std::os::unix::io::AsRawFd::as_raw_fd(&read_file);

    let read_flow = Flow::with_user_data(
        Operation::Read {
            fd: read_fd,
            buf: read_buf.as_mut_ptr(),
            len: read_buf.len(),
            offset: 0,
        },
        2,
    );

    let submitted = backend.submit(&[read_flow]).expect("submit failed");
    assert_eq!(submitted, 1);

    backend.wait(1_000_000).expect("wait failed");

    let mut results = Vec::new();
    backend.reap(&mut results).expect("reap failed");
    assert!(!results.is_empty(), "no completions");
    assert!(results[0].is_ok(), "read failed: {}", results[0].raw());
    assert_eq!(results[0].bytes(), Some(test_data.len()));
    assert_eq!(results[0].user_data, 2);

    assert_eq!(&read_buf, test_data);

    // Cleanup
    drop(read_file);
    std::fs::remove_file(&tmpfile).ok();
}

#[test]
fn test_close_operation() {
    let backend = KqueueBackend::new().expect("failed to create kqueue backend");

    let tmpfile = std::env::temp_dir().join("torus_kqueue_close.txt");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmpfile)
        .expect("failed to open temp file");
    let fd = std::os::unix::io::AsRawFd::as_raw_fd(&file);

    // Prevent the file from being closed by Rust's Drop
    std::mem::forget(file);

    let flow = Flow::new(Operation::Close { fd });
    let submitted = backend.submit(&[flow]).expect("submit failed");
    assert_eq!(submitted, 1);

    backend.wait(1_000_000).expect("wait failed");

    let mut results = Vec::new();
    backend.reap(&mut results).expect("reap failed");
    assert!(!results.is_empty());
    assert!(results[0].is_ok());

    std::fs::remove_file(&tmpfile).ok();
}
