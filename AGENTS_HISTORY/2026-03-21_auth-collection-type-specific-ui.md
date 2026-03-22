# Auth Collection Type-Specific UI

**Date:** 2026-03-21 14:56
**Task ID:** qnn7c4g6vask8mu
**Phase:** 10

## Summary

Implemented collection type-specific UI for auth collections in the schema editor. When the collection type is set to 'auth', the editor now shows:

1. **Auth System Fields (read-only)** - Displays the 5 auto-included auth fields (email, emailVisibility, verified, password, tokenKey) with lock icons and descriptions, making it clear they cannot be removed or renamed.

2. **Auth Settings Editor** - A configurable panel for auth-specific settings:
   - Authentication method toggles (Email/Password, OAuth2, OTP, MFA)
   - MFA session duration (conditionally shown when MFA is enabled)
   - Minimum password length with validation (min 1)
   - Email verification requirement toggle
   - Identity fields configuration (comma-separated)

3. **Save payload integration** - The `authOptions` object is included in the API payload only when the collection type is 'auth'.

4. **Dynamic section labels** - The fields section heading changes to "Additional Fields" when type is auth.

## Files Modified

- `frontend/src/components/pages/CollectionEditorPage.tsx` - Integrated AuthFieldsDisplay and AuthSettingsEditor components, added authOptions to form state and save payload
- `frontend/src/components/pages/CollectionEditorPage.test.tsx` - Added 15 integration tests for auth collection UI

## Files Created

- `frontend/src/components/schema/AuthFieldsDisplay.tsx` - Read-only display of auto-included auth system fields
- `frontend/src/components/schema/AuthFieldsDisplay.test.tsx` - 12 unit tests
- `frontend/src/components/schema/AuthSettingsEditor.tsx` - Configurable auth settings editor with DEFAULT_AUTH_OPTIONS
- `frontend/src/components/schema/AuthSettingsEditor.test.tsx` - 22 unit tests

## Test Results

All 658 tests pass (21 test files), including 49 new tests across the 3 test files.
