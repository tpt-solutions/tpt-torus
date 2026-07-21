# tpt-torus-hw

Hardware Bypass layer for [TPT Torus](https://github.com/tpt-solutions/tpt-torus): SPDK (storage), DPDK (networking), and GPU-Direct (DMA orchestration) integration.

Feature-gated: enable `spdk`, `dpdk`, and/or `gpu_direct` to build against those integrations (each requires the corresponding native library/driver installed). All features are off by default.

See the [main repository](https://github.com/tpt-solutions/tpt-torus) for the full design document and usage guide.
