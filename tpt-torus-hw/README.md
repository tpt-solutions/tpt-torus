# tpt-torus-hw

Hardware Bypass layer for [TPT Torus](https://github.com/tpt-solutions/tpt-torus): SPDK (storage), DPDK (networking), and GPU-Direct (DMA orchestration) integration.

Feature-gated: enable `spdk`, `dpdk`, and/or `gpu_direct` to build against those integrations (each requires the corresponding native library/driver installed). All features are off by default, so the default `cargo test --workspace` run does not exercise this crate's feature-gated code paths.

CI compiles this crate with all three features enabled (`cargo build -p tpt-torus-hw --features spdk,dpdk,gpu_direct`) as a smoke check, but does not run its tests/benches under those features — none of `ubuntu-latest`/`windows-latest`/`macos-latest` runners have SPDK, DPDK, or a CUDA-capable GPU installed. `spdk`/`dpdk` are currently API-surface stubs (every call returns `HwError::NotAvailable`; no real libspdk/libdpdk linkage yet), so the smoke build validates Rust-level compilation only, not integration correctness. `gpu_direct` loads `libcuda`/`nvcuda` at runtime via `libloading`, so it also compiles without CUDA present but will return `HwError`/`DmaError::GpuNotAvailable` at runtime on those runners.

See the [main repository](https://github.com/tpt-solutions/tpt-torus) for the full design document and usage guide.
