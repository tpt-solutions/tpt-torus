# cargo-torus-new

A `cargo` subcommand that scaffolds a new [TPT Torus](https://github.com/tpt-solutions/tpt-torus) project — the cross-platform async I/O library that unifies `io_uring` (Linux), IOCP (Windows), and kqueue (macOS/BSD) behind one Virtual Torus API.

Equivalent in spirit to `cargo new`, but the generated project is pre-wired against [`torus-rs`](https://crates.io/crates/torus-rs), the ergonomic facade crate, and comes with a working `Flow`/`Operation` example instead of an empty `main.rs`.

## Installation

```bash
cargo install cargo-torus-new
```

## Usage

```bash
cargo torus-new <project-name> [--path <dir>]
```

- `<project-name>` — name of the new project (and its directory).
- `--path <dir>` — create the project under `<dir>` instead of the current directory.

## What it generates

```
<project-name>/
├── Cargo.toml   # depends on `torus = "0.1.0"`
└── src/
    └── main.rs  # opens a Torus with the platform-default backend and
                 # submits a Flow::Read, demonstrating submit/wait/reap
```

The generated `main.rs` is a minimal, runnable example of the raw `Flow`/`Operation` API via the `torus::open()` constructor — a starting point to build from, not a toy.

## Relationship to other crates

`cargo-torus-new` only writes files; it does not depend on any other TPT Torus crate at build time. The projects it generates depend on [`torus-rs`](https://crates.io/crates/torus-rs), which in turn wraps [`tpt-torus-core`](https://crates.io/crates/tpt-torus-core) and the platform backend crates.

## License

Licensed under either of [MIT](https://opensource.org/licenses/MIT) or [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) at your option.
