# Superuser / Admin System

**Date**: 2026-03-21
**Task**: Implement superuser management and admin authentication
**Status**: Complete

## Summary

Implemented the superuser/admin system for Zerobase, allowing admin accounts to authenticate via email/password and receive JWT tokens that bypass all access rules.

## Changes Made

### 1. SuperuserService and SuperuserRepository trait (`zerobase-core`)

**File**: `crates/zerobase-core/src/services/superuser_service.rs` (new)

- `SuperuserRepository` trait: `find_by_id`, `find_by_email`, `insert`, `update`, `delete`, `list_all`, `count`
- `SuperuserService<R>`: generic over `SuperuserRepository` for testability
  - `create_superuser` — validates email (lowercased, trimmed), password (min 8 chars), checks duplicates, hashes password, generates tokenKey
  - `authenticate` — email/password verification, returns record with tokenKey for JWT generation
  - `get_superuser`, `list_superusers`, `delete_superuser` — CRUD with sensitive field stripping
  - `has_superusers`, `ensure_initial_superuser` — first-run setup support
- Well-known constants: `SUPERUSERS_COLLECTION_ID` (`pbc_superusers0`), `SUPERUSERS_COLLECTION_NAME` (`_superusers`)
- 14 unit tests with MockSuperuserRepo and TestHasher

**Files modified**:
- `crates/zerobase-core/src/services/mod.rs` — added `pub mod superuser_service` and re-export
- `crates/zerobase-core/src/lib.rs` — added `pub use services::SuperuserService`

### 2. SuperuserRepository for Database (`zerobase-db`)

**File**: `crates/zerobase-db/src/superuser_repo.rs` (new)

- Implements `SuperuserRepository` for `Database`
- Uses `read_conn()` for queries, `with_write_conn()` for mutations
- Handles SQLite value → JSON conversion
- 5 integration tests with in-memory SQLite

**File modified**: `crates/zerobase-db/src/lib.rs` — added `pub mod superuser_repo`

### 3. System migration updates (`zerobase-db`)

**File modified**: `crates/zerobase-db/src/migrations/system.rs`

- `_superusers` table now includes `tokenKey TEXT NOT NULL DEFAULT ''` column
- Added `register_system_collections()` — inserts `_superusers` into `_collections` with `system = 1` so the auth middleware can resolve superuser JWT tokens
- Uses `SUPERUSERS_COLLECTION_ID` constant from core

**File modified**: `crates/zerobase-db/src/schema_repo.rs`

- `list_collections` now filters `WHERE system = 0` to exclude system collections from user-facing listings

### 4. Admin auth endpoint (`zerobase-api`)

**File**: `crates/zerobase-api/src/handlers/admins.rs` (new)

- `POST /_/api/admins/auth-with-password` — superuser email/password login
- Returns `{ token, admin: { id, email, collectionId, collectionName, created, updated } }`
- Auth errors mapped to 400 (matching PocketBase behavior)
- JWT generated with well-known `_superusers` collection ID

**File**: `crates/zerobase-api/src/lib.rs` (modified)

- Added `admin_routes()` builder function
- Exported `AdminAuthState`

**File**: `crates/zerobase-api/src/handlers/mod.rs` (modified) — added `pub mod admins`

### 5. Auth middleware update

**File modified**: `crates/zerobase-api/src/middleware/auth_context.rs`

- Replaced local `SUPERUSERS_COLLECTION` constant with imported `SUPERUSERS_COLLECTION_NAME` from core for consistency

### 6. Integration tests

**File**: `crates/zerobase-api/tests/admin_auth_endpoints.rs` (new)

- 6 tests covering:
  - Successful authentication returns token and admin record
  - Wrong password returns 400
  - Unknown email returns 400
  - Empty identity returns 400
  - Empty password returns 400
  - Email case normalization

## Architecture

```
POST /_/api/admins/auth-with-password
  -> AdminAuthState { SuperuserService, TokenService }
    -> SuperuserService::authenticate(email, password)
      -> SuperuserRepository::find_by_email
      -> PasswordHasher::verify
    -> TokenService::generate(admin_id, SUPERUSERS_COLLECTION_ID, Auth, tokenKey)
  <- { token, admin }
```

Superuser tokens work with the existing auth middleware because:
1. `_superusers` is registered in `_collections` with the well-known `pbc_superusers0` ID
2. Auth middleware resolves JWT `collection_id` → collection name via `SchemaLookup`
3. When `collection.name == "_superusers"`, `AuthInfo::is_superuser` is set to `true`
4. `require_superuser` middleware checks `AuthInfo::is_superuser`

## Test Results

All 1541+ tests pass across the workspace with 0 failures.
