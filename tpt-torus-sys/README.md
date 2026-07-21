# tpt-torus-sys

Raw, unsafe FFI bindings to `io_uring`, IOCP, and `kqueue` for [TPT Torus](https://github.com/tpt-solutions/tpt-torus).

This crate is intentionally low-level: syscall signatures, opcodes, and C-layout structs matching the kernel ABI. Application code should use [`tpt-torus-core`](https://crates.io/crates/tpt-torus-core) instead.

See the [main repository](https://github.com/tpt-solutions/tpt-torus) for the full design document and usage guide.
