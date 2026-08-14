//! Basic file I/O example using the TPT Torus kqueue backend (macOS/BSD).
//!
//! NOTE: This example requires macOS/BSD with kqueue support.
//! Run with: `cargo run --example file_io -p tpt-torus-backend-kqueue`

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::io::AsRawFd;

    use tpt_torus_backend_kqueue::KqueueBackend;
    use tpt_torus_core::backend::Backend;
    use tpt_torus_core::flow::Flow;
    use tpt_torus_core::operation::Operation;

    // Create a kqueue backend with a 256-entry ring.
    let backend = KqueueBackend::new()?;

    // Prepare a file for writing.
    let tmpfile = std::env::temp_dir().join("torus_kqueue_example.txt");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmpfile)?;
    let fd = file.as_raw_fd();

    // Submit a write operation.
    let message = b"Hello from TPT Torus (kqueue)!\n";
    let write_flow = Flow::with_user_data(
        Operation::Write {
            fd,
            buf: message.as_ptr(),
            len: message.len(),
            offset: 0,
        },
        1, // user data to identify this operation
    );

    let submitted = backend.submit(&[write_flow])?;
    println!("Submitted {} operation(s)", submitted);

    // Wait for completion, then reap the result.
    backend.wait(1_000_000)?;
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

    // Read it back.
    drop(file);
    let file = std::fs::File::open(&tmpfile)?;
    let fd = file.as_raw_fd();
    let mut buf = vec![0u8; 64];
    let read_flow = Flow::with_user_data(
        Operation::Read {
            fd,
            buf: buf.as_mut_ptr(),
            len: buf.len(),
            offset: 0,
        },
        2,
    );

    let _ = backend.submit(&[read_flow])?;
    backend.wait(1_000_000)?;
    let mut results = Vec::new();
    let _ = backend.reap(&mut results)?;
    for result in &results {
        if result.is_ok() {
            let n = result.bytes().unwrap_or(0);
            println!(
                "  Read {} bytes: {:?}",
                n,
                String::from_utf8_lossy(&buf[..n])
            );
        }
    }

    drop(file);
    let _ = std::fs::remove_file(&tmpfile);

    println!("Done!");
    Ok(())
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
fn main() {
    println!("This example requires macOS/BSD with kqueue support.");
}
