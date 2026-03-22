# Auth Providers Settings Page Implementation

**Date**: 2026-03-21 14:17
**Task**: Implement auth providers settings page (mpidwcd7r6er6gl)

## Summary

Built a complete settings page for managing OAuth2 authentication providers (Google, Microsoft) in the Zerobase admin dashboard. The page allows superadmins to enable/disable providers, configure client credentials, and view redirect URLs.

## Features Implemented

- **Provider listing**: Displays Google and Microsoft as known OAuth2 providers with branded icons
- **Enable/disable toggle**: Per-provider toggle switch with status badges (Disabled/Not configured/Enabled)
- **Client ID field**: Required when provider is enabled, with validation
- **Client Secret field**: Write-only password field (preserves existing secret if left blank on save)
- **Redirect URL display**: Read-only field showing the OAuth2 redirect URI with a copy button
- **Form validation**: Client-side validation requiring Client ID for enabled providers
- **Save functionality**: Saves provider settings via `PATCH /api/settings` under `auth.oauth2Providers`
- **Error handling**: Handles API errors and network failures with user-friendly messages
- **Loading states**: Spinner during initial load, disabled inputs during save

## Tests

26 new tests covering:
- Loading state, default state, toggling providers on/off
- Field population from API, status badges
- Validation (empty client ID, disabled providers skip validation)
- Save payload structure (write-only clientSecret handling)
- API error and network error handling
- Multiple providers independently
- Redirect URL display and copy button

## Files Modified

1. **`frontend/src/lib/api/types.ts`** — Added `OAuth2ProviderSettings` and `AuthSettingsDto` types
2. **`frontend/src/components/pages/AuthProvidersPage.tsx`** — New component (main page)
3. **`frontend/src/components/pages/AuthProvidersPage.test.tsx`** — New test file (26 tests)
4. **`frontend/src/pages/settings/auth-providers.astro`** — New Astro page at `/_/settings/auth-providers`
5. **`frontend/src/components/Sidebar.tsx`** — Added "Auth Providers" nav item with shield icon
6. **`frontend/src/components/Sidebar.test.tsx`** — Updated item count assertions (4 → 5)

## Test Results

All 516 tests passing (26 new + 490 existing).
