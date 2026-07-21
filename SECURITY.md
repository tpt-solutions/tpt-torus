# TPT Torus — Threat Model & Security Mitigations

This document mirrors spec.txt Section 5, expanded with implementation details.

## Threat 1: Invalid Pointer to Kernel via SQ

**Description:** User-space application passes a freed or invalid pointer to the kernel via the Submission Queue, leading to kernel panic or privilege escalation.

**Severity:** Critical

**Mitigation: Buffer Leasing + Torus Panic**

The Buffer Leasing system (`lease.rs`) requires all memory passed to I/O operations to be registered with a `LeaseRegistry` before use:

1. **Registration:** Applications call `LeaseRegistry::register()` to declare memory regions. This creates a whitelist of valid buffer addresses.

2. **Checkout/Checkin:** Before each I/O operation, the backend calls `LeaseRegistry::checkout()` to verify the buffer is registered and mark it as in-flight. After completion, `LeaseRegistry::checkin()` releases the reference.

3. **Overlap Detection:** The registry detects overlapping memory regions and rejects registrations that would create ambiguity.

4. **In-Flight Protection:** Buffers that are in-flight cannot be unregistered, preventing accidental deallocation during active I/O.

5. **Torus Panic:** If a lease violation is detected (e.g., unregistered buffer, out-of-bounds access), `TorusPanic` triggers a safe user-space abort with a descriptive error message. This prevents the invalid pointer from ever reaching the kernel.

**Verification:** See `tests/lease_test.rs` for comprehensive tests of lease registration, checkout/checkin, overlap detection, and in-flight protection.

## Threat 2: Container Resource Exhaustion via io_uring

**Description:** Malicious container exhausts kernel resources via io_uring by submitting excessive operations.

**Severity:** High

**Mitigation: Cgroup-Aware Resource Limiting**

The `cgroup` module (`cgroup.rs`) implements automatic resource limiting:

1. **Memory Detection:** Reads cgroup v2 memory limits from `/sys/fs/cgroup/memory.max` or cgroup v1 from `/sys/fs/cgroup/memory/memory.limit_in_bytes`. Falls back to system memory via `/proc/meminfo`.

2. **Ring Size Capping:** SQ/CQ sizes are scaled based on available memory, capped at 4096 entries maximum.

3. **In-Flight Limiting:** Maximum concurrent operations are limited based on memory availability, preventing resource exhaustion.

4. **Default Limits:** When cgroup limits are not detected, sensible defaults are used (1024 ring entries, 1024 max in-flight).

**Verification:** See `cgroup::tests` for resource limit detection and limiter behavior tests.

## Threat 3: Use-After-Free on I/O Buffers

**Description:** Application frees a buffer while an I/O operation is still in-flight, causing the kernel to access freed memory.

**Severity:** Critical

**Mitigation: Buffer Leasing In-Flight Tracking**

The Buffer Leasing system tracks in-flight references:

1. **Reference Counting:** Each registered region maintains an `in_flight` counter. Operations increment this counter on submission and decrement on completion.

2. **Unregister Guard:** `LeaseRegistry::unregister()` refuses to unregister regions with active in-flight operations, returning `LeaseError::InFlight`.

3. **Verification:** `LeaseRegistry::verify()` can check if a buffer is currently in-flight.

**Verification:** See `test_unregister_in_flight` test.

## Threat 4: Kernel Corruption from Invalid Pointers

**Description:** Even with Buffer Leasing, a bug in the backend could allow invalid pointers to reach the kernel.

**Severity:** Critical

**Mitigation: Torus Panic as Last Resort**

If an invalid pointer somehow passes the lease checks, `TorusPanic` provides a final safety net:

1. **Descriptive Abort:** Prints detailed information about the violation (address, error message, stack trace).

2. **Safe Shutdown:** Calls `std::process::abort()` to terminate the process cleanly rather than allowing undefined behavior.

3. **No Recovery:** The panic is intentionally non-recoverable — the process must be restarted to ensure a clean state.

**Verification:** See `torus_panic::tests`.

## Design Principles

### Fail-Safe Defaults

Buffer registration and strict sandboxing are enabled by default. The `unsafe` keyword must be used explicitly to:
- Register memory regions (`LeaseRegistry::register()` is unsafe)
- Access the raw API (`Torus::raw()` is unsafe)
- Create operations with raw pointers (`Operation::Read { buf, .. }` contains raw pointers)

### Progressive Disclosure

The high-level async/await API (`async_api.rs`) covers ~90% of use cases without exposing raw pointers:
- `TorusAsync::read()`, `write()`, `recv()`, `send()`, `accept()`, `connect()`, `close()`
- All methods accept safe references (`&[u8]`, `&mut [u8]`)
- The raw API is available via `Torus::raw()` for the remaining 10%

### Zero-Cost Abstraction

The unified API compiles to the same machine code as native calls:
- On Linux, `Flow`/`Result` map directly to io_uring SQE/CQE
- The async/await wrapper adds no runtime overhead beyond the Future trait
- The cgroup limiter is checked at submission time only, not in hot paths

## Security Review Checklist

- [x] All buffer operations go through `LeaseRegistry`
- [x] In-flight buffers cannot be unregistered
- [x] Out-of-bounds access is detected
- [x] Overlapping regions are rejected
- [x] Torus Panic triggers on lease violations
- [x] Cgroup limits prevent resource exhaustion
- [x] Raw API requires explicit `unsafe` opt-out
- [x] High-level API covers common use cases safely
