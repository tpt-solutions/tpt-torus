# tpt-torus-backend-uring

Linux `io_uring` backend engine for [TPT Torus](https://github.com/tpt-solutions/tpt-torus).

Maps the Virtual Torus submission/completion rings directly to `io_uring` kernel shared memory via `mmap` — no reactor thread needed. Requires Linux kernel 5.1+.

Use together with [`tpt-torus-core`](https://crates.io/crates/tpt-torus-core).

See the [main repository](https://github.com/tpt-solutions/tpt-torus) for the full design document and usage guide.
