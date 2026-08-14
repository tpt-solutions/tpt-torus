# torus-go

Go bindings for [TPT Torus](https://github.com/tpt-solutions/tpt-torus), a
cross-platform async I/O library (io_uring / IOCP / kqueue) with optional
hardware bypass.

> **Status:** scaffold. The binding code is real and targets the stable C ABI
> in `torus.h`, but it has not yet been compiled/run here (the Go toolchain was
> unavailable in the environment where it was generated).

## Build

```sh
# 1. Build the C library backing the bindings.
cargo build -p tpt-torus-cxx --release

# 2. Build / test the Go package (cgo links libtpt_torus_cxx).
cd torus-go
go build ./...
go test ./...
```

The cgo `LDFLAGS` in `torus.go` point at `../../target/release`; adjust if your
library location differs. On Windows use `-ltpt_torus_cxx` against the `.dll`
(and ensure it is on `PATH` at runtime); on macOS/Linux against the `.dylib`/`.so`.

## Usage

```go
h, err := torus.Create(1024)
if err != nil { panic(err) }
defer h.Destroy()

n, err := h.Submit([]torus.Operation{{
    Op:     torus.OpRead,
    Fd:     fd,
    Buf:    buf,
    Offset: 0,
}})
_ = n
comps, _ := h.Reap(1)
```

## License

MIT OR Apache-2.0.
