# Changelog

All notable changes to `tpt-torus-sys` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `ioprio_flags` module with `IORING_ACCEPT_MULTISHOT` and `IORING_RECV_MULTISHOT` for multishot accept/recv.
- `enter_flags` constants (`IORING_ENTER_GETEVENTS`, `IORING_ENTER_SQ_WAIT`, `IORING_ENTER_SQ_TICKET`, `IORING_ENTER_EXT_ARG`).
- Provided-buffer ring types for multishot recv: `io_uring_buf` and `io_uring_buf_reg`, plus the `IORING_REGISTER_PBUF_RING` / `IORING_UNREGISTER_PBUF_RING` opcodes.
- `mmap_offsets` module with the fixed `IORING_OFF_SQ_RING`, `IORING_OFF_CQ_RING`, and `IORING_OFF_SQES` ring mmap offsets.
- Compile-time size assertions for `io_uring_sqe` (64 bytes), `io_uring_buf` (16 bytes), and `io_uring_buf_reg` (40 bytes).

### Changed

- Linux io_uring syscalls (`io_uring_setup`, `io_uring_enter`, `io_uring_enter2`, `io_uring_register`, `io_uring_unregister`) are now Rust wrapper functions over `libc::syscall` instead of `extern "C"` declarations. No libc symbol exists for them; they are normally reached through liburing.
- `io_uring_unregister` is a thin wrapper over `SYS_io_uring_register`, since there is no separate unregister syscall.

### Fixed

- `io_uring_sqe` now matches the kernel's 64-byte layout: the trailing padding is replaced by `addr3` (`u64`).

## [0.1.0] - 2026-07-22

### Added

- Raw `#[repr(C, packed)]` / `#[repr(C)]` io_uring structs: `io_uring_sqe`, `io_uring_cqe`, `io_uring_params`, `io_sqring_offsets`, `io_cqring_offsets`.
- Full io_uring constant modules: `opcodes`, `sqe_flags`, `setup_flags`, `features`, `cqe_flags`.
- Linux syscall `extern` declarations for `io_uring_setup`, `io_uring_enter`, `io_uring_register`, `io_uring_unregister`.
- Convenience helpers `queue_init` (maps `-errno` to a readable `&'static str`) and `queue_exit`.
