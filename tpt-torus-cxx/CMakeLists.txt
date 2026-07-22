cmake_minimum_required(VERSION 3.15)
project(tpt-torus-cxx LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Options
option(TORUS_BUILD_EXAMPLES "Build TPT Torus C++ examples" ON)
option(TORUS_BUILD_TESTS "Build TPT Torus C++ tests" ON)

# Find the Rust-built static library
# Users must build the Rust library first: cargo build -p tpt-torus-cxx --release
find_library(TORUS_LIB
    NAMES tpt_torus_cxx
    PATHS
        ${CMAKE_SOURCE_DIR}/../../target/release
        ${CMAKE_SOURCE_DIR}/../../target/debug
        $ENV{HOME}/.cargo/lib
    NO_DEFAULT_PATH
)

if(NOT TORUS_LIB)
    message(WARNING
        "tpt-torus-cxx static library not found. "
        "Build it first with: cargo build -p tpt-torus-cxx --release"
    )
endif()

# Header-only interface target for the C++ wrapper
add_library(torus-cxx INTERFACE)
target_include_directories(torus-cxx INTERFACE
    ${CMAKE_SOURCE_DIR}/include
)

# Link the Rust static library if found
if(TORUS_LIB)
    target_link_libraries(torus-cxx INTERFACE ${TORUS_LIB})

    # Platform-specific dependencies
    if(UNIX AND NOT APPLE)
        target_link_libraries(torus-cxx INTERFACE pthread dl m)
    elseif(APPLE)
        target_link_libraries(torus-cxx INTERFACE pthread m)
    elseif(WIN32)
        target_link_libraries(torus-cxx INTERFACE ws2_32 Advapi32 Userenv Bcrypt Ntdll)
    endif()
endif()

# Examples
if(TORUS_BUILD_EXAMPLES)
    add_subdirectory(examples)
endif()

# Tests
if(TORUS_BUILD_TESTS AND TORUS_LIB)
    enable_testing()
    add_subdirectory(tests)
endif()
