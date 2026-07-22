//! Cross-platform TCP echo server using TPT Torus.
//!
//! This example demonstrates the Flow/Result API for socket I/O.
//! It listens on a TCP port, accepts connections, and echoes back
//! any data received.
//!
//! Run with: `cargo run --example echo_server`
//!
//! NOTE: The backend is selected per-platform:
//! - Linux: io_uring
//! - Windows: IOCP
//! - macOS/BSD: kqueue

#[cfg(target_os = "linux")]
fn create_backend(entries: u32) -> Result<Box<dyn tpt_torus_core::backend::Backend>, Box<dyn std::error::Error>> {
    Ok(Box::new(tpt_torus_backend_uring::UringBackend::new(entries)?))
}

#[cfg(target_os = "windows")]
fn create_backend(entries: u32) -> Result<Box<dyn tpt_torus_core::backend::Backend>, Box<dyn std::error::Error>> {
    Ok(Box::new(tpt_torus_backend_iocp::IocpBackend::new(entries)?))
}

#[cfg(target_os = "macos")]
fn create_backend(entries: u32) -> Result<Box<dyn tpt_torus_core::backend::Backend>, Box<dyn std::error::Error>> {
    Ok(Box::new(tpt_torus_backend_kqueue::KqueueBackend::new(entries)?))
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn create_backend(_entries: u32) -> Result<Box<dyn tpt_torus_core::backend::Backend>, Box<dyn std::error::Error>> {
    Err("Unsupported platform".into())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = create_backend(256)?;
    let torus = tpt_torus_core::Torus::new(256, backend)?;

    // Create a TCP listener
    let listener = std::net::TcpListener::bind("127.0.0.1:8080")?;
    listener.set_nonblocking(true)?;
    let listener_fd = std::os::unix::io::AsRawFd::as_raw_fd(&listener);

    println!("Echo server listening on 127.0.0.1:8080");

    let mut buf = vec![0u8; 4096];
    let mut results = Vec::new();

    loop {
        // Try to accept a connection
        let accept_flow = tpt_torus_core::flow::Flow::with_user_data(
            tpt_torus_core::operation::Operation::Accept {
                fd: listener_fd,
                addr: std::ptr::null_mut(),
                addrlen: std::ptr::null_mut(),
            },
            0,
        );

        let _ = torus.submit(&accept_flow);
        let _ = torus.wait(100_000); // 100ms timeout
        let count = torus.reap(&mut results)?;

        for result in &results {
            if result.is_ok() {
                let client_fd = result.bytes().unwrap_or(0) as i32;
                println!("Accepted connection, fd={}", client_fd);

                // Read from client
                let read_flow = tpt_torus_core::flow::Flow::with_user_data(
                    tpt_torus_core::operation::Operation::Read {
                        fd: client_fd,
                        buf: buf.as_mut_ptr(),
                        len: buf.len(),
                        offset: 0,
                    },
                    1,
                );
                let _ = torus.submit(&read_flow);
                let _ = torus.wait(1_000_000);
                let _ = torus.reap(&mut results);

                if let Some(last) = results.last() {
                    if last.is_ok() {
                        let n = last.bytes().unwrap_or(0) as usize;
                        if n > 0 {
                            // Echo back
                            let write_flow = tpt_torus_core::flow::Flow::with_user_data(
                                tpt_torus_core::operation::Operation::Write {
                                    fd: client_fd,
                                    buf: buf.as_ptr(),
                                    len: n,
                                    offset: 0,
                                },
                                2,
                            );
                            let _ = torus.submit(&write_flow);
                            let _ = torus.wait(1_000_000);
                            let _ = torus.reap(&mut results);
                            println!("Echoed {} bytes", n);
                        }
                    }
                }
            }
        }
        results.clear();
    }
}
