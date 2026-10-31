# Changelog

All notable changes to `tpt-torus-hw` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Real, runtime-loaded native integration for `spdk` / `dpdk`: load `libspdk` / `libdpdk` and call the real NVMe / poll-mode I/O APIs; gracefully degrade to `NotAvailable` when the native library is absent.
- `xdp` feature: AF_XDP / eBPF high-performance networking (`XdpSocket`, `XdpUmem`, `XdpProgram`).

- CUDA context operations in `cuda`: `ctx_destroy`, `ctx_set_current`, and `ctx_synchronize`.
- NVMe passthrough commands (`NvmePassthroughCmd`) in `nvme_gpu`.
- `GpuDirectNvme::with_pool` for explicit DMA pool sizing, and a `total_blocks` accessor.
- `DmaPoolBuilder` fields are public, and `GpuDmaPool::device_id` is exposed.

### Changed

- `spdk` / `dpdk` features now enable `libloading` for dynamic linkage.
- `dma_pool` is gated to Linux.
- io_uring FFI in `nvme_gpu` goes through `tpt-torus-sys` instead of direct `libc::syscall` and hand-written io_uring structs. The crate now depends on `tpt-torus-sys`.
- `GpuDirectNvmeDriver` is renamed to `GpuDirectNvme`. This is a breaking change for any code that used the old name.

### Fixed

- The SPDK symbol-not-found error message now includes the symbol name.
- io_uring ring mmaps use the fixed `IORING_OFF_*` offsets and no longer pass `MAP_POPULATE`.

## [0.1.0] - 2026-07-22

### Added

- Hardware Bypass layer with `spdk`, `dpdk`, `gpu_direct`, and `xdp` feature gates.
- `HwError` / `HwResult` error types and `cuda` driver API FFI.
- (Initial release: API-surface stubs; integrations returned `NotAvailable`.)
