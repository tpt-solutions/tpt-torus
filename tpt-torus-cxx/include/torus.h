/*
 * torus.h — C ABI for TPT Torus.
 *
 * This header declares the stable `extern "C"` interface exported by the
 * `tpt-torus-cxx` crate (built as a cdylib / static lib). It is the contract
 * used by the `torus-go` and `torus-py` language bindings.
 *
 * Build the shared library with:
 *     cargo build -p tpt-torus-cxx --release
 * and link against `target/release/tpt_torus_cxx` (`.so`/`.dll`/`.dylib`).
 */

#ifndef TORUS_H
#define TORUS_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque Torus instance handle. */
typedef struct TorusHandle TorusHandle;

/* Completion callback invoked when an operation finishes.
 *   result    — bytes transferred, or negative errno on failure
 *   user_data — value supplied at submission time
 *   context   — context pointer supplied at submission time
 */
typedef void (*torus_completion_cb)(int64_t result, uint64_t user_data, void *context);

/* Operation types for torus_submit_batch. */
enum {
    TORUS_OP_READ  = 0,
    TORUS_OP_WRITE = 1,
    TORUS_OP_RECV  = 2,
    TORUS_OP_SEND  = 3,
    TORUS_OP_CLOSE = 4
};

/* C-compatible operation descriptor for batch submission. */
typedef struct {
    uint32_t op_type;   /* TORUS_OP_* */
    int32_t  fd;
    uint8_t *buf;       /* read/recv: *mut, write/send: *const */
    size_t   len;
    uint64_t offset;    /* for read/write */
    uint64_t user_data;
} TorusOperation;

/* C-compatible completion result. */
typedef struct {
    int64_t  result;
    uint64_t user_data;
} TorusCompletion;

/* Create a Torus instance with the platform-default backend.
 * `ring_entries` must be a power of two. `backend` is currently ignored
 * (pass 0). Returns NULL on failure. */
TorusHandle *torus_create(uint32_t ring_entries, int32_t backend);

/* Destroy a Torus instance previously returned by torus_create. */
void torus_destroy(TorusHandle *handle);

/* Submit a read operation. The callback is invoked synchronously once the
 * operation completes (the C API currently uses a blocking completion path). */
int32_t torus_read(TorusHandle *handle, int32_t fd, uint8_t *buf, size_t len,
                   uint64_t offset, uint64_t user_data,
                   torus_completion_cb callback, void *context);

int32_t torus_write(TorusHandle *handle, int32_t fd, const uint8_t *buf, size_t len,
                    uint64_t offset, uint64_t user_data,
                    torus_completion_cb callback, void *context);

int32_t torus_recv(TorusHandle *handle, int32_t fd, uint8_t *buf, size_t len,
                   uint64_t user_data, torus_completion_cb callback, void *context);

int32_t torus_send(TorusHandle *handle, int32_t fd, const uint8_t *buf, size_t len,
                   uint64_t user_data, torus_completion_cb callback, void *context);

int32_t torus_close(TorusHandle *handle, int32_t fd, uint64_t user_data,
                    torus_completion_cb callback, void *context);

/* Submit a batch of operations. Returns the number submitted, or negative errno. */
int32_t torus_submit_batch(TorusHandle *handle, const TorusOperation *ops, uint32_t count);

/* Wait for completions (timeout in microseconds, 0 = infinite). */
int32_t torus_wait(TorusHandle *handle, uint64_t timeout_us);

/* Reap up to `max_results` completions into `results`. Returns the count reaped. */
int32_t torus_reap(TorusHandle *handle, TorusCompletion *results, uint32_t max_results);

/* Number of in-flight operations. */
uint32_t torus_in_flight(const TorusHandle *handle);

#ifdef __cplusplus
}
#endif

#endif /* TORUS_H */
