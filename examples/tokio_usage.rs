//! Example of using TPT Torus with the tokio runtime.
//!
//! The `TorusAsync` API works with any async runtime that properly handles
//! waker registration (tokio, async-std, smol, etc.).
//!
//! Run with: `cargo run --example tokio_usage`
//!
//! NOTE: Requires the `tokio` crate. Add to Cargo.toml:
//! ```toml
//! [dependencies]
//! tokio = { version = "1", features = ["full"] }
//! ```

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = create_backend(256)?;
    let torus = tpt_torus_core::async_api::TorusAsync::new(256, backend)?;

    // Create a temporary file for the demo
    let tmpfile = std::env::temp_dir().join("torus_tokio_demo.txt");
    let data = b"Hello from tokio + TPT Torus!\n";

    // Write to file using the async API
    {
        let file = std::fs::File::create(&tmpfile)?;
        #[cfg(unix)]
        let fd = std::os::unix::io::AsRawFd::as_raw_fd(&file);
        #[cfg(windows)]
        let fd = std::os::windows::io::AsRawHandle::as_raw_handle(&file) as i32;

        let write_result = torus.write(fd, data, 0)?;
        println!("Write: {} bytes", write_result);
    }

    // Read from file using the async API
    {
        let file = std::fs::File::open(&tmpfile)?;
        #[cfg(unix)]
        let fd = std::os::unix::io::AsRawFd::as_raw_fd(&file);
        #[cfg(windows)]
        let fd = std::os::windows::io::AsRawHandle::as_raw_handle(&file) as i32;

        let mut buf = vec![0u8; 4096];
        let n = torus.read(fd, &mut buf, 0)?;
        let contents = String::from_utf8_lossy(&buf[..n]);
        println!("Read: {} bytes: {:?}", n, contents);
    }

    // Cleanup
    std::fs::remove_file(&tmpfile)?;

    println!("Done! TPT Torus works with tokio.");
    Ok(())
}
