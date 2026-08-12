"""Python bindings for TPT Torus.

This module loads the C ABI exported by the ``tpt-torus-cxx`` crate (built as a
shared library) via CFFI and exposes a small Pythonic wrapper.

Build the backing library first::

    cargo build -p tpt-torus-cxx --release

then, from this directory::

    pip install -e .
"""

from __future__ import annotations

import ctypes
import os
from typing import List, Optional

from cffi import FFI

__all__ = ["Torus", "Operation", "Completion", "OpType", "TorusError"]

_HERE = os.path.dirname(os.path.abspath(__file__))

# Locate the compiled C library produced by `cargo build -p tpt-torus-cxx`.
_CANDIDATES = [
    os.path.join(_HERE, "..", "target", "release", "tpt_torus_cxx.dll"),
    os.path.join(_HERE, "..", "target", "release", "libtpt_torus_cxx.so"),
    os.path.join(_HERE, "..", "target", "release", "libtpt_torus_cxx.dylib"),
    os.path.join(_HERE, "libtpt_torus_cxx.so"),
]


def _find_lib() -> str:
    for path in _CANDIDATES:
        if os.path.exists(path):
            return path
    raise TorusError(
        "Could not find the compiled tpt_torus_cxx library. "
        "Run `cargo build -p tpt-torus-cxx --release` first."
    )


class TorusError(Exception):
    """Raised on Torus C-API failures."""


ffi = FFI()
ffi.cdef(
    """
    typedef struct TorusHandle TorusHandle;
    typedef void (*torus_completion_cb)(int64_t result, uint64_t user_data, void *context);

    #define TORUS_OP_READ  0
    #define TORUS_OP_WRITE 1
    #define TORUS_OP_RECV  2
    #define TORUS_OP_SEND  3
    #define TORUS_OP_CLOSE 4

    typedef struct {
        uint32_t op_type;
        int32_t  fd;
        uint8_t *buf;
        size_t   len;
        uint64_t offset;
        uint64_t user_data;
    } TorusOperation;

    typedef struct {
        int64_t  result;
        uint64_t user_data;
    } TorusCompletion;

    TorusHandle *torus_create(uint32_t ring_entries, int32_t backend);
    void torus_destroy(TorusHandle *handle);
    int32_t torus_submit_batch(TorusHandle *handle, const TorusOperation *ops, uint32_t count);
    int32_t torus_reap(TorusHandle *handle, TorusCompletion *results, uint32_t max_results);
    uint32_t torus_in_flight(const TorusHandle *handle);
    """
)

_LIB = ffi.dlopen(_find_lib())


class OpType:
    READ = 0
    WRITE = 1
    RECV = 2
    SEND = 3
    CLOSE = 4


class Completion:
    """Result of a finished operation."""

    __slots__ = ("result", "user_data")

    def __init__(self, result: int, user_data: int) -> None:
        self.result = result
        self.user_data = user_data


class Operation:
    """A single batched I/O request."""

    def __init__(
        self,
        op: int,
        fd: int,
        buf: Optional[bytes],
        offset: int = 0,
        user_data: int = 0,
    ) -> None:
        self.op = op
        self.fd = fd
        self.buf = buf
        self.offset = offset
        self.user_data = user_data


class Torus:
    """Wrapper around a TPT Torus instance."""

    def __init__(self, ring_entries: int = 1024) -> None:
        self._handle = _LIB.torus_create(ring_entries, 0)
        if self._handle == ffi.NULL:
            raise TorusError("torus_create failed")

    def __del__(self) -> None:
        if getattr(self, "_handle", ffi.NULL) != ffi.NULL:
            _LIB.torus_destroy(self._handle)
            self._handle = ffi.NULL

    def close(self) -> None:
        if self._handle != ffi.NULL:
            _LIB.torus_destroy(self._handle)
            self._handle = ffi.NULL

    def submit(self, ops: List[Operation]) -> int:
        if not ops:
            return 0
        cops = ffi.new("TorusOperation[]", len(ops))
        buffers = []  # keep Python buffers alive for the duration of the call
        for i, op in enumerate(ops):
            cops[i].op_type = op.op
            cops[i].fd = op.fd
            cops[i].offset = op.offset
            cops[i].user_data = op.user_data
            if op.buf is not None:
                buf = ffi.from_buffer(op.buf)
                buffers.append(buf)
                cops[i].buf = buf
                cops[i].len = len(op.buf)
            else:
                cops[i].len = 0
        n = _LIB.torus_submit_batch(self._handle, cops, len(ops))
        if n < 0:
            raise TorusError("submit_batch failed")
        return n

    def reap(self, max_results: int = 64) -> List[Completion]:
        results = ffi.new("TorusCompletion[]", max_results)
        n = _LIB.torus_reap(self._handle, results, max_results)
        if n < 0:
            raise TorusError("reap failed")
        return [Completion(results[i].result, results[i].user_data) for i in range(n)]

    def in_flight(self) -> int:
        return _LIB.torus_in_flight(self._handle)

    @property
    def handle(self) -> "ctypes._Pointer":  # type: ignore[name-defined]
        return self._handle
