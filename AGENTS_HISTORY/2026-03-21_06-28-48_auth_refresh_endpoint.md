# Implement Token Refresh Endpoint

**Date:** 2026-03-21 06:28:48
**Task ID:** uk6hj8p3t7xk0rp

## Summary

Implemented the `POST /api/collections/:collection/auth-refresh` endpoint that allows users to refresh their auth token. The endpoint accepts a valid (non-expired) auth token via the Authorization header, validates it against the user's current tokenKey in the database, and returns a new token with extended expiry along with the latest user record.

## Implementation Details

- The handler uses the `RequireAuth` extractor to enforce authentication (returns 401 for unauthenticated requests)
- Validates that the token's collection matches the requested collection path (returns 400 on mismatch)
- Loads the fresh user record from the database to verify the tokenKey hasn't been rotated
- Generates a new auth token with the default expiry duration
- Strips sensitive fields (password, tokenKey) from the response record

## Tests Written (9 integration tests)

1. `auth_refresh_returns_new_token_and_record` - Happy path: valid token returns new token + record
2. `auth_refresh_without_token_returns_401` - No auth header returns 401
3. `auth_refresh_with_expired_token_returns_401` - Expired tokens rejected
4. `auth_refresh_with_invalid_token_returns_401` - Invalid/garbage tokens rejected
5. `auth_refresh_with_changed_token_key_returns_401` - Rotated tokenKey invalidates refresh
6. `auth_refresh_for_nonexistent_collection_returns_401` - Bad collection in path
7. `auth_refresh_collection_mismatch_returns_400` - Token collection != path collection
8. `auth_refresh_returns_fresh_token_with_correct_format` - Token format validation
9. `auth_refresh_returns_latest_record_data` - Response includes up-to-date record fields

## Files Modified

- `crates/zerobase-api/src/handlers/auth.rs` - Added `auth_refresh` handler function
- `crates/zerobase-api/src/lib.rs` - Registered auth-refresh route in `auth_routes()`

## Files Created

- `crates/zerobase-api/tests/auth_refresh.rs` - 9 integration tests

## Test Results

- All 224 tests pass (9 new + 215 existing)
- No regressions
