# tpt-torus-backend-uring

Linux `io_uring` backend engine for [TPT Torus](https://github.com/tpt-solutions/tpt-torus) — a unified, cross-platform async I/O framework.

This crate implements the `tpt_torus_core::backend::Backend` trait by mapping the Virtual Torus submission/completion rings **directly onto kernel shared memory** via `mmap`. There is no reactor thread: `io_uring_enter` is the only syscall, and completions are read straight out of the kernel-owned CQ ring.

Requires **Linux kernel 5.1+** (`io_uring`); `IORING_SETUP_SQPOLL` and fixed-buffer I/O require newer kernels.

## What it provides

- `UringBackend::new(entries)` — create a ring of `entries` SQ/CQ slots (must be a power of two).
- `UringBackend::new_with_sqpoll(entries, sq_thread_idle)` — enable kernel SQPOLL (polled submissions on a dedicated kernel thread, eliminating `io_uring_enter` on the hot path at the cost of a CPU core). SQPOLL must be set at setup time.
- `UringBackend::register_buffers` / `unregister_buffers` — kernel fixed-buffer registration (`IORING_REGISTER_BUFFERS`) for zero-copy `IORING_OP_READ_FIXED` / `WRITE_FIXED`.
- `submit_multi_accept` / `submit_multi_recv` — multishot accept/recv (real `SOCK_MULTISHOT` / `MSG_MULTISHOT`), which stay armed across completions.
- `cancel_multi` — cancel a multishot operation via `IORING_OP_ASYNC_CANCEL`.

## Installation

```toml
[dependencies]
tpt-torus-core = "0.1.0"
tpt-torus-backend-uring = "0.1.0"
```

## Quick start

```rust,no_run
use tpt_torus_core::{Torus, Flow, Operation};
use tpt_torus_backend_uring::UringBackend;

let backend = UringBackend::new(256).expect("io_uring");
let torus = Torus::new(256, Box::new(backend)).expect("torus");

let mut buf = vec![0u8; 4096];
let flow = Flow::new(Operation::Read { fd: 3, buf: buf.as_mut_ptr(), len: 4096, offset: 0 });
torus.submit(&flow).expect("submit");
torus.wait(1_000_000).expect("wait");
let mut results = Vec::new();
torus.reap(&mut results).expect("reap");
# drop(buf);
```

### SQPOLL

```rust,no_run
use tpt_torus_backend_uring::UringBackend;
let backend = UringBackend::new_with_sqpoll(256, 0).expect("sqpoll ring");
```

## Platform notes

- Linux only (`#![cfg(target_os = "linux")]`). On other platforms `cargo build -p tpt-torus-backend-uring` compiles an empty crate.
- Vectored I/O (`readv`/`writev`) currently emits one SQE per buffer with sequential offsets; native `IORING_OP_READV`/`WRITEV` with iovecs is a future optimization.

## Relationship to other crates

Depends on `tpt-torus-sys` (raw io_uring FFI) and `tpt-torus-core` (the `Backend` trait and ring types). Selected automatically by `torus-rs` on Linux.

## Building & testing

```bash
cargo test   -p tpt-torus-backend-uring   # Linux runners only
cargo build  -p tpt-torus-backend-uring
```

## License

Licensed under either of [MIT](https://opensource.org/licenses/MIT) or [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) at your option.
