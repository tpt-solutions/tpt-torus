//! Basic file I/O example using the TPT Torus IOCP backend (Windows).
//!
//! NOTE: This example requires Windows with IOCP support.
//! Run with: `cargo run --example file_io -p tpt-torus-backend-iocp`

#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    use tpt_torus_backend_iocp::IocpBackend;
    use tpt_torus_core::backend::Backend;
    use tpt_torus_core::flow::Flow;
    use tpt_torus_core::operation::Operation;

    // FILE_FLAG_OVERLAPPED is required so the file handle can be bound to the
    // IOCP port for asynchronous completion.
    const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;

    // Create an IOCP backend with a 256-entry ring.
    let backend = IocpBackend::new()?;

    // Prepare a file for writing.
    let tmpfile = std::env::temp_dir().join("torus_iocp_example.txt");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .custom_flags(FILE_FLAG_OVERLAPPED)
        .open(&tmpfile)?;

    // Bind the file HANDLE to the IOCP port so its completions are posted there.
    let handle = file.as_raw_handle();
    unsafe {
        if !backend.associate(handle as windows_sys::Win32::Foundation::HANDLE) {
            return Err("failed to associate file handle with IOCP".into());
        }
    }
    let fd = handle as i32;

    // Submit a write operation.
    let message = b"Hello from TPT Torus (IOCP)!\n";
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
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OVERLAPPED)
        .open(&tmpfile)?;
    let handle = file.as_raw_handle();
    unsafe {
        if !backend.associate(handle as windows_sys::Win32::Foundation::HANDLE) {
            return Err("failed to associate file handle with IOCP".into());
        }
    }
    let fd = handle as i32;
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

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("This example requires Windows with IOCP support.");
}
