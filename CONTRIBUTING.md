# Contributing to TPT Torus

Thanks for your interest in contributing! This document covers the basics.

## Getting Started

1. Fork the repository and clone your fork
2. Install Rust stable (MSRV is 1.87.0)
3. Run `cargo build --workspace` and `cargo test --workspace` to verify your setup

## Development Workflow

### Code Style

- Run `cargo fmt --all` before committing
- Run `cargo clippy --workspace --all-targets -- -D warnings` and fix all warnings
- Follow existing code patterns in each crate

### Platform-Gated Code

Backend crates are platform-gated at the crate root:

- `tpt-torus-backend-uring` — Linux only (`#![cfg(target_os = "linux")]`)
- `tpt-torus-backend-iocp` — Windows only (`#![cfg(target_os = "windows")]`)
- `tpt-torus-backend-kqueue` — macOS/BSD (`#![cfg(unix)]`)

Changes to a backend won't be exercised locally on a different OS. CI runs the full matrix across ubuntu-latest, windows-latest, and macos-latest.

### Testing

```bash
cargo test --workspace                    # all tests
cargo test -p tpt-torus-core              # core tests only
cargo test -p tpt-torus-core -- test_name # single test by name
```

Integration tests for platform-specific backends only run on their target OS in CI.

### Commit Messages

Write clear, concise commit messages. Focus on *why* the change was made, not just *what* changed.

## Pull Requests

1. Create a feature branch from `main`
2. Make your changes with tests where applicable
3. Ensure CI passes (formatting, clippy, build, test on all platforms)
4. Open a PR with a description of the change and motivation

## Architecture

Read `spec.txt` before making architectural decisions. The design rationale and threat model are documented there. `todo.md` tracks the phase-by-phase roadmap.

Key invariants:

- Ring sizes must be powers of two (enforced by assertion)
- `Operation` variants carry raw pointers intentionally — safety comes from `LeaseRegistry`, not the types themselves
- Every backend must be `Send + Sync` (required by `Arc<Torus>`)
- `tpt-torus-sys` struct layouts must match kernel/OS ABIs byte-for-byte

## License

By contributing, you agree that your contributions will be licensed under the MIT OR Apache-2.0 license.
