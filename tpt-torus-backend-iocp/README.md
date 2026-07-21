# tpt-torus-backend-iocp

Windows IOCP backend engine for [TPT Torus](https://github.com/tpt-solutions/tpt-torus).

Runs a background reactor thread that drains completions from a Windows I/O Completion Port and translates them into Virtual Torus completions.

Use together with [`tpt-torus-core`](https://crates.io/crates/tpt-torus-core).

See the [main repository](https://github.com/tpt-solutions/tpt-torus) for the full design document and usage guide.
