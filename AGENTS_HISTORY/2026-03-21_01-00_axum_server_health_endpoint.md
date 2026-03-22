# Axum Server with Health Check Endpoint

**Date:** 2026-03-21 01:00
**Task ID:** 2o0gcotjskfmten

## Summary

Enhanced the existing axum server setup to meet all acceptance criteria for the health endpoint task:

1. **JSON health response** — Changed `GET /api/health` from returning plain text `"ok"` to returning `{"status":"ok"}` as JSON via `axum::Json`.
2. **CORS middleware** — Added `tower-http` `CorsLayer` with permissive defaults (`allow_origin(Any)`, `allow_methods(Any)`, `allow_headers(Any)`) to the API router.
3. **Graceful shutdown** — Implemented `shutdown_signal()` in `zerobase-server` that listens for SIGINT (Ctrl-C) and SIGTERM (Unix), wired into `axum::serve::with_graceful_shutdown`.
4. **Integration tests** — Created `health_endpoint.rs` with 6 tests that spin up a real TCP listener using `TcpListener::bind("127.0.0.1:0")` and exercise the server with `reqwest`. Updated existing `tracing_integration.rs` tests for the new JSON response format.

## Test Results

- **53 total tests** pass across the workspace
- **14 tests** specific to `zerobase-api` (2 unit + 6 health_endpoint integration + 6 tracing integration)

## Files Modified

- `crates/zerobase-api/src/lib.rs` — Added CORS layer, changed health response to JSON
- `crates/zerobase-server/src/main.rs` — Added graceful shutdown with signal handling
- `crates/zerobase-api/tests/tracing_integration.rs` — Updated health check assertion for JSON, added CORS preflight test
- `crates/zerobase-api/tests/health_endpoint.rs` — **New file** — Full integration tests over TCP
