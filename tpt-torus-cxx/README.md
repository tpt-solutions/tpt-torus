# tpt-torus-cxx

C-compatible FFI and C++20 coroutine wrapper for [TPT Torus](https://github.com/tpt-solutions/tpt-torus).

Builds as `cdylib`/`staticlib` in addition to a regular Rust `lib`, and ships a C++ header (`include/torus.hpp`) providing coroutine-based `co_await` access to Torus operations.

See the [main repository](https://github.com/tpt-solutions/tpt-torus) for the full design document and usage guide.
