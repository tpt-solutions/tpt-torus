# tpt-torus-backend-kqueue

macOS/BSD `kqueue` backend engine for [TPT Torus](https://github.com/tpt-solutions/tpt-torus).

Runs a background reactor thread using native `kevent`/`kqueue` for socket I/O (`EVFILT_READ`/`EVFILT_WRITE`); file I/O is dispatched to a thread pool since `kqueue` has no native async file I/O.

Use together with [`tpt-torus-core`](https://crates.io/crates/tpt-torus-core).

See the [main repository](https://github.com/tpt-solutions/tpt-torus) for the full design document and usage guide.
