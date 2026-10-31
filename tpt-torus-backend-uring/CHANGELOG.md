# Changelog

All notable changes to `tpt-torus-backend-uring` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `UringBackend::new_with_sqpoll` — enable `IORING_SETUP_SQPOLL` at ring setup (replaces the no-op `enable_sqpoll`).
- Multishot accept/recv: `submit_multi_accept`, `submit_multi_recv` (real `SOCK_MULTISHOT` / `MSG_MULTISHOT` flags) and `cancel_multi`.
- Zero-copy fixed-buffer I/O: `register_buffers` / `unregister_buffers` using `IORING_REGISTER_BUFFERS`, and automatic switching to `IORING_OP_READ/WRITE_FIXED` for buffers whose base matches a registered region.
- Provided-buffer ring (`IORING_REGISTER_PBUF_RING`) backing multishot recv: the kernel selects a buffer per completion via `IOSQE_BUFFER_SELECT`. The ring is registered lazily and unregistered and unmapped on drop.

### Changed

- `reap()` decrements `in_flight` only for CQEs that disarm their SQE (`IORING_CQE_F_MORE` clear). Armed multishot accept/recv therefore keep `in_flight` non-zero, and `wait()` blocks for the next completion instead of returning immediately.
- io_uring setup, enter, and register calls go through the `tpt-torus-sys` syscall wrappers.
- The multishot recv integration test is skipped on kernels older than 5.20.

### Deprecated

- `UringBackend::enable_sqpoll` — SQPOLL must be set at setup time via `new_with_sqpoll`.

### Fixed

- Submissions now write SQ array indices, so the kernel can locate the SQEs. Previously only the head and tail were advanced.
- Ring mmaps use the fixed `IORING_OFF_*` offsets from `tpt-torus-sys` instead of offsets derived from `io_uring_params`.
- `MAP_POPULATE` is no longer passed to ring mmaps, since the kernel rejects it for the io_uring offsets.
- Multishot recv no longer fails with `-EINVAL`: the SQE `len` is zero when buffer selection is used.

## [0.1.0] - 2026-07-22

### Added

- `UringBackend` mapping Virtual Torus rings directly to io_uring kernel shared memory via `mmap` (no reactor thread).
- Submission path translating `Operation` variants into io_uring SQEs.
- Completion reaping straight from the kernel CQ ring.
