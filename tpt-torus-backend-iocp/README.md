# tpt-torus-backend-iocp

Windows **I/O Completion Port (IOCP)** backend engine for [TPT Torus](https://github.com/tpt-solutions/tpt-torus) — a unified, cross-platform async I/O framework.

This crate implements `tpt_torus_core::backend::Backend` by running a **background reactor thread** that drains completions from a Windows I/O Completion Port and translates them into Virtual Torus completions (`TorusResult`). It uses `windows-sys` for the IOCP / Winsock / File FFI. A `TorusOverlapped` wrapper carries `user_data` through the `OVERLAPPED` struct so completions are matched back to the submitting `Flow`.

Windows only (`cfg(windows)`).

## What it provides

- `IocpBackend::new()` — create the backend, spinning up the background completion reactor thread.
- Full `Backend` impl covering the same `Operation` set as the other engines (read/write/send/recv/accept/connect/close), bridged onto native overlapped I/O (`ReadFileEx` / `WSASend` / `WSARecv` / `AcceptEx`, etc.).

## Installation

```toml
[dependencies]
tpt-torus-core = "0.1.0"
tpt-torus-backend-iocp = "0.1.0"
```

## Quick start

```rust,no_run
use tpt_torus_core::{Torus, Flow, Operation};
use tpt_torus_backend_iocp::IocpBackend;

let backend = IocpBackend::new().expect("iocp");
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

- Windows only. On non-Windows targets `cargo build -p tpt-torus-backend-iocp` compiles an empty crate.
- `SafeHandle` wraps raw Windows handles to provide `Send + Sync` without relying on the raw handle type being `Send`/`Sync`.

## Relationship to other crates

Depends on `tpt-torus-sys` and `tpt-torus-core`. Selected automatically by `torus-rs` on Windows.

## Building & testing

```bash
cargo test   -p tpt-torus-backend-iocp   # Windows runners only
cargo build  -p tpt-torus-backend-iocp
```

## License

Licensed under either of [MIT](https://opensource.org/licenses/MIT) or [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) at your option.
