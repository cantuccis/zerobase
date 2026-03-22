# Auth System Architecture Design

**Date:** 2026-03-21 14:00
**Task ID:** w1ymnemsusf7fr4
**Task:** Design authentication system architecture

## Summary

Designed the complete extensible authentication system architecture for Zerobase, covering all auth flows that mirror PocketBase's capabilities. The architecture doc defines:

1. **Core traits**: `AuthMethod` (password, OTP, passkeys), `AuthProvider` (OAuth2 — Google, Microsoft), `TokenService` (JWT lifecycle), `PasswordHasher` (Argon2id abstraction), `AuthRepository` (DB operations for auth).

2. **JWT token structure**: HS256 tokens with claims including `sub`, `collectionId`, `collectionName`, `type` (auth vs admin), `iat`, `exp`, and `tokenKey` for instant invalidation.

3. **Auth flows documented**:
   - Password registration and login
   - OTP (one-time password) via email
   - OAuth2 with PKCE (Google, Microsoft)
   - Superuser authentication
   - MFA (multi-factor authentication)
   - Token refresh
   - Email verification, password reset, email change

4. **Data model**: New system tables `_externalAuths`, `_otps`, `_mfas` alongside existing `_superusers`.

5. **API endpoints**: 14 auth collection endpoints + 7 superuser endpoints, all matching PocketBase semantics.

6. **Extensibility**: Adding new auth methods = implement `AuthMethod` trait. Adding new OAuth2 providers = implement `AuthProvider` trait. No modification of existing code required.

7. **Security**: Argon2id passwords, HMAC-SHA256 JWTs, PKCE for OAuth2, tokenKey rotation for instant invalidation, constant-time comparisons, single-use OTPs.

## Files Modified

- **Created:** `docs/architecture/auth-system.md` — Complete authentication system architecture document

## Files Reviewed (for context)

- `crates/zerobase-auth/src/lib.rs` — Existing placeholder
- `crates/zerobase-auth/Cargo.toml` — Dependencies (jsonwebtoken, argon2, oauth2 already present)
- `crates/zerobase-core/src/schema/collection.rs` — AuthOptions, CollectionType::Auth, system fields
- `crates/zerobase-core/src/configuration.rs` — AuthSettings (token_secret, token_duration_secs)
- `crates/zerobase-core/src/error.rs` — ZerobaseError::Auth variant
- `crates/zerobase-api/src/middleware/auth_context.rs` — Current placeholder auth extraction
- `crates/zerobase-api/src/middleware/require_superuser.rs` — Current placeholder superuser guard
- `crates/zerobase-db/src/migrations/system.rs` — Existing _superusers table
