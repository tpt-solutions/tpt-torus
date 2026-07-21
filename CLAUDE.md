# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

TPT Torus is a cross-platform async I/O framework for Rust. It abstracts Linux `io_uring`, Windows IOCP, and macOS/BSD `kqueue` behind a single ring-buffer API (the "Virtual Torus"), so application code is written once against `Flow` (submission, replaces raw SQE) / `Result` (completion, replaces raw CQE) and runs natively on all three OSes.

The full design rationale, threat model, and roadmap live in `spec.txt` — read it before making architectural decisions. `todo.md` tracks the phase-by-phase task checklist derived from that spec; keep it in sync with `[x]` when completing roadmap items.

## Commands

This is a Cargo workspace. Standard commands operate across all member crates unless scoped with `-p`.

```
cargo build --workspace                      # build everything
cargo test --workspace                       # run all tests (unit + integration)
cargo test -p tpt-torus-core lease_test      # run a single integration test file
cargo test -p tpt-torus-core -- test_name    # run a single test by name
cargo fmt --all -- --check                   # CI formatting gate
cargo clippy --workspace --all-targets -- -D warnings   # CI lint gate
```

Backend crates are platform-gated at the crate root (`#![cfg(target_os = "linux")]` etc.), so `tpt-torus-backend-uring` only builds/tests on Linux, `tpt-torus-backend-iocp` only on Windows, and `tpt-torus-backend-kqueue` only on Unix (`cfg(unix)`, covering macOS/BSD). CI (`.github/workflows/ci.yml`) runs the full matrix across `ubuntu-latest`, `windows-latest`, and `macos-latest` — a change to one backend won't be exercised locally on a different OS.

## Architecture

### Crate layout (mirrors the spec's eventual multi-repo split)

- **`tpt-torus-sys`** — raw `unsafe` FFI: syscall signatures, opcodes, and C-layout structs (`io_uring_sqe`, `io_uring_cqe`, `io_uring_params`, etc.) matching the kernel ABI exactly (`#[repr(C, packed)]`/`#[repr(C)]`). No safe wrappers beyond thin syscall helpers (`queue_init`, `queue_exit`). Never add safe abstractions here — that's `tpt-torus-core`'s job.
- **`tpt-torus-core`** — the public-facing abstraction. Owns:
  - `Torus` (`lib.rs`) — the thread-safe handle; holds a `SubmissionRing`/`CompletionRing` pair plus a `Mutex<Box<dyn Backend>>`.
  - `backend::Backend` — the trait every OS engine implements (`submit`, `reap`, `wait`, `in_flight`). This is the seam between the OS-agnostic core and OS-specific engines.
  - `flow.rs` / `operation.rs` / `result.rs` — the `Flow`/`Operation`/`Result` (CQE-equivalent) types shared by every backend.
  - `rings.rs` — the virtual SQ/CQ ring structures. On Linux these fields get reused/mirrored by direct kernel mmap (see below); on other OSes they're pure user-space queues drained by a reactor thread.
  - `lease.rs` + `torus_panic.rs` — the Safe API: `LeaseRegistry` tracks registered memory regions and rejects out-of-bounds/overlapping/in-flight buffer access; a violation converts into a `TorusPanic` (`torus_panic!` macro) which aborts the process deliberately rather than letting a bad pointer reach the kernel. This is the direct implementation of the threat model in spec.txt §5.
  - `async_api.rs` — the high-level `TorusAsync` wrapper with per-operation `Future` types (`ReadFuture`, `WriteFuture`, etc.). Each future's `poll` does submit-then-reap; note the current futures don't register real wakers with the backend (they re-poll instead of parking), so this is a scaffold, not a wakeup-correct executor integration yet.
- **`tpt-torus-backend-uring`** — Linux only. `UringBackend` calls `io_uring_setup`/`io_uring_enter` directly and `mmap`s the kernel's SQ/CQ/SQE regions (`MmapRing`), so the "virtual" ring in `tpt-torus-core` is backed by real kernel shared memory here — no reactor thread needed.
- **`tpt-torus-backend-iocp`** — Windows only. `IocpBackend` runs a background reactor thread that calls `GetQueuedCompletionStatusEx` in a loop and pushes results into a `Mutex<VecDeque<TorusResult>>` woken via `Condvar`. Uses `windows-sys` for IOCP/Winsock/File FFI. `TorusOverlapped` carries `user_data` through the `OVERLAPPED` struct so completions can be matched back to the submitting `Flow`.
- **`tpt-torus-backend-kqueue`** — macOS/BSD (`cfg(unix)`). Same reactor-thread pattern as IOCP, built on raw `kevent`/`kqueue` FFI declared locally in this crate (not in `tpt-torus-sys`, since kqueue's ABI is simple enough to inline). Socket I/O uses native `EVFILT_READ`/`EVFILT_WRITE`; file I/O is dispatched to a thread pool since kqueue has no native async file I/O.

### Key invariants to preserve

- Ring sizes (`ring_entries` passed to `Torus::new`, and the raw ring constructors) must be powers of two — this is asserted, not just documented, since head/tail wraparound math (`wrapping_sub(head) & (entries - 1)`) depends on it.
- `Operation` variants carry raw pointers (`*mut u8`, `*const libc::sockaddr`) directly — this is intentional (it mirrors the zero-copy submission path to the kernel), but it means any *safe* buffer-safety guarantee must come from the caller going through `LeaseRegistry`, not from the `Operation`/`Flow` types themselves.
- Every backend's `Backend` impl must be `Send + Sync` (`Torus` requires this to be shared via `Arc`) — the existing backends do this via explicit `unsafe impl` plus wrapper types for raw handles (e.g. `SafeHandle` in the IOCP backend) rather than relying on the raw handle being `Send`/`Sync` itself.
- `torus-sys`'s struct layouts must match the kernel/OS ABI byte-for-byte; if you change one, check it against the actual kernel headers, not just internal consistency.

## Publishing to crates.io

Every inter-crate dependency is `{ path = "...", version = "0.1.0" }`. This is required for `cargo publish` to work at all, but it also means `cargo package`/`cargo publish` resolve the *version* requirement against the live crates.io index even for a purely local dry run — a crate depending on an unpublished sibling will fail to package with "no matching package named `X` found" until that sibling is actually live. Practically: verify leaf crates first (`tpt-torus-sys`, `tpt-torus-core` have no unpublished deps and can be packaged/verified locally at any time), then publish in strict dependency order and expect downstream crates to be unverifiable locally until their deps are published:

```
tpt-torus-sys → tpt-torus-core → {tpt-torus-backend-uring, tpt-torus-backend-iocp, tpt-torus-backend-kqueue, tpt-torus-cxx, tpt-torus-hw}
```

Bump `workspace.package.version` in the root `Cargo.toml` for releases — it's inherited by every crate via `version.workspace = true`, so all crates version together.
