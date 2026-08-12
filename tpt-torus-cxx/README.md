# tpt-torus-cxx

C-compatible FFI layer and C++20 coroutine wrapper for [TPT Torus](https://github.com/tpt-solutions/tpt-torus) — a unified, cross-platform async I/O framework.

This crate builds as a **`cdylib` / `staticlib`** (in addition to a regular Rust `lib`) and exposes a stable **C ABI** (`torus.h`) that any language with a C FFI can link against. It is the long-term contract for the non-Rust bindings (`torus-go`, `torus-py`) and for C/C++ callers directly. A C++ header (`include/torus.hpp`) adds `co_await`-based coroutine access to Torus operations.

## What it provides

- `torus.h` — the stable C ABI (kept under `include/`), exported by the `cdylib`.
- `torus.hpp` — C++20 coroutine wrapper providing `co_await torus.read(...)`, `co_await torus.write(...)`, etc.
- C entry points: `torus_create`, submit/reap/wait, and operation constructors.
- `generate-header` binary (`cargo run --bin generate-header -p tpt-torus-cxx`) regenerates `torus.h` via `cbindgen` (requires the `generate-header` feature).

## Installation

For Rust consumers:

```toml
[dependencies]
tpt-torus-cxx = "0.1.0"
```

For C/C++/Go/Python consumers, build the library and link it:

```bash
cargo build -p tpt-torus-cxx --release
# link the produced libtpt_torus_cxx (cdylib or staticlib) and include torus.h / torus.hpp
```

`torus_create` constructs the **platform-default backend** (io_uring on Linux, IOCP on Windows, kqueue on macOS/BSD), so you don't need a Rust side-channel.

## C++ example

```cpp
#include "torus.hpp"

torus::Torus torus(256);
auto result = co_await torus.read(fd, buf, len, 0);
if (result.ok()) {
    std::cout << "Read " << result.bytes() << " bytes\n";
}
```

## Features

| Feature            | Effect                                                                 |
|--------------------|------------------------------------------------------------------------|
| `generate-header`  | Enables the `generate-header` binary that regenerates `torus.h` via `cbindgen`. |

## Relationship to other crates

Depends on `tpt-torus-core` and, per target, a backend crate (`tpt-torus-backend-uring` / `-iocp` / `-kqueue`) for the `torus_create` default backend. The C ABI is the contract used by the `torus-go` and `torus-py` bindings.

## Building & testing

```bash
cargo build -p tpt-torus-cxx --release
cargo run   -p tpt-torus-cxx --bin generate-header --features generate-header
```

## License

Licensed under either of [MIT](https://opensource.org/licenses/MIT) or [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) at your option.
