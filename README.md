# TPT Torus

A unified, cross-platform, high-performance asynchronous I/O framework for Rust.

TPT Torus abstracts OS-specific I/O multiplexing (Linux `io_uring`, Windows IOCP, macOS/BSD `kqueue`) behind a single, memory-safe, zero-cost API — the **Virtual Torus**. Application code is written once against a consistent ring-buffer paradigm (`Flow` for submission, `Result` for completion) and runs natively on every supported OS.

See [`spec.txt`](./spec.txt) for the full design document, and [`todo.md`](./todo.md) for the project roadmap and task checklist.

## Status

Early scaffolding — see `todo.md` for current progress. Not yet usable.

## Architecture

- **`tpt-torus-sys`** — raw, unsafe FFI bindings to `io_uring`, IOCP, and `kqueue`.
- **`tpt-torus-core`** — the Virtual Torus abstraction, the Safe API (Buffer Leasing, Torus Panic), and the `Torus` handle.
- **`tpt-torus-backend-uring`** — Linux `io_uring` engine.
- **`tpt-torus-backend-iocp`** — Windows IOCP engine.
- **`tpt-torus-backend-kqueue`** — macOS/BSD `kqueue` engine.

## License

Licensed under either of [MIT](./LICENSE-MIT) or [Apache License, Version 2.0](./LICENSE-APACHE) at your option.
