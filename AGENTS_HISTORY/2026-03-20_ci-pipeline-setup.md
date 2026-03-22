# CI Pipeline Setup

**Date:** 2026-03-20
**Task:** Set up CI pipeline with GitHub Actions

## Summary

Created a GitHub Actions CI workflow at `.github/workflows/ci.yml` with four parallel jobs:

1. **Rustfmt** — checks formatting with `cargo fmt --all --check`
2. **Clippy** — lints with `cargo clippy --workspace --all-targets -- -D warnings`
3. **Tests** — runs `cargo test --workspace`
4. **Build (release)** — builds with `cargo build --workspace --release`

The pipeline triggers on push to `main` and on pull requests targeting `main`. Cargo caching is handled via `Swatinem/rust-cache@v2` on clippy, test, and build jobs. The fmt job skips caching since it only checks formatting and doesn't compile.

All four steps were verified locally and pass on the current project state.

## Files Modified

- `.github/workflows/ci.yml` (created)
