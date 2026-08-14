# tpt-torus-sys

Raw, unsafe FFI bindings to `io_uring` (Linux), IOCP (Windows), and `kqueue` (macOS/BSD) for [TPT Torus](https://github.com/tpt-solutions/tpt-torus) — a unified, cross-platform asynchronous I/O framework.

> ⚠️ **This crate is `unsafe` by design.** It exposes syscall signatures, opcodes, and `#[repr(C)]` / `#[repr(C, packed)]` structs that mirror the kernel/OS ABI byte-for-byte. Application and library code should depend on [`tpt-torus-core`](https://crates.io/crates/tpt-torus-core) instead; `tpt-torus-sys` exists to be the single, audited source of low-level definitions that the safe abstraction builds on.

## What's inside

- **`io_uring` (Linux)** — `io_uring_sqe`, `io_uring_cqe`, `io_uring_params`, and the `io_sqring_offsets` / `io_cqring_offsets` structs, plus the full set of ABI constants: `opcodes`, `sqe_flags`, `ioprio_flags` (multishot accept/recv), `setup_flags`, `features`, `cqe_flags`, `enter_flags`. Thin syscall helpers `queue_init` / `queue_exit` wrap `io_uring_setup` and `close`.
- **IOCP (Windows)** — the type/constant surface used by `tpt-torus-backend-iocp`.
- **kqueue (macOS/BSD)** — raw `kevent`/`kqueue` FFI is declared inline in `tpt-torus-backend-kqueue` (the kqueue ABI is small enough to inline), so `tpt-torus-sys` focuses on `io_uring` + shared helpers.

All structs are laid out to match the kernel headers exactly. If you change a field, verify it against the real kernel/OS definitions — internal consistency is **not** sufficient.

## Platform support

| Platform  | ABI surface                                  | Compiles |
|-----------|----------------------------------------------|----------|
| Linux     | `io_uring_*` structs + syscalls + constants  | yes (`cfg(target_os = "linux")`) |
| Windows   | IOCP constants/defs                          | yes      |
| macOS/BSD | (kqueue inlined in the backend crate)        | yes      |

The crate is safe to include as a dependency on any target; the Linux `extern` block and helpers are gated so they only compile where relevant.

## Installation

```toml
[dependencies]
tpt-torus-sys = "0.1.0"
```

## Example

```rust,ignore
use tpt_torus_sys::*;

let mut params: io_uring_params = unsafe { std::mem::zeroed() };
let fd = queue_init(256, &mut params).expect("io_uring_setup failed");
// ... build SQEs via opcodes::IORING_OP_READ, etc. ...
queue_exit(fd);
```

## Relationship to other crates

```text
tpt-torus-sys  →  tpt-torus-core  →  { backends, torus-rs, tpt-torus-cxx, tpt-torus-hw }
```

- Depend on this crate only if you are implementing a new backend or need raw kernel access.
- Everything else should go through [`tpt-torus-core`](https://crates.io/crates/tpt-torus-core).

## Building & testing

This crate is a dependency of the workspace and has no standalone tests:

```bash
cargo build -p tpt-torus-sys
cargo doc   -p tpt-torus-sys --open
```

## License

Licensed under either of [MIT](https://opensource.org/licenses/MIT) or [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) at your option.
