//! Cross-platform file copy using TPT Torus.
//!
//! This example demonstrates the Flow/Result API for file I/O.
//! It copies a file using the Torus async I/O framework.
//!
//! Run with: `cargo run --example file_copy -- <source> <dest>`
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
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <source> <dest>", args[0]);
        std::process::exit(1);
    }

    let source = &args[1];
    let dest = &args[2];

    let backend = create_backend(256)?;
    let torus = tpt_torus_core::Torus::new(256, backend)?;

    let src_file = std::fs::File::open(source)?;
    let dst_file = std::fs::File::create(dest)?;

    #[cfg(unix)]
    let src_fd = std::os::unix::io::AsRawFd::as_raw_fd(&src_file);
    #[cfg(unix)]
    let dst_fd = std::os::unix::io::AsRawFd::as_raw_fd(&dst_file);

    #[cfg(windows)]
    let src_fd = std::os::windows::io::AsRawSocket::as_raw_socket(&src_file) as i32;
    #[cfg(windows)]
    let dst_fd = std::os::windows::io::AsRawSocket::as_raw_socket(&dst_file) as i32;

    let mut buf = vec![0u8; 65536]; // 64KB buffer
    let mut offset: u64 = 0;
    let mut results = Vec::new();
    let mut total_bytes: u64 = 0;

    loop {
        // Read from source
        let read_flow = tpt_torus_core::flow::Flow::with_user_data(
            tpt_torus_core::operation::Operation::Read {
                fd: src_fd,
                buf: buf.as_mut_ptr(),
                len: buf.len(),
                offset,
            },
            1,
        );

        torus.submit(&read_flow)?;
        torus.wait(1_000_000)?;
        torus.reap(&mut results)?;

        let n = results.last()
            .and_then(|r| r.bytes())
            .unwrap_or(0) as usize;

        if n == 0 {
            break; // EOF
        }

        // Write to destination
        let write_flow = tpt_torus_core::flow::Flow::with_user_data(
            tpt_torus_core::operation::Operation::Write {
                fd: dst_fd,
                buf: buf.as_ptr(),
                len: n,
                offset,
            },
            2,
        );

        torus.submit(&write_flow)?;
        torus.wait(1_000_000)?;
        torus.reap(&mut results)?;

        total_bytes += n as u64;
        offset += n as u64;
        results.clear();
    }

    println!("Copied {} bytes from {} to {}", total_bytes, source, dest);
    Ok(())
}
