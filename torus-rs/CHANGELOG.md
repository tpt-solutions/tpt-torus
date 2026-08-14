# Changelog

All notable changes to `torus-rs` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `torus-rs`: ergonomic Rust facade crate with `torus::open()` and full `tpt-torus-core` API re-export.
- `hardware` feature (plus `spdk` / `dpdk` / `gpu_direct`) exposing the `hw` module for hardware bypass.
