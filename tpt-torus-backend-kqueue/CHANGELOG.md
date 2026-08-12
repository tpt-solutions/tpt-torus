# Changelog

All notable changes to `tpt-torus-backend-kqueue` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

_No crate-specific changes yet._

## [0.1.0] - 2026-07-22

### Added

- `KqueueBackend` event-driven reactor using raw `kevent`/`kqueue` FFI (inlined in this crate).
- Socket I/O via native `EVFILT_READ` / `EVFILT_WRITE`.
- File I/O dispatched to a thread pool (kqueue has no native async file I/O).
