# Security Audit & Hardening

**Task ID:** `yd62enzi7fq6xpn`
**Date:** 2026-03-21
**Scope:** Full OWASP Top 10 security audit of all Zerobase crates

## Audit Summary

Audited all endpoints and core logic for: SQL injection, XSS, CSRF, path traversal, timing attacks, and token security.

## Findings & Fixes

### Fix 1: `sanitize_table_name` was a no-op (SQL Injection defense-in-depth)
- **File:** `crates/zerobase-db/src/record_repo.rs`
- **Risk:** Low (table names are already quote-wrapped and validated upstream)
- **Fix:** Added assertion-based validation rejecting `"`, `;`, `\0`, `\\`, `\n`, `\r`
- **Tests:** 6 new tests (`sanitize_table_name_rejects_*`, `sanitize_table_name_accepts_valid_names`)

### Fix 2: Content-Disposition header injection
- **File:** `crates/zerobase-api/src/handlers/files.rs`
- **Risk:** Medium — filenames were interpolated unsanitized into HTTP headers
- **Fix:** Added `sanitize_content_disposition_filename()` stripping `"`, `\\`, `\n`, `\r`, `\0`
- **Tests:** 5 new tests (strips quotes, backslashes, newlines/null, preserves normal names, header injection attempt)

### Fix 3: Email template XSS
- **File:** `crates/zerobase-core/src/email/templates.rs`
- **Risk:** Medium — user-controllable values (app name, URLs, OTP codes) were interpolated unescaped into HTML email bodies
- **Fix:** Added `html_escape()` function encoding `&`, `<`, `>`, `"`, `'`; applied to all 4 templates (verification, password_reset, email_change, otp)
- **Tests:** 4 new tests (html_escape unit test, verification/otp/password_reset XSS tests)

### Fix 4: Path traversal backslash hardening
- **File:** `crates/zerobase-files/src/local.rs`
- **Risk:** Low (Linux-only currently, but defense-in-depth for cross-platform)
- **Fix:** Added backslash rejection to `validate_key()` — prevents Windows-style path traversal (`col1\..\..\etc\passwd`)
- **Tests:** 1 new test (`validate_key_rejects_backslashes`)

### Fix 5: Security response headers
- **File:** `crates/zerobase-api/src/middleware/security_headers.rs` (new)
- **Risk:** Low — missing standard security headers
- **Fix:** Added middleware injecting `X-Content-Type-Options: nosniff`, `X-Frame-Options: SAMEORIGIN`, `X-XSS-Protection: 0`, `Referrer-Policy: strict-origin-when-cross-origin`, `Permissions-Policy: interest-cohort=()`
- **Wired into:** Both `api_router_full` and `api_router_with_auth_full` in `crates/zerobase-api/src/lib.rs`
- **Tests:** 1 new test verifying all 5 headers present

## Items Confirmed Secure (No Fix Needed)

| Area | Status | Notes |
|------|--------|-------|
| SQL Injection | **Secure** | All user values bound as parameters via `filter.rs` query builder; table names now validated |
| Password Hashing | **Secure** | Argon2id with OWASP params (19MB memory, 2 iterations); constant-time comparison built into argon2 crate |
| JWT Tokens | **Secure** | HS256 with per-user `tokenKey` invalidation; proper type/expiry validation; `SecretString` for key storage |
| CSRF | **N/A** | Token-based auth via `Authorization` header — not vulnerable to CSRF (HTML forms can't set custom headers) |
| CORS | **Acceptable** | Permissive by design (API-first service, same as PocketBase); safe with token-based auth |
| Path Traversal | **Secure** | `validate_key` rejects `..`, absolute paths, null bytes, backslashes; `resolve_path` double-checks containment |
| FTS5 Injection | **Secure** | `sanitize_search_query` strips special chars, only allows alphanumeric + underscore + apostrophe |
| HTML Sanitization | **Secure** | `ammonia` crate sanitizes editor field HTML; email templates now use `html_escape` |
| Rate Limiting | **Secure** | Per-IP rate limiting via middleware with configurable category limits |
| Body Size Limits | **Secure** | Configurable per-route body size limiting via middleware |
| File Upload | **Secure** | MIME type validation, size limits, random filename generation with special char sanitization |

## Test Results

- **New security tests:** 17 tests added across 4 crates
- **All tests pass** (1 pre-existing flaky smoke test unrelated to changes)
- **Build:** Clean compilation with no new warnings

## Files Modified

1. `crates/zerobase-db/src/record_repo.rs` — `sanitize_table_name` + 6 tests
2. `crates/zerobase-api/src/handlers/files.rs` — `sanitize_content_disposition_filename` + 5 tests
3. `crates/zerobase-core/src/email/templates.rs` — `html_escape` + 4 tests
4. `crates/zerobase-files/src/local.rs` — backslash rejection in `validate_key` + 1 test
5. `crates/zerobase-api/src/middleware/security_headers.rs` — new file, security headers middleware + 1 test
6. `crates/zerobase-api/src/middleware/mod.rs` — added `security_headers` module
7. `crates/zerobase-api/src/lib.rs` — wired security headers into both router builders
