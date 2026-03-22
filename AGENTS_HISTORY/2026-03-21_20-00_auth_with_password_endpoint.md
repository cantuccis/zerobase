# Phase 6: Email/Password Authentication Endpoint

**Task ID:** `adeqwefbpk64jvr`
**Date:** 2026-03-21
**Status:** Complete

## Summary

Implemented the `POST /api/collections/:collection/auth-with-password` endpoint for email/password login, returning a JWT token and user record on success.

## Changes

### 1. `crates/zerobase-core/src/services/record_service.rs`
- Added `authenticate_with_password(collection_name, identity, password)` method to `RecordService`
- Verifies collection is auth type with email auth enabled
- Looks up user by identity fields (e.g. email) using `find_many` with filter
- Verifies password against stored hash using the configured `PasswordHasher`
- Returns user record with password stripped but tokenKey retained for JWT generation

### 2. `crates/zerobase-api/src/handlers/auth.rs` (new)
- `AuthState<R, S>` struct bundling `RecordService` and `TokenService`
- `AuthWithPasswordRequest` deserialization struct (identity + password)
- `auth_with_password` handler:
  - Validates input (non-empty identity and password)
  - Calls `RecordService::authenticate_with_password`
  - Maps auth errors to 400 (matching PocketBase behavior)
  - Generates JWT auth token via `TokenService`
  - Returns `{ "token": "...", "record": { ... } }` with password and tokenKey stripped

### 3. `crates/zerobase-api/src/handlers/mod.rs`
- Added `pub mod auth;`

### 4. `crates/zerobase-api/src/lib.rs`
- Added `auth_routes(record_service, token_service)` function
- Registers `POST /api/collections/{collection_name}/auth-with-password`
- Exported `AuthState` for use by the server composition root

### 5. `crates/zerobase-api/tests/auth_endpoints.rs` (new)
- 8 integration tests covering:
  - Successful login returns token + record (200)
  - Wrong password returns 400
  - Unknown email returns 400
  - Non-auth collection returns 400
  - Nonexistent collection returns 404
  - Empty identity returns 400
  - Empty password returns 400
  - Token contains correct user ID and collection ID

## Test Results

All 8 new tests pass. Full workspace test suite remains green with no regressions.

## Architecture Notes

- The auth handler uses a separate `AuthState` that bundles `RecordService` + `TokenService`, following the existing pattern of typed state per route group
- Authentication errors are returned as 400 (not 401) to match PocketBase's behavior of not distinguishing "user not found" from "wrong password"
- The `authenticate_with_password` method on `RecordService` keeps password verification at the service layer, keeping the handler thin
- Token generation uses the existing `TokenService` trait, allowing the server to inject `JwtTokenService` in production
