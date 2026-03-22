# Implement Admin Login Page

**Date:** 2026-03-21 13:03
**Task ID:** zuidl5fd90h9mgv

## Summary

Implemented the superuser login page at `/_/login` for the Zerobase admin dashboard. The login flow authenticates via the `POST /_/api/admins/auth-with-password` endpoint, stores the JWT token in localStorage, and redirects to the dashboard on success. The dashboard page (`/_/`) is protected with an AuthGuard that redirects unauthenticated users to the login page.

## What Was Built

### Auth Layer (`src/lib/auth/`)
- **`client.ts`** — Singleton `ZerobaseClient` instance configured for the browser (localStorage token persistence, same-origin API base URL).
- **`AuthContext.tsx`** — React context providing `admin`, `loading`, `login()`, and `logout()` to the component tree.
- **`index.ts`** — Barrel exports for the auth module.

### Components (`src/components/`)
- **`LoginForm.tsx`** — Email/password form with:
  - Client-side validation (required fields)
  - Server-side error display (API errors, field-level validation)
  - Loading state with spinner and disabled inputs
  - Proper accessibility: labels, `aria-invalid`, `aria-describedby`, `autocomplete` attributes
  - Email whitespace trimming
  - Redirect to `/_/` on success
- **`LoginPage.tsx`** — Wraps LoginForm in AuthProvider for use as an Astro React island.
- **`AuthGuard.tsx`** — Redirects to login if unauthenticated, shows loading spinner during auth check.
- **`Dashboard.tsx`** — Protected dashboard with header (showing admin email), sign out button, and content area.

### Pages (`src/pages/`)
- **`login.astro`** — Login page using `LoginPage` component with `client:load`.
- **`index.astro`** — Updated to use the `Dashboard` React island (protected by AuthGuard).

### Tests
- **`LoginForm.test.tsx`** — 12 tests covering:
  - Rendering (fields, labels, heading)
  - Client-side validation (empty fields)
  - Successful login + redirect
  - API error display (general + field-level)
  - Network failure error
  - Loading state (disabled inputs, spinner text)
  - Error clearing on resubmit
  - Autocomplete attributes
  - Email whitespace trimming
- **`AuthContext.test.tsx`** — 5 tests covering:
  - Initial loading state resolution
  - Login sets admin state
  - Login propagates ApiError
  - Logout clears state and calls client.logout
  - useAuth throws outside AuthProvider

**All 101 tests pass (4 test files).**

## Files Modified

- `frontend/src/lib/auth/client.ts` (new)
- `frontend/src/lib/auth/AuthContext.tsx` (new)
- `frontend/src/lib/auth/AuthContext.test.tsx` (new)
- `frontend/src/lib/auth/index.ts` (new)
- `frontend/src/components/LoginForm.tsx` (new)
- `frontend/src/components/LoginForm.test.tsx` (new)
- `frontend/src/components/LoginPage.tsx` (new)
- `frontend/src/components/AuthGuard.tsx` (new)
- `frontend/src/components/Dashboard.tsx` (new)
- `frontend/src/pages/login.astro` (new)
- `frontend/src/pages/index.astro` (modified — now uses Dashboard component)
