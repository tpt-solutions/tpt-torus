# torus-rs

Ergonomic Rust bindings for [TPT Torus](https://github.com/tpt-solutions/tpt-torus) — a cross-platform asynchronous I/O library that unifies `io_uring` (Linux), IOCP (Windows), and kqueue (macOS/BSD) behind one Virtual Torus API, with optional hardware bypass.

`torus-rs` is a **batteries-included facade** over [`tpt-torus-core`](https://crates.io/crates/tpt-torus-core) and the platform backends. It re-exports the full core API and adds a platform-aware `open()` constructor so you don't have to pick a backend by hand.

## Features

- `open(ring_entries)` — platform-aware `Torus` constructor (picks the right backend for the current OS).
- Full re-export of the `tpt-torus-core` API (`Torus`, `Flow`, `Operation`, `LeaseRegistry`, …).
- `hardware` feature (plus `spdk` / `dpdk` / `gpu_direct`) exposes the `hw` module for user-space NVMe / networking / GPU-Direct bypass.

## Installation

```toml
[dependencies]
torus-rs = "0.1.0"

# with hardware bypass:
torus-rs = { version = "0.1.0", features = ["hardware"] }
```

## Example

```rust,no_run
use torus::{open, Flow, Operation};

let torus = open(1024)?;
let mut buf = vec![0u8; 4096];
let flow = Flow::new(Operation::Read {
    fd: 3,
    buf: buf.as_mut_ptr(),
    len: 4096,
    offset: 0,
});
torus.submit(&flow)?;
torus.wait(1_000_000)?;
let mut results = Vec::new();
torus.reap(&mut results)?;
# Ok::<(), torus::Error>(())
```

## Feature flags

| Feature      | Effect                                          |
|--------------|-------------------------------------------------|
| `hardware`   | Re-export `tpt-torus-hw` as the `hw` module.    |
| `spdk`       | Enable SPDK in the `hw` module.                  |
| `dpdk`       | Enable DPDK in the `hw` module.                  |
| `gpu_direct` | Enable GPU-Direct in the `hw` module.            |

## Relationship to other crates

`torus-rs` depends on `tpt-torus-core`, the three backend crates, and (optionally) `tpt-torus-hw`. It is the recommended entry point for Rust applications; the lower-level crates can be used directly if you need finer control.

## Building & testing

```bash
cargo test   -p torus-rs
cargo build  -p torus-rs --features hardware
```

## License

Licensed under either of [MIT](https://opensource.org/licenses/MIT) or [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) at your option.
