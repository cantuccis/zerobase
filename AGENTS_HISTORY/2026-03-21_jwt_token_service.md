# JWT Token Service Implementation

**Date:** 2026-03-21
**Task ID:** gkx8knspedwq9yz
**Phase:** 6 — Authentication

## Summary

Implemented JWT token generation and validation using the `jsonwebtoken` crate with HMAC-SHA256 signing. The implementation follows PocketBase's token structure with `id`, `collectionId`, `type`, and `tokenKey` claims.

### What was built

1. **Token types and claims** (`zerobase-core/src/auth.rs`):
   - `TokenType` enum: `Auth`, `Refresh`, `File` with serde serialization
   - `TokenClaims` struct with camelCase JSON serialization matching PocketBase
   - `ValidatedToken` wrapper for decoded claims
   - `TokenService` trait with `generate()`, `validate()`, and `validate_with_key()` methods

2. **JwtTokenService** (`zerobase-auth/src/token.rs`):
   - HMAC-SHA256 signing/verification via `jsonwebtoken` crate
   - Configurable default duration with per-call overrides
   - `from_settings()` constructor using `AuthSettings`
   - Zero-leeway expiry validation
   - Specific error messages for expired tokens, invalid signatures, and type mismatches
   - Default duration constants: AUTH (14 days), REFRESH (90 days), FILE (3 minutes)

3. **28 comprehensive tests** covering:
   - Token generation for all three types (auth, refresh, file)
   - Validation happy paths
   - Claims content and timestamp verification
   - Custom vs default duration
   - Expired token rejection
   - Invalid signature rejection (wrong secret)
   - Tampered token rejection
   - Token type mismatch rejection
   - Token key invalidation (per-user revocation)
   - Refresh flow end-to-end
   - Malformed input handling (empty, garbage, random base64)
   - Configuration integration (`from_settings`)
   - Serialization roundtrips for TokenType and TokenClaims

## Files Modified

- `crates/zerobase-core/src/auth.rs` — Added `TokenType`, `TokenClaims`, `ValidatedToken`, `TokenService` trait
- `crates/zerobase-core/src/lib.rs` — Added re-exports for token types
- `crates/zerobase-auth/src/token.rs` — **New file**: `JwtTokenService` implementation + 28 tests
- `crates/zerobase-auth/src/lib.rs` — Added `token` module and `JwtTokenService` re-export
- `crates/zerobase-auth/Cargo.toml` — Added `secrecy` dependency

## Test Results

- 42 tests in zerobase-auth (28 new token tests + 14 existing password tests)
- 0 failures across the entire workspace (1484+ tests)
