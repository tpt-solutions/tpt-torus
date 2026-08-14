# tpt-torus-core

The **Virtual Torus** abstraction for [TPT Torus](https://github.com/tpt-solutions/tpt-torus) — a unified, cross-platform asynchronous I/O framework that abstracts Linux `io_uring`, Windows IOCP, and macOS/BSD `kqueue` behind one ring-buffer API.

This crate is the public-facing heart of Torus. It is OS-agnostic: it defines the submission/completion model, the safe buffer-leasing API, and the `Backend` trait that every platform engine implements. You pair it with a backend crate for your target OS.

## Key concepts

- **`Torus`** — the thread-safe context object (shareable via `Arc`). Owns a virtual `SubmissionRing`/`CompletionRing` pair and delegates real I/O to a `Mutex<Box<dyn Backend>>`.
- **`Flow`** — a submission: wraps an `Operation` (read/write/accept/connect/recv/send/close/readv/writev) plus `user_data`. Replaces a raw SQE.
- **`Result`** (`TorusResult`) — a completion: carries the result code and the submitting `user_data`. Replaces a raw CQE.
- **`Backend`** trait — the seam every OS engine implements (`submit`, `reap`, `wait`, `in_flight`, buffer registration).
- **Safe API** — `LeaseRegistry` tracks registered memory regions; `TorusPanic` deliberately aborts on lease violations instead of letting a bad pointer reach the kernel.
- **`async_api::TorusAsync`** — high-level `async`/`await` wrapper with per-operation futures (`ReadFuture`, `WriteFuture`, …).
- **`TorusPool`** — a round-robin pool of `Torus` instances for concurrency across multiple backends/rings.

## Installation

```toml
[dependencies]
tpt-torus-core = "0.1.0"
```

Backends are separate crates. Pick the one for your platform:

| Platform  | Backend crate                |
|-----------|------------------------------|
| Linux     | `tpt-torus-backend-uring`    |
| Windows   | `tpt-torus-backend-iocp`     |
| macOS/BSD | `tpt-torus-backend-kqueue`   |

## Quick start

```rust,no_run
use tpt_torus_core::{Torus, Flow, Operation};
use tpt_torus_backend_uring::UringBackend;

let backend = UringBackend::new(256).expect("create io_uring");
let torus = Torus::new(256, Box::new(backend)).expect("create torus");

let mut buf = vec![0u8; 4096];
let flow = Flow::new(Operation::Read {
    fd: 3,
    buf: buf.as_mut_ptr(),
    len: 4096,
    offset: 0,
});
torus.submit(&flow).expect("submit");
torus.wait(1_000_000).expect("wait");

let mut results = Vec::new();
torus.reap(&mut results).expect("reap");
# drop(buf);
```

## Safe API (Buffer Leasing)

Memory safety is enforced at the framework level. Register buffers before use so Torus can verify every I/O touches a tracked region:

```rust,no_run
use tpt_torus_core::lease::LeaseRegistry;
use tpt_torus_core::Torus;
# fn make_torus() -> Torus { unimplemented!() }
# let torus = make_torus();
let registry = LeaseRegistry::new();
let mut buf = vec![0u8; 4096];
unsafe { registry.register(buf.as_mut_ptr(), buf.len()) };

// Enable kernel fixed-buffer (zero-copy) I/O on io_uring:
torus.register_leases(&registry).ok();
```

A violation (out-of-bounds / overlapping / in-flight access) is converted into a `TorusPanic` that aborts the process deliberately — never letting a bad pointer reach the kernel.

## Features

| Feature   | Effect                                                                 |
|-----------|------------------------------------------------------------------------|
| `tracing` | Emits `tracing` spans/events for submission, completion, and lease activity. |

## Relationship to other crates

```text
tpt-torus-sys  →  tpt-torus-core  →  { backends, torus-rs, tpt-torus-cxx, tpt-torus-hw }
```

`torus-rs` is a batteries-included facade that re-exports this crate's entire API and adds a platform-aware `open()` helper. `tpt-torus-cxx` and `tpt-torus-hw` build on top of it.

## Building & testing

```bash
cargo test   -p tpt-torus-core
cargo bench  -p tpt-torus-core   # criterion benches under [[bench]] torus_bench
cargo doc    -p tpt-torus-core --open
```

## License

Licensed under either of [MIT](https://opensource.org/licenses/MIT) or [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) at your option.
