# Implement Storage Settings Page

**Date:** 2026-03-21 14:10 UTC
**Task ID:** atdmfgb3698mws1

## Summary

Built a storage settings page in the frontend admin dashboard that allows superadmins to toggle between local filesystem storage and S3-compatible object storage. The implementation follows the exact same patterns as the existing SMTP settings section.

### Features Implemented

- **Storage mode toggle**: Switch between local filesystem and S3 storage via a toggle switch
- **S3 configuration fields**: Bucket (required), Region (required), Endpoint, Access Key, Secret Key, Force Path Style
- **Validation**: Client-side validation ensures bucket and region are provided when S3 is enabled
- **Write-only secret key**: Secret key is never displayed from API reads; left blank to keep existing value
- **Status badge**: Shows "Local storage", "Not configured", or "S3" depending on configuration state
- **Independent save**: Storage settings have their own save button, separate from SMTP settings
- **Error handling**: API errors and network errors handled with user-friendly messages
- **Field error clearing**: Validation errors clear when the user modifies the corresponding field

### Tests Added (20 new tests)

- Default state rendering (heading, status badge, hidden fields)
- Toggle S3 on/off shows/hides fields
- Loading configured S3 settings from API
- Bucket required validation
- Region required validation
- Field error clearing on change
- No validation when S3 disabled
- Successful save
- API error on save
- Network error on save
- Secret key write-only behavior (not sent when blank, sent when typed)
- Force path style toggle
- Correct payload structure verification

**All 490 tests pass across 15 test files.**

## Files Modified

1. `frontend/src/lib/api/types.ts` — Added `S3Settings` interface
2. `frontend/src/components/pages/SettingsPage.tsx` — Added storage settings section with S3 toggle, form fields, validation, save handler, and status badge
3. `frontend/src/components/pages/SettingsPage.test.tsx` — Added 20 new test cases for storage settings, plus S3 test data fixtures
