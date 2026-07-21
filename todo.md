# TPT Torus — Project Todo

Tracking checklist derived from `spec.txt`. Organized by roadmap phase (Section 7), with a cross-cutting section for constraints that apply across all phases (Sections 4-5).

## Phase 0 — Project Setup

- [x] Initialize Cargo workspace (`Cargo.toml` at repo root)
- [x] Create `tpt-torus-sys` crate skeleton (raw unsafe FFI bindings)
- [x] Create `tpt-torus-core` crate skeleton (Virtual Torus abstraction, Safe API, Torus handle)
- [x] Create `tpt-torus-backend-uring` crate skeleton (Linux)
- [x] Create `tpt-torus-backend-iocp` crate skeleton (Windows)
- [x] Create `tpt-torus-backend-kqueue` crate skeleton (macOS/BSD)
- [x] Write `README.md` (executive summary, architecture overview, quickstart)
- [x] Add `LICENSE-MIT` and `LICENSE-APACHE`
- [x] Add `.gitignore` (Rust defaults)
- [x] Set up GitHub Actions CI matrix (Linux, Windows, macOS) — build + test
- [x] `git init` and initial commit
- [x] Confirm `cargo build --workspace` and `cargo test --workspace` succeed on all stub crates

## Phase 1 (Months 1-3) — Virtual Torus + Linux io_uring Alpha

- [x] Design core Virtual Torus types: virtual Submission Queue (SQ) and Completion Queue (CQ)
- [x] Define the `Torus` handle (single thread-safe context object)
- [x] Define `Flow` type (submission abstraction, replaces raw SQE)
- [x] Define `Result` type (completion abstraction, replaces raw CQE)
- [x] Implement `torus-sys` raw FFI bindings to io_uring (Linux)
- [x] Implement `tpt-torus-backend-uring`: map virtual SQ/CQ directly to io_uring kernel shared memory
- [x] Implement basic file I/O operations via Flow/Result API
- [x] Implement basic socket I/O operations via Flow/Result API
- [x] Write integration tests for file + socket I/O on Linux
- [x] Write basic usage examples/docs for the Flow/Result API
- [x] Cut alpha release (Linux-only, file + socket I/O)

## Phase 2 (Months 4-6) — Windows & macOS Backends, Feature Parity

- [x] Implement `tpt-torus-sys` raw FFI bindings to IOCP (Windows)
- [x] Implement `tpt-torus-sys` raw FFI bindings to kqueue (macOS/BSD)
- [x] Implement `tpt-torus-backend-iocp`: interface to IOCP, utilizing kernel-managed thread pools
- [x] Implement `tpt-torus-backend-kqueue`: interface to kqueue for event notification
- [x] Design and implement the lock-free background reactor (adaptive draining) for Windows/macOS:
  - [x] Drains virtual SQ
  - [x] Translates virtual ops into native IOCP calls (Windows)
  - [x] Translates virtual ops into native kqueue calls (macOS)
  - [x] Populates virtual CQ upon completion
- [x] Port file + socket I/O test suite to run against Windows backend
- [x] Port file + socket I/O test suite to run against macOS backend
- [x] Validate feature parity across Linux/Windows/macOS (same Flow/Result API, same test suite passes on all three)
- [x] Update CI matrix to run full test suite on all three OSes

## Phase 3 (Months 7-9) — Safe API

- [x] Design Buffer Leasing system (app registers memory regions instead of passing raw pointers)
- [x] Implement Lease tracking (lock/track in-flight buffers in user-space)
- [x] Implement Torus Panic (descriptive, safe user-space abort on lease violation, prevents kernel corruption)
- [x] Add opt-out path for raw/unsafe pointer usage (must be explicit, per Fail-Safe Defaults principle)
- [x] Design high-level async/await wrapper API (Rust)
- [x] Implement high-level async/await wrapper API (Rust)
- [x] Design high-level async/await wrapper API (C++)
- [x] Implement high-level async/await wrapper API (C++)
- [x] Write tests proving Torus Panic triggers correctly on freed/invalid buffer access
- [x] Write tests proving 90% of use cases are servable via the high-level API alone (progressive disclosure)

## Phase 4 (Months 10-12) — Hardware Bypass

- [ ] Integrate SPDK for user-space storage I/O (NVMe bypass) — `tpt-torus-hw/src/spdk.rs` API surface exists but every op returns `HwError::NotAvailable`; no real libspdk linkage (see Platform Review Follow-ups)
- [ ] Integrate DPDK for user-space networking I/O — `tpt-torus-hw/src/dpdk.rs` API surface exists but every op returns `HwError::NotAvailable`; no real libdpdk linkage (see Platform Review Follow-ups)
- [x] Design GPU-Direct orchestration API (DMA transfers, NVMe -> GPU VRAM, bypassing system RAM)
- [x] Implement GPU-Direct orchestration API
- [x] Integrate GPU-Direct with io_uring for real NVMe submissions
- [x] Write benchmarks comparing Hardware Bypass path vs. standard kernel path
- [x] Write docs/examples for SPDK/DPDK/GPU-Direct usage

## Cross-Cutting: Design Principles & Threat Model (apply across all phases)

- [x] Verify Progressive Disclosure: high-level async/await API covers ~90% of use cases; raw Virtual Torus API available for manual batching/linking
- [x] Verify Fail-Safe Defaults: buffer registration and strict sandboxing enabled by default; unsafe raw pointers require explicit opt-out
- [x] Benchmark Zero-Cost Abstraction claim: unified API on Linux must compile to the same machine code as native io_uring calls (no measurable overhead)
- [x] Implement cgroup-aware resource limiting (cap SQ/CQ size and in-flight request count based on container quotas) — mitigates kernel resource exhaustion threat
- [x] Security review: confirm no invalid/freed pointer can reach the kernel via the SQ (Buffer Leasing + Torus Panic mitigation from Phase 3)
- [x] Document threat model and mitigations in repo (mirror spec.txt Section 5)

## Later / Stretch — Ecosystem Split & Language Bindings

- [ ] Split `tpt-torus-core`, `tpt-torus-sys`, and backend crates into separate repos (once workspace is stable)
- [ ] Create `torus-rs` native Rust bindings package (if distinct from `tpt-torus-core` public API)
- [ ] Create `torus-go` Go bindings
- [ ] Create `torus-py` Python bindings (PyO3/CFFI)
- [ ] Set up `tpt-torus` as the meta-repo / landing page once components are split out

## crates.io Publish Prep

- [x] Replace placeholder `repository`/add `homepage` in workspace `Cargo.toml` (now points at `https://github.com/tpt-solutions/tpt-torus`)
- [x] Add `version` alongside every `path` dependency (crates.io requires this to resolve published deps)
- [x] Add `readme`, `keywords`, `categories` to every crate's `Cargo.toml`
- [x] Add a per-crate `README.md` stub (crates.io renders whatever `readme` points to; a workspace-root-only README isn't visible on sub-crate pages)
- [x] Confirm all 7 crate names (`tpt-torus-sys`, `tpt-torus-core`, `tpt-torus-backend-uring`, `tpt-torus-backend-iocp`, `tpt-torus-backend-kqueue`, `tpt-torus-cxx`, `tpt-torus-hw`) are unclaimed on crates.io
- [x] `cargo build --workspace` and `cargo package --allow-dirty -p <crate>` succeed for crates with no unpublished deps (`tpt-torus-sys`, `tpt-torus-core`)
- [ ] Publish in dependency order (each step requires the previous to be live on crates.io first, since `cargo package`/`publish` resolve path+version deps against the registry, not just the local path):
  1. `cargo publish -p tpt-torus-sys`
  2. `cargo publish -p tpt-torus-core`
  3. `cargo publish -p tpt-torus-backend-uring`
  4. `cargo publish -p tpt-torus-backend-iocp`
  5. `cargo publish -p tpt-torus-backend-kqueue`
  6. `cargo publish -p tpt-torus-cxx`
  7. `cargo publish -p tpt-torus-hw`
- [ ] Commit the real `Cargo.lock` (currently gitignored) or decide to keep ignoring it for these library crates
- [ ] Decide whether to publish `tpt-torus-backend-uring`/`-iocp`/`-kqueue` now even though their content is `#![cfg(...)]`-gated to a single OS each (they'll build as empty crates on other platforms, which is fine, but worth a conscious decision)
- [ ] After first publish, verify `cargo add tpt-torus-core` from a scratch project pulls in the expected dependency tree

## Platform Review Follow-ups (2026-07-22)

Findings from a full-codebase review; see `spec.txt` §5 for the threat model these safety items relate to.

### Bugs / correctness

- [x] Reconcile `todo.md` checkmarks against actual implementation status — several Phase 4 items are marked `[x]` but the underlying code is stubbed (see below); merge or delete the stray untracked `todo 1260721.md`
- [ ] Implement real SPDK integration in `tpt-torus-hw/src/spdk.rs` (currently a stub returning "requires SPDK to be installed and linked", `spdk.rs:147`)
- [ ] Implement real DPDK integration in `tpt-torus-hw/src/dpdk.rs` (currently always returns `HwError::NotAvailable`, `dpdk.rs:102,191,199,207,263,270`)
- [x] Run `tpt-torus-hw` hardware-bypass tests in default CI, or clearly document that `gpu_direct`/`spdk`/`dpdk` features are opt-in and untested by default
- [ ] Fix `async_api.rs` futures to register real wakers with the backend reactor instead of busy re-polling (`async_api.rs:190-516`, every `*Future::poll`); code-level TODO/tracking note added in the meantime (`async_api.rs` module doc, see below) — actual waker-registration fix still open
- [ ] Implement `Operation::Accept`/`Operation::Connect` in `tpt-torus-backend-iocp` via `AcceptEx`/`ConnectEx` instead of returning ENOSYS (`tpt-torus-backend-iocp/src/lib.rs:365-382`)
- [x] Fix TOCTOU race in `LeaseRegistry::register` — check-then-insert must happen under a single write lock, not read-then-write (`tpt-torus-core/src/lease.rs:52-78`)
- [x] Add a concurrent/multithreaded test for `LeaseRegistry::register` to cover the race above
- [x] Harden `lease.rs` range/counter edge cases: `regions.range(..addr + 1)` overflow at `addr == usize::MAX`, `in_flight: u32` unchecked increment (use `saturating_add`)
- [ ] Add cbindgen-generated header for `tpt-torus-cxx` instead of hand-maintained `torus.hpp` (currently can drift from `lib.rs`)
- [ ] Add CMake/build-system integration and a runnable example for `tpt-torus-cxx`
- [ ] Add a cross-backend conformance test suite that runs the identical `Operation` set against uring/iocp/kqueue and asserts identical results

### Missing features vs. spec.txt

- [ ] Real tokio/async-std executor interop layer built on `async_api.rs` (replaces busy-poll futures above)
- [ ] `torus-go` / `torus-py` language bindings (tracked under Later/Stretch above, calling out explicitly here as review follow-up)

### Innovation / architecture

- [ ] Shard the `Torus`/`Backend` lock — replace the single global `Mutex<Box<dyn Backend>>` (`backend.rs:9-27`, `lib.rs:34-92`) with per-core `Torus` instances or lock-free SQ/CQ to avoid serializing all I/O across threads
- [ ] Wire io_uring registered buffers/files (`IORING_REGISTER_BUFFERS`) into `LeaseRegistry` for zero-copy fixed-buffer I/O
- [ ] Add multi-shot accept/recv and SQPOLL mode support to `tpt-torus-backend-uring`
- [ ] Add a uniform batched/vectored submit API (`submitv`) across all three backends
- [ ] Add tracing/observability hooks — a span per Flow submit→completion, latency histograms per `Operation` type

### Adoption / usability / automation

- [ ] Add workspace-level runnable examples covering all three backends (e.g. `examples/echo_server.rs`, `examples/file_copy.rs`) — currently only one example exists (`tpt-torus-backend-uring/examples/file_io.rs`, Linux-only)
- [ ] Add a minimal `TorusAsync` example (caveat as scaffold until the waker fix above lands)
- [ ] Add `CONTRIBUTING.md`
- [ ] Add `CHANGELOG.md`
- [x] Wire `tpt-torus-hw/benches/hw_bench.rs` into CI (even a "does it run" smoke job)
- [x] Add an MSRV pin/check to CI
- [ ] Add fuzzing (e.g. `cargo-fuzz`) for the FFI/parsing boundary in `tpt-torus-sys`
- [ ] Add code coverage reporting to CI
- [ ] Convert the root README's `ignore`-tagged code sample into a real doctest or point it at a compiled example file
