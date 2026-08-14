//! Basic file I/O example using the TPT Torus Flow/Result API.
//!
//! NOTE: This example requires Linux with io_uring support (kernel 5.1+).
//! Run with: cargo run --example file_io -p tpt-torus-backend-uring

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use tpt_torus_backend_uring::UringBackend;
    use tpt_torus_core::backend::Backend;
    use tpt_torus_core::flow::Flow;
    use tpt_torus_core::operation::Operation;

    // Create an io_uring backend with 256 entries
    let backend = UringBackend::new(256)?;

    // Prepare a file for writing
    let tmpfile = std::env::temp_dir().join("torus_example.txt");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmpfile)?;
    let fd = std::os::unix::io::AsRawFd::as_raw_fd(&file);

    // Create a write operation
    let message = b"Hello from TPT Torus!\n";
    let write_flow = Flow::with_user_data(
        Operation::Write {
            fd,
            buf: message.as_ptr(),
            len: message.len(),
            offset: 0,
        },
        1, // user data to identify this operation
    );

    // Submit the write
    let submitted = backend.submit(&[write_flow])?;
    println!("Submitted {} operation(s)", submitted);

    // Wait for completion
    backend.wait(1_000_000)?;

    // Reap the result
    let mut results = Vec::new();
    let count = backend.reap(&mut results)?;
    println!("Reaped {} completion(s)", count);

    for result in &results {
        if result.is_ok() {
            println!(
                "  Operation {} completed: {} bytes written",
                result.user_data,
                result.bytes().unwrap_or(0)
            );
        } else {
            eprintln!(
                "  Operation {} failed: errno {}",
                result.user_data,
                result.error().unwrap_or(0)
            );
        }
    }

    // Verify the file contents
    drop(file);
    let contents = std::fs::read_to_string(&tmpfile)?;
    println!("File contents: {:?}", contents);

    // Cleanup
    std::fs::remove_file(&tmpfile)?;

    println!("Done!");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("This example requires Linux with io_uring support (kernel 5.1+).");
}
