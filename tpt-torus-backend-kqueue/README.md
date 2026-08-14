# tpt-torus-backend-kqueue

macOS/BSD **kqueue** backend engine for [TPT Torus](https://github.com/tpt-solutions/tpt-torus) — a unified, cross-platform async I/O framework.

This crate implements `tpt_torus_core::backend::Backend`. It creates a kqueue fd and spawns a background reactor thread, and the kqueue/`kevent` FFI is declared inline in this crate (kqueue's ABI is simple enough to inline rather than live in `tpt-torus-sys`).

**Current state:** operations are executed **synchronously** inside `submit` via blocking `libc` syscalls and the results are posted immediately to the virtual CQ. The kqueue fd and background reactor thread exist, but the reactor currently polls `kevent` with an empty change list and does not yet translate events into completions — every completion is produced synchronously by `submit`. This is a functional, correct backend, but it does not yet provide true event-driven asynchronous I/O through kqueue.

- File I/O: `pread` / `pwrite` / `preadv` / `pwritev`
- Socket I/O: `recv` / `send` / `accept` / `connect`
- Lifecycle: `close`

Available on macOS/BSD only (`cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))`) — `kqueue`/`kevent` are not present on Linux despite it also being `cfg(unix)`.

## What it provides

- `KqueueBackend::new()` — create the backend, open the kqueue fd, and start the reactor thread.
- Synchronous `Backend` impl covering the full `Operation` set (read/write/readv/writev/send/recv/accept/connect/close), bridged onto blocking `libc` syscalls.

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
- Operations run synchronously in `submit`; the reactor thread is currently a placeholder and does not drive completions.
- Because `submit` blocks on `libc` syscalls, I/O throughput is bounded by the calling thread; wiring the reactor to `EVFILT_READ`/`EVFILT_WRITE` for true async socket I/O and a thread pool for file I/O is future work (tracked in the project roadmap).

## Relationship to other crates

Depends on `tpt-torus-sys` and `tpt-torus-core`. Selected automatically by `torus-rs` on macOS/BSD.

## Building & testing

```bash
cargo test   -p tpt-torus-backend-kqueue   # macOS/BSD runners only
cargo build  -p tpt-torus-backend-kqueue
```

## License

Licensed under either of [MIT](https://opensource.org/licenses/MIT) or [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) at your option.
