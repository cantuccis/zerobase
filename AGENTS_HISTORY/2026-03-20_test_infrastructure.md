# Test Infrastructure and Helpers Setup

**Date:** 2026-03-20
**Task:** Set up test infrastructure and helpers

## Summary

Created a shared test utilities module for `zerobase-api` integration tests, providing:

1. **`TestApp`** — Spawns a full API server on an OS-assigned random port in a background tokio task. Automatically cleans up (aborts the task) on drop. Provides `client()` and `url()` helpers.

2. **`TestClient`** — Wraps `reqwest::Client` with ergonomic methods for all HTTP verbs (`get`, `post`, `put`, `patch`, `delete`, `request`), plus convenience "fire and forget" helpers (`get_response`, `post_json`, `put_json`, `patch_json`, `delete_response`).

3. **Assertion helpers:**
   - `assert_status` — Check response status code
   - `assert_json_response` — Assert status + deserialize JSON body
   - `assert_header` — Check specific header value
   - `assert_header_exists` — Check header presence
   - `assert_request_id_is_uuid` — Validate x-request-id is a UUID

4. **14 example/validation tests** covering all test infrastructure components.

## Design Decision

The architecture doc suggested workspace-root `tests/` directory, but Cargo workspaces don't support integration tests at the workspace root. Tests are placed in `crates/zerobase-api/tests/common/mod.rs` following the standard Rust pattern for shared integration test helpers.

## Test Results

All 67 workspace tests pass (14 new + 53 existing).

## Files Modified

- `crates/zerobase-api/tests/common/mod.rs` — **Created** — TestApp, TestClient, assertion helpers
- `crates/zerobase-api/tests/test_infrastructure.rs` — **Created** — 14 tests validating the test infrastructure
