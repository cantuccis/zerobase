# MFA (TOTP) Support

**Date:** 2026-03-21
**Task ID:** aqayd4km88f3w99
**Phase:** 6 — Authentication & Authorization

## Summary

Implemented TOTP-based Multi-Factor Authentication (MFA) with recovery codes across the `zerobase-auth` and `zerobase-api` crates.

## Changes

### Dependencies (workspace `Cargo.toml`)
- Added `totp-rs = { version = "5", features = ["qr", "gen_secret"] }` for TOTP generation/verification
- Added `data-encoding = "2"` for base32 encoding/decoding

### `zerobase-core` — Token Type
- **`src/auth.rs`**: Added `MfaPartial` variant to `TokenType` enum and its `Display` impl
- **`src/auth.rs`** (`token.rs` durations): Added `MFA_PARTIAL: u64 = 5 * 60` constant

### `zerobase-auth` — MFA Service
- **`src/mfa.rs`** (new): Full MFA service implementation
  - `MfaService<R, S>` with TOTP setup, confirmation, verification, disable, and status check
  - `request_mfa_setup()` — generates TOTP secret, stores pending setup, returns secret + QR URI
  - `confirm_mfa_setup()` — verifies TOTP code, stores secret + hashed recovery codes on user record
  - `auth_with_mfa()` — validates MFA partial token, verifies TOTP or recovery code, returns full auth token
  - `disable_mfa()` — clears MFA secret and recovery codes
  - `is_mfa_enabled()` — static check on user record
  - Recovery codes: 8 codes, 10 chars each, alphanumeric, case-insensitive hashed storage
  - TOTP config: SHA-1, 6 digits, 30s step, 1 step skew (RFC 6238)
- **`src/lib.rs`**: Added `pub mod mfa` and `pub use mfa::MfaService`

### `zerobase-api` — HTTP Handlers & Routes
- **`src/handlers/mfa.rs`** (new): Three endpoint handlers
  - `POST /api/collections/:collection/records/:id/request-mfa-setup`
  - `POST /api/collections/:collection/records/:id/confirm-mfa`
  - `POST /api/collections/:collection/auth-with-mfa`
- **`src/handlers/mod.rs`**: Added `pub mod mfa`
- **`src/lib.rs`**: Added `mfa_routes()` function and `MfaState` export
- **`src/handlers/auth.rs`**: Modified `auth_with_password` to check `is_mfa_enabled()` and return `{ mfaToken, mfaRequired: true }` instead of a full auth token when MFA is active

## Testing

- 16 unit tests in `zerobase-auth::mfa::tests` covering:
  - TOTP instance creation (valid/invalid base32)
  - Code generation, verification, rejection, and skew tolerance
  - Recovery code generation (count, uniqueness, length)
  - Recovery code hashing (determinism, case-insensitivity, whitespace trimming)
  - `is_mfa_enabled` for various record states
  - QR URI format validation
- Full workspace test suite: 350+ tests passing

## Architecture Notes

- Follows existing patterns: in-memory pending store (like OTP), Arc-wrapped services, trait-generic handlers
- MFA state stored on user records via `mfaSecret` and `mfaRecoveryCodes` fields
- Two-step auth flow: password auth returns partial token, then `auth-with-mfa` exchanges it for full token
- Recovery codes are consumed on use (removed from stored list)
