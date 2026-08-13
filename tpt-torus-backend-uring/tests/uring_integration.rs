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

/// `IORING_OP_RECV` multishot (`IORING_RECV_MULTISHOT`) was added in Linux
/// 5.20. Older kernels reject the submission with `-EINVAL`, so skip the test
/// there — it verifies a feature the running kernel can't exercise.
fn kernel_supports_recv_multishot() -> bool {
    if let Ok(release) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        let mut it = release.trim().split('.');
        let major = it.next().and_then(|s| s.parse::<u32>().ok());
        let minor = it.next().and_then(|s| s.parse::<u32>().ok());
        if let (Some(major), Some(minor)) = (major, minor) {
            return (major, minor) >= (5, 20);
        }
    }
    // If we can't determine the version, assume support so the test still runs.
    true
}

/// Proves `submit_multi_recv` actually arms io_uring's multishot mode: a
/// single submission must yield more than one completion as separate reads
/// arrive on the same socket, without the caller re-submitting recv in
/// between. This is a regression test for a bug where the multishot bit was
/// written to the wrong SQE field (`op_flags` instead of `ioprio`), which
/// silently fell back to one-shot behavior.
#[test]
fn test_multi_shot_recv_yields_multiple_completions() {
    if !kernel_supports_recv_multishot() {
        eprintln!(
            "skipping test_multi_shot_recv_yields_multiple_completions: \
             kernel < 5.20 lacks IORING_RECV_MULTISHOT"
        );
        return;
    }

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
        .submit_multi_recv(fd, buf.as_mut_ptr(), buf.len(), 2, 99)
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

    // The multishot op is still armed, so `in_flight` must stay non-zero even
    // though one completion was already reaped. (This is the bug the fix
    // addresses: previously `reap()` dropped `in_flight` to 0 after the first
    // completion, making `wait()`'s `min_complete` heuristic return 0 and bail
    // immediately instead of blocking for the next completion.)
    assert_eq!(backend.in_flight(), 1);

    // Let the client send the second chunk only now, then wait for the next
    // completion without resubmitting — proves the op is still armed and that
    // `wait()` now blocks correctly (min_complete=1) for the armed multishot op.
    tx_go.send(()).expect("sync send failed");
    backend.wait(5_000_000).expect("wait 2 failed");
    let mut results = Vec::new();
    backend.reap(&mut results).expect("reap 2 failed");
    // The peer's socket closes when the client thread returns, so the kernel
    // may have also queued an EOF completion (`res == 0`, no MORE bit) during
    // the wait. Assert that the "second" payload arrived rather than requiring
    // exactly one completion.
    let got_second = results.iter().any(|r| {
        r.is_ok()
            && r.user_data == 99
            && r.bytes().map_or(false, |n| n > 0 && &buf[..n] == b"second")
    });
    assert!(got_second, "second completion never arrived");

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
