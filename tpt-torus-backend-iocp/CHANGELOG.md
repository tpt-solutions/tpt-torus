# Changelog

All notable changes to `tpt-torus-backend-iocp` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `iocp_file_io` example demonstrating file reads and writes through the backend.

### Fixed

- `ERROR_IO_PENDING` (and `WSA_IO_PENDING`) from `ReadFile`, `WriteFile`, `WSARecv`, and `WSASend` no longer frees the `OVERLAPPED` or posts a spurious failure completion. Ownership stays with the reactor until the operation completes.

## [0.1.0] - 2026-07-22

### Added

- `IocpBackend` running a background reactor thread that drains an I/O Completion Port and translates native completions into Virtual Torus completions.
- `TorusOverlapped` / `SafeHandle` wrappers carrying `user_data` through `OVERLAPPED` for completion matching.
- `windows-sys`-based IOCP / Winsock / File FFI.
