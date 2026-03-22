# Microsoft OAuth2 Provider Implementation

**Date:** 2026-03-21
**Task ID:** 0urrwrwzggbh76n

## Summary

Implemented the Microsoft OAuth2 provider (`MicrosoftProvider`) following the same architecture as the existing Google provider. The implementation supports the complete Azure AD v2.0 authorization code flow with PKCE, token exchange, and user info retrieval from Microsoft Graph API.

### Key Features
- **Azure AD v2.0 endpoints** using the `common` tenant (supports both personal and organizational accounts)
- **PKCE (S256)** support for enhanced security
- **Microsoft Graph API** integration for user profile (`/v1.0/me`)
- **Smart email extraction**: prefers `mail` field, falls back to `userPrincipalName`, filters out non-email UPNs (e.g., `live.com#...`)
- **Default scopes**: `openid`, `email`, `profile`, `User.Read`
- **Endpoint overrides** via `OAuthProviderConfig` for single-tenant Azure AD deployments
- **Custom HTTP client injection** for testing
- **Full raw JSON preservation** in `OAuthUserInfo::raw`

### Test Coverage (26 tests)
- Unit tests: provider name, auth URL generation, scope handling, PKCE, response deserialization, email extraction logic
- Integration tests with `wiremock`: token exchange (success/failure/no-refresh), user info (org account, personal account, minimal, error, hash UPN), full OAuth flow, form parameter verification

## Files Modified

| File | Action |
|------|--------|
| `crates/zerobase-auth/src/providers/microsoft.rs` | **Created** - Full Microsoft OAuth2 provider with 26 tests |
| `crates/zerobase-auth/src/providers/mod.rs` | **Modified** - Added `microsoft` module, `MicrosoftProvider` re-export, and factory registration |
