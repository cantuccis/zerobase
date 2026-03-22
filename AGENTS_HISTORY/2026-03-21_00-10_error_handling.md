# Error Handling Implementation

**Date:** 2026-03-21 00:10
**Task:** Configure project-wide error handling with thiserror

## Summary

Implemented a unified error type hierarchy in `zerobase-core` using `thiserror`. The `ZerobaseError` enum provides 7 variants covering all common failure modes, with HTTP status code mapping, user-facing error body generation, `From` conversions for common source errors, and convenience constructors.

## Design Decisions

- **Framework-agnostic**: The core crate maps to HTTP status codes via a `u16` method, keeping axum out of the domain layer. The API crate will implement `IntoResponse` separately.
- **User safety**: `is_user_facing()` and `error_response_body()` ensure internal/database errors never leak sensitive details to API clients.
- **Conflict variant added**: Beyond the 6 requested variants, added `Conflict` (409) since uniqueness violations are fundamental to a BaaS with schema management.
- **Source chaining**: Database, Auth, and Internal variants carry optional `Box<dyn Error>` sources for diagnostic chains.
- **`ErrorResponseBody`**: A serde-serializable struct matching PocketBase's error JSON format (`code`, `message`, `data`).

## Files Modified

- `crates/zerobase-core/Cargo.toml` — added `rusqlite` and `r2d2` dependencies for `From` conversions
- `crates/zerobase-core/src/lib.rs` — added `error` module and re-exports
- `crates/zerobase-core/src/error.rs` — **new file** — full error hierarchy with 20 unit tests

## Test Results

20 tests passing:
- 7 status code mapping tests (one per variant)
- 2 Display formatting tests
- 2 `is_user_facing` tests
- 4 `error_response_body` tests (hidden details, field errors, resource ID, JSON serialization)
- 2 `From` conversion tests (rusqlite, serde_json)
- 2 source chaining tests
- 1 `Result` alias test
