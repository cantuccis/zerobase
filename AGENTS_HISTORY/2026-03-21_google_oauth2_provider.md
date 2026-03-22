# Google OAuth2 Provider Implementation

**Date:** 2026-03-21
**Task:** Implement Google OAuth2 provider (o1x7bumaylpvck3)
**Phase:** 6

## Summary

Implemented the `GoogleProvider` struct, the first concrete OAuth2 provider in Zerobase. It implements the `OAuthProvider` trait from `zerobase-core` and handles the complete Google OAuth2 authorization code flow with PKCE (S256) support.

### What was built

- **`GoogleProvider`** — Full OAuth2 authorization code flow implementation for Google:
  - Authorization URL generation with PKCE S256 code challenge
  - Token exchange via Google's `https://oauth2.googleapis.com/token` endpoint
  - User info retrieval from Google's `https://www.googleapis.com/oauth2/v3/userinfo` endpoint
  - Default scopes: `openid`, `email`, `profile` (with extra scope support)
  - All endpoints overridable via `OAuthProviderConfig` for testing/custom deployments
  - Custom HTTP client injection via `with_http_client()` for testing

- **`providers` module** — Extensible module structure for concrete OAuth2 providers:
  - `register_default_providers()` function to register all built-in provider factories
  - Ready for additional providers (GitHub, Microsoft, etc.)

- **23 comprehensive tests** covering:
  - Unit tests: provider name, auth URL params, scopes, PKCE, SHA-256, URL encoding, response deserialization
  - Integration tests with `wiremock`: token exchange (success/failure), user info retrieval (full/minimal/unverified/error), full end-to-end flow

### Architecture decisions

- Used `reqwest::Client` directly for HTTP calls (already a workspace dependency)
- Implemented SHA-256 in pure Rust (no extra dependency for a single PKCE use case)
- Raw Google userinfo JSON preserved in `OAuthUserInfo::raw` for extensibility
- Factory pattern registration via `register_default_providers()` for clean startup wiring

## Files Modified

- `crates/zerobase-auth/src/providers/mod.rs` — **NEW** — Provider module with factory registration
- `crates/zerobase-auth/src/providers/google.rs` — **NEW** — Google OAuth2 provider implementation + tests
- `crates/zerobase-auth/src/lib.rs` — Added `providers` module and re-exports
- `crates/zerobase-auth/Cargo.toml` — Added `wiremock` dev-dependency

## Test Results

- 23 Google provider tests: all pass
- 141 total auth crate tests: all pass
- Full workspace build: clean (no errors)
