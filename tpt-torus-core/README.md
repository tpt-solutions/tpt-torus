# tpt-torus-core

The Virtual Torus abstraction, Safe API, and `Torus` handle for [TPT Torus](https://github.com/tpt-solutions/tpt-torus) — a unified, cross-platform async I/O framework.

Provides the `Flow`/`Result` submission-completion API, the `Backend` trait implemented by each OS-specific engine, Buffer Leasing and Torus Panic (the Safe API), and a high-level `async`/`await` wrapper.

Pair this crate with a backend crate for your target OS: [`tpt-torus-backend-uring`](https://crates.io/crates/tpt-torus-backend-uring) (Linux), [`tpt-torus-backend-iocp`](https://crates.io/crates/tpt-torus-backend-iocp) (Windows), or [`tpt-torus-backend-kqueue`](https://crates.io/crates/tpt-torus-backend-kqueue) (macOS/BSD).

See the [main repository](https://github.com/tpt-solutions/tpt-torus) for the full design document and usage guide.
