//! Minimal `TorusAsync` example — read a file using the ergonomic async facade.
//!
//! This shows the high-level API that covers ~90% of use cases: no manual
//! ring management, no waker wiring. `TorusAsync` picks up completions via a
//! background reaper thread and wakes the awaiting task.
//!
//! Run with: `cargo run --example hello_read -p tpt-torus-core`
//!
//! NOTE: On Windows the fd-based `TorusAsync` API requires the file handle to
//! be bound to the IOCP port (via `IocpBackend::associate`), which is not yet
//! exposed through this facade; this example therefore runs on Linux and
//! macOS/BSD only.

#[cfg(not(target_os = "windows"))]
use std::future::Future;
#[cfg(not(target_os = "windows"))]
use std::task::{Context, Poll, Wake};

#[cfg(not(target_os = "windows"))]
use tpt_torus_core::async_api::TorusAsync;
#[cfg(not(target_os = "windows"))]
use tpt_torus_core::backend::Backend;

/// Tiny single-threaded executor so the example has no external runtime dep.
///
/// The `TorusAsync` reaper thread calls `Waker::wake` on completion; our waker
/// just unparks this thread, and we re-poll until the future is `Ready`.
#[cfg(not(target_os = "windows"))]
fn block_on<F: Future>(fut: F) -> F::Output {
    struct ThreadWaker;
    impl Wake for ThreadWaker {
        fn wake(self: std::sync::Arc<Self>) {
            std::thread::current().unpark();
        }
    }

    let mut fut = std::pin::pin!(fut);
    let waker = std::sync::Arc::new(ThreadWaker).into();
    let mut cx = Context::from_waker(&waker);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(val) => return val,
            Poll::Pending => std::thread::park_timeout(std::time::Duration::from_millis(20)),
        }
    }
}

#[cfg(all(
    unix,
    not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))
))]
fn platform_backend() -> Box<dyn Backend> {
    Box::new(tpt_torus_backend_uring::UringBackend::new(256).expect("io_uring backend"))
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn platform_backend() -> Box<dyn Backend> {
    Box::new(tpt_torus_backend_kqueue::KqueueBackend::new().expect("kqueue backend"))
}

#[cfg(all(
    unix,
    not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))
))]
fn raw_fd(file: &std::fs::File) -> i32 {
    use std::os::unix::io::AsRawFd;
    file.as_raw_fd()
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn raw_fd(file: &std::fs::File) -> i32 {
    use std::os::unix::io::AsRawFd;
    file.as_raw_fd()
}

#[cfg(not(target_os = "windows"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let torus = TorusAsync::new(256, platform_backend())?;

    // Write something we can read back.
    let tmpfile = std::env::temp_dir().join("torus_hello_read.txt");
    std::fs::write(&tmpfile, b"hello from TorusAsync")?;

    let file = std::fs::File::open(&tmpfile)?;
    let fd = raw_fd(&file);

    let mut buf = vec![0u8; 64];
    let bytes = block_on(torus.read(fd, &mut buf, 0))?;
    let bytes = bytes as usize;

    println!(
        "read {} bytes: {:?}",
        bytes,
        String::from_utf8_lossy(&buf[..bytes])
    );

    drop(file);
    let _ = std::fs::remove_file(&tmpfile);
    Ok(())
}

#[cfg(target_os = "windows")]
fn main() {
    println!(
        "This example requires handle association not yet exposed through the \
         fd-based TorusAsync API on Windows (IOCP). It runs on Linux and macOS/BSD."
    );
}
