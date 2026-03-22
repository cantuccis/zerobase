# E2E Tests for Admin Dashboard

**Date:** 2026-03-21
**Task ID:** j36iwg8i7r7gmr0
**Status:** Complete

## Summary

Created a comprehensive Playwright E2E test suite for the Zerobase admin dashboard with 95 test cases across 13 spec files covering all main user flows.

## Architecture

- **Global setup** (`global-setup.ts`): Authenticates as superuser and saves browser storage state to `e2e/.auth/admin.json` for reuse across tests
- **Fixtures** (`fixtures.ts`): Provides `adminPage` (pre-authenticated page) and `api` (ApiHelper) fixtures for programmatic test data setup/teardown
- **Serial execution**: Tests run with `workers: 1` since they share backend state
- **Target**: Tests run against the real backend at `localhost:8090` (not the Astro dev server)

## Test Files (95 tests total)

| File | Tests | Coverage |
|------|-------|----------|
| `login.spec.ts` | 6 | Login form rendering, validation, error handling, successful auth, redirect |
| `navigation.spec.ts` | 11 | All 8 sidebar nav links, header sign-out, active item highlighting |
| `overview.spec.ts` | 4 | Dashboard heading, server health, collection statistics |
| `collections.spec.ts` | 8 | Collection list, search, create (base/auth), name validation, delete with dialog |
| `schema-editor.spec.ts` | 6 | Edit mode, existing fields, add fields, type selector, API preview, rules |
| `records-browser.spec.ts` | 8 | Records table, columns, count, create/edit records, empty state |
| `settings.spec.ts` | 10 | SMTP settings, S3 storage, Meta/Sender, toggle, save, test email, auth providers |
| `logs.spec.ts` | 7 | Log entries, filters, HTTP method/status, date/status range filtering, statistics |
| `backups.spec.ts` | 8 | Create backup, backup list, file size/date, download/delete/restore actions |
| `webhooks.spec.ts` | 6 | Webhook list, create form, URL input, event types, collection selector |
| `api-docs.spec.ts` | 5 | API docs heading, endpoints, collection endpoints, curl examples, filter docs |
| `accessibility.spec.ts` | 11 | ARIA landmarks, labels, aria-current, form validation, keyboard access, focus trap |
| `home.spec.ts` | 5 | Dashboard heading, title, branding, skip-to-content, main landmark |

## Files Modified

- `frontend/playwright.config.ts` — Updated to target real backend, added global setup project
- `frontend/package.json` — Added `test:e2e:ui` and `test:e2e:headed` scripts
- `frontend/.gitignore` — Added `e2e/.auth/` directory
- `frontend/e2e/home.spec.ts` — Updated to use custom fixtures

## Files Created

- `frontend/e2e/global-setup.ts`
- `frontend/e2e/fixtures.ts`
- `frontend/e2e/login.spec.ts`
- `frontend/e2e/navigation.spec.ts`
- `frontend/e2e/overview.spec.ts`
- `frontend/e2e/collections.spec.ts`
- `frontend/e2e/schema-editor.spec.ts`
- `frontend/e2e/records-browser.spec.ts`
- `frontend/e2e/settings.spec.ts`
- `frontend/e2e/logs.spec.ts`
- `frontend/e2e/backups.spec.ts`
- `frontend/e2e/webhooks.spec.ts`
- `frontend/e2e/api-docs.spec.ts`
- `frontend/e2e/accessibility.spec.ts`

## Key Design Decisions

1. **Real backend testing**: Tests target port 8090 where the Rust backend serves the embedded SPA, not the Astro dev server
2. **ApiHelper fixture**: Programmatic test data setup/teardown via API calls avoids flaky UI-based setup
3. **Storage state reuse**: Global setup authenticates once, all test files reuse the saved auth state
4. **Fresh context for login tests**: Login specs use `storageState: { cookies: [], origins: [] }` to test unauthenticated flows
5. **Cleanup in finally blocks**: Tests that create resources always clean up in `finally` blocks or `afterAll`
