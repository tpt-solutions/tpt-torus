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
- [ ] Cut alpha release (Linux-only, file + socket I/O)

## Phase 2 (Months 4-6) — Windows & macOS Backends, Feature Parity

- [ ] Implement `torus-sys` raw FFI bindings to IOCP (Windows)
- [ ] Implement `torus-sys` raw FFI bindings to kqueue (macOS/BSD)
- [ ] Implement `torus-backend-iocp`: interface to IOCP, utilizing kernel-managed thread pools
- [ ] Implement `torus-backend-kqueue`: interface to kqueue for event notification
- [ ] Design and implement the lock-free background reactor (adaptive draining) for Windows/macOS:
  - [ ] Drains virtual SQ
  - [ ] Translates virtual ops into native IOCP calls (Windows)
  - [ ] Translates virtual ops into native kqueue calls (macOS)
  - [ ] Populates virtual CQ upon completion
- [ ] Port file + socket I/O test suite to run against Windows backend
- [ ] Port file + socket I/O test suite to run against macOS backend
- [ ] Validate feature parity across Linux/Windows/macOS (same Flow/Result API, same test suite passes on all three)
- [ ] Update CI matrix to run full test suite on all three OSes

## Phase 3 (Months 7-9) — Safe API

- [ ] Design Buffer Leasing system (app registers memory regions instead of passing raw pointers)
- [ ] Implement Lease tracking (lock/track in-flight buffers in user-space)
- [ ] Implement Torus Panic (descriptive, safe user-space abort on lease violation, prevents kernel corruption)
- [ ] Add opt-out path for raw/unsafe pointer usage (must be explicit, per Fail-Safe Defaults principle)
- [ ] Design high-level async/await wrapper API (Rust)
- [ ] Implement high-level async/await wrapper API (Rust)
- [ ] Design high-level async/await wrapper API (C++)
- [ ] Implement high-level async/await wrapper API (C++)
- [ ] Write tests proving Torus Panic triggers correctly on freed/invalid buffer access
- [ ] Write tests proving 90% of use cases are servable via the high-level API alone (progressive disclosure)

## Phase 4 (Months 10-12) — Hardware Bypass

- [ ] Integrate SPDK for user-space storage I/O (NVMe bypass)
- [ ] Integrate DPDK for user-space networking I/O
- [ ] Design GPU-Direct orchestration API (DMA transfers, NVMe -> GPU VRAM, bypassing system RAM)
- [ ] Implement GPU-Direct orchestration API
- [ ] Write benchmarks comparing Hardware Bypass path vs. standard kernel path
- [ ] Write docs/examples for SPDK/DPDK/GPU-Direct usage

## Cross-Cutting: Design Principles & Threat Model (apply across all phases)

- [ ] Verify Progressive Disclosure: high-level async/await API covers ~90% of use cases; raw Virtual Torus API available for manual batching/linking
- [ ] Verify Fail-Safe Defaults: buffer registration and strict sandboxing enabled by default; unsafe raw pointers require explicit opt-out
- [ ] Benchmark Zero-Cost Abstraction claim: unified API on Linux must compile to the same machine code as native io_uring calls (no measurable overhead)
- [ ] Implement cgroup-aware resource limiting (cap SQ/CQ size and in-flight request count based on container quotas) — mitigates kernel resource exhaustion threat
- [ ] Security review: confirm no invalid/freed pointer can reach the kernel via the SQ (Buffer Leasing + Torus Panic mitigation from Phase 3)
- [ ] Document threat model and mitigations in repo (mirror spec.txt Section 5)

## Later / Stretch — Ecosystem Split & Language Bindings

- [ ] Split `tpt-torus-core`, `tpt-torus-sys`, and backend crates into separate repos (once workspace is stable)
- [ ] Create `torus-rs` native Rust bindings package (if distinct from `tpt-torus-core` public API)
- [ ] Create `torus-go` Go bindings
- [ ] Create `torus-py` Python bindings (PyO3/CFFI)
- [ ] Set up `tpt-torus` as the meta-repo / landing page once components are split out
