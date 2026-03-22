# OAuth2 Provider Abstraction Design

**Date:** 2026-03-21
**Task ID:** 5mbve62gu2ahl6i
**Phase:** 6

## Summary

Designed and implemented a trait-based OAuth2 provider system in `zerobase-core` following the established architecture pattern (traits in core, implementations in auth crate). The abstraction supports the full OAuth2 authorization code flow with PKCE, a provider registry/factory pattern for dynamic provider management, and comprehensive test infrastructure.

## What Was Built

### Core Trait: `OAuthProvider`
- `name()` / `display_name()` — provider identification
- `auth_url(state, redirect_url)` — generates authorization URL with PKCE support
- `exchange_code(code, redirect_url, code_verifier)` — exchanges auth code for tokens
- `get_user_info(token)` — fetches normalized user profile from provider

### Data Types
- `OAuthProviderConfig` — client credentials and endpoint overrides (camelCase serialization)
- `AuthUrlResponse` — authorization URL + state + optional PKCE verifier
- `OAuthToken` — access token, refresh token, expiry
- `OAuthUserInfo` — normalized user profile (id, email, email_verified, name, avatar_url, raw JSON)

### Provider Registry (Factory Pattern)
- `OAuthProviderRegistry` — supports both pre-built instances and factory functions
- `register()` — register a fully configured provider
- `register_factory()` — register a factory for on-demand provider creation
- `create()` — create provider from factory + config
- `get()` — look up pre-registered provider
- `available_providers()` / `has_provider()` — introspection

### Test Infrastructure
- `MockOAuthProvider` — deterministic mock for integration testing
- Builder pattern via `with_user_info()` for custom test scenarios
- 22 unit tests covering all types, serialization, registry operations, and async mock behavior

## Files Modified
- `Cargo.toml` (workspace) — added `async-trait = "0.1"` dependency
- `crates/zerobase-core/Cargo.toml` — added `async-trait`, `tokio` (dev)
- `crates/zerobase-auth/Cargo.toml` — added `async-trait`, `reqwest`
- `crates/zerobase-core/src/lib.rs` — added `oauth` module and re-exports
- `crates/zerobase-core/src/oauth.rs` — **new file** — full OAuth2 abstraction (22 tests)

## Test Results
- 22 tests passing
- Full workspace compiles cleanly
