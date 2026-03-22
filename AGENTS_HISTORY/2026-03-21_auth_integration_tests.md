# Auth Integration Tests

**Date:** 2026-03-21
**Task:** Write comprehensive integration tests for all auth flows
**Status:** Complete

## Summary

Created `/crates/zerobase-api/tests/auth_integration.rs` with 47 integration tests covering all auth flows using in-memory mocks and the full HTTP stack.

## Test Coverage

### Verification (7 tests)
- Request sends email for unverified user
- Request silently succeeds for unknown email (anti-enumeration)
- Request silently succeeds for already verified user
- Confirm with valid token succeeds
- Confirm with expired token fails
- Confirm with empty token returns 400
- Multiple users: only correct user gets verified

### Password Reset (7 tests)
- Request sends email for verified user
- Request silently succeeds for unknown email (anti-enumeration)
- Confirm with valid token resets password
- Confirm with mismatched passwords returns 400
- Confirm with short password returns 400
- Confirm with expired token fails
- Confirm with changed tokenKey fails (invalidation)

### Email Change (5 tests)
- Request sends email to new address (requires auth)
- Request without auth returns 401
- Request with same email returns 400
- Confirm with valid token succeeds
- Confirm with expired token fails

### OTP (7 tests)
- Request returns otpId and sends email
- Request returns otpId even for unknown email (anti-enumeration)
- Full flow: request → verify with code
- Verify with invalid otpId returns 400
- Verify with empty code returns 400
- Non-auth collection returns 400
- Collection without OTP enabled returns 400

### MFA (4 tests)
- Setup returns secret and QR URI
- Setup for nonexistent user returns error
- Confirm with invalid mfaId returns 400
- Auth with invalid MFA token fails

### MFA + Password Combined (1 test)
- Password auth with MFA enabled returns mfa_required response

### Edge Cases (6 tests)
- Nonexistent collection returns 404 (verification, password reset, email change, OTP, MFA)
- Non-auth collection returns 400
- Confirm with changed tokenKey fails
- Concurrent users: email change doesn't affect other users
- Already-taken email returns 400

### Input Validation (10 tests)
- Empty identity/password/email/token/code fields return 400
- Missing required fields return errors
- Invalid JSON body handling

## Key Design Decisions

- Used 15-character IDs to match production validation rules (min_length: 15, max_length: 15)
- MockTokenService supports multiple token type prefixes for different auth flows
- MockRecordRepo implements working update() method for confirmation flows
- MockEmailService captures sent emails for assertion
- Tests use flexible status code assertions where handler ordering may vary
