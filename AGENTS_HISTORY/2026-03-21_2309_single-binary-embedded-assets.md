# Build Single Binary with Embedded Assets

**Date:** 2026-03-21 23:09 UTC
**Task ID:** bnf36hpxb90hd1m
**Phase:** 12

## Summary

Configured the Zerobase project to produce a single, self-contained binary with the admin dashboard embedded, set up cross-compilation for multiple targets, created a release build script, and wrote comprehensive smoke tests.

## What Was Done

### 1. Build Metadata Embedding (`build.rs`)
- Created `crates/zerobase-server/build.rs` that captures git hash, build date, target triple, rustc version, and build profile at compile time
- Updated `cmd_version()` in `main.rs` to display all build metadata
- Gracefully handles missing git repository (outputs "unknown")

### 2. Cross-Compilation Configuration
- Created `Cross.toml` at workspace root for the `cross` tool
- Configured targets: Linux amd64/arm64, macOS amd64/arm64, Windows amd64

### 3. Release Build Script
- Created `scripts/release-build.sh` with full CLI:
  - `--target <triple>` for specific target builds
  - `--all` for building all supported targets
  - `--skip-frontend` to use existing frontend dist/
  - `--no-strip` to skip binary stripping
- Builds frontend (pnpm/npm), compiles Rust binary, creates archives (.tar.gz/.zip), generates SHA-256 checksums
- Outputs to `dist/<target>/zerobase[.exe]`

### 4. CI/CD Pipeline Update
- Updated `.github/workflows/ci.yml` with:
  - **smoke-test** job: runs after build, executes the smoke test suite
  - **cross-build** job: matrix build for 5 targets (Linux amd64/arm64, macOS amd64/arm64, Windows amd64), runs on main branch and tags
  - **release** job: creates GitHub releases with archives and checksums on version tags

### 5. Smoke Tests
- Created `crates/zerobase-server/tests/smoke_test.rs` with 3 tests:
  - `binary_starts_and_serves_dashboard`: starts server on ephemeral port, verifies HTML at `/_/`, login page, JS/CSS assets with correct MIME types and cache headers, SPA fallback, and graceful shutdown
  - `binary_version_subcommand_works`: verifies version output includes build metadata
  - `binary_size_is_reasonable`: sanity check that binary is between 1-200 MB
- All 3 smoke tests pass, plus all existing ~2,500+ workspace tests continue to pass

### Results
- **Binary size:** 38 MB (release, with embedded dashboard)
- **All tests passing:** 0 failures across entire workspace
- **Version output example:**
  ```
  zerobase v0.1.0
    git commit:  unknown-dirty
    build date:  2026-03-21T23:09:24Z
    target:      x86_64-unknown-linux-gnu
    rustc:       rustc 1.93.1
    profile:     release
  ```

## Files Modified

- `crates/zerobase-server/build.rs` — **NEW** — build metadata embedding
- `crates/zerobase-server/src/main.rs` — updated `cmd_version()` with build metadata
- `crates/zerobase-server/Cargo.toml` — added `reqwest` (blocking), `libc` dev-dependencies
- `crates/zerobase-server/tests/smoke_test.rs` — **NEW** — smoke test suite
- `Cross.toml` — **NEW** — cross-compilation configuration
- `scripts/release-build.sh` — **NEW** — release build script
- `.github/workflows/ci.yml` — updated with smoke-test, cross-build, and release jobs
