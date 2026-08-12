# ADR: Splitting the Torus Workspace into Independent Repositories

- **Status:** Proposed (pending execution)
- **Context:** The project currently lives as a single Cargo workspace
  (`tpt-torus`) containing 7 crates plus the `torus-rs` facade. As the API
  stabilizes we want each publishable crate to live in its own repository, with
  `tpt-torus` becoming a meta-repo / landing page.

## Goals

1. Each crate (`tpt-torus-sys`, `tpt-torus-core`, `tpt-torus-backend-uring`,
   `tpt-torus-backend-iocp`, `tpt-torus-backend-kqueue`, `tpt-torus-cxx`,
   `tpt-torus-hw`, `torus-rs`) gets its own git repository and issue tracker.
2. `tpt-torus` becomes a thin meta-repo with the landing-page README, CI that
   smoke-builds the published crates from crates.io, and links to each repo.
3. The non-Rust bindings (`torus-go`, `torus-py`) and the C++ wrapper
   (`tpt-torus-cxx`) keep their own repositories, bound to the C ABI
   (`torus.h`), which is forward-compatible by design.

## Constraints / Decisions

- **Dependency order is fixed by the publish sequence** (see `todo.md` →
  "crates.io Publish Prep"): `tpt-torus-sys` → `tpt-torus-core` →
  `tpt-torus-backend-uring` → `tpt-torus-backend-iocp` →
  `tpt-torus-backend-kqueue` → `tpt-torus-cxx` → `tpt-torus-hw`. Each repo's
  `Cargo.toml` already carries `version` alongside every `path` dependency, so
  once a dependency is live on crates.io the dependent repo can switch from a
  `path` dependency to the versioned crates.io dependency.
- **The C ABI is the long-term contract.** `tpt-torus-cxx` exports `torus.h`;
  the Go/Python bindings depend only on it, not on Rust internals. Splitting the
  Rust crates does not affect them.
- **CI must keep running cross-platform** (Linux/Windows/macOS) against the real
  backends. Each repo should inherit the existing GitHub Actions matrix.

## Execution Steps

1. Freeze the current API (this is the trigger condition: "once the workspace is
   stable").
2. For each crate, topologically in publish order:
   - Create the new repo, copy the crate directory, add its own `Cargo.toml`,
     `README.md`, `LICENSE-*`, CI workflow, and `CHANGELOG.md`.
   - Replace intra-workspace `path` deps with versioned crates.io deps for
     already-published siblings.
   - `cargo package --allow-dirty` to verify it builds standalone.
   - Publish to crates.io (if public).
3. Convert `tpt-torus` into a meta-repo: delete the crate sources, keep a
   landing-page `README.md` (already maintained), a CI job that `cargo add`s and
   smoke-builds the published crates, and pointers to each sub-repo.
4. Update `docs/adr-repo-split.md` to "Accepted" and remove the corresponding
   `todo.md` "Later / Stretch" items.

## Risks

- **Version skew:** publish order matters; a dependent published before its
  dependency resolves `path` (not crates.io) and breaks. Mitigated by the
  explicit ordering above and CI `cargo package` checks.
- **Cross-repo CI duplication:** each repo needs its own matrix. Acceptable
  trade-off for independent release cadence.
- **C ABI drift:** keep `torus.h` under a compatibility policy; bump the major
  version of `tpt-torus-cxx` on any breaking change.
