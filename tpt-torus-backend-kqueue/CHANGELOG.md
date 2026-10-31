# Changelog

All notable changes to `tpt-torus-backend-kqueue` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Socket operations are reactor-driven: `accept` and `recv` register `EVFILT_READ`, and `connect` and `send` register `EVFILT_WRITE`. The reactor performs the I/O when the fd is ready and posts the completion.
- File `read` / `write` / `readv` / `writev` run on a thread pool that issues positional `pread` / `pwrite`, sized from `available_parallelism`.
- `kqueue_file_io` example.

### Changed

- Platform gating uses an explicit list of macOS, iOS, FreeBSD, OpenBSD, NetBSD, and DragonFly targets instead of `cfg(unix)`. Linux is `unix` but has no kqueue.

### Fixed

- `submit()` returns `Result<usize>` as the `Backend` trait requires.
- `Recv` and `Send` pass the buffer pointer itself instead of a reference to it.
- `KEvent` implements `Default` by hand, since `*mut c_void` has no `Default`.
- `Accept` and `Connect` no longer carry redundant pointer casts.
- Result error codes are negated consistently with the `-errno` convention.
- Registrations are no longer made for synchronous operations, which had produced spurious zero-byte completions.

## [0.1.0] - 2026-07-22

### Added

- `KqueueBackend` event-driven reactor using raw `kevent`/`kqueue` FFI (inlined in this crate).
- Socket I/O via native `EVFILT_READ` / `EVFILT_WRITE`.
- File I/O dispatched to a thread pool (kqueue has no native async file I/O).
