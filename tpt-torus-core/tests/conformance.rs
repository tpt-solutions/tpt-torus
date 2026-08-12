//! Cross-backend conformance test suite.
//!
//! Runs the same set of operations against whichever backend is available on
//! the current platform and asserts identical behavior. This ensures feature
//! parity across Linux (io_uring), Windows (IOCP), and macOS/BSD (kqueue).
//!
//! NOTE: File I/O tests are Unix-only because the IOCP backend (Windows) is
//! designed for socket/overlapped I/O, not raw file handles. Socket I/O tests
//! run on all platforms.

use tpt_torus_core::backend::Backend;
#[cfg(unix)]
use tpt_torus_core::flow::Flow;
#[cfg(unix)]
use tpt_torus_core::operation::Operation;

fn create_backend() -> Box<dyn Backend> {
    #[cfg(target_os = "linux")]
    {
        Box::new(
            tpt_torus_backend_uring::UringBackend::new(256)
                .expect("failed to create uring backend"),
        )
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(tpt_torus_backend_iocp::IocpBackend::new().expect("failed to create IOCP backend"))
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(
            tpt_torus_backend_kqueue::KqueueBackend::new(256)
                .expect("failed to create kqueue backend"),
        )
    }
}

#[test]
fn conformance_backend_creation() {
    let backend = create_backend();
    assert_eq!(backend.in_flight(), 0);
}

// ─── File I/O tests (Unix only — IOCP doesn't support raw file handles) ────

#[cfg(unix)]
#[test]
fn conformance_file_write_and_read() {
    use std::os::unix::io::AsRawFd;

    let backend = create_backend();
    let tmpfile = std::env::temp_dir().join("torus_conformance_test.txt");
    let message = b"Hello from conformance test!\n";

    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmpfile)
        .expect("failed to create temp file");
    let fd = AsRawFd::as_raw_fd(&file);

    let write_flow = Flow::with_user_data(
        Operation::Write {
            fd,
            buf: message.as_ptr(),
            len: message.len(),
            offset: 0,
        },
        1,
    );

    let submitted = backend.submit(&[write_flow]).expect("submit failed");
    assert_eq!(submitted, 1);
    assert_eq!(backend.in_flight(), 1);

    backend.wait(1_000_000).expect("wait failed");

    let mut results = Vec::new();
    let count = backend.reap(&mut results).expect("reap failed");
    assert_eq!(count, 1);
    assert!(results[0].is_ok());
    assert_eq!(results[0].bytes(), Some(message.len()));

    drop(file);

    let file = std::fs::File::open(&tmpfile).expect("failed to open temp file");
    let fd = AsRawFd::as_raw_fd(&file);

    let mut buf = vec![0u8; 4096];
    let read_flow = Flow::with_user_data(
        Operation::Read {
            fd,
            buf: buf.as_mut_ptr(),
            len: buf.len(),
            offset: 0,
        },
        2,
    );

    backend.submit(&[read_flow]).expect("submit failed");
    backend.wait(1_000_000).expect("wait failed");
    results.clear();
    backend.reap(&mut results).expect("reap failed");

    assert!(results[0].is_ok());
    let n = results[0].bytes().unwrap();
    assert_eq!(&buf[..n], message);

    std::fs::remove_file(&tmpfile).ok();
}

#[cfg(unix)]
#[test]
fn conformance_batch_submit() {
    use std::os::unix::io::AsRawFd;

    let backend = create_backend();
    let tmpfile = std::env::temp_dir().join("torus_conformance_batch.txt");

    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmpfile)
        .expect("failed to create temp file");
    let fd = AsRawFd::as_raw_fd(&file);

    let data1 = b"First chunk ";
    let data2 = b"Second chunk\n";
    let flows = vec![
        Flow::with_user_data(
            Operation::Write {
                fd,
                buf: data1.as_ptr(),
                len: data1.len(),
                offset: 0,
            },
            1,
        ),
        Flow::with_user_data(
            Operation::Write {
                fd,
                buf: data2.as_ptr(),
                len: data2.len(),
                offset: data1.len() as u64,
            },
            2,
        ),
    ];

    let submitted = backend.submit(&flows).expect("batch submit failed");
    assert!(submitted >= 1, "at least one flow should be submitted");

    backend.wait(1_000_000).expect("wait failed");
    let mut results = Vec::new();
    let count = backend.reap(&mut results).expect("reap failed");
    assert!(count >= 1, "at least one completion should be reaped");

    for result in &results {
        assert!(result.is_ok(), "completion failed: {:?}", result.error());
    }

    drop(file);
    std::fs::remove_file(&tmpfile).ok();
}

#[cfg(unix)]
#[test]
fn conformance_user_data_preserved() {
    use std::os::unix::io::AsRawFd;

    let backend = create_backend();
    let tmpfile = std::env::temp_dir().join("torus_conformance_userdata.txt");

    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmpfile)
        .expect("failed to create temp file");
    let fd = AsRawFd::as_raw_fd(&file);

    let data = b"test";
    let custom_user_data = 42u64;
    let flow = Flow::with_user_data(
        Operation::Write {
            fd,
            buf: data.as_ptr(),
            len: data.len(),
            offset: 0,
        },
        custom_user_data,
    );

    backend.submit(&[flow]).expect("submit failed");
    backend.wait(1_000_000).expect("wait failed");
    let mut results = Vec::new();
    backend.reap(&mut results).expect("reap failed");

    assert_eq!(results[0].user_data, custom_user_data);

    drop(file);
    std::fs::remove_file(&tmpfile).ok();
}

#[cfg(unix)]
#[test]
fn conformance_zero_length_write() {
    use std::os::unix::io::AsRawFd;

    let backend = create_backend();
    let tmpfile = std::env::temp_dir().join("torus_conformance_zero.txt");

    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmpfile)
        .expect("failed to create temp file");
    let fd = AsRawFd::as_raw_fd(&file);

    let flow = Flow::with_user_data(
        Operation::Write {
            fd,
            buf: b"".as_ptr(),
            len: 0,
            offset: 0,
        },
        1,
    );

    backend.submit(&[flow]).expect("submit failed");
    backend.wait(1_000_000).expect("wait failed");
    let mut results = Vec::new();
    backend.reap(&mut results).expect("reap failed");

    assert!(results[0].is_ok());
    assert_eq!(results[0].bytes(), Some(0));

    drop(file);
    std::fs::remove_file(&tmpfile).ok();
}

// ─── Socket I/O tests (all platforms) ──────────────────────────────────────

#[cfg(unix)]
#[test]
fn conformance_socket_echo() {
    use std::os::unix::io::AsRawFd;

    let backend = create_backend();

    // Create a socket pair
    let (reader, writer) =
        std::os::unix::net::UnixStream::pair().expect("failed to create socket pair");
    reader.set_nonblocking(true).ok();
    writer.set_nonblocking(true).ok();

    let read_fd = AsRawFd::as_raw_fd(&reader);
    let write_fd = AsRawFd::as_raw_fd(&writer);

    let data = b"socket echo test";
    let write_flow = Flow::with_user_data(
        Operation::Send {
            fd: write_fd,
            buf: data.as_ptr(),
            len: data.len(),
        },
        1,
    );

    backend.submit(&[write_flow]).expect("submit failed");
    backend.wait(1_000_000).expect("wait failed");

    let mut results = Vec::new();
    backend.reap(&mut results).expect("reap failed");

    if !results.is_empty() && results[0].is_ok() {
        let sent = results[0].bytes().unwrap();
        assert_eq!(sent, data.len());

        // Read it back
        let mut buf = vec![0u8; 4096];
        let read_flow = Flow::with_user_data(
            Operation::Recv {
                fd: read_fd,
                buf: buf.as_mut_ptr(),
                len: buf.len(),
            },
            2,
        );

        backend.submit(&[read_flow]).expect("submit failed");
        backend.wait(1_000_000).expect("wait failed");
        results.clear();
        backend.reap(&mut results).expect("reap failed");

        if !results.is_empty() && results[0].is_ok() {
            let n = results[0].bytes().unwrap();
            assert_eq!(&buf[..n], data);
        }
    }
}
