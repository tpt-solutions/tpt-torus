# AGENTS.md

Compact guidance for Kilo (and other agents) working in this repo.
Commands and constraints below are verified against `Cargo.toml`, `.github/workflows/ci.yml`, and `CONTRIBUTING.md`/`CLAUDE.md`.

## What this is

Cross-platform async I/O framework in Rust. One ring-buffer API ("Virtual Torus") over OS backends:
`Flow` (submission, replaces SQE) / `Result` (completion, replaces CQE). Backends: Linux `io_uring`,
Windows IOCP, macOS/BSD `kqueue`. `tpt-torus-hw` adds SPDK/DPDK/GPU-Direct bypass.

Read `spec.txt` before architectural decisions; keep `todo.md` `[x]` in sync with the roadmap.

## Commands

Cargo workspace — commands run across all members unless scoped with `-p`.

```
cargo build --workspace                       # build everything
cargo test --workspace                        # all tests (unit + integration)
cargo test -p tpt-torus-core -- test_name    # single test by name
cargo fmt --all -- --check                    # CI formatting gate
cargo clippy --workspace --all-targets -- -D warnings   # CI lint gate (warnings = fail)
cargo bench -p tpt-torus-core                # benchmarks
cargo build -p tpt-torus-hw --features spdk,dpdk,gpu_direct  # HW bypass (opt-in; CI smoke build)
```

Fuzzing (FFI/parsing boundary): `cargo fuzz run <target>` from `fuzz/` (targets: flow_creation, result_parsing, operation_validate).

## CI / repo quirks

- **Default branch is `master`** (CI triggers on `master`), not `main` — `CONTRIBUTING.md` says branch from `main`, which contradicts the actual repo.
- CI runs a three-OS matrix: `ubuntu-latest`, `windows-latest`, `macos-latest` — formatting → clippy → build → test.
- MSRV is **1.87.0** (pinned in CI and `rust-version`); clippy uses `-D warnings`, so fix all lints before assuming CI passes.
- Backend crates are platform-gated at the crate root (`#![cfg(target_os = ...)]`): `backend-uring` builds only on Linux, `backend-iocp` only on Windows, `backend-kqueue` on an explicit macOS/BSD `target_os` list (not `cfg(unix)` — Linux is `unix` too but has no `kqueue`/`kevent`). A change to one backend is **not** exercised locally on another OS — rely on CI.

## Architecture essentials

- `tpt-torus-sys` — raw `unsafe` FFI only. Never add safe wrappers here (that's `core`'s job); struct layouts must match kernel/OS ABI byte-for-byte.
- `tpt-torus-core` — public abstraction: `Torus` handle, `Backend` trait (the OS-agnostic↔OS-specific seam), `Flow`/`Operation`/`Result`, `rings.rs`, `lease.rs`+`torus_panic.rs` (Safe API), `async_api.rs` (`TorusAsync` — currently busy-repoll scaffold, no real waker registration yet).
- `tpt-torus-backend-uring` — Linux: real kernel `mmap` shared memory, no reactor thread.
- `tpt-torus-backend-iocp` / `-kqueue` — background reactor thread pattern; kqueue file I/O dispatched to a thread pool (no native async file I/O).
- `tpt-torus-hw` — SPDK/DPDK/GPU-Direct behind feature flags; uses runtime `libloading` and degrades to `NotAvailable` when native libs are absent.
- `torus-rs` — ergonomic Rust facade (`torus::open()` + re-export). `torus-go`/`torus-py` are separate out-of-tree bindings over `torus.h`.

## Invariants to preserve

- Ring sizes passed to `Torus::new` must be **powers of two** (asserted; wraparound math depends on it).
- `Operation` variants carry raw pointers intentionally — safety comes from `LeaseRegistry`, not the types.
- Every backend `Backend` impl must be `Send + Sync` (`Arc<Torus>` requires it).
- Publishing order (crates.io): `tpt-torus-sys` → `tpt-torus-core` → the rest. Path deps use `version = "0.1.0"`; downstream crates can't be packaged locally until deps are live. Bump `workspace.package.version` for releases.

## Known open issues worth knowing

- `UringBackend::wait` `min_complete` heuristic is fixed: `reap()` now only decrements `in_flight` for disarming CQEs (`IORING_CQE_F_MORE` clear), so armed multishot accept/recv keep `in_flight > 0` and `wait()` blocks correctly for the next completion.
- `async_api.rs` futures still busy-repoll instead of registering wakers.
