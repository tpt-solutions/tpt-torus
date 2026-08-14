# Changelog

All notable changes to `tpt-torus-core` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Zero-copy fixed-buffer I/O: `LeaseRegistry::as_register_buffers`, `Backend::register_buffers` / `unregister_buffers` (no-op default), and `Torus::register_leases`, which bridges registered regions to the kernel (`IORING_REGISTER_BUFFERS` on io_uring).
- `TorusPool`: round-robin pool of `Torus` instances for concurrent submission across multiple rings/backends.

### Fixed

- TOCTOU race in `LeaseRegistry::register` (check-then-insert now under a single write lock).
- Range overflow at `addr == usize::MAX` in `LeaseRegistry`.
- Unchecked `in_flight` counter overflow (now uses `saturating_add` / `saturating_sub`).

## [0.1.0] - 2026-07-22

### Added

- `Torus` handle — thread-safe context object shareable via `Arc`.
- Virtual Torus abstraction with `SubmissionRing` / `CompletionRing`.
- `Flow` (submission) and `Result` (completion) types replacing raw SQE/CQE.
- `Backend` trait implemented by each OS engine (`submit`, `reap`, `wait`, `in_flight`).
- Safe API: `LeaseRegistry` for buffer registration and `TorusPanic` for lease violations.
- High-level async/await API (`TorusAsync`) with per-operation futures.
- Cgroup-aware resource limiting (`cgroup` module).
- `observability` module and optional `tracing` feature.
