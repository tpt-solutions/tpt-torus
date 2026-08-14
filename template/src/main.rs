//! A new TPT Torus project (scaffolded via `cargo generate`).
//!
//! TPT Torus unifies Linux io_uring / Windows IOCP / macOS-BSD kqueue behind a
//! single Virtual Torus ring-buffer API. See https://github.com/tpt-solutions/tpt-torus.

use torus::{open, Flow, Operation};

fn main() -> Result<(), torus::Error> {
    // Open a Torus with the platform-default backend (1024 ring entries).
    let torus = open(1024)?;

    // Submit a read of standard input, then wait for and reap the completion.
    let mut buf = [0u8; 1024];
    let flow = Flow::new(Operation::Read {
        fd: 0,
        buf: buf.as_mut_ptr(),
        len: buf.len(),
        offset: 0,
    });
    torus.submit(&flow)?;
    torus.wait(1_000_000)?;

    let mut results = Vec::new();
    torus.reap(&mut results)?;
    for result in &results {
        if let Some(bytes) = result.bytes() {
            println!("read {} bytes from fd {}", bytes, result.user_data);
        }
    }
    Ok(())
}
