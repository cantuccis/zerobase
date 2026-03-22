# Implement Admin Dashboard API Client

**Date:** 2026-03-21 12:56
**Task ID:** o551t6p9c0x8hyy

## Summary

Created a comprehensive, type-safe TypeScript API client for the Zerobase admin dashboard. The client covers all backend endpoints with full TypeScript type definitions matching the Rust API's serialization format (camelCase via `#[serde(rename_all = "camelCase")]`).

### Key features implemented:

- **Type-safe API client** (`ZerobaseClient` class) with methods for all endpoints
- **Token management** with pluggable `TokenStore` interface (MemoryTokenStore + LocalStorageTokenStore)
- **Structured error handling** via `ApiError` class with convenience getters (isValidation, isUnauthorized, etc.)
- **Complete TypeScript types** matching all Rust response/request types
- **78 unit tests** covering all endpoints, error handling, token management, URL encoding, and content-type handling

### Endpoints covered:

- **Admin auth:** `adminAuthWithPassword`
- **Collection auth:** `authWithPassword`, `authRefresh`, `authMethods`, `authWithOAuth2`
- **OTP:** `requestOtp`, `authWithOtp`
- **MFA:** `requestMfaSetup`, `confirmMfa`, `authWithMfa`
- **Passkeys:** `requestPasskeyRegister`, `confirmPasskeyRegister`, `authWithPasskeyBegin`, `authWithPasskeyFinish`
- **Verification:** `requestVerification`, `confirmVerification`
- **Password reset:** `requestPasswordReset`, `confirmPasswordReset`
- **Email change:** `requestEmailChange`, `confirmEmailChange`
- **External auths:** `listExternalAuths`, `unlinkExternalAuth`
- **Collections:** `listCollections`, `createCollection`, `getCollection`, `updateCollection`, `deleteCollection`, `exportCollections`, `importCollections`, `listIndexes`, `addIndex`, `removeIndex`
- **Records:** `listRecords`, `getRecord`, `createRecord`, `updateRecord`, `deleteRecord`, `countRecords`
- **Settings:** `getSettings`, `updateSettings`, `getSetting`, `resetSetting`
- **Logs:** `listLogs`, `getLogStats`, `getLog`
- **Backups:** `listBackups`, `createBackup`, `downloadBackup`, `deleteBackup`, `restoreBackup`
- **Files:** `getFileToken`, `getFileUrl`
- **Batch:** `batch`
- **Health:** `health`

## Files Modified

- `frontend/src/lib/api/types.ts` — **Created** — All TypeScript types matching Rust API
- `frontend/src/lib/api/client.ts` — **Created** — ZerobaseClient class with all endpoint methods
- `frontend/src/lib/api/index.ts` — **Created** — Barrel export
- `frontend/src/lib/api/client.test.ts` — **Created** — 78 unit tests

## Test Results

- **78 tests passed**, 0 failed
- Duration: 608ms
