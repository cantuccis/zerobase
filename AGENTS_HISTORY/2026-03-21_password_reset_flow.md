# Password Reset Flow Implementation

**Date:** 2026-03-21
**Task ID:** 3hne7dbm43pq5pe
**Phase:** 6

## Summary

Implemented the complete password reset flow with two endpoints:
- `POST /api/collections/:collection/request-password-reset` — sends a time-limited reset email
- `POST /api/collections/:collection/confirm-password-reset` — validates token and sets new password

The implementation follows the same architecture as the existing email verification flow, with:
- Silent success for unknown emails (prevents email enumeration)
- Time-limited tokens (1 hour expiry)
- Automatic tokenKey rotation on password change (invalidates all existing auth/refresh tokens)
- Token single-use enforcement via tokenKey check
- Password validation (min 8 chars, confirmation match)

## Files Modified

### New Files
- `crates/zerobase-auth/src/password_reset.rs` — `PasswordResetService` with request/confirm logic and 16 unit tests
- `crates/zerobase-api/src/handlers/password_reset.rs` — HTTP handlers for both endpoints

### Modified Files
- `crates/zerobase-core/src/auth.rs` — Added `PasswordReset` variant to `TokenType` enum
- `crates/zerobase-auth/src/token.rs` — Added `PASSWORD_RESET` duration constant (1 hour), updated display/serialization tests
- `crates/zerobase-auth/src/lib.rs` — Registered `password_reset` module and re-exported `PasswordResetService`
- `crates/zerobase-api/src/handlers/mod.rs` — Registered `password_reset` handler module
- `crates/zerobase-api/src/lib.rs` — Added `password_reset_routes()` builder function, exported `PasswordResetState`

## Tests Added (16 new tests)

### Request Password Reset
1. `request_reset_sends_email_for_existing_user` — verifies email sent with correct token
2. `request_reset_silent_success_for_unknown_email` — prevents email enumeration
3. `request_reset_fails_for_empty_email` — validation error
4. `request_reset_fails_for_non_auth_collection` — rejects base collections
5. `request_reset_fails_for_unknown_collection` — 404 for missing collection
6. `request_reset_email_contains_reset_url` — verifies email body content
7. `request_reset_email_failure_returns_error` — SMTP failure handling

### Confirm Password Reset
8. `confirm_reset_updates_password` — password hashed and tokenKey rotated
9. `confirm_reset_fails_with_empty_token` — validation error
10. `confirm_reset_fails_with_empty_password` — validation error
11. `confirm_reset_fails_when_passwords_dont_match` — confirmation mismatch
12. `confirm_reset_fails_when_password_too_short` — minimum 8 characters
13. `confirm_reset_fails_with_invalid_token` — expired/invalid JWT
14. `confirm_reset_fails_with_collection_mismatch` — cross-collection token rejection
15. `confirm_reset_fails_with_invalidated_token_key` — revoked token detection
16. `confirm_reset_fails_for_nonexistent_user` — 404 for missing user
17. `confirm_reset_fails_for_non_auth_collection` — rejects base collections
18. `confirm_reset_token_used_once_only` — token invalidated after use via tokenKey rotation

## Test Results

All workspace tests passing: 1590+ tests, 0 failures.
