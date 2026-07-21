//! Integration tests for the io_uring backend.
//!
//! These tests require Linux with io_uring support (kernel 5.1+).

#![cfg(target_os = "linux")]

use tpt_torus_backend_uring::UringBackend;
use tpt_torus_core::backend::Backend;
use tpt_torus_core::flow::Flow;
use tpt_torus_core::operation::Operation;

#[test]
fn test_uring_backend_creation() {
    let backend = UringBackend::new(256);
    assert!(
        backend.is_ok(),
        "failed to create uring backend: {:?}",
        backend.err()
    );
}

#[test]
fn test_file_write_and_read() {
    let backend = UringBackend::new(256).expect("failed to create uring backend");

    let test_data = b"hello from torus!";
    let mut read_buf = vec![0u8; test_data.len()];

    // Create a temp file
    let tmpfile = std::env::temp_dir().join("torus_test_file.txt");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmpfile)
        .expect("failed to open temp file");
    let fd = std::os::unix::io::AsRawFd::as_raw_fd(&file);

    // Submit a write
    let write_flow = Flow::with_user_data(
        Operation::Write {
            fd,
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
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok(), "write failed: {}", results[0].raw());
    assert_eq!(results[0].bytes(), Some(test_data.len()));
    assert_eq!(results[0].user_data, 1);

    drop(file);

    // Read back
    let file = std::fs::OpenOptions::new()
        .read(true)
        .open(&tmpfile)
        .expect("failed to open temp file for read");
    let fd = std::os::unix::io::AsRawFd::as_raw_fd(&file);

    let read_flow = Flow::with_user_data(
        Operation::Read {
            fd,
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
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok(), "read failed: {}", results[0].raw());
    assert_eq!(results[0].bytes(), Some(test_data.len()));
    assert_eq!(results[0].user_data, 2);

    assert_eq!(&read_buf, test_data);

    // Cleanup
    drop(file);
    std::fs::remove_file(&tmpfile).ok();
}

#[test]
fn test_batch_submit() {
    let backend = UringBackend::new(256).expect("failed to create uring backend");

    let tmpfile = std::env::temp_dir().join("torus_test_batch.txt");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmpfile)
        .expect("failed to open temp file");
    let fd = std::os::unix::io::AsRawFd::as_raw_fd(&file);

    let data1 = b"first ";
    let data2 = b"second ";

    let flows = vec![
        Flow::with_user_data(
            Operation::Write {
                fd,
                buf: data1.as_ptr(),
                len: data1.len(),
                offset: 0,
            },
            10,
        ),
        Flow::with_user_data(
            Operation::Write {
                fd,
                buf: data2.as_ptr(),
                len: data2.len(),
                offset: data1.len() as u64,
            },
            20,
        ),
    ];

    let submitted = backend.submit(&flows).expect("batch submit failed");
    assert_eq!(submitted, 2);

    backend.wait(1_000_000).expect("wait failed");

    let mut results = Vec::new();
    let count = backend.reap(&mut results).expect("reap failed");
    assert_eq!(count, 2);

    // Verify results contain both user data values
    let user_data: Vec<u64> = results.iter().map(|r| r.user_data).collect();
    assert!(user_data.contains(&10));
    assert!(user_data.contains(&20));

    // Verify file contents
    drop(file);
    let contents = std::fs::read(&tmpfile).expect("read file");
    assert_eq!(&contents, b"first second ");

    std::fs::remove_file(&tmpfile).ok();
}

#[test]
fn test_in_flight_counter() {
    let backend = UringBackend::new(256).expect("failed to create uring backend");
    assert_eq!(backend.in_flight(), 0);

    let tmpfile = std::env::temp_dir().join("torus_test_inflight.txt");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmpfile)
        .expect("failed to open temp file");
    let fd = std::os::unix::io::AsRawFd::as_raw_fd(&file);

    let data = b"test";
    let flow = Flow::new(Operation::Write {
        fd,
        buf: data.as_ptr(),
        len: data.len(),
        offset: 0,
    });

    backend.submit(&[flow]).expect("submit failed");
    assert_eq!(backend.in_flight(), 1);

    backend.wait(1_000_000).expect("wait failed");

    let mut results = Vec::new();
    backend.reap(&mut results).expect("reap failed");
    assert_eq!(backend.in_flight(), 0);

    drop(file);
    std::fs::remove_file(&tmpfile).ok();
}
