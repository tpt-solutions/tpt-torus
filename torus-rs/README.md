# torus-rs

Ergonomic Rust bindings for [TPT Torus](https://github.com/tpt-solutions/tpt-torus), a
cross-platform asynchronous I/O library that unifies `io_uring` (Linux), IOCP (Windows),
and kqueue (macOS/BSD) behind one Virtual Torus API.

This crate is a batteries-included facade over `tpt-torus-core` and the platform backends.

## Features

- `open(ring_entries)` — platform-aware `Torus` constructor.
- Full re-export of the `tpt-torus-core` API (`Torus`, `Flow`, `Operation`, `LeaseRegistry`, …).
- `hardware` feature (plus `spdk` / `dpdk` / `gpu_direct`) exposes the `hw` module for
  user-space NVMe / networking / GPU-Direct bypass.

## Example

```rust,no_run
use torus::{open, Flow, Operation};

let torus = open(1024)?;
let flow = Flow::new(Operation::Read { fd: 0, buf: std::ptr::null_mut(), len: 0, offset: 0 });
torus.submit(&flow)?;
# Ok::<(), torus::Error>(())
```

## License

Licensed under either of MIT or Apache-2.0 at your option.
