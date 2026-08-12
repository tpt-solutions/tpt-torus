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

/// Proves `submit_multi_recv` actually arms io_uring's multishot mode: a
/// single submission must yield more than one completion as separate reads
/// arrive on the same socket, without the caller re-submitting recv in
/// between. This is a regression test for a bug where the multishot bit was
/// written to the wrong SQE field (`op_flags` instead of `ioprio`), which
/// silently fell back to one-shot behavior.
#[test]
fn test_multi_shot_recv_yields_multiple_completions() {
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::io::AsRawFd;
    use std::sync::mpsc;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind failed");
    let addr = listener.local_addr().expect("local_addr failed");

    let (tx_go, rx_go) = mpsc::channel::<()>();
    let (tx_done, rx_done) = mpsc::channel::<()>();

    let client = thread::spawn(move || {
        let mut stream = TcpStream::connect(addr).expect("connect failed");
        // Send the first chunk, then wait until the server has reaped it
        // before sending the second, so each completion is checked against
        // an undisturbed buffer.
        stream.write_all(b"first!").expect("write 1 failed");
        rx_go.recv().expect("sync recv failed");
        stream.write_all(b"second").expect("write 2 failed");
        tx_done.send(()).ok();
    });

    let (server_stream, _) = listener.accept().expect("accept failed");
    let fd = server_stream.as_raw_fd();

    let backend = UringBackend::new(256).expect("failed to create uring backend");

    let mut buf = vec![0u8; 64];
    backend
        .submit_multi_recv(fd, buf.as_mut_ptr(), buf.len(), 99)
        .expect("submit_multi_recv failed");

    // First completion.
    backend.wait(5_000_000).expect("wait 1 failed");
    let mut results = Vec::new();
    backend.reap(&mut results).expect("reap 1 failed");
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok(), "recv 1 failed: {}", results[0].raw());
    assert_eq!(results[0].user_data, 99);
    let n1 = results[0].bytes().expect("missing byte count");
    assert_eq!(&buf[..n1], b"first!");

    // Let the client send the second chunk only now, then reap without
    // resubmitting — proves the op is still armed from the one submission.
    //
    // Note: `reap()` decrements `in_flight` back to 0 after the first
    // completion above, so `wait()`'s `min_complete` would be 0 here and it
    // would return immediately instead of blocking for the second
    // completion. Poll `reap()` directly instead of relying on `wait()`.
    tx_go.send(()).expect("sync send failed");
    let mut results = Vec::new();
    for _ in 0..500 {
        backend.reap(&mut results).expect("reap 2 failed");
        if !results.is_empty() {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(results.len(), 1, "second completion never arrived");
    assert!(results[0].is_ok(), "recv 2 failed: {}", results[0].raw());
    assert_eq!(results[0].user_data, 99);
    let n2 = results[0].bytes().expect("missing byte count");
    assert_eq!(&buf[..n2], b"second");

    rx_done.recv().expect("client join signal failed");
    client.join().expect("client thread panicked");
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
