# Changelog

All notable changes to TPT Torus will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- CONTRIBUTING.md with development guidelines
- CHANGELOG.md
- `torus-rs`: ergonomic Rust facade crate with `torus::open()` and full core API re-export
- `torus-go`: Go (cgo) bindings over the stable C ABI (`torus.h`)
- `torus-py`: Python (CFFI) bindings over the stable C ABI (`torus.h`)
- `torus.h` C ABI header in `tpt-torus-cxx/include`, exported by the cdylib for non-Rust languages
- `docs/adr-repo-split.md`: decision record for splitting the workspace into independent repos
- `spdk` / `dpdk` features now perform real, runtime-loaded native integration
  (load `libspdk` / `libdpdk` and call the real NVMe / poll-mode I/O APIs; gracefully
  degrade to `NotAvailable` when the native library is absent)

### Changed

- `tpt-torus-cxx` `torus_create` now constructs the platform-default backend, making
  the C ABI usable from C/C++/Go/Python without a Rust side-channel
- `tpt-torus-hw` `spdk` / `dpdk` features enable `libloading` for dynamic linkage

### Fixed

- TOCTOU race in `LeaseRegistry::register` (check-then-insert now under single write lock)
- Range overflow at `addr == usize::MAX` in `LeaseRegistry`
- Unchecked `in_flight` counter overflow (now uses `saturating_add`)

## [0.1.0] - 2026-07-22

### Added

- Virtual Torus abstraction with Submission/Completion ring API
- `Flow` (submission) and `Result` (completion) types replacing raw SQE/CQE
- `Torus` handle — thread-safe context object shareable via `Arc`
- **Linux backend** (`tpt-torus-backend-uring`): io_uring with mmap-based kernel shared memory
- **Windows backend** (`tpt-torus-backend-iocp`): IOCP with background reactor thread
- **macOS/BSD backend** (`tpt-torus-backend-kqueue`): kqueue with event-driven reactor
- Safe API: `LeaseRegistry` for buffer registration, `TorusPanic` for lease violations
- High-level async/await API (`TorusAsync`) with per-operation futures
- C FFI layer and C++20 coroutine header (`tpt-torus-cxx`)
- Hardware Bypass layer: SPDK, DPDK, and GPU-Direct integration (API stubs)
- Cgroup-aware resource limiting
- Zero-cost abstraction verified (~700ps Flow creation, ~235ps Result inspection)
- Cross-platform CI matrix (Linux, Windows, macOS)
- MSRV pinned at Rust 1.87.0
