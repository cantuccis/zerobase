# SMTP Settings Page Implementation

**Task ID:** g6hufhr3nv9gpit
**Date:** 2026-03-21
**Phase:** 10 - SMTP Settings Page

## Summary

Implemented a full SMTP settings page for the Zerobase admin dashboard, including backend test-email endpoint, frontend settings UI, and comprehensive tests.

## Changes Made

### Backend

- **`crates/zerobase-api/src/handlers/settings.rs`** — Added `test_email` handler, `TestEmailRequest` struct, and `TestEmailState` struct. The endpoint validates the recipient, checks SMTP is enabled, and sends a test email via the `EmailService` trait.
- **`crates/zerobase-api/src/lib.rs`** — Updated `settings_routes` to accept an `Arc<dyn EmailService>` parameter. Split into two sub-routers with different Axum state types (one for CRUD settings, one for test-email) merged together, with the test-email route first to avoid wildcard `{key}` matching.
- **`crates/zerobase-api/tests/settings_endpoints.rs`** — Added `MockEmailService` and `FailingEmailService` test doubles. Added 5 new integration tests: send when enabled, reject when disabled, reject empty recipient, error on send failure, 401 without auth.

### Frontend

- **`frontend/src/lib/api/types.ts`** — Added `SmtpSettings`, `MetaSenderSettings`, `TestEmailRequest`, `TestEmailResponse` interfaces.
- **`frontend/src/lib/api/client.ts`** — Added `testEmail(to)` method to the API client.
- **`frontend/src/components/pages/SettingsPage.tsx`** — Complete rewrite from stub. Features:
  - Loads and saves SMTP + meta sender settings
  - Toggle switches for SMTP enabled and TLS
  - Conditional field rendering (fields only visible when SMTP enabled)
  - Client-side validation (host required, port range, sender address required)
  - Write-only password field handling (placeholder, only sent if non-empty)
  - Connection status badge (Disabled / Not configured / Configured)
  - Send Test Email section with recipient input and error/success feedback
- **`frontend/src/components/pages/SettingsPage.test.tsx`** — 24 tests covering loading state, default state, toggling SMTP, field population, validation errors, save success/failure, password handling, test email send/error, load errors, and TLS toggle.

## Test Results

- **Backend:** 31 tests passed (all settings endpoint tests including 5 new test-email tests)
- **Frontend:** 24 tests passed (all SettingsPage tests)
- **TypeScript:** No new type errors introduced (pre-existing errors in client.test.ts unrelated to this work)
