# Changelog

All notable changes to `tpt-torus-cxx` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `torus.h` C ABI header under `include/`, exported by the `cdylib` for non-Rust languages.
- `torus_create` now constructs the platform-default backend (io_uring / IOCP / kqueue), making the C ABI usable from C/C++/Go/Python without a Rust side-channel.

## [0.1.0] - 2026-07-22

### Added

- C-compatible FFI (`torus.h`) and C++20 coroutine wrapper (`torus.hpp`) over the `Torus` handle.
- `cdylib` + `staticlib` + `lib` crate types for cross-language linking.
