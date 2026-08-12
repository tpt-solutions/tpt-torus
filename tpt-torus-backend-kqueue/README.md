# tpt-torus-backend-kqueue

macOS/BSD **kqueue** backend engine for [TPT Torus](https://github.com/tpt-solutions/tpt-torus) — a unified, cross-platform async I/O framework.

This crate implements `tpt_torus_core::backend::Backend` using the same background-reactor pattern as IOCP, built on raw `kevent`/`kqueue` FFI (declared inline in this crate, since kqueue's ABI is simple enough to inline rather than live in `tpt-torus-sys`). Socket I/O uses native `EVFILT_READ` / `EVFILT_WRITE` for true async I/O; **file I/O is dispatched to a thread pool** because kqueue has no native async file I/O on macOS/BSD.

Available on Unix (`cfg(unix)`, covering macOS/BSD).

## What it provides

- `KqueueBackend::new()` — create the backend and start the event reactor thread.
- Socket operations via native `EVFILT_READ`/`EVFILT_WRITE`; file read/write delegated to a worker thread pool.

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

- Unix only (`cfg(unix)`), covering macOS and the BSDs. On Windows/Linux `cargo build -p tpt-torus-backend-kqueue` compiles an empty crate.
- File I/O is performed on a thread pool rather than through kqueue; socket I/O is event-driven.

## Relationship to other crates

Depends on `tpt-torus-sys` and `tpt-torus-core`. Selected automatically by `torus-rs` on macOS/BSD.

## Building & testing

```bash
cargo test   -p tpt-torus-backend-kqueue   # macOS/BSD runners only
cargo build  -p tpt-torus-backend-kqueue
```

## License

Licensed under either of [MIT](https://opensource.org/licenses/MIT) or [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) at your option.
