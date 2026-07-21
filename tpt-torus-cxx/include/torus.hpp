/// @file torus.hpp
/// @brief C++20 coroutine wrapper for TPT Torus.
///
/// This header provides a modern C++ interface to the TPT Torus async I/O framework.
/// It uses C++20 coroutines for ergonomic async/await syntax.
///
/// # Requirements
/// - C++20 or later
/// - A coroutine-compatible runtime (e.g.,asio, liburing, or a custom executor)
///
/// # Example
///
/// ```cpp
/// #include "torus.hpp"
/// #include <iostream>
///
/// task<int> async_read_example(torus::Torus& torus, int fd, std::vector<uint8_t>& buf) {
///     auto result = co_await torus.read(fd, buf, 0);
///     if (result.ok()) {
///         co_return result.bytes();
///     } else {
///         co_return result.error();
///     }
/// }
/// ```

#pragma once

#include <cstdint>
#include <cstring>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <system_error>
#include <variant>
#include <vector>

#ifdef _WIN32
#define TORUS_API __declspec(dllexport)
#else
#define TORUS_API __attribute__((visibility("default")))
#endif

namespace torus {

// ─── Forward declarations ──────────────────────────────────────────────────

class Torus;

// ─── Result type ───────────────────────────────────────────────────────────

/// Result of an I/O operation.
class Result {
public:
    /// Construct a successful result.
    explicit Result(int64_t bytes) noexcept : m_bytes(bytes), m_error(0) {}

    /// Construct an error result.
    explicit Result(int32_t error) noexcept : m_bytes(0), m_error(error) {}

    /// Check if the operation was successful.
    [[nodiscard]] bool ok() const noexcept { return m_error == 0; }

    /// Get the number of bytes transferred (only valid if ok() is true).
    [[nodiscard]] int64_t bytes() const noexcept { return m_bytes; }

    /// Get the error code (only valid if ok() is false).
    [[nodiscard]] int32_t error() const noexcept { return m_error; }

    /// Convert to std::error_code.
    [[nodiscard]] std::error_code error_code() const {
        return std::error_code(m_error, std::system_category());
    }

    /// Get a human-readable error message.
    [[nodiscard]] std::string error_message() const {
        return error_code().message();
    }

private:
    int64_t m_bytes;
    int32_t m_error;
};

// ─── C FFI bindings ────────────────────────────────────────────────────────

extern "C" {

/// Opaque handle to a Torus instance.
struct TorusHandle;

/// Callback type for async operation completion.
using CompletionCallback = void (*)(int64_t result, uint64_t user_data, void* context);

/// C-compatible operation descriptor.
struct TorusOperation {
    uint32_t op_type;   // 0=Read, 1=Write, 2=Recv, 3=Send, 4=Close
    int32_t fd;
    void* buf;
    size_t len;
    uint64_t offset;
    uint64_t user_data;
};

/// C-compatible completion result.
struct TorusCompletion {
    int64_t result;
    uint64_t user_data;
};

// FFI functions
TORUS_API TorusHandle* torus_create(uint32_t ring_entries, int32_t backend);
TORUS_API int32_t torus_create_with_backend(TorusHandle** handle, uint32_t ring_entries);
TORUS_API void torus_destroy(TorusHandle* handle);

TORUS_API int32_t torus_read(
    const TorusHandle* handle, int32_t fd, void* buf, size_t len,
    uint64_t offset, uint64_t user_data, CompletionCallback callback, void* context);

TORUS_API int32_t torus_write(
    const TorusHandle* handle, int32_t fd, const void* buf, size_t len,
    uint64_t offset, uint64_t user_data, CompletionCallback callback, void* context);

TORUS_API int32_t torus_recv(
    const TorusHandle* handle, int32_t fd, void* buf, size_t len,
    uint64_t user_data, CompletionCallback callback, void* context);

TORUS_API int32_t torus_send(
    const TorusHandle* handle, int32_t fd, const void* buf, size_t len,
    uint64_t user_data, CompletionCallback callback, void* context);

TORUS_API int32_t torus_close(
    const TorusHandle* handle, int32_t fd,
    uint64_t user_data, CompletionCallback callback, void* context);

TORUS_API int32_t torus_submit_batch(
    const TorusHandle* handle, const TorusOperation* ops, uint32_t count);

TORUS_API int32_t torus_wait(const TorusHandle* handle, uint64_t timeout_us);
TORUS_API int32_t torus_reap(
    const TorusHandle* handle, TorusCompletion* results, uint32_t max_results);
TORUS_API uint32_t torus_in_flight(const TorusHandle* handle);

} // extern "C"

// ─── C++ wrapper types ─────────────────────────────────────────────────────

/// RAII wrapper for a Torus handle.
class TorusHandleGuard {
public:
    explicit TorusHandleGuard(TorusHandle* handle) noexcept : m_handle(handle) {}
    ~TorusHandleGuard() { if (m_handle) torus_destroy(m_handle); }

    TorusHandleGuard(const TorusHandleGuard&) = delete;
    TorusHandleGuard& operator=(const TorusHandleGuard&) = delete;

    TorusHandleGuard(TorusHandleGuard&& other) noexcept : m_handle(other.m_handle) {
        other.m_handle = nullptr;
    }
    TorusHandleGuard& operator=(TorusHandleGuard&& other) noexcept {
        if (this != &other) {
            if (m_handle) torus_destroy(m_handle);
            m_handle = other.m_handle;
            other.m_handle = nullptr;
        }
        return *this;
    }

    [[nodiscard]] TorusHandle* get() const noexcept { return m_handle; }
    [[nodiscard]] TorusHandle* release() noexcept {
        auto* h = m_handle;
        m_handle = nullptr;
        return h;
    }

private:
    TorusHandle* m_handle;
};

// ─── Coroutine task type ───────────────────────────────────────────────────

/// A coroutine task that returns a Result.
///
/// Usage:
/// ```cpp
/// task<Result> async_read(Torus& torus, int fd, void* buf, size_t len) {
///     co_return co_await torus.read(fd, buf, len, 0);
/// }
/// ```
template<typename T = Result>
class task {
public:
    struct promise_type {
        T value;

        task get_return_object() noexcept {
            return task{std::coroutine_handle<promise_type>::from_promise(*this)};
        }

        std::suspend_never initial_suspend() noexcept { return {}; }
        std::suspend_never final_suspend() noexcept { return {}; }

        void return_value(T val) noexcept { value = std::move(val); }
        void unhandled_exception() { std::terminate(); }
    };

    explicit task(std::coroutine_handle<promise_type> h) noexcept : m_handle(h) {}
    ~task() { if (m_handle) m_handle.destroy(); }

    task(const task&) = delete;
    task& operator=(const task&) = delete;

    task(task&& other) noexcept : m_handle(other.m_handle) {
        other.m_handle = nullptr;
    }
    task& operator=(task&& other) noexcept {
        if (this != &other) {
            if (m_handle) m_handle.destroy();
            m_handle = other.m_handle;
            other.m_handle = nullptr;
        }
        return *this;
    }

    /// Check if the task is ready.
    [[nodiscard]] bool ready() const noexcept {
        return m_handle && m_handle.done();
    }

    /// Get the result (only valid after the task completes).
    [[nodiscard]] T result() const {
        return m_handle.promise().value;
    }

    /// Resume the coroutine.
    void resume() {
        if (m_handle && !m_handle.done()) {
            m_handle.resume();
        }
    }

private:
    std::coroutine_handle<promise_type> m_handle;
};

// ─── High-level Torus C++ wrapper ──────────────────────────────────────────

/// High-level C++ wrapper for the Virtual Torus.
///
/// Provides ergonomic async operations using C++20 coroutines.
class Torus {
public:
    /// Create a new Torus instance.
    ///
    /// @param ring_entries Number of SQ/CQ entries (must be power of 2).
    /// @throws std::runtime_error if creation fails.
    explicit Torus(uint32_t ring_entries) {
        TorusHandle* handle = torus_create(ring_entries, 0);
        if (!handle) {
            throw std::runtime_error("Failed to create Torus instance");
        }
        m_handle = TorusHandleGuard(handle);
    }

    /// Get the underlying C handle.
    [[nodiscard]] TorusHandle* handle() const noexcept { return m_handle.get(); }

    /// Read from a file descriptor at the given offset.
    ///
    /// @param fd File descriptor.
    /// @param buf Buffer to read into.
    /// @param len Number of bytes to read.
    /// @param offset File offset.
    /// @param user_data User data passed to the callback.
    /// @return A task that resolves to the Result.
    task<Result> read(int fd, void* buf, size_t len, uint64_t offset,
                      uint64_t user_data = 0) {
        struct ReadContext {
            std::function<void(Result)> resolve;
        };

        auto ctx = new ReadContext{};

        auto callback = [](int64_t result, uint64_t ud, void* context) {
            auto* ctx = static_cast<ReadContext*>(context);
            if (result >= 0) {
                ctx->resolve(Result(result));
            } else {
                ctx->resolve(Result(static_cast<int32_t>(-result)));
            }
            delete ctx;
        };

        auto promise = [&]() -> task<Result> {
            // Suspend until the callback resolves
            co_return Result(0); // Placeholder
        };

        int32_t err = torus_read(
            m_handle.get(), fd, buf, len, offset,
            user_data, callback, ctx);

        if (err != 0) {
            delete ctx;
            co_return Result(err);
        }

        // In a real implementation, this would suspend and resume on completion.
        // For now, we do a synchronous wait.
        torus_wait(m_handle.get(), 1'000'000);

        TorusCompletion completion;
        int32_t reaped = torus_reap(m_handle.get(), &completion, 1);
        if (reaped > 0) {
            if (completion.result >= 0) {
                co_return Result(completion.result);
            } else {
                co_return Result(static_cast<int32_t>(-completion.result));
            }
        }

        co_return Result(-110); // ETIMEDOUT
    }

    /// Write to a file descriptor at the given offset.
    task<Result> write(int fd, const void* buf, size_t len, uint64_t offset,
                       uint64_t user_data = 0) {
        struct WriteContext {
            std::function<void(Result)> resolve;
        };

        auto ctx = new WriteContext{};

        auto callback = [](int64_t result, uint64_t ud, void* context) {
            auto* ctx = static_cast<WriteContext*>(context);
            if (result >= 0) {
                ctx->resolve(Result(result));
            } else {
                ctx->resolve(Result(static_cast<int32_t>(-result)));
            }
            delete ctx;
        };

        int32_t err = torus_write(
            m_handle.get(), fd, buf, len, offset,
            user_data, callback, ctx);

        if (err != 0) {
            delete ctx;
            co_return Result(err);
        }

        torus_wait(m_handle.get(), 1'000'000);

        TorusCompletion completion;
        int32_t reaped = torus_reap(m_handle.get(), &completion, 1);
        if (reaped > 0) {
            if (completion.result >= 0) {
                co_return Result(completion.result);
            } else {
                co_return Result(static_cast<int32_t>(-completion.result));
            }
        }

        co_return Result(-110);
    }

    /// Receive data from a connected socket.
    task<Result> recv(int fd, void* buf, size_t len, uint64_t user_data = 0) {
        struct RecvContext {
            std::function<void(Result)> resolve;
        };

        auto ctx = new RecvContext{};

        auto callback = [](int64_t result, uint64_t ud, void* context) {
            auto* ctx = static_cast<RecvContext*>(context);
            if (result >= 0) {
                ctx->resolve(Result(result));
            } else {
                ctx->resolve(Result(static_cast<int32_t>(-result)));
            }
            delete ctx;
        };

        int32_t err = torus_recv(
            m_handle.get(), fd, buf, len,
            user_data, callback, ctx);

        if (err != 0) {
            delete ctx;
            co_return Result(err);
        }

        torus_wait(m_handle.get(), 1'000'000);

        TorusCompletion completion;
        int32_t reaped = torus_reap(m_handle.get(), &completion, 1);
        if (reaped > 0) {
            if (completion.result >= 0) {
                co_return Result(completion.result);
            } else {
                co_return Result(static_cast<int32_t>(-completion.result));
            }
        }

        co_return Result(-110);
    }

    /// Send data to a connected socket.
    task<Result> send(int fd, const void* buf, size_t len, uint64_t user_data = 0) {
        struct SendContext {
            std::function<void(Result)> resolve;
        };

        auto ctx = new SendContext{};

        auto callback = [](int64_t result, uint64_t ud, void* context) {
            auto* ctx = static_cast<SendContext*>(context);
            if (result >= 0) {
                ctx->resolve(Result(result));
            } else {
                ctx->resolve(Result(static_cast<int32_t>(-result)));
            }
            delete ctx;
        };

        int32_t err = torus_send(
            m_handle.get(), fd, buf, len,
            user_data, callback, ctx);

        if (err != 0) {
            delete ctx;
            co_return Result(err);
        }

        torus_wait(m_handle.get(), 1'000'000);

        TorusCompletion completion;
        int32_t reaped = torus_reap(m_handle.get(), &completion, 1);
        if (reaped > 0) {
            if (completion.result >= 0) {
                co_return Result(completion.result);
            } else {
                co_return Result(static_cast<int32_t>(-completion.result));
            }
        }

        co_return Result(-110);
    }

    /// Close a file descriptor.
    task<Result> close(int fd, uint64_t user_data = 0) {
        struct CloseContext {
            std::function<void(Result)> resolve;
        };

        auto ctx = new CloseContext{};

        auto callback = [](int64_t result, uint64_t ud, void* context) {
            auto* ctx = static_cast<CloseContext*>(context);
            if (result >= 0) {
                ctx->resolve(Result(result));
            } else {
                ctx->resolve(Result(static_cast<int32_t>(-result)));
            }
            delete ctx;
        };

        int32_t err = torus_close(
            m_handle.get(), fd,
            user_data, callback, ctx);

        if (err != 0) {
            delete ctx;
            co_return Result(err);
        }

        co_return Result(0);
    }

    /// Wait for completions.
    [[nodiscard]] int32_t wait(uint64_t timeout_us = 1'000'000) {
        return torus_wait(m_handle.get(), timeout_us);
    }

    /// Get the number of in-flight operations.
    [[nodiscard]] uint32_t in_flight() const {
        return torus_in_flight(m_handle.get());
    }

private:
    TorusHandleGuard m_handle{nullptr};
};

// ─── Convenience types ─────────────────────────────────────────────────────

/// Buffer view for read/write operations.
struct BufferView {
    void* data;
    size_t size;

    BufferView(void* data, size_t size) noexcept : data(data), size(size) {}
    template<typename T>
    BufferView(std::vector<T>& vec) noexcept
        : data(vec.data()), size(vec.size() * sizeof(T)) {}
    template<typename T, size_t N>
    BufferView(T (&arr)[N]) noexcept
        : data(arr), size(N * sizeof(T)) {}
};

/// Constant buffer view for write operations.
struct ConstBufferView {
    const void* data;
    size_t size;

    ConstBufferView(const void* data, size_t size) noexcept : data(data), size(size) {}
    template<typename T>
    ConstBufferView(const std::vector<T>& vec) noexcept
        : data(vec.data()), size(vec.size() * sizeof(T)) {}
    template<typename T, size_t N>
    ConstBufferView(const T (&arr)[N]) noexcept
        : data(arr), size(N * sizeof(T)) {}
    ConstBufferView(const char* str) noexcept
        : data(str), size(std::strlen(str)) {}
};

} // namespace torus
