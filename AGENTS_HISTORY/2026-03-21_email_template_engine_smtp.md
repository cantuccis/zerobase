# Email Template Engine & SMTP Service Enhancement

**Task ID:** b3511nghbusmmfr
**Date:** 2026-03-21
**Status:** Complete

## Summary

Built a centralized email template engine in `zerobase-core` and refactored all four auth services to use it. Enhanced `SmtpEmailService` with STARTTLS support and a connection test method.

## Changes

### New Files

| File | Description |
|------|-------------|
| `crates/zerobase-core/src/email/mod.rs` | Replaced `email.rs` with directory module, adds `pub mod templates` |
| `crates/zerobase-core/src/email/templates.rs` | Type-safe email template engine with 4 template types and 23 unit tests |

### Modified Files

| File | Description |
|------|-------------|
| `crates/zerobase-auth/src/email.rs` | Added STARTTLS support (port 587 auto-detection), `test_connection()` method, 8 unit tests |
| `crates/zerobase-auth/src/verification.rs` | Refactored to use `EmailTemplateEngine` instead of inline templates |
| `crates/zerobase-auth/src/password_reset.rs` | Refactored to use `EmailTemplateEngine` instead of inline templates |
| `crates/zerobase-auth/src/email_change.rs` | Refactored to use `EmailTemplateEngine` instead of inline templates |
| `crates/zerobase-auth/src/otp.rs` | Refactored to use `EmailTemplateEngine` instead of inline templates, updated `extract_otp_from_email` test helper |
| `crates/zerobase-api/src/lib.rs` | Updated route builder functions to accept `EmailTemplateEngine` parameter |

### Deleted Files

| File | Description |
|------|-------------|
| `crates/zerobase-core/src/email.rs` | Replaced by `email/mod.rs` directory module |

## Architecture

### Email Template Engine (`zerobase-core::email::templates`)

- `EmailTemplateEngine` — configured with `app_name`, renders all template types
- `VerificationContext` — verification email data (to, verification_url, expiry_text)
- `PasswordResetContext` — password reset email data (to, reset_url, expiry_text)
- `EmailChangeContext` — email change confirmation data (to, confirm_url, expiry_text)
- `OtpContext` — OTP code email data (to, otp_code, expiry_text)

Each template renders both HTML (styled, with buttons/links) and plain-text bodies.

### SmtpEmailService Enhancements

- **STARTTLS**: When `tls=true` and `port=587`, uses `SmtpTransport::starttls_relay()` for STARTTLS negotiation
- **Implicit TLS**: When `tls=true` and port != 587, uses `SmtpTransport::relay()` (typically port 465)
- **Plaintext**: When `tls=false`, uses `SmtpTransport::builder_dangerous()`
- **Connection testing**: `test_connection()` issues a NOOP command to verify SMTP connectivity

## Tests

- 23 template engine tests (all template types, custom app name, HTML structure)
- 8 SmtpEmailService tests (construction modes, invalid recipient, mock service)
- All 199 auth crate tests pass
- Full workspace test suite passes (0 failures)
