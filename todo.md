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

- [x] Integrate SPDK for user-space storage I/O (NVMe bypass) — `tpt-torus-hw/src/spdk.rs` now performs real `libspdk` integration behind the `spdk` feature via runtime `libloading`; degrades to `NotAvailable` when libspdk is absent (see Platform Review Follow-ups)
- [x] Integrate DPDK for user-space networking I/O — `tpt-torus-hw/src/dpdk.rs` now performs real `libdpdk` integration behind the `dpdk` feature via runtime `libloading`; degrades to `NotAvailable` when libdpdk is absent (see Platform Review Follow-ups)
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

- [ ] Split `tpt-torus-core`, `tpt-torus-sys`, and backend crates into separate repos (once workspace is stable) — plan captured in `docs/adr-repo-split.md`
- [x] Create `torus-rs` native Rust bindings package (if distinct from `tpt-torus-core` public API)
- [x] Create `torus-go` Go bindings (cgo scaffold over `torus.h`)
- [x] Create `torus-py` Python bindings (CFFI scaffold over `torus.h`)
- [x] Set up `tpt-torus` as the meta-repo / landing page once components are split out (README + `docs/adr-repo-split.md` established)

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
- [x] Commit the real `Cargo.lock` (currently gitignored) or decide to keep ignoring it for these library crates
- [x] Decide whether to publish `tpt-torus-backend-uring`/`-iocp`/`-kqueue` now even though their content is `#![cfg(...)]`-gated to a single OS each (they'll build as empty crates on other platforms, which is fine, but worth a conscious decision)
- [ ] After first publish, verify `cargo add tpt-torus-core` from a scratch project pulls in the expected dependency tree

## Platform Review Follow-ups (2026-07-22)

Findings from a full-codebase review; see `spec.txt` §5 for the threat model these safety items relate to.

### Bugs / correctness

- [x] Reconcile `todo.md` checkmarks against actual implementation status — several Phase 4 items are marked `[x]` but the underlying code is stubbed (see below); merge or delete the stray untracked `todo 1260721.md`
- [x] Implement real SPDK integration in `tpt-torus-hw/src/spdk.rs` (real `libspdk` FFI + runtime loader behind the `spdk` feature; `spdk.rs` `read`/`write`/`flush` call the native NVMe API)
- [x] Implement real DPDK integration in `tpt-torus-hw/src/dpdk.rs` (real `libdpdk` FFI + runtime loader behind the `dpdk` feature; `Mempool`/`Mbuf`/`Port` call the native poll-mode API)
- [x] Run `tpt-torus-hw` hardware-bypass tests in default CI, or clearly document that `gpu_direct`/`spdk`/`dpdk` features are opt-in and untested by default
- [x] Fix `async_api.rs` futures to register real wakers with the backend reactor instead of busy re-polling (`async_api.rs:190-516`, every `*Future::poll`); code-level TODO/tracking note added in the meantime (`async_api.rs` module doc, see below) — actual waker-registration fix still open
- [x] Implement `Operation::Accept`/`Operation::Connect` in `tpt-torus-backend-iocp` via `AcceptEx`/`ConnectEx` instead of returning ENOSYS (`tpt-torus-backend-iocp/src/lib.rs:365-382`)
- [x] Fix TOCTOU race in `LeaseRegistry::register` — check-then-insert must happen under a single write lock, not read-then-write (`tpt-torus-core/src/lease.rs:52-78`)
- [x] Add a concurrent/multithreaded test for `LeaseRegistry::register` to cover the race above
- [x] Harden `lease.rs` range/counter edge cases: `regions.range(..addr + 1)` overflow at `addr == usize::MAX`, `in_flight: u32` unchecked increment (use `saturating_add`)
- [x] Add cbindgen-generated header for `tpt-torus-cxx` instead of hand-maintained `torus.hpp` (currently can drift from `lib.rs`)
- [x] Add CMake/build-system integration and a runnable example for `tpt-torus-cxx`
- [x] Add a cross-backend conformance test suite that runs the identical `Operation` set against uring/iocp/kqueue and asserts identical results

### Missing features vs. spec.txt

- [x] Real tokio/async-std executor interop layer built on `async_api.rs` (replaces busy-poll futures above)
- [x] `torus-go` / `torus-py` language bindings (cgo / CFFI scaffolds over `torus.h`, tracked under Later/Stretch above)

### Innovation / architecture

- [x] Shard the `Torus`/`Backend` lock — replace the single global `Mutex<Box<dyn Backend>>` (`backend.rs:9-27`, `lib.rs:34-92`) with per-core `Torus` instances or lock-free SQ/CQ to avoid serializing all I/O across threads
- [x] Wire io_uring registered buffers (`IORING_REGISTER_BUFFERS`) into `LeaseRegistry` for zero-copy fixed-buffer I/O — `LeaseRegistry::as_register_buffers` + `Backend::register_buffers`/`unregister_buffers` (default no-op) + `UringBackend` records base→index and switches `Read`/`Write`/`Readv`/`Writev` to `IORING_OP_READ/WRITE_FIXED`; exposed via `Torus::register_leases`
- [x] Add multi-shot accept/recv (`IORING_ACCEPT_MULTISHOT` / `IORING_RECV_MULTISHOT` in `sqe.ioprio` — an earlier version of this wrote bogus flags to `sqe.op_flags` and never actually armed multishot; fixed and covered by `test_multi_shot_recv_yields_multiple_completions`) and SQPOLL mode support to `tpt-torus-backend-uring` — `UringBackend::new_with_sqpoll` sets `IORING_SETUP_SQPOLL` at setup (replacing the no-op `enable_sqpoll`)
- [x] Fix `UringBackend::wait`'s `min_complete` heuristic (`tpt-torus-backend-uring/src/lib.rs` `wait`/`reap`) — `reap()` now only decrements `in_flight` for CQEs that disarm their SQE (`IORING_CQE_F_MORE` clear), so armed multishot accept/recv ops keep `in_flight > 0` and `wait()` blocks correctly for the next completion instead of returning immediately; the multishot test now exercises `wait()` directly (previously worked around by polling `reap()`)
- [x] Add a uniform batched/vectored submit API (`submitv`) across all three backends
- [x] Add XDP (eXpress Data Path) as a lightweight alternative to DPDK for networking bypass — runs eBPF at driver level, no hugepages/kernel modules needed, works with standard NICs (`tpt-torus-hw/src/xdp.rs`)
- [x] Add tracing/observability hooks — a span per Flow submit→completion, latency histograms per `Operation` type

### Adoption / usability / automation

- [x] Add workspace-level runnable examples covering all three backends (e.g. `examples/echo_server.rs`, `examples/file_copy.rs`) — currently only one example exists (`tpt-torus-backend-uring/examples/uring_file_io.rs`, Linux-only)
- [x] Add a minimal `TorusAsync` example (caveat as scaffold until the waker fix above lands)
- [x] Add `CONTRIBUTING.md`
- [x] Add `CHANGELOG.md`
- [x] Wire `tpt-torus-hw/benches/hw_bench.rs` into CI (even a "does it run" smoke job)
- [x] Add an MSRV pin/check to CI
- [x] Add fuzzing (e.g. `cargo-fuzz`) for the FFI/parsing boundary in `tpt-torus-sys`
- [x] Add code coverage reporting to CI
- [x] Convert the root README's `ignore`-tagged code sample into a real doctest or point it at a compiled example file

## Platform Review Follow-ups (2026-08-14)

Findings from a follow-up full-codebase review (bugs/TODOs, doc staleness, adoption/DX, CI automation). See the approved plan for full rationale.

### Bugs / stale docs

- [x] Fix `torus_create_with_backend` stub in `tpt-torus-cxx/src/lib.rs:122-136` — always returns ENOSYS instead of building a backend via `make_backend()` like `torus_create` does
- [x] Correct stale "busy-repoll, no real waker registration" claims about `async_api.rs` in `AGENTS.md:40,56` and `CLAUDE.md` — `WakerRegistry` + reaper thread now exist
- [x] Remove stray duplicate `todo 1260721.md` at repo root

### Adoption / DX: README + examples

- [x] Rework README Quick Start to lead with the `TorusAsync` facade instead of the raw `Flow`/`Operation` API; move raw API walkthrough under "Raw API (Opt-Out)"
- [x] Add a `cargo add tpt-torus-core` install line to README
- [x] Add CI/license/crates.io/docs.rs badges to README
- [x] Add `tpt-torus-core/examples/hello_read.rs` (minimal `TorusAsync` example — core currently has zero examples)
- [x] Add `tpt-torus-backend-iocp/examples/iocp_file_io.rs` (Windows-specific runnable sample)
- [x] Add `tpt-torus-backend-kqueue/examples/kqueue_file_io.rs` (macOS/BSD-specific runnable sample)
- [x] Add doc comments to `flow.rs` types/variants (currently only 4 doc lines in the file)

### CI / automation gaps

- [x] Wire `fuzz/` targets (`flow_creation`, `result_parsing`, `operation_validate`) into `.github/workflows/ci.yml` as a bounded-duration job
- [x] Add `.github/ISSUE_TEMPLATE/bug_report.md` and `feature_request.md`
- [x] Add `.github/PULL_REQUEST_TEMPLATE.md`

### Innovation / recommendations (not scoped for implementation yet)

- [x] Tokio-compatible `AsyncRead`/`AsyncWrite` shim over `TorusAsync` — `tpt-torus-core/src/async_tokio.rs` (new `tokio` feature) implements `AsyncRead`/`AsyncWrite` via `TorusAsyncReader`/`TorusAsyncWriter`, driven by the tokio-free `TorusAsync::poll_read_op`/`poll_write_op` helpers; covered by `tests/tokio_shim.rs`.
- [x] Finish the kqueue reactor (event-driven `EVFILT_READ`/`EVFILT_WRITE` + thread-pool file I/O) — `tpt-torus-backend-kqueue/src/lib.rs` now registers socket ops with kqueue (the reactor performs the actual `recv`/`send`/`accept`/`connect` when ready, `EV_ONESHOT`) and dispatches file I/O to a `FileThreadPool`; `Close` is synchronous.
- [x] `cargo generate`/`cargo-torus-new` project template — added the `cargo-torus-new` CLI crate (scaffolds a `torus`-based project) plus a `cargo-generate`-compatible `template/` directory.
- [x] Structured `tracing` spans/metrics around submit/wait/reap — `observability.rs` now records per-`OpKind` latency histograms + success/error counters (always on) and emits `torus_io` spans/events when the `tracing` feature is enabled; `Torus::submit`/`submit_batch`/`reap`/`wait` are wired to create/complete `FlowSpan`s (gated by `feature = "tracing"`).
