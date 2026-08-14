# Changelog

All notable changes to `tpt-torus-backend-uring` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `UringBackend::new_with_sqpoll` — enable `IORING_SETUP_SQPOLL` at ring setup (replaces the no-op `enable_sqpoll`).
- Multishot accept/recv: `submit_multi_accept`, `submit_multi_recv` (real `SOCK_MULTISHOT` / `MSG_MULTISHOT` flags) and `cancel_multi`.
- Zero-copy fixed-buffer I/O: `register_buffers` / `unregister_buffers` using `IORING_REGISTER_BUFFERS`, and automatic switching to `IORING_OP_READ/WRITE_FIXED` for buffers whose base matches a registered region.

### Deprecated

- `UringBackend::enable_sqpoll` — SQPOLL must be set at setup time via `new_with_sqpoll`.

## [0.1.0] - 2026-07-22

### Added

- `UringBackend` mapping Virtual Torus rings directly to io_uring kernel shared memory via `mmap` (no reactor thread).
- Submission path translating `Operation` variants into io_uring SQEs.
- Completion reaping straight from the kernel CQ ring.
