# Changelog

All notable changes to `tpt-torus-hw` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Real, runtime-loaded native integration for `spdk` / `dpdk`: load `libspdk` / `libdpdk` and call the real NVMe / poll-mode I/O APIs; gracefully degrade to `NotAvailable` when the native library is absent.
- `xdp` feature: AF_XDP / eBPF high-performance networking (`XdpSocket`, `XdpUmem`, `XdpProgram`).

### Changed

- `spdk` / `dpdk` features now enable `libloading` for dynamic linkage.

## [0.1.0] - 2026-07-22

### Added

- Hardware Bypass layer with `spdk`, `dpdk`, `gpu_direct`, and `xdp` feature gates.
- `HwError` / `HwResult` error types and `cuda` driver API FFI.
- (Initial release: API-surface stubs; integrations returned `NotAvailable`.)
