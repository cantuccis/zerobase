# Email Change Flow

**Date:** 2026-03-21
**Task ID:** fbgr8wd0ep1cubl
**Phase:** 6 — Email Change Flow

## Summary

Implemented the full email change flow following the existing request/confirm pattern used by verification and password reset.

## Changes Made

### Core Types (`zerobase-core/src/auth.rs`)
- Added `EmailChange` variant to `TokenType` enum
- Added `new_email: Option<String>` field to `TokenClaims` (skipped when `None` in serialization)
- Added `generate_with_new_email` method to `TokenService` trait with default implementation

### Token Service (`zerobase-auth/src/token.rs`)
- Implemented `generate_with_new_email` on `JwtTokenService`
- Added `EMAIL_CHANGE` duration constant (1 hour)
- Updated all `TokenClaims` struct literals to include `new_email: None`

### Email Change Service (`zerobase-auth/src/email_change.rs`) — **NEW**
- `EmailChangeService<R, S>` with `request_email_change` and `confirm_email_change` methods
- Request flow: validates auth collection, checks email differs, checks uniqueness, generates JWT with `newEmail` claim, sends confirmation to new address
- Confirm flow: validates token, extracts `new_email` from claims, checks `tokenKey`, re-checks uniqueness (race condition safety), updates email + sets `verified=true`, rotates `tokenKey`
- 16+ unit tests covering happy paths, validation, edge cases, and security scenarios

### HTTP Handlers (`zerobase-api/src/handlers/email_change.rs`) — **NEW**
- `POST /api/collections/:collection/request-email-change` — requires auth (`RequireAuth`), extracts user ID from JWT
- `POST /api/collections/:collection/confirm-email-change` — public endpoint, token-based
- Request bodies with `#[serde(rename_all = "camelCase")]` for `newEmail` field

### Route Wiring (`zerobase-api/src/lib.rs`)
- Added `email_change_routes` function following existing pattern
- Exported `EmailChangeState`

### Fixes Across Codebase
- Updated `TokenClaims` struct literals in `verification.rs`, `password_reset.rs`, `auth_middleware.rs`, `auth_refresh.rs` tests to include `new_email: None`

## Design Decisions
- `new_email` embedded in JWT token rather than stored in a separate DB table (follows PocketBase approach)
- `tokenKey` explicitly rotated on confirm to invalidate all existing sessions
- Email uniqueness re-checked at confirmation time to handle race conditions
- Silent success on errors during request to prevent email enumeration

## Verification
- `cargo build` — passes
- `cargo test` — all tests pass
- `cargo clippy` — no new warnings
