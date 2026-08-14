# tpt-torus-fuzz

Fuzz targets for [TPT Torus](https://github.com/tpt-solutions/tpt-torus), built with [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) and `libfuzzer-sys`.

These targets continuously fuzz the core submission/completion types to surface panics, undefined behavior, and memory-safety issues in the Virtual Torus core API.

## Targets

| Target               | Exercises                                              |
|----------------------|--------------------------------------------------------|
| `flow_creation`      | `Flow` construction from arbitrary `Operation` inputs. |
| `result_parsing`     | `Result` / `TorusResult` parsing of arbitrary completion data. |
| `operation_validate` | `Operation` validation paths.                          |

## Running

```bash
cargo +nightly fuzz run flow_creation
cargo +nightly fuzz run result_parsing
cargo +nightly fuzz run operation_validate
```

> Requires the nightly toolchain (cargo-fuzz + libfuzzer). This crate is `publish = false` and is not part of the released workspace.

## License

Licensed under either of [MIT](https://opensource.org/licenses/MIT) or [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) at your option.
