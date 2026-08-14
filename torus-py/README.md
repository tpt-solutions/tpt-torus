# torus-py

Python bindings for [TPT Torus](https://github.com/tpt-solutions/tpt-torus),
a cross-platform async I/O library (io_uring / IOCP / kqueue) with optional
hardware bypass.

> **Status:** scaffold. The binding code is real (CFFI over the stable C ABI in
> `torus.h`) but has not been compiled/run here (the Python toolchain was
> unavailable in the environment where it was generated).

## Build

```sh
# 1. Build the C library backing the bindings.
cargo build -p tpt-torus-cxx --release

# 2. Install the Python package.
cd torus-py
pip install -e .
```

## Usage

```python
from torus import Torus, Operation, OpType

t = Torus(ring_entries=1024)
t.submit([Operation(OpType.READ, fd, buf, offset=0)])
for c in t.reap():
    print(c.result, c.user_data)
```

## License

MIT OR Apache-2.0.
