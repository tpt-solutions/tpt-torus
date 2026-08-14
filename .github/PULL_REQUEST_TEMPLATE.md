<!--
Thanks for contributing to TPT Torus! Please follow the guidelines below.
See CONTRIBUTING.md for the full process (branching, formatting, linting, tests).
-->

## Summary

<!-- What does this PR do and why? Link any related issues with "Fixes #123". -->

## Backend / scope

- [ ] Core (`tpt-torus-core`)
- [ ] Linux io_uring backend
- [ ] Windows IOCP backend
- [ ] macOS/BSD kqueue backend
- [ ] Hardware bypass (`tpt-torus-hw`)
- [ ] Language bindings
- [ ] Docs / examples / CI

## Checklist

- [ ] `cargo fmt --all` has been run
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] Changes are documented (code docs / `CHANGELOG.md` / `todo.md` if roadmap-relevant)
- [ ] Public API changes are reflected in `torus.h` / `torus.hpp` / bindings as needed

## Notes for reviewers

<!-- Anything non-obvious: platform-gating, unsafe blocks, performance trade-offs. -->
