# Auth Methods Listing Endpoint

**Task ID:** ukx5h2vxodji6it
**Date:** 2026-03-21
**Status:** Complete

## Summary

Implemented the `GET /api/collections/:collection/auth-methods` endpoint that returns all available authentication methods for a collection in PocketBase-compatible format.

## Changes Made

### `crates/zerobase-core/src/schema/collection.rs`
- Added `mfa_enabled: bool` and `mfa_duration: u64` fields to `AuthOptions` struct
- Updated `Default` impl to include the new MFA fields

### `crates/zerobase-auth/src/oauth2.rs`
- Replaced flat `AuthMethodInfo` response with structured `AuthMethodsResponse` containing four typed sections:
  - `PasswordAuthMethod` (enabled, identity_fields)
  - `OAuth2AuthMethod` (enabled, providers list with auth URLs)
  - `OtpAuthMethod` (enabled)
  - `MfaAuthMethod` (enabled, duration)
- Updated `list_auth_methods` to generate OAuth2 auth URLs with PKCE code verifiers per provider

### `crates/zerobase-api/src/handlers/oauth2.rs`
- Updated handler to serialize the new `AuthMethodsResponse` directly

### `crates/zerobase-api/tests/oauth2_endpoints.rs`
- Added 8 comprehensive integration tests for the auth-methods endpoint:
  - Full structured response validation with OAuth2 providers
  - Default auth collection (password only)
  - OTP + MFA enabled
  - OAuth2 disabled with empty providers
  - No auth required (public endpoint)
  - Non-auth collection returns 400
  - Nonexistent collection returns 404
  - Password disabled

## Test Results

All 18 oauth2 endpoint tests pass. Full suite: 0 failures.
