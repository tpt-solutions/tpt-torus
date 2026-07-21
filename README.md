# TPT Torus

A unified, cross-platform, high-performance asynchronous I/O framework for Rust.

TPT Torus abstracts OS-specific I/O multiplexing (Linux `io_uring`, Windows IOCP, macOS/BSD `kqueue`) behind a single, memory-safe, zero-cost API — the **Virtual Torus**. Application code is written once against a consistent ring-buffer paradigm (`Flow` for submission, `Result` for completion) and runs natively on every supported OS.

For ultra-low latency requirements, TPT Torus also provides a **Hardware Bypass** layer that integrates SPDK (NVMe), DPDK (networking), and GPU-Direct (DMA orchestration) for direct user-space hardware access.

## Status

**Phase 4 complete** — all core features implemented. See `todo.md` for detailed progress.

- 42 tests passing across 7 crates
- Zero-cost abstraction verified via benchmarks (~700ps Flow creation, ~235ps Result inspection)
- Cross-platform: Linux (io_uring), Windows (IOCP), macOS/BSD (kqueue)

## Quick Start

```rust
use tpt_torus_core::async_api::TorusAsync;
use tpt_torus_core::flow::Flow;
use tpt_torus_core::operation::Operation;

// Create a Torus instance (platform-specific backend)
let torus = Torus::new(256, Box::new(UringBackend::new(256)?))?;

// Submit a read operation
let mut buf = vec![0u8; 4096];
let flow = Flow::new(Operation::Read {
    fd: file_fd,
    buf: buf.as_mut_ptr(),
    len: 4096,
    offset: 0,
});
torus.submit(&flow)?;

// Wait for completion
torus.wait(1_000_000)?;
let mut results = Vec::new();
torus.reap(&mut results)?;
```

## Architecture

```text
┌─────────────────────────────────────────────────────────┐
│                 Application Layer                       │
│  (TorusAsync / Flow / Result / C++ Coroutines)         │
├─────────────────────────────────────────────────────────┤
│              Safe API Layer                             │
│  (Buffer Leasing / Torus Panic / Resource Limiting)    │
├─────────────────────────────────────────────────────────┤
│              Hardware Bypass Layer                      │
│  (SPDK / DPDK / GPU-Direct)                            │
├─────────────────────────────────────────────────────────┤
│           Virtual Torus (Core Abstraction)              │
│  (SubmissionRing / CompletionRing / Torus Handle)      │
├─────────────────────────────────────────────────────────┤
│     Native Backends (io_uring / IOCP / kqueue)          │
└─────────────────────────────────────────────────────────┘
```

## Crates

| Crate | Description |
|-------|-------------|
| `tpt-torus-sys` | Raw, unsafe FFI bindings to io_uring, IOCP, and kqueue |
| `tpt-torus-core` | Virtual Torus abstraction, Safe API, async/await wrappers |
| `tpt-torus-backend-uring` | Linux io_uring engine with mmap-based kernel shared memory |
| `tpt-torus-backend-iocp` | Windows IOCP engine with background reactor thread |
| `tpt-torus-backend-kqueue` | macOS/BSD kqueue engine with event-driven reactor |
| `tpt-torus-cxx` | C FFI layer and C++20 coroutine header |
| `tpt-torus-hw` | Hardware Bypass: SPDK, DPDK, and GPU-Direct integration |

## Features

### Cross-Platform I/O

Write once, run everywhere. The same `Flow`/`Result` API works on all platforms:

```rust
// This code works on Linux, Windows, and macOS
let flow = Flow::new(Operation::Read { fd, buf, len, offset });
torus.submit(&flow)?;
```

### Safe API (Buffer Leasing)

Memory safety is enforced at the framework level:

```rust
use tpt_torus_core::lease::LeaseRegistry;

let registry = LeaseRegistry::new();
unsafe {
    // Register buffer regions
    registry.register_mut(buf.as_mut_ptr(), buf.len())?;
    
    // Buffers are automatically tracked during I/O
    // Torus Panic triggers if safety is violated
}
```

### Raw API (Opt-Out)

For advanced use cases, bypass safety checks explicitly:

```rust
unsafe {
    let raw = torus.raw();
    raw.submit_read(fd, buf_ptr, len, offset)?;
}
```

### C++20 Coroutines

Modern C++ with coroutine support:

```cpp
#include "torus.hpp"

torus::Torus torus(256);
auto result = co_await torus.read(fd, buf, len, 0);
if (result.ok()) {
    std::cout << "Read " << result.bytes() << " bytes\n";
}
```

### Hardware Bypass

Direct hardware access for ultra-low latency:

```rust
use tpt_torus_hw::gpu_direct::GpuDirect;

let mut gd = GpuDirect::new(0, 4)?; // GPU device 0, 4 DMA engines
let gpu_buf = GpuBuffer::new(0, dev_ptr, size);

// NVMe → GPU VRAM (bypasses system RAM)
gd.nvme_to_gpu(lba, &gpu_buf, 0, len)?;
gd.sync_all()?;
```

## Benchmarks

Verified zero-cost abstraction:

| Operation | Time |
|-----------|------|
| Flow creation | ~700 ps |
| Result inspection | ~235 ps |
| Ring publish (atomic) | ~4 ns |
| Ring available check | ~620 ps |
| Lease verify | ~15 ns |
| Resource limiter check | ~0.5 ns |

Run benchmarks: `cargo bench -p tpt-torus-core`

## Security

- **Buffer Leasing**: All memory regions must be registered before use
- **Torus Panic**: Safe abort on lease violations (prevents kernel corruption)
- **Cgroup Limiting**: Automatic resource caps based on container quotas
- **Fail-Safe Defaults**: Safety features enabled by default, `unsafe` required to opt out

See [`SECURITY.md`](./SECURITY.md) for the full threat model.

## Building

```bash
# Build everything
cargo build --workspace

# Run tests
cargo test --workspace

# Run benchmarks
cargo bench -p tpt-torus-core

# Check formatting and lints
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

### Feature Flags

For `tpt-torus-hw`:
```bash
cargo build -p tpt-torus-hw --features spdk      # SPDK integration
cargo build -p tpt-torus-hw --features dpdk      # DPDK integration
cargo build -p tpt-torus-hw --features gpu_direct # GPU-Direct
```

## Platform Support

| Platform | Backend | Status |
|----------|---------|--------|
| Linux | io_uring | Full support |
| Windows | IOCP | Full support |
| macOS/BSD | kqueue | Full support |
| Linux + SPDK | SPDK | API ready (requires SPDK) |
| Linux + DPDK | DPDK | API ready (requires DPDK) |
| Linux + CUDA | GPU-Direct | API ready (requires CUDA) |

## License

Licensed under either of [MIT](./LICENSE-MIT) or [Apache License, Version 2.0](./LICENSE-APACHE) at your option.
