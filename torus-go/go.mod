module github.com/tpt-solutions/torus-go

go 1.22

// torus-go: Go bindings for TPT Torus.
//
// These bindings call the stable C ABI declared in torus.h (exported by the
// tpt-torus-cxx crate). They use cgo and avoid C callbacks by routing all I/O
// through the synchronous batch + reap path.
