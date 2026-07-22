/// @file basic_usage.cpp
/// @brief Basic example of using TPT Torus from C++.
///
/// Build with CMake:
///   mkdir build && cd build
///   cmake .. -DTORUS_BUILD_EXAMPLES=ON
///   cmake --build .
///   ./basic_usage

#include "torus.hpp"

#include <cstdio>
#include <cstring>
#include <vector>

int main() {
    // Create a Torus instance (platform-specific backend)
    torus::Torus torus(256);
    if (!torus.valid()) {
        std::fprintf(stderr, "Failed to create Torus instance\n");
        return 1;
    }

    // Create a temporary file
    const char* path = "torus_cpp_example.txt";
    const char* message = "Hello from TPT Torus C++!\n";

    // Open file for writing
    FILE* f = std::fopen(path, "wb");
    if (!f) {
        std::fprintf(stderr, "Failed to open file\n");
        return 1;
    }

    // Get the file descriptor
    // On Linux/macOS, we can use fileno(); on Windows, _fileno()
#ifdef _WIN32
    int fd = _fileno(f);
#else
    int fd = fileno(f);
#endif

    // Write using Torus
    std::vector<uint8_t> write_buf(message, message + std::strlen(message));
    auto write_result = torus.write(fd, write_buf, 0);
    std::fclose(f);

    if (write_result.ok()) {
        std::printf("Wrote %ld bytes\n", write_result.bytes());
    } else {
        std::fprintf(stderr, "Write failed: %s\n", write_result.error_message().c_str());
        return 1;
    }

    // Read back
    f = std::fopen(path, "rb");
    if (!f) {
        std::fprintf(stderr, "Failed to reopen file\n");
        return 1;
    }

#ifdef _WIN32
    fd = _fileno(f);
#else
    fd = fileno(f);
#endif

    std::vector<uint8_t> read_buf(4096);
    auto read_result = torus.read(fd, read_buf, 0);
    std::fclose(f);

    if (read_result.ok()) {
        auto n = static_cast<size_t>(read_result.bytes());
        std::printf("Read %ld bytes: %.*s\n", read_result.bytes(), static_cast<int>(n), read_buf.data());
    } else {
        std::fprintf(stderr, "Read failed: %s\n", read_result.error_message().c_str());
        return 1;
    }

    // Cleanup
    std::remove(path);

    std::printf("Done!\n");
    return 0;
}
