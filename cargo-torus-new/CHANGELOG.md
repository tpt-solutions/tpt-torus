# Changelog

All notable changes to `cargo-torus-new` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-07-22

### Added

- `cargo torus-new <project-name> [--path <dir>]` subcommand that scaffolds a minimal project depending on the `torus` ergonomic facade.
- Generated project includes a working `src/main.rs` demonstrating the `Flow`/`Operation` submit/wait/reap cycle via `torus::open()`.
