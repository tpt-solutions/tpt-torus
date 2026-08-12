# tpt-torus-hw

Hardware Bypass layer for [TPT Torus](https://github.com/tpt-solutions/tpt-torus) — direct user-space access to storage and networking hardware, bypassing the OS kernel entirely for maximum performance.

Provides **SPDK** (NVMe storage), **DPDK** (networking), **GPU-Direct** (DMA orchestration between NVMe and GPU VRAM), **CUDA** (driver API), and **XDP** (eBPF high-performance networking) integrations on top of the Virtual Torus core.

All integrations are **feature-gated** and degrade gracefully to `HwError::NotAvailable` when the corresponding native library/driver is absent at runtime.

## Modules

| Module       | Feature       | Purpose                                                  |
|--------------|---------------|----------------------------------------------------------|
| `spdk`       | `spdk`        | User-space NVMe via `libspdk` (controllers, namespaces, commands). |
| `dpdk`       | `dpdk`        | Poll-mode networking via `libdpdk` (`Mempool`, `Mbuf`).  |
| `gpu_direct` | `gpu_direct`  | `GpuDirect` + `GpuBuffer` for NVMe↔GPU VRAM DMA.         |
| `cuda`       | `gpu_direct`  | CUDA Driver API FFI (dynamically loaded).               |
| `dma_pool`   | `gpu_direct` (unix) | DMA-capable memory pool.                          |
| `nvme_gpu`   | `gpu_direct` (linux) | NVMe→GPU transfer orchestration.                   |
| `xdp`        | `xdp`         | AF_XDP / eBPF packet processing without hugepages.       |

## Installation

```toml
[dependencies]
tpt-torus-hw = { version = "0.1.0", features = ["spdk", "dpdk", "gpu_direct"] }
```

Enable only the features you need. With no features, the crate compiles to a thin shell that returns `NotAvailable`.

## Quick start (GPU-Direct)

```rust,no_run
use tpt_torus_hw::gpu_direct::GpuDirect;

let mut gd = GpuDirect::new(0, 4).expect("gpu direct"); // GPU 0, 4 DMA engines
let gpu_buf = tpt_torus_hw::gpu_direct::GpuBuffer::new(0, dev_ptr, size);
gd.nvme_to_gpu(lba, &gpu_buf, 0, len).expect("dma");
gd.sync_all().expect("sync");
```

## Feature flags

| Feature      | Effect                                                          |
|--------------|----------------------------------------------------------------|
| `spdk`       | Enable SPDK (loads `libspdk` at runtime, calls real NVMe API).  |
| `dpdk`       | Enable DPDK (loads `libdpdk` at runtime).                      |
| `gpu_direct` | Enable GPU-Direct + CUDA orchestration.                        |
| `xdp`        | Enable XDP (Linux only, no external deps).                      |

All four are **off by default**. With a feature enabled but the native library absent, calls return `HwError::NotAvailable` rather than panicking.

## Relationship to other crates

Built on `tpt-torus-core`. Exposed to Rust users through `torus-rs`'s `hardware` feature (and `spdk`/`dpdk`/`gpu_direct`).

## Building & testing

```bash
# Smoke-compile with all integrations (does not require hardware):
cargo build -p tpt-torus-hw --features spdk,dpdk,gpu_direct
cargo build -p tpt-torus-hw --features xdp
cargo bench -p tpt-torus-hw
```

> CI compiles this crate with `spdk,dpdk,gpu_direct` as a smoke check but does not run its tests/benches under those features — CI runners have no SPDK/DPDK/CUDA hardware.

## License

Licensed under either of [MIT](https://opensource.org/licenses/MIT) or [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) at your option.
