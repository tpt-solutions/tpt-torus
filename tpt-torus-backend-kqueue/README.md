# tpt-torus-backend-kqueue

macOS/BSD **kqueue** backend engine for [TPT Torus](https://github.com/tpt-solutions/tpt-torus) — a unified, cross-platform async I/O framework.

This crate implements `tpt_torus_core::backend::Backend`. It creates a kqueue fd and runs a background reactor thread. The kqueue/`kevent` FFI is declared inline in this crate, since kqueue's ABI is simple enough to inline rather than live in `tpt-torus-sys`.

Socket operations use native kqueue readiness events. Submission registers the fd with `EVFILT_READ` or `EVFILT_WRITE`, and the reactor performs the `recv` / `send` / `accept` / `connect` once the fd is ready, then posts the completion. File operations are not supported by kqueue asynchronously, so they run on a thread pool that issues positional `pread` / `pwrite`.

- Socket I/O (reactor, `EVFILT_READ` / `EVFILT_WRITE`): `recv` / `send` / `accept` / `connect`
- File I/O (thread pool, `pread` / `pwrite`): `read` / `write` / `readv` / `writev`
- Lifecycle: `close`

Available on macOS/BSD only (`cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))`) — `kqueue`/`kevent` are not present on Linux despite it also being `cfg(unix)`.

## What it provides

- `KqueueBackend::new()` — create the backend, open the kqueue fd, start the reactor thread, and start the file I/O thread pool.
- `Backend` impl covering the full `Operation` set (read/write/readv/writev/send/recv/accept/connect/close), with socket readiness handled by the reactor and file I/O handled by the pool.

## Installation

```toml
[dependencies]
tpt-torus-core = "0.1.0"
tpt-torus-backend-kqueue = "0.1.0"
```

## Quick start

```rust,no_run
use tpt_torus_core::{Torus, Flow, Operation};
use tpt_torus_backend_kqueue::KqueueBackend;

let backend = KqueueBackend::new().expect("kqueue");
let torus = Torus::new(256, Box::new(backend)).expect("torus");

let mut buf = vec![0u8; 4096];
let flow = Flow::new(Operation::Read { fd: 3, buf: buf.as_mut_ptr(), len: 4096, offset: 0 });
torus.submit(&flow).expect("submit");
torus.wait(1_000_000).expect("wait");
let mut results = Vec::new();
torus.reap(&mut results).expect("reap");
# drop(buf);
```

## Platform notes

- macOS/BSD only. On Windows/Linux `cargo build -p tpt-torus-backend-kqueue` compiles an empty crate.
- `accept`, `recv`, and `send` do not block `submit`: submission only registers interest, and the reactor produces the completion. `connect` issues the `connect` call during submission and then waits for write readiness, so the socket should be non-blocking.
- File operations run on a pool sized to the available parallelism, so file throughput is bounded by the pool and the underlying filesystem.

## Relationship to other crates

Depends on `tpt-torus-sys` and `tpt-torus-core`. Selected automatically by `torus-rs` on macOS/BSD.

## Building & testing

```bash
cargo test   -p tpt-torus-backend-kqueue   # macOS/BSD runners only
cargo build  -p tpt-torus-backend-kqueue
```

## License

Licensed under either of [MIT](https://opensource.org/licenses/MIT) or [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) at your option.
