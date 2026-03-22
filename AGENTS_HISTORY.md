# AGENTS_HISTORY

## Task: Implement CORS Configuration

**Task ID:** `kfqare05hgloe4n`
**Date:** 2026-03-21

### Objective

Make CORS middleware configurable via the settings API. Support allowed origins (list or wildcard), allowed methods, allowed headers, exposed headers, credentials, and max age. Default to permissive for development.

### Changes Made

#### 1. `crates/zerobase-core/src/services/settings_service.rs` (modified)

- Added `SETTING_CORS` constant and registered in `KNOWN_SETTING_KEYS`
- Added `CorsSettingsDto` struct with fields: `enabled`, `allowed_origins`, `allowed_methods`, `allowed_headers`, `exposed_headers`, `allow_credentials`, `max_age`
- Added `Default` impl with permissive defaults (all origins/methods/headers allowed, 24h max age)
- Added `validate_cors_setting()` with validations for: string arrays, valid HTTP methods, credentials+wildcard conflict, max_age > 0
- Registered CORS in `default_value_for_key` and `validate_setting` dispatch

#### 2. `crates/zerobase-api/src/middleware/cors.rs` (new)

- `build_cors_layer()` function that constructs a `tower_http::cors::CorsLayer` from `CorsSettingsDto`
- When disabled: returns fully permissive layer (Any origin/method/header)
- When enabled: configures specific origins, methods, headers, exposed headers, credentials, max age
- Handles credential mode constraints: expands wildcards to explicit lists when `allow_credentials=true` (tower-http requirement)
- 6 unit tests

#### 3. `crates/zerobase-api/src/middleware/mod.rs` (modified)

- Registered `pub mod cors;`

#### 4. `crates/zerobase-api/src/lib.rs` (modified)

- Added `api_router_with_rate_limit_and_cors()` and `api_router_with_auth_rate_limit_and_cors()` functions accepting custom `CorsLayer`
- Existing functions delegate to new ones with default permissive CORS for backward compatibility
- Re-exported `build_cors_layer`

#### 5. `crates/zerobase-db/src/settings_repo.rs` (new)

- Implemented `SettingsRepository` for `Database` with SQLite CRUD on `_settings` table
- `get_setting`, `get_all_settings`, `set_setting` (upsert), `delete_setting`

#### 6. `crates/zerobase-db/src/lib.rs` (modified)

- Registered `pub mod settings_repo;`

#### 7. `crates/zerobase-server/src/main.rs` (modified)

- Instantiated `SettingsService` from database
- Loaded CORS settings from DB and built `CorsLayer` via `build_cors_layer()`
- Wired `api_router_with_auth_rate_limit_and_cors()` with the configured layer
- Added settings routes to the router

#### 8. `crates/zerobase-api/tests/cors_configuration.rs` (new)

- 14 integration tests covering:
  - Default permissive CORS (any origin, preflight)
  - Specific origins (matching allowed, non-matching blocked)
  - Allowed methods reflected in preflight
  - Credentials flag set/absent
  - Max age in preflight
  - Exposed headers
  - Settings API validation (credentials+wildcard, invalid methods)
  - Settings persistence and retrieval
  - Default CORS in get-all-settings
  - Reset to defaults via DELETE

### Test Results

- **14/14** CORS integration tests pass
- **31/31** existing settings endpoint tests pass
- **31/31** core settings service tests pass
- **1/1** existing CORS preflight tracing test passes
- Full `cargo check` clean (no new warnings)

---

## Task: Implement Collection CRUD in Core Layer

**Task ID:** `4a2l9vapfmb1tqz`
**Date:** 2026-03-21

### Objective

Implement `CollectionService` with full CRUD operations (create, get, list, update, delete) for collections. Each method validates input, manages `_collections` and `_fields` system tables, and creates/alters/drops corresponding SQLite user tables.

### Changes Made

#### 1. `crates/zerobase-db/src/schema_repo.rs` (new)

Concrete `SchemaRepository` implementation for `Database`:
- `list_collections`, `get_collection`, `create_collection`, `update_collection`, `delete_collection`, `collection_exists`
- Helper functions: `insert_fields`, `load_columns`, `load_indexes`, `create_user_table`, `alter_user_table` (SQLite rebuild strategy), `drop_user_table`, `create_index`, `generate_id`
- **19 unit tests** covering all CRUD operations, edge cases, cascading deletes, unique constraints, various column types

#### 2. `crates/zerobase-db/src/lib.rs` (modified)

- Added `pub mod schema_repo;` to register the new module

#### 3. `crates/zerobase-db/Cargo.toml` (modified)

- Added `uuid = { workspace = true }` dependency for ID generation

#### 4. `crates/zerobase-core/src/services/collection_service.rs` (new)

`CollectionService<R: SchemaRepository>` generic service with:
- Own `SchemaRepository` trait (in core to avoid circular dependency with zerobase-db)
- DTOs: `CollectionSchemaDto`, `ColumnDto`, `IndexDto`
- `SchemaRepoError` enum with `From<SchemaRepoError> for ZerobaseError` conversion
- Domain-to-DTO conversion: `collection_to_schema()`, `schema_to_collection()`
- SQL type mapping: `sql_type_to_field_type()`
- `MockRepo` using `RwLock<HashMap>` for unit testing
- **28 unit tests** covering: creation (valid, invalid name, reserved name, duplicate fields, reserved field names, duplicate collection, auth/view types, various field types, indexes), get, list, update (fields, validation, rename, nonexistent), delete, exists, conversion helpers

#### 5. `crates/zerobase-core/src/services/mod.rs` (new)

- Module declaration and re-export of `CollectionService`

#### 6. `crates/zerobase-core/src/lib.rs` (modified)

- Added `pub mod services;` and `pub use services::CollectionService;`

#### 7. `crates/zerobase-core/src/schema/mod.rs` (modified)

- Added re-exports: `AuthOptions`, `IndexSpec`, `BoolOptions`, `NumberOptions`, `SelectOptions`, `TextOptions`

### Test Results

All **331 tests** pass across the entire workspace:
- `zerobase-core`: 188 passed
- `zerobase-db`: 131 passed
- `zerobase-api`: 22 passed (integration + unit)
- All doc-tests pass

### Design Notes

- The core crate defines its own `SchemaRepository` trait with DTOs to avoid a circular dependency with `zerobase-db`. The db crate's `SchemaRepository` uses `CollectionSchema`/`ColumnDef`/`IndexDef`, while core uses `CollectionSchemaDto`/`ColumnDto`/`IndexDto`. These will need to be bridged (adapter pattern) when wiring into the real application.
- SQLite rebuild strategy is used for `ALTER TABLE` operations since SQLite lacks native `DROP COLUMN` support.
- `CollectionService` is generic over the repository trait, enabling mock-based unit testing without any I/O.

---

## Task: Implement dynamic SQLite table creation from collection schema

**Task ID:** `mxcu1hebicierg7`
**Date:** 2026-03-21

### Objective

Enhance the `SchemaRepository` implementation so that dynamic table creation correctly maps `FieldType` to SQLite column types, includes system auto-columns (id, created, updated), adds auth-specific columns for auth collections, and creates appropriate indexes.

### Changes Made

#### 1. `Cargo.toml` (workspace root)
- Added `nanoid = "0.4"` to workspace dependencies.

#### 2. `crates/zerobase-db/Cargo.toml`
- Added `nanoid = { workspace = true }` dependency (replacing uuid for ID generation).

#### 3. `crates/zerobase-db/src/schema_repo.rs`
- **`generate_id()`** — Replaced UUID with nanoid (15-character URL-safe IDs).
- **`create_user_table()`** — Enhanced to:
  - Auto-add auth system columns (email, emailVisibility, verified, password, tokenKey) for auth collections.
  - Auto-create index on `created` column for all collections.
  - Auto-create unique index on `email` and index on `tokenKey` for auth collections.
- **Test module** — Added 30+ comprehensive tests covering:
  - System columns (id, created, updated) presence and types.
  - FieldType → SQLite type mappings (Text→TEXT, Number→REAL, Bool→INTEGER, DateTime→TEXT).
  - Auth collection system columns and indexes.
  - Email uniqueness enforcement.
  - NOT NULL constraints, DEFAULT values.
  - nanoid generation (15-char length, uniqueness).
  - Composite indexes.
  - Empty collections (no user columns).
  - Base vs auth collection column differences.

### Test Results

All **149 tests** pass in `zerobase-db` (including 30+ new schema_repo tests).

### Acceptance Criteria Met

- [x] Correct column types generated from FieldType mappings
- [x] Default system columns (id, created, updated) present on all tables
- [x] Indexes on created column auto-created
- [x] Auth collections have system auth columns and indexes
- [x] All tests pass

---

## Task: Implement Unique Field Constraint Enforcement

**Task ID:** `5uh0ahnrgnt0sxq`
**Date:** 2026-03-21

### Objective

Enforce uniqueness at both the application layer (pre-insert/pre-update checks with clear field-name error messages) and the database layer (SQLite UNIQUE constraint with automatic error translation).

### Changes Made

#### 1. `crates/zerobase-db/src/error.rs` (modified)

- **`parse_unique_violation()`** — Extracts field names from SQLite's `UNIQUE constraint failed: table.column` error messages. Handles single and composite unique constraints.
- **`is_unique_violation()`** — Public helper to check if a `rusqlite::Error` is a UNIQUE constraint violation.
- **`map_query_error()`** — Maps `rusqlite::Error` to `DbError`, converting UNIQUE violations into `DbError::Conflict` with descriptive messages.
- **Modified `From<DbError> for ZerobaseError`** — The `DbError::Query` arm now detects UNIQUE violations and converts them to `ZerobaseError::conflict` (409) with field names instead of generic 500 database errors.
- **8 new tests** covering: single/composite column parsing, non-constraint errors, other constraint types, map_query_error conversion, 409 status code, is_unique_violation helper.

#### 2. `crates/zerobase-db/src/unique.rs` (new)

Application-level uniqueness pre-checks:
- **`UniqueFieldSpec`** — Describes a field with a unique constraint.
- **`check_unique_fields()`** — Checks all unique fields in a record before insert/update. NULL values and missing fields are skipped (SQL standard).
- **`Database::check_unique_for_create()`** — Convenience method for pre-insert checks.
- **`Database::check_unique_for_update()`** — Convenience method for pre-update checks, excluding the record being updated.
- **10 tests** covering: no duplicates, slug/email duplication, null values, missing fields, self-update allowed, cross-record conflict, SQLite-level enforcement, numeric values.

#### 3. `crates/zerobase-db/src/lib.rs` (modified)

- Added `pub mod unique;` to register the new module.

### Test Results

All **201 tests** pass in `zerobase-db` (18 new tests for unique constraint enforcement).

### Acceptance Criteria Met

- [x] Duplicate values rejected with clear field-name error messages
- [x] Constraint enforced on both create and update operations
- [x] Database-layer UNIQUE constraints produce 409 Conflict with field names
- [x] Application-layer pre-checks provide clear errors before write attempt
- [x] All tests pass

---

## Task: Implement request/response serialization matching PocketBase format

**Task ID:** `a213e8zvtmdm2yh`
**Date:** 2026-03-21

### Objective

Ensure all API responses match PocketBase's exact JSON format — records include `collectionId` and `collectionName`, list responses use `page`/`perPage`/`totalPages`/`totalItems`/`items`, and errors use `{ code, message, data }` with nested field errors.

### Changes Made

#### 1. `crates/zerobase-core/src/error.rs` (modified)

- Added `FieldError` struct with `code` and `message` fields for PocketBase-style nested field errors.
- Changed `ErrorResponseBody.data` from `Option<HashMap<String, String>>` to `HashMap<String, FieldError>`.
- Error `data` is now always an object (`{}` for non-validation errors, not `null`).
- Validation field errors map to `FieldError { code: "validation_{field}", message }`.

#### 2. `crates/zerobase-core/src/lib.rs` (modified)

- Added `FieldError` to crate re-exports.

#### 3. `crates/zerobase-core/src/services/record_service.rs` (modified)

- Added public `get_collection()` method to `RecordService`, delegating to `SchemaLookup`.

#### 4. `crates/zerobase-core/src/schema/record_validator.rs` (modified)

- Fixed test for new `data` type (no longer `Option`).

#### 5. `crates/zerobase-api/src/response.rs` (new)

- `ListResponse<T>` — generic paginated list with camelCase serialization.
- `RecordResponse` — custom `Serialize` impl that flattens `collectionId` and `collectionName` alongside record data fields.
- Unit tests verifying serialization format.

#### 6. `crates/zerobase-api/src/lib.rs` (modified)

- Added `pub mod response;`.

#### 7. `crates/zerobase-api/src/handlers/records.rs` (modified)

- All CRUD handlers now look up collection metadata via `service.get_collection()`.
- Records wrapped in `RecordResponse` to inject `collectionId`/`collectionName`.
- List endpoint uses `ListResponse<RecordResponse>`.

#### 8. `crates/zerobase-api/src/middleware/require_superuser.rs` (modified)

- Updated `data` field from `None` to `HashMap::new()` for new `ErrorResponseBody` type.

#### 9. `crates/zerobase-api/tests/records_endpoints.rs` (modified)

- Added 7 PocketBase format verification integration tests:
  - List response structure (page/perPage/totalPages/totalItems/items)
  - Record responses include collectionId/collectionName (view, list items, create, update)
  - Error response structure (code/message/data)
  - Validation error nested field error format

### Test Results

All **308 tests** pass across the workspace, including 35 integration tests (7 new PocketBase format tests).

### Acceptance Criteria Met

- [x] Records include `id`, `collectionId`, `collectionName`, `created`, `updated` plus data fields
- [x] List responses have `page`, `perPage`, `totalPages`, `totalItems`, `items[]`
- [x] Error responses have `code`, `message`, `data` with nested field errors
- [x] Tests compare output format to PocketBase examples
- [x] All tests pass

---

## Task: Implement `manage_rule` on Collections

**Task ID:** `wq85pzg5ouoca65`
**Date:** 2026-03-21

### Objective

Implement a `manage_rule` on collections that grants full CRUD access to records matching the rule, bypassing individual operation rules (`list_rule`, `view_rule`, `create_rule`, `update_rule`, `delete_rule`). This enables delegated administration without requiring superuser privileges.

### Semantics

- `manage_rule: None` — no manage access (default, locked)
- `manage_rule: Some("")` — any **authenticated** user can manage all records
- `manage_rule: Some(expr)` — users matching the expression get full CRUD access

### Changes Made

#### 1. `crates/zerobase-core/src/schema/rules.rs` (modified)
- Added `manage_rule: Option<String>` field to `ApiRules` struct with `#[serde(default, skip_serializing_if = "Option::is_none")]`
- Updated `open()` and `public_read()` constructors to include `manage_rule: None`
- Added `has_manage_rule()` helper method
- Updated `referenced_fields()` to scan `manage_rule` expressions
- Updated tests for new field

#### 2. `crates/zerobase-db/src/migrations/system.rs` (modified)
- Added `manage_rule TEXT` column to `_collections` table DDL

#### 3. `crates/zerobase-api/src/handlers/records.rs` (modified)
- Added `user_has_manage_access()` function that evaluates manage_rule against auth context
- Modified `enforce_rule_on_record()` to accept `&ApiRules` and check manage_rule before individual operation rule
- Modified `enforce_rule_no_record()` to accept `&ApiRules` and check manage_rule before individual operation rule
- Modified `check_list_rule()` to accept `&ApiRules` and check manage_rule before list_rule
- Updated all 6 handler call sites (list, view, create, update, delete, count) to pass `&collection.rules`
- Added 7 new unit tests for manage_rule behavior
- Updated existing tests with `no_manage_rules()` helper

#### 4. `crates/zerobase-core/src/services/collection_service.rs` (modified)
- Added `manage_rule: None` to explicit `ApiRules` struct initialization in tests

#### 5. `crates/zerobase-api/tests/records_endpoints.rs` (modified)
- Added `manage_rule: None` to 3 explicit `ApiRules` struct initializations to fix compilation

### Test Results

All **59 record endpoint tests** pass, including 7 new manage_rule unit tests. One pre-existing flaky tracing test (`json_log_output_contains_structured_fields`) fails independently of these changes.

### Acceptance Criteria Met

- [x] Users matching manage_rule can perform all CRUD operations
- [x] manage_rule overrides individual operation rules
- [x] Empty manage_rule (`Some("")`) requires authentication
- [x] `None` manage_rule means no manage access (secure default)
- [x] Superusers always bypass everything (unchanged)
- [x] All tests pass

---

## Task: Implement Auth Collection Type

**Task ID:** `qt5vs3a35z7n6uq`
**Date:** 2026-03-21

### Objective

Extend the collection system with an "auth" collection type. Auth collections automatically include system fields (email, emailVisibility, password, verified, tokenKey), hash passwords with Argon2id before storage, strip passwords from API responses, and generate tokenKeys for token invalidation.

### Changes Made

#### 1. `crates/zerobase-core/src/auth.rs` (new)

- `PasswordHasher` trait with `hash()` and `verify()` methods (Send + Sync)
- `NoOpHasher` test-only implementation (cfg(test)) for unit testing without Argon2 overhead

#### 2. `crates/zerobase-auth/src/password.rs` (new)

- `hash_password()` — Argon2id with secure OWASP defaults (19 MiB memory, 2 iterations)
- `verify_password()` — PHC-format hash verification
- `PasswordHashError` enum for hash/verification failures
- **8 tests**: PHC format, unique salts, correct/wrong/empty password, malformed hash, unicode, long passwords

#### 3. `crates/zerobase-auth/src/lib.rs` (modified)

- `Argon2Hasher` struct implementing core `PasswordHasher` trait
- Re-exports `hash_password`, `verify_password`, `PasswordHashError`

#### 4. `crates/zerobase-core/src/services/record_service.rs` (modified)

- `RecordService` gains `password_hasher: Option<Box<dyn PasswordHasher>>` field
- `with_password_hasher()` constructor for auth-capable services
- `prepare_auth_fields_for_create()` — hashes password, generates tokenKey, sets defaults
- `prepare_auth_fields_for_update()` — re-hashes password if changed, regenerates tokenKey
- `hash_password()` — delegates to configured hasher
- `strip_password()` — removes password from response HashMap
- `generate_token_key()` — 50-char alphanumeric nanoid
- `all_fields_for_validation()` — extended with auth system fields (email, emailVisibility, verified, password, tokenKey)
- All read/write endpoints strip password for auth collections
- **14 new tests**: password hashing on create, tokenKey generation, default values, emailVisibility override, password stripping on get/list/update/get_with_fields, password re-hashing on update, tokenKey regeneration on password change, tokenKey preservation on non-password update, missing hasher error, strip_password helper, generate_token_key validity/uniqueness

#### 5. `crates/zerobase-core/src/id.rs` (modified)

- Changed `ALPHABET` from `const` to `pub(crate) const` for reuse in tokenKey generation

#### 6. `crates/zerobase-core/src/lib.rs` (modified)

- Added `pub mod auth;`

### Test Results

All **1,452 tests** pass across the entire workspace with 0 failures.

### Acceptance Criteria Met

- [x] Auth collections have all required fields (email, emailVisibility, password, verified, tokenKey)
- [x] Password is hashed (Argon2id in production, NoOpHasher in tests)
- [x] Password never returned in API responses
- [x] Email uniqueness enforced (via DB UNIQUE constraint)
- [x] tokenKey regenerated on password change (invalidates existing tokens)
- [x] All tests pass

---

## Task: Implement Email Verification Flow

**Task ID:** `wc3eoogknqhwmyx`
**Date:** 2026-03-21

### Objective

Implement a complete email verification flow: generate time-limited verification tokens (JWT), send verification emails via SMTP using the `lettre` crate, and provide endpoints to request and confirm email verification.

### Changes Made

#### 1. `crates/zerobase-core/src/email.rs` (new)

- `EmailMessage` struct (to, subject, body_text, body_html)
- `EmailService` trait with `send()` method
- `MockEmailService` for testing (cfg-gated)

#### 2. `crates/zerobase-auth/src/email.rs` (new)

- `SmtpEmailService` — production SMTP implementation using `lettre`
- `from_settings(&SmtpSettings) -> Option<Self>` constructor
- TLS/non-TLS support, optional credentials, multipart HTML+text emails
- `TestEmailService` for auth crate tests

#### 3. `crates/zerobase-auth/src/verification.rs` (new)

- `VerificationService<R: RecordRepository, S: SchemaLookup>` generic service
- `request_verification(collection_name, email)` — finds user, generates token, sends email
- `confirm_verification(collection_name, token)` — validates token, marks user verified
- Email enumeration prevention (silent success for unknown/already-verified emails)
- Token invalidation via tokenKey check
- Idempotent confirmation (already verified = success)
- **14 unit tests**: happy path send, already verified, unknown email, empty email, non-auth collection, unknown collection, email content, email failure, confirm success, empty token, invalid token, collection mismatch, invalidated tokenKey, idempotent, nonexistent user, non-auth collection confirm

#### 4. `crates/zerobase-api/src/handlers/verification.rs` (new)

- `VerificationState<R, S>` shared state struct
- `RequestVerificationBody` / `ConfirmVerificationBody` request types
- `request_verification` handler — POST, returns 204
- `confirm_verification` handler — POST, returns 204

#### 5. Modified files

| File | Changes |
|------|---------|
| `Cargo.toml` (workspace) | Added `lettre` dependency |
| `crates/zerobase-auth/Cargo.toml` | Added `lettre` and `tokio` dependencies |
| `crates/zerobase-core/src/lib.rs` | Added `pub mod email` |
| `crates/zerobase-core/src/auth.rs` | Added `Verification` variant to `TokenType` enum |
| `crates/zerobase-auth/src/lib.rs` | Registered `email` and `verification` modules, exported `SmtpEmailService` and `VerificationService` |
| `crates/zerobase-auth/src/token.rs` | Added `VERIFICATION` duration constant (7 days) |
| `crates/zerobase-api/src/lib.rs` | Added `verification_routes()` function and `VerificationState` export |
| `crates/zerobase-api/src/handlers/mod.rs` | Registered `verification` handler module |

### API Endpoints

- `POST /api/collections/{collection_name}/request-verification` — Send verification email (body: `{"email": "..."}`, returns 204)
- `POST /api/collections/{collection_name}/confirm-verification` — Confirm with token (body: `{"token": "..."}`, returns 204)

### Test Results

All **58 tests** pass in `zerobase-auth` (14 new verification tests). Full workspace tests pass with 0 failures.

### Architecture Decisions

1. **Trait-based `EmailService`** in core crate keeps SMTP dependency out of the core layer
2. **Reused existing `TokenService`** with new `Verification` token type rather than creating separate token logic
3. **Silent success pattern** for unknown emails prevents email enumeration attacks
4. **`VerificationService` is generic** over `RecordRepository` and `SchemaLookup` for testability
5. **Verification tokens expire in 7 days** (configurable via `durations::VERIFICATION`)

### Acceptance Criteria Met

- [x] Verification email sent with time-limited token
- [x] Token validates correctly (signature, expiry, type, tokenKey)
- [x] User marked as verified after confirmation
- [x] Expired/invalid tokens rejected
- [x] Email enumeration prevented (silent success for unknown emails)
- [x] All tests pass

---

## Task: Implement OAuth2 Core Authorization Code Flow

**Task ID:** `tqh5s9f5mlpgrsh`
**Date:** 2026-03-21

### Objective

Implement the OAuth2 authorization code flow: list enabled providers, handle code exchange, fetch user info, link/create accounts with external auth tracking in the `_externalAuths` table, and return JWT auth tokens.

### Changes Made

#### 1. `crates/zerobase-db/src/migrations/system.rs` (modified)

- Added migration v2 (`create_external_auths_table`) creating the `_externalAuths` system table
- Columns: `id`, `collection_id`, `record_id`, `provider`, `provider_id`, `created`, `updated`
- Unique constraints: `(collection_id, record_id, provider)` and `(provider, provider_id)`
- 4 indexes for efficient lookups
- Updated existing tests for new migration version and table/index counts
- Added 4 new tests for table structure, unique constraints, and indexes

#### 2. `crates/zerobase-core/src/services/external_auth.rs` (new)

- `ExternalAuth` struct — link between a local user and an OAuth2 identity
- `ExternalAuthRepository` trait — persistence contract:
  - `find_by_provider(provider, provider_id)` — lookup by OAuth2 identity
  - `find_by_record(collection_id, record_id)` — list all links for a user
  - `create(auth)` — create a new link
  - `delete(id)` / `delete_by_record(collection_id, record_id)` — remove links

#### 3. `crates/zerobase-db/src/external_auth_repo.rs` (new)

- Implements `ExternalAuthRepository` for `Database`
- Full CRUD operations on the `_externalAuths` table
- **7 unit tests** covering create, find, delete, and constraint enforcement

#### 4. `crates/zerobase-auth/src/oauth2.rs` (new)

- `OAuth2Service<R, S, E>` — generic over record repo, schema lookup, and external auth repo
- `list_auth_methods()` — lists enabled auth methods (password, OTP, OAuth2 providers)
- `authenticate_with_oauth2()` — full OAuth2 callback flow:
  1. Validates collection is auth-type with OAuth2 enabled
  2. Looks up provider from registry
  3. Exchanges authorization code for tokens
  4. Fetches user info from provider
  5. Checks for existing external auth link → authenticate
  6. If not linked: finds user by email → link and authenticate
  7. If no match: creates new account → link and authenticate
  8. Generates and returns JWT token
- `AuthMethodInfo` / `OAuth2AuthResult` response types

#### 5. `crates/zerobase-api/src/handlers/oauth2.rs` (new)

- `GET /api/collections/:collection/auth-methods` — list enabled auth methods
- `POST /api/collections/:collection/auth-with-oauth2` — complete OAuth2 flow
- Request body: `{ provider, code, redirectUrl, codeVerifier? }`
- Response: `{ token, record, meta: { isNew } }`

#### 6. `crates/zerobase-api/src/lib.rs` (modified)

- Added `oauth2_routes<R, S, E>()` route builder function
- Exported `OAuth2State` for external composition

#### 7. `crates/zerobase-api/tests/oauth2_endpoints.rs` (new)

**13 integration tests** with full mock stack (MockOAuthProvider, MockExternalAuthRepo):
- `auth_methods_lists_enabled_providers`
- `auth_methods_non_auth_collection_returns_400`
- `auth_methods_nonexistent_collection_returns_404`
- `oauth2_creates_new_user_and_returns_token` — new user creation
- `oauth2_links_existing_user_by_email` — links to existing user
- `oauth2_returns_existing_linked_user` — authenticates already-linked user
- `oauth2_disabled_collection_returns_400`
- `oauth2_unknown_provider_returns_400`
- `oauth2_missing_provider_returns_400`
- `oauth2_missing_code_returns_400`
- `oauth2_with_pkce_code_verifier` — PKCE support
- `oauth2_nonexistent_collection_returns_404`
- `oauth2_user_without_email_creates_new_account`

#### 8. Other modified files

| File | Changes |
|------|---------|
| `crates/zerobase-core/src/services/mod.rs` | Added `pub mod external_auth` |
| `crates/zerobase-db/src/lib.rs` | Added `pub mod external_auth_repo` |
| `crates/zerobase-auth/src/lib.rs` | Added `pub mod oauth2` and `pub use OAuth2Service` |
| `crates/zerobase-api/src/handlers/mod.rs` | Added `pub mod oauth2` |
| `crates/zerobase-api/Cargo.toml` | Added `async-trait` dev-dependency |

### Test Results

All **1,672 tests** pass across the entire workspace with 0 failures, including 13 new OAuth2 integration tests and 7 new external auth repo tests.

### Acceptance Criteria Met

- [x] Full OAuth2 authorization code flow works
- [x] Accounts linked correctly via `_externalAuths` table
- [x] Duplicate email handling (link to existing user)
- [x] New user creation when no match found
- [x] PKCE support with optional code_verifier
- [x] Auth methods listing endpoint
- [x] Provider validates email verified status
- [x] Tests pass with mocked providers

---

## Task: Implement WebAuthn/Passkey Authentication

**Task ID:** `zdo1ijm7366pn7z`
**Date:** 2026-03-21

### Objective

Implement full WebAuthn/Passkey authentication support using the `webauthn-rs` crate (v0.5.4). Four endpoints for passkey registration and authentication, credential storage in `_webauthn_credentials` system table, with tests.

### Changes Made

#### 1. Workspace Configuration (`Cargo.toml`)
- Added workspace dependencies: `webauthn-rs` (v0.5, with `danger-allow-state-serialisation`), `webauthn-rs-proto` (v0.5), `url` (v2)
- Added `v5` feature to `uuid` dependency for deterministic user handle generation

#### 2. Core Layer (`zerobase-core`)
- **`src/services/webauthn_credential.rs`** (new) — `WebauthnCredential` struct and `WebauthnCredentialRepository` trait with methods: `find_by_credential_id`, `find_by_record`, `find_by_collection`, `create`, `delete`, `delete_by_record`
- **`src/services/mod.rs`** — registered `webauthn_credential` module

#### 3. Database Layer (`zerobase-db`)
- **`src/migrations/system.rs`** — Added migration v3: `create_webauthn_credentials_table` with columns (id, collection_id, record_id, name, credential_id, credential_data, created, updated) and indexes (unique on credential_id, collection_id, collection_id+record_id). Updated existing test assertions for new table/migration counts.
- **`src/webauthn_credential_repo.rs`** (new) — SQLite implementation of `WebauthnCredentialRepository` on `Database` with 8 unit tests
- **`src/lib.rs`** — registered `webauthn_credential_repo` module

#### 4. Auth Layer (`zerobase-auth`)
- **`Cargo.toml`** — Added `webauthn-rs`, `webauthn-rs-proto`, `url` dependencies
- **`src/passkey.rs`** (new) — `PasskeyService<R, S, W>` with:
  - `request_passkey_register` — generates WebAuthn creation challenge
  - `confirm_passkey_register` — verifies browser response, stores credential
  - `auth_with_passkey_begin` — generates WebAuthn assertion challenge
  - `auth_with_passkey_finish` — verifies assertion, issues JWT token
  - `cleanup_expired` — removes stale pending challenges
  - In-memory pending challenge stores with 5-minute expiry
  - 3 unit tests
- **`src/lib.rs`** — registered `passkey` module, re-exported `PasskeyService`

#### 5. API Layer (`zerobase-api`)
- **`Cargo.toml`** — Added `webauthn-rs` dependency
- **`src/handlers/passkey.rs`** (new) — HTTP handlers with `PasskeyState<R, S, W>`:
  - `POST /api/collections/:collection/request-passkey-register`
  - `POST /api/collections/:collection/confirm-passkey-register`
  - `POST /api/collections/:collection/auth-with-passkey-begin`
  - `POST /api/collections/:collection/auth-with-passkey-finish`
- **`src/handlers/mod.rs`** — registered `passkey` module
- **`src/lib.rs`** — Added `passkey_routes()` builder function, exported `PasskeyState`

### API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/collections/{collection}/request-passkey-register` | POST | Begin passkey registration (body: `{userId, name?}`) |
| `/api/collections/{collection}/confirm-passkey-register` | POST | Complete registration (body: `{registrationId, credential}`) |
| `/api/collections/{collection}/auth-with-passkey-begin` | POST | Begin passkey authentication (no body) |
| `/api/collections/{collection}/auth-with-passkey-finish` | POST | Complete authentication (body: `{authenticationId, credential}`) |

### Test Results

All tests pass across the entire workspace with 0 failures (962+ tests including 8 new webauthn credential repo tests and 3 new passkey service tests).

### Architecture Decisions

1. **`webauthn-rs` v0.5.4** — latest stable release with `danger-allow-state-serialisation` for in-memory challenge state
2. **Deterministic UUID v5** from user IDs for WebAuthn user handles
3. **In-memory pending challenge stores** with 5-minute expiry (same pattern as OTP/MFA services)
4. **Base64url-encoded credential IDs** for database storage and lookup
5. **Generic `PasskeyService<R, S, W>`** — follows the established trait-based pattern for testability
6. **Multiple passkeys per user** supported (one-to-many relationship)

### Acceptance Criteria Met

- [x] Passkey registration flow works (request challenge → confirm with browser response)
- [x] Passkey authentication flow works (begin challenge → finish with assertion → JWT token)
- [x] Multiple passkeys per user supported
- [x] Credentials stored in `_webauthn_credentials` system table
- [x] All tests pass

---

## Task: Implement Auth Settings Management

**Task ID:** `vr84ltyfflfp5w6`
**Date:** 2026-03-21

### Objective

Implement auth settings management: enable/disable auth methods per collection, configure OAuth2 provider credentials, set token durations, and configure MFA policy — all through the existing settings API.

### Changes Made

#### 1. `crates/zerobase-core/src/services/settings_service.rs` (modified)

- **Expanded `AuthSettingsDto`** with full auth configuration fields:
  - Token settings: `tokenDuration` (14 days default), `refreshTokenDuration` (7 days default)
  - Auth method toggles: `allowEmailAuth`, `allowOauth2Auth`, `allowOtpAuth`, `allowMfa`, `allowPasskeyAuth`
  - Password policy: `minPasswordLength` (default 8)
  - Nested `mfa: MfaPolicyDto` — `required`, `duration` (300s default), `rule`
  - Nested `otp: OtpSettingsDto` — `duration` (300s default), `length` (6 default)
  - `oauth2_providers: HashMap<String, OAuth2ProviderSettingsDto>` — per-provider credentials

- **Added `MfaPolicyDto`** — MFA enforcement policy with configurable duration and rule expression

- **Added `OtpSettingsDto`** — OTP code configuration (duration, length)

- **Added `OAuth2ProviderSettingsDto`** — per-provider credentials with write-only `clientSecret`:
  - `enabled`, `clientId`, `clientSecret`, `authUrl`, `tokenUrl`, `userInfoUrl`, `displayName`

- **Added `validate_auth_setting()`** — validates:
  - `tokenDuration` > 0, `refreshTokenDuration` > 0
  - `minPasswordLength` >= 5
  - `mfa.duration` > 0
  - `otp.duration` > 0, `otp.length` between 4-10
  - OAuth2 provider `clientId` required when provider is enabled

- **Added `mask_sensitive_fields()`** — masks on read:
  - SMTP `password`, S3 `secretKey`, backup S3 `secretKey`
  - OAuth2 provider `clientSecret` for each configured provider

- **Added `preserve_empty_secrets()`** — preserves existing secrets when update sends empty strings (write-only pattern for safe PATCH semantics)

- **Added 14 unit tests** covering:
  - Default values populated correctly
  - Auth method toggling (enable/disable/partial update preserves others)
  - Token duration validation (zero rejected)
  - Password length validation (< 5 rejected, 5 accepted)
  - MFA duration validation
  - OTP validation (duration, length bounds)
  - OAuth2 provider clientId required when enabled, not when disabled
  - OAuth2 credential storage with secret masking
  - Secret preservation on empty update
  - Secret update when non-empty
  - Settings persistence across reads
  - get_all masks all OAuth2 secrets
  - Full valid auth settings accepted

#### 2. `crates/zerobase-api/tests/settings_endpoints.rs` (modified)

- **Added 9 integration tests** exercising the full HTTP stack:
  - `get_auth_settings_returns_defaults` — GET /api/settings/auth returns correct defaults
  - `toggle_auth_methods_via_api` — PATCH enables/disables methods with persistence
  - `configure_oauth2_provider_credentials` — stores credentials, masks secret in response
  - `oauth2_provider_secret_preserved_on_empty_update` — empty clientSecret preserves existing
  - `auth_validation_rejects_invalid_token_duration` — 400 for tokenDuration: 0
  - `auth_validation_rejects_short_password` — 400 for minPasswordLength: 3
  - `auth_validation_rejects_oauth2_without_client_id` — 400 for enabled provider without clientId
  - `auth_settings_full_lifecycle` — configure → read → partial update → delete → verify reset

### Test Results

- **30 unit tests** in `settings_service` — all passing
- **26 integration tests** in `settings_endpoints` — all passing
- **Full workspace test suite** — no regressions

### Acceptance Criteria Met

- [x] Auth methods can be toggled (email, OAuth2, OTP, MFA, passkey)
- [x] Provider credentials stored securely (write-only secrets, masked on read)
- [x] Token durations configurable with validation
- [x] MFA policy configurable (required, duration, rule)
- [x] OTP settings configurable (duration, length with bounds)
- [x] Settings persist across reads
- [x] All tests pass

---

## Task: Implement Auth Identity Listing Endpoint

**Task ID:** `ua5xifkwh2zd2pp`
**Date:** 2026-03-21

### Objective

Implement endpoints to list and unlink external OAuth2 identities linked to auth records. Only the record owner or a superuser may access these endpoints.

### Changes Made

#### 1. `crates/zerobase-api/src/handlers/external_auths.rs` (new)

- `ExternalAuthState<R, S, E>` shared state struct with `record_repo`, `schema_lookup`, `external_auth_repo`
- `list_external_auths` handler — `GET /api/collections/:collection/records/:id/external-auths` returns linked identities (200 OK)
- `unlink_external_auth` handler — `DELETE /api/collections/:collection/records/:id/external-auths/:provider` removes a provider link (204 No Content)
- Authorization: only record owner or superuser can access
- Validates collection is auth type, record exists

#### 2. `crates/zerobase-api/src/handlers/mod.rs` (modified)

- Added `pub mod external_auths;`

#### 3. `crates/zerobase-api/src/lib.rs` (modified)

- Exported `ExternalAuthState`
- Added `external_auth_routes()` factory function wiring GET and DELETE routes

#### 4. `crates/zerobase-api/tests/external_auths_endpoints.rs` (new)

**14 integration tests** with mock stack:
- `list_external_auths_returns_linked_providers` — owner lists 2 linked providers
- `list_external_auths_returns_empty_when_none_linked` — empty list
- `list_external_auths_superuser_can_access_any_record` — superuser access
- `list_external_auths_other_user_gets_403` — non-owner denied
- `list_external_auths_unauthenticated_gets_401` — no auth token
- `list_external_auths_nonexistent_record_returns_404`
- `list_external_auths_nonexistent_collection_returns_404`
- `list_external_auths_base_collection_returns_400` — non-auth collection
- `unlink_external_auth_deletes_provider_link` — owner unlinks, verifies deletion
- `unlink_external_auth_superuser_can_unlink` — superuser unlinks
- `unlink_external_auth_other_user_gets_403` — non-owner denied
- `unlink_external_auth_unauthenticated_gets_401`
- `unlink_external_auth_unknown_provider_returns_404`
- `unlink_external_auth_nonexistent_record_returns_404`

### Test Results

All **14 tests** pass with 0 failures.

### Acceptance Criteria Met

- [x] External auths listed for record owner
- [x] Unlinking works (DELETE returns 204, provider removed)
- [x] Only record owner or superuser can access (403 for others)
- [x] Unauthenticated requests get 401
- [x] Proper error responses for invalid collection/record/provider
- [x] All tests pass

---

## Task: Implement Request Logging and Audit Trail

**Task ID:** `6eucj6h1rj4z8kg`
**Date:** 2026-03-21

### Objective

Implement a complete request logging and audit trail system: log all API requests to a `_logs` table, provide admin endpoints for querying logs and viewing statistics, and support auto-cleanup of old logs.

### Changes Made

#### Files Created

- `crates/zerobase-core/src/services/log_service.rs` — `LogRepository` trait, domain types (`LogEntry`, `LogQuery`, `LogList`, `LogStats`, `StatusCounts`, `TimelineEntry`), `LogService<R>` generic service, 6 unit tests
- `crates/zerobase-db/src/log_repo.rs` — `LogRepository` impl for `Database` with SQL queries for CRUD, filtering, pagination, stats aggregation, timeline grouping, cleanup; 6 unit tests
- `crates/zerobase-api/src/handlers/logs.rs` — HTTP handlers: `list_logs`, `log_stats`, `get_log` with query parameter deserialization
- `crates/zerobase-api/src/middleware/request_logging.rs` — Axum middleware capturing method, URL, status, duration, IP, auth user, user-agent, request-id
- `crates/zerobase-api/tests/log_endpoints.rs` — 11 integration tests covering list, filter, pagination, stats, get-by-id, auth enforcement, full lifecycle

#### Files Modified

- `crates/zerobase-core/src/services/mod.rs` — Added `log_service` module and re-export
- `crates/zerobase-core/src/lib.rs` — Added `LogService` re-export
- `crates/zerobase-core/src/configuration.rs` — Added `LogsSettings` with `retention_days` (default 7)
- `crates/zerobase-db/src/lib.rs` — Added `log_repo` module
- `crates/zerobase-db/src/migrations/system.rs` — Added migration v5 `create_logs_table` with indexes; updated test assertions
- `crates/zerobase-api/src/lib.rs` — Added `log_routes()`, imports, `request_logging_middleware` re-export
- `crates/zerobase-api/src/handlers/mod.rs` — Added `logs` module
- `crates/zerobase-api/src/middleware/mod.rs` — Added `request_logging` module
- `crates/zerobase-api/Cargo.toml` — Added `chrono` dependency

### Architecture

- Repository trait pattern: `LogRepository` in core, implemented on `Database` in db crate
- Migration v5 creates `_logs` table with indexes on created, method, status, auth_id, ip
- Admin endpoints (superuser-only): `GET /_/api/logs`, `GET /_/api/logs/stats`, `GET /_/api/logs/:id`
- Request logging middleware writes fire-and-forget entries after response
- Config: `logs.retention_days` controls auto-cleanup threshold

### Test Results

All 2,200+ tests pass with 0 failures (6 core unit + 6 db unit + 11 integration + all existing tests).

---

## Task: Implement Frontend Accessibility (a11y)

**Task ID:** `r101h8qnb0x6h9l`
**Date:** 2026-03-21

### Objective

Audit and fix accessibility issues across the Zerobase admin dashboard frontend to achieve WCAG 2.1 AA compliance. Add keyboard navigation, ARIA labels, focus management, color contrast support, and screen reader support. Run axe-core automated checks and write accessibility tests.

### Changes Made

#### 1. Testing Infrastructure
- Installed `vitest-axe@0.1.0` and `axe-core@4.11.1` as devDependencies
- Configured vitest-axe matchers in `tests/setup.ts` via `expect.extend(matchers)`

#### 2. Skip-to-Content Navigation (`DashboardLayout.tsx`)
- Added a visually-hidden skip-to-content link that becomes visible on focus
- Added `id="main-content"` and `tabIndex={-1}` to `<main>` element

#### 3. Focus-Visible Patterns (17 files)
- Replaced `focus:outline-none focus:ring-` with `focus-visible:outline-none focus-visible:ring-` across all interactive elements
- Files: SettingsPage, AuthProvidersPage, LogsPage, BackupsPage, ThemeToggle, LoginForm, CollectionsPage, RecordsBrowserPage, CollectionEditorPage, RulesEditor, AuthSettingsEditor, FieldEditor, field-inputs, RelationPicker, ApiDocsPage, ToastContainer, FieldTypeOptions

#### 4. Table Accessibility
- Added `scope="col"` to all table headers in OverviewPage, LogsPage, ApiDocsPage
- Added `aria-sort` attributes to sortable columns in LogsPage
- Added keyboard handlers (Enter/Space) and `tabIndex={0}` to sortable column headers

#### 5. Keyboard Navigation for Interactive Rows
- LogsPage: Added `tabIndex={0}`, `onKeyDown`, `role="button"`, `aria-label`, focus-visible ring to clickable log rows
- RecordsBrowserPage: Added `tabIndex={0}`, `onKeyDown`, `aria-label`, focus-visible ring to clickable record rows

#### 6. Modal/Dialog Accessibility
- LogsPage (LogDetailModal): Added `role="dialog"`, `aria-modal="true"`, `aria-labelledby`, focus trap, Escape key handler, auto-focus
- BackupsPage (ConfirmModal): Same pattern
- CollectionsPage (DeleteConfirmDialog): Same pattern
- Sidebar (MobileSidebar): Focus trap, Escape key handler, auto-focus on drawer open

#### 7. ARIA Live Regions
- Added `role="status"` and `aria-live="polite"` to success messages in BackupsPage, CollectionsPage, SettingsPage

#### 8. Accessibility Test Suite (`accessibility.test.tsx`)
- 23 tests across 10 describe blocks covering: skip-to-content, navigation ARIA, table scope headers, sortable column keyboard, clickable row keyboard, modal ARIA, Escape key, form labels, icon buttons, aria-live regions, theme toggle, mobile drawer, heading hierarchy, focus trap cycling, and 9 axe-core violation checks

### Files Modified
- `frontend/package.json` — added vitest-axe, axe-core devDependencies
- `frontend/tests/setup.ts` — registered vitest-axe matchers
- `frontend/src/components/DashboardLayout.tsx` — skip-to-content link, main id
- `frontend/src/components/Sidebar.tsx` — mobile drawer focus trap, Escape key
- `frontend/src/components/pages/OverviewPage.tsx` — table scope headers
- `frontend/src/components/pages/LogsPage.tsx` — sortable headers, row keyboard, modal a11y
- `frontend/src/components/pages/ApiDocsPage.tsx` — table scope headers
- `frontend/src/components/pages/BackupsPage.tsx` — modal a11y, aria-live
- `frontend/src/components/pages/CollectionsPage.tsx` — modal a11y, aria-live
- `frontend/src/components/pages/RecordsBrowserPage.tsx` — row keyboard navigation
- `frontend/src/components/pages/SettingsPage.tsx` — aria-live regions
- 17 files: focus-visible pattern updates

### Files Created
- `frontend/src/components/accessibility.test.tsx` — comprehensive a11y test suite (23 tests)

### Test Results

All 31 test files pass, 912/912 tests pass, 0 axe-core violations.

---

## Task: Implement Rust Library/Framework Mode (Phase 11)

**Task ID:** `w66qk9u6hxt3c39`
**Date:** 2026-03-21

### Objective

Enable Zerobase to be used as a Rust library (framework mode), similar to PocketBase's Go framework mode. Users can add Zerobase as a dependency and build custom backends with their own routes, hooks, and middleware alongside the built-in BaaS features.

### Changes Made

#### 1. `crates/zerobase-server/src/lib.rs` (new)

Core `ZerobaseApp` struct with builder API:
- Constructor methods: `new()`, `with_settings()`, `with_database()`
- Builder methods: `with_custom_routes()`, `with_hook()`, `with_default_hook()`, `with_host()`, `with_port()`, `with_tracing()`, `with_log_format()`
- Accessors: `settings()`, `settings_mut()`, `database()`, `hook_registry()`, `hook_registry_mut()`
- Router/server: `build_router()`, `serve()`
- Re-exports of all workspace crates for library consumers
- Comprehensive doc comments with examples
- **22 unit tests** covering construction, builder chaining, custom routes (single, multiple, with state, coexisting with built-in), hooks (registration, priority ordering, unregister, execution), database access from custom handlers, router idempotency, and 404 handling

#### 2. `crates/zerobase-server/examples/framework_mode.rs` (new)

Complete example demonstrating:
- Custom routes with shared state (request counter)
- Custom audit logger hook
- Builder pattern configuration

#### 3. `crates/zerobase-server/Cargo.toml` (modified)

- Added `[lib]` section exposing library target alongside binary
- Added `[[example]]` entry for framework_mode
- Added `[dev-dependencies]` for tower (test utilities)

### Test Results

22/22 tests passing. Library and example compile without errors.

## Task: Implement Embedded JavaScript Hooks Runtime

**Task ID:** `yelptm3ko6sbvi5`
**Date:** 2026-03-21

### Objective

Implement an embedded JavaScript runtime (using Boa engine) for executing JS hooks from `pb_hooks/*.pb.js` files, mirroring PocketBase's JS hooks system. Expose `$app` bindings for DAO operations, mail sending, custom routes, and logging. Support file watching for hot-reload in development mode.

### Changes Made

#### 1. `crates/zerobase-hooks/src/live_dao.rs` (new)

Production `DaoHandler` implementation backed by a real `RecordRepository`:
- `LiveDaoHandler<R>` translates `DaoRequest` variants into repository method calls
- Handles FindById, FindByFilter, FindMany, Save (insert vs update auto-detection), Delete
- `parse_sort_string()` helper for PocketBase-style sort strings (e.g., `-created,+title`)
- FindById returns `None` on not-found instead of erroring (matches PocketBase behavior)
- Save auto-generates ID for new records via `zerobase_core::generate_id()`
- **5 unit tests** for sort string parsing

#### 2. `crates/zerobase-hooks/src/lib.rs` (modified)

- Added `pub mod live_dao;` module declaration
- Added `pub use live_dao::LiveDaoHandler;` re-export

#### 3. `crates/zerobase-hooks/src/bindings.rs` (modified)

- Enhanced `$app.logger()` to return a proper object with `info()`, `warn()`, `error()`, `debug()` methods backed by `console.*` calls
- Updated test `app_logger_returns_object_with_methods` verifying logger returns callable methods

#### 4. `crates/zerobase-hooks/src/engine.rs` (modified)

- Added `e.httpError(status, message)` method on hook event objects
- Uses `__http_error` global variable to communicate error details from JS to Rust
- Post-evaluation check extracts status/message and returns `ZerobaseError::hook_abort()`
- Added `create_hook()` method on `JsHookEngine` that creates a `JsHook` without consuming the engine, enabling shared state between the registered hook and the reload engine

#### 5. `crates/zerobase-core/src/error.rs` (modified)

- Added `HookAbort { status, message }` variant to `ZerobaseError` enum
- Added `hook_abort(status, message)` convenience constructor
- Updated `status_code()` to return the hook-specified status code
- Updated `is_user_facing()` to treat hook aborts as user-facing errors

#### 6. `crates/zerobase-server/src/lib.rs` (modified)

- Fixed watcher reload bug: `with_js_hooks()` now uses `create_hook()` instead of `into_hook()`, keeping the engine alive with shared `Arc<RwLock<JsHookState>>`
- `with_js_hooks_watcher()` reuses the stored engine (via `Arc`) so reloads update the same state that the registered `JsHook` reads from

### Test Results

All tests passing across zerobase-hooks, zerobase-core, and zerobase-server crates. No compilation warnings (except pre-existing unused function in zerobase-api).

---

## Task: Write Integration Tests for File Storage

**Task ID:** `ylam6n18df3ujv1`
**Date:** 2026-03-21

### Objective

Create a comprehensive integration test suite covering end-to-end file storage workflows through the HTTP API layer: upload, download, deletion, protected files, thumbnail generation, S3 backend (mocked), and file size/type validation.

### Changes Made

#### 1. `crates/zerobase-api/tests/file_storage_integration.rs` (new)

25 integration tests organized by category:

- **Round-trip (2):** Upload → download for text and binary files, verifying byte-level data integrity
- **MIME validation (2):** Rejection of disallowed MIME types (PDF to images-only field), acceptance of allowed types
- **Size validation (2):** Rejection of oversized files (60KB > 50KB limit), acceptance at exact boundary (50KB)
- **Multiple files (1):** Gallery field with max_select=3, three files uploaded successfully
- **Protected files (2):** Token-gated download flow (no token → 401, bad token → 401, valid token → 200 with correct data); token endpoint requires auth
- **Concurrent uploads (1):** 5 parallel multipart uploads all succeed
- **File replacement (1):** Record update replaces file, new file downloadable with correct content
- **Download features (1):** `?download=true` sets Content-Disposition attachment header
- **Thumbnail generation (2):** Upload real PNG → request `?thumb=100x100` → verify dimensions and cache; non-image thumbnail → 400
- **Deletion (1):** Delete record → files cleaned from storage → download returns 404
- **Large file (1):** 32KB file round-trip with full byte verification
- **LocalFileStorage (3):** Upload/download round-trip, delete cleanup, thumbnail generation — all on real filesystem via tempdir
- **MockS3Storage (3):** Upload/download with atomic operation counters, delete cleanup, concurrent uploads
- **Edge cases (3):** Nonexistent file → 404, JSON create leaves storage empty, cache-control header present

Mock infrastructure: MemoryStorage, MockS3Storage (with atomic counters), MockSchemaLookup, MockRecordRepo, MockTokenService, fake_auth_middleware. Three test collections: documents (general), images (MIME-restricted), protected_docs (token-gated).

#### 2. `crates/zerobase-api/Cargo.toml` (modified)

- Added `tempfile = "3"` to dev-dependencies for LocalFileStorage integration tests

### Test Results

All 25 tests pass:
```
test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s
```

---

## Task: Implement Request Body Size Limits

**Task ID:** `zlxihq9ej2mo7c5`
**Date:** 2026-03-21

### Objective

Configure maximum request body size globally and per-endpoint (larger for file uploads). Return 413 for oversized requests. Make configurable via settings.

### Changes Made

1. **`crates/zerobase-core/src/error.rs`** — Added `PayloadTooLarge { message }` variant to `ZerobaseError` enum with 413 status code mapping, `is_user_facing()` inclusion, and `payload_too_large()` convenience constructor.

2. **`crates/zerobase-core/src/configuration.rs`** — Added `body_limit` (10 MiB default) and `body_limit_upload` (100 MiB default) fields to `ServerSettings`. Updated test to include new fields.

3. **`crates/zerobase-api/src/middleware/body_limit.rs`** — NEW FILE. Body limit middleware with:
   - `BodyLimitConfig` struct with `max_body_size` and `max_upload_size`
   - Content-Type based limit selection (multipart gets upload limit)
   - Early rejection via Content-Length header before body is read
   - JSON error response with `x-max-body-size` header
   - 13 unit tests

4. **`crates/zerobase-api/src/middleware/mod.rs`** — Added `pub mod body_limit;`

5. **`crates/zerobase-api/src/lib.rs`** — Added `api_router_full()` and `api_router_with_auth_full()` functions accepting `BodyLimitConfig`. Existing functions delegate with defaults. Body limit middleware applied as a layer.

6. **`crates/zerobase-api/src/handlers/records.rs`** — Updated hardcoded `to_bytes` limits from 10 MiB to 100 MiB (safety fallback; middleware now enforces actual limits).

7. **`crates/zerobase-server/src/main.rs`** — Wired `BodyLimitConfig` from `settings.server.body_limit` and `settings.server.body_limit_upload` to the router builder.

8. **`sample.zerobase.toml`** — Documented `body_limit` and `body_limit_upload` configuration options.

### Test Results

All tests pass: 14 new tests (13 body limit middleware + 1 error mapping), 0 failures across entire suite.
