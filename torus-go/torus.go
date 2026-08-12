// Package torus provides Go bindings for TPT Torus, a cross-platform
// asynchronous I/O library (io_uring / IOCP / kqueue) with optional hardware
// bypass.
//
// The bindings target the C ABI in `torus.h`, exported by the `tpt-torus-cxx`
// crate as a shared/static library. Build that library first:
//
//	cargo build -p tpt-torus-cxx --release
//
// then point the cgo LDFLAGS at the produced `tpt_torus_cxx` library.
package torus

/*
#cgo LDFLAGS: -L${SRCDIR}/../../target/release -ltpt_torus_cxx
#include "../../tpt-torus-cxx/include/torus.h"
#include <stdlib.h>
*/
import "C"

import (
	"errors"
	"unsafe"
)

// OpType enumerates the I/O operations supported by a batch submission.
type OpType int

const (
	OpRead  OpType = C.TORUS_OP_READ
	OpWrite OpType = C.TORUS_OP_WRITE
	OpRecv  OpType = C.TORUS_OP_RECV
	OpSend  OpType = C.TORUS_OP_SEND
	OpClose OpType = C.TORUS_OP_CLOSE
)

// Completion is the result of a finished operation.
type Completion struct {
	Result   int64
	UserData uint64
}

// Handle is an opaque Torus instance.
type Handle struct {
	ptr *C.TorusHandle
}

// Create opens a Torus instance with `ringEntries` SQ/CQ slots (must be a
// power of two). The platform-default backend is selected automatically.
func Create(ringEntries uint32) (*Handle, error) {
	h := C.torus_create(C.uint32_t(ringEntries), 0)
	if h == nil {
		return nil, errors.New("torus: torus_create failed")
	}
	return &Handle{ptr: h}, nil
}

// Destroy releases the Torus instance. The handle must not be used afterwards.
func (h *Handle) Destroy() {
	if h.ptr != nil {
		C.torus_destroy(h.ptr)
		h.ptr = nil
	}
}

// Operation describes a single batched I/O request.
type Operation struct {
	Op       OpType
	Fd       int32
	Buf      []byte
	Offset   uint64
	UserData uint64
}

// Submit schedules one or more operations. It returns the number successfully
// submitted, or an error.
func (h *Handle) Submit(ops []Operation) (int, error) {
	if len(ops) == 0 {
		return 0, nil
	}
	cops := make([]C.TorusOperation, len(ops))
	for i, op := range ops {
		cops[i].op_type = C.uint32_t(op.Op)
		cops[i].fd = C.int32_t(op.Fd)
		cops[i].offset = C.uint64_t(op.Offset)
		cops[i].user_data = C.uint64_t(op.UserData)
		if len(op.Buf) > 0 {
			cops[i].buf = (*C.uint8_t)(unsafe.Pointer(&op.Buf[0]))
		}
		cops[i].len = C.size_t(len(op.Buf))
	}
	n := C.torus_submit_batch(h.ptr, &cops[0], C.uint32_t(len(ops)))
	if n < 0 {
		return 0, errors.New("torus: submit_batch failed")
	}
	return int(n), nil
}

// Reap collects up to `max` completions.
func (h *Handle) Reap(max int) ([]Completion, error) {
	if max <= 0 {
		return nil, nil
	}
	results := make([]C.TorusCompletion, max)
	n := C.torus_reap(h.ptr, &results[0], C.uint32_t(max))
	if n < 0 {
		return nil, errors.New("torus: reap failed")
	}
	out := make([]Completion, 0, n)
	for i := 0; i < int(n); i++ {
		out = append(out, Completion{
			Result:   int64(results[i].result),
			UserData: uint64(results[i].user_data),
		})
	}
	return out, nil
}

// InFlight returns the number of in-flight operations.
func (h *Handle) InFlight() uint32 {
	return uint32(C.torus_in_flight(h.ptr))
}
