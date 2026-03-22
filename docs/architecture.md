# Zerobase Architecture

> A Pocketbase-compatible Backend-as-a-Service built in Rust with axum, SQLite, and a layered architecture inspired by *Zero to Production in Rust*.

---

## 1. Crate Organization (Cargo Workspace)

Zerobase uses a **Cargo workspace** to enforce module boundaries at the compilation level. Each crate has a single responsibility and explicit dependencies on sibling crates.

```
zerobase/
├── Cargo.toml                  # Workspace root
├── crates/
│   ├── zerobase-core/          # Domain types, collection schema, validation rules
│   ├── zerobase-db/            # SQLite engine, migrations, query builder
│   ├── zerobase-auth/          # Authentication strategies, token management, OAuth2
│   ├── zerobase-api/           # Axum routes, middleware, JSON API, SSE realtime
│   ├── zerobase-files/         # File storage abstraction (local + S3)
│   ├── zerobase-admin/         # Admin dashboard API endpoints + static assets
│   └── zerobase-server/        # Binary entrypoint, configuration, startup
├── tests/                      # Integration tests (cross-crate)
├── docs/                       # Architecture, plans, ADRs
└── frontend/                   # AstroJS admin dashboard (separate build)
```

### Dependency Graph

```
zerobase-server
  ├── zerobase-api
  │     ├── zerobase-core
  │     ├── zerobase-db
  │     ├── zerobase-auth
  │     ├── zerobase-files
  │     └── zerobase-admin
  ├── zerobase-core      (re-export for configuration)
  └── (config, tracing, CLI)

zerobase-api
  ├── zerobase-core
  ├── zerobase-db
  ├── zerobase-auth
  └── zerobase-files

zerobase-auth
  ├── zerobase-core
  └── zerobase-db

zerobase-files
  ├── zerobase-core
  └── zerobase-db

zerobase-db
  └── zerobase-core

zerobase-admin
  ├── zerobase-core
  ├── zerobase-db
  └── zerobase-auth
```

**Rule:** Dependencies flow downward. `zerobase-core` depends on nothing else in the workspace. No circular dependencies.

---

## 2. Layered Architecture

Each crate maps to one or more layers of a clean architecture. The layers are:

### Layer 1: Domain (`zerobase-core`)

Pure Rust types with no I/O, no framework dependencies. This is the vocabulary of the entire system.

**Contains:**
- **Collection schema types** — `Collection`, `Field`, `FieldType`, `CollectionType` (base, auth, view)
- **Record types** — `Record`, `RecordId`, typed field values
- **Access rules** — `Rule` (filter expressions for list/view/create/update/delete/manage)
- **Validation** — schema validation logic, field constraints, rule parsing
- **Events** — domain event definitions (record created, updated, deleted)
- **Error types** — `DomainError` enum shared across the project
- **Traits** — `Identifiable`, `Timestamped`, `Validatable`

**Key design decisions:**
- All collection schema changes are type-safe. `FieldType` is an enum (`Text`, `Number`, `Bool`, `DateTime`, `File`, `Relation`, `Select`, `Email`, `Url`, `Editor`, `Json`, `Autodate`).
- `Collection` is the central aggregate. It owns its fields, rules, and indexes.
- No `String` for IDs — use `CollectionId` and `RecordId` newtypes.

### Layer 2: Application / Use Cases (spread across crates)

Business logic that orchestrates domain objects and infrastructure. Lives in each crate's top-level modules.

**Examples:**
- `zerobase-auth`: `authenticate()`, `register()`, `refresh_token()`, `verify_otp()`
- `zerobase-db`: `create_record()`, `apply_migration()`, `expand_relations()`
- `zerobase-files`: `upload_file()`, `generate_thumbnail()`, `create_file_token()`

### Layer 3: Infrastructure (`zerobase-db`, `zerobase-files`, `zerobase-auth`)

Concrete implementations of persistence, external services, and protocols.

- **`zerobase-db`** — SQLite via `rusqlite` (or `sqlx` with SQLite). Manages the data file, connection pooling, migrations, and query execution.
- **`zerobase-files`** — Local filesystem and S3-compatible storage via `aws-sdk-s3` or `rust-s3`. Thumbnail generation via `image` crate.
- **`zerobase-auth`** — OAuth2 via `oauth2` crate, JWT via `jsonwebtoken`, password hashing via `argon2`, OTP generation, passkey/WebAuthn via `webauthn-rs`.

### Layer 4: API / Presentation (`zerobase-api`, `zerobase-admin`)

HTTP layer built on **axum**. Translates HTTP requests into application calls and domain types.

- **`zerobase-api`** — Auto-generated CRUD routes per collection, realtime SSE, file serving, filtering/sorting/pagination.
- **`zerobase-admin`** — Superuser-only endpoints for schema management, backups, settings, logs. Serves the AstroJS frontend as static assets.

### Layer 5: Composition Root (`zerobase-server`)

The binary. Wires everything together:
- Parse CLI args and configuration
- Initialize SQLite database
- Build the axum `Router` by composing all route layers
- Start the HTTP server

---

## 3. Key Abstractions and Traits

### 3.1 Storage Traits

```rust
// zerobase-db
#[async_trait]
pub trait RecordRepository: Send + Sync {
    async fn find_one(&self, collection: &str, id: &RecordId) -> Result<Record, DbError>;
    async fn find_many(&self, collection: &str, query: &RecordQuery) -> Result<RecordList, DbError>;
    async fn create(&self, collection: &str, record: &Record) -> Result<Record, DbError>;
    async fn update(&self, collection: &str, id: &RecordId, record: &Record) -> Result<Record, DbError>;
    async fn delete(&self, collection: &str, id: &RecordId) -> Result<(), DbError>;
}

#[async_trait]
pub trait SchemaRepository: Send + Sync {
    async fn list_collections(&self) -> Result<Vec<Collection>, DbError>;
    async fn get_collection(&self, name: &str) -> Result<Collection, DbError>;
    async fn create_collection(&self, collection: &Collection) -> Result<Collection, DbError>;
    async fn update_collection(&self, name: &str, collection: &Collection) -> Result<Collection, DbError>;
    async fn delete_collection(&self, name: &str) -> Result<(), DbError>;
}
```

### 3.2 Auth Traits

```rust
// zerobase-auth
pub trait AuthMethod: Send + Sync {
    fn method_name(&self) -> &str;
    async fn authenticate(&self, credentials: &AuthCredentials) -> Result<AuthIdentity, AuthError>;
}

pub trait AuthProvider: Send + Sync {
    fn provider_name(&self) -> &str;
    async fn begin_auth(&self) -> Result<AuthRedirect, AuthError>;
    async fn complete_auth(&self, callback: &AuthCallback) -> Result<ExternalIdentity, AuthError>;
}
```

New auth methods (passkeys, magic links) implement `AuthMethod`. New OAuth providers (Google, Microsoft, GitHub) implement `AuthProvider`. **This is the extensibility point.**

### 3.3 File Storage Trait

```rust
// zerobase-files
#[async_trait]
pub trait FileStorage: Send + Sync {
    async fn upload(&self, path: &str, data: &[u8], content_type: &str) -> Result<(), StorageError>;
    async fn download(&self, path: &str) -> Result<Vec<u8>, StorageError>;
    async fn delete(&self, path: &str) -> Result<(), StorageError>;
    async fn exists(&self, path: &str) -> Result<bool, StorageError>;
}
```

Implementations: `LocalFileStorage`, `S3FileStorage`.

---

## 4. Application State and Dependency Injection

Following axum patterns, application state is shared via `Arc<AppState>` passed through axum's `State` extractor.

```rust
// zerobase-server
pub struct AppState {
    pub db: SqlitePool,                          // Connection pool
    pub schema_repo: Box<dyn SchemaRepository>,
    pub record_repo: Box<dyn RecordRepository>,
    pub file_storage: Box<dyn FileStorage>,
    pub auth_manager: AuthManager,               // Holds registered AuthMethod + AuthProvider instances
    pub config: AppConfig,
    pub event_bus: EventBus,                     // For realtime SSE
}
```

All handlers receive `State<Arc<AppState>>` — no global state, fully testable with mock implementations.

---

## 5. Error Handling Strategy

### 5.1 Error Hierarchy

Each crate defines its own error enum. Errors convert upward via `From` implementations.

```
DomainError (zerobase-core)
  ├── ValidationError { field, message }
  ├── RuleError { rule, reason }
  └── NotFound { entity, id }

DbError (zerobase-db)
  ├── Domain(DomainError)
  ├── ConnectionError(String)
  ├── QueryError(String)
  ├── MigrationError(String)
  └── Conflict { entity, id }

AuthError (zerobase-auth)
  ├── Domain(DomainError)
  ├── InvalidCredentials
  ├── TokenExpired
  ├── ProviderError { provider, message }
  ├── MfaRequired { method }
  └── Unauthorized

StorageError (zerobase-files)
  ├── NotFound(String)
  ├── PermissionDenied(String)
  └── IoError(std::io::Error)

ApiError (zerobase-api)
  ├── Domain(DomainError)
  ├── Db(DbError)
  ├── Auth(AuthError)
  ├── Storage(StorageError)
  ├── BadRequest(String)
  └── Internal(String)
```

### 5.2 HTTP Error Mapping

`ApiError` implements `IntoResponse` for axum, mapping each variant to the appropriate HTTP status code and a JSON error body:

```json
{
  "code": 400,
  "message": "Validation failed.",
  "data": {
    "field": "email",
    "message": "must be a valid email address"
  }
}
```

| Error | HTTP Status |
|-------|-------------|
| `ValidationError` | 400 |
| `BadRequest` | 400 |
| `InvalidCredentials` | 401 |
| `Unauthorized` | 401 |
| `TokenExpired` | 401 |
| `NotFound` | 404 |
| `Conflict` | 409 |
| `Internal` | 500 |

### 5.3 Error Crate

Use `thiserror` for defining error enums. Use `anyhow` only in the binary crate (`zerobase-server`) for top-level error context. Library crates never use `anyhow`.

---

## 6. Configuration

### 6.1 Approach

Configuration follows 12-factor app principles with layered sources:

1. **Defaults** — hardcoded sensible defaults
2. **Config file** — `zerobase.toml` (optional)
3. **Environment variables** — `ZEROBASE_` prefix
4. **CLI arguments** — override everything

Use the `config` crate for layered merging and `clap` for CLI argument parsing.

### 6.2 Configuration Structure

```rust
// zerobase-server
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub auth: AuthConfig,
    pub storage: StorageConfig,
    pub admin: AdminConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,           // default: "127.0.0.1"
    pub port: u16,              // default: 8090 (same as Pocketbase)
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub path: PathBuf,          // default: "zb_data/data.db"
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub token_duration_secs: u64,
    pub oauth_providers: Vec<OAuthProviderConfig>,
    pub mfa_enabled: bool,
    pub otp_enabled: bool,
    pub passkeys_enabled: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    pub backend: StorageBackend,  // Local or S3
    pub local_path: PathBuf,      // default: "zb_data/storage"
    pub s3: Option<S3Config>,
}
```

### 6.3 Environments

Configuration files per environment:

```
config/
├── base.toml       # Shared defaults
├── development.toml
├── testing.toml
└── production.toml
```

`ZEROBASE_ENV` (default: `development`) selects which overlay file is merged on top of `base.toml`.

---

## 7. Auto-Generated CRUD API

The central feature of Zerobase: every collection automatically gets REST endpoints.

### 7.1 Route Generation

On startup (and on schema change), Zerobase reads all collections from the database and generates routes:

```
GET    /api/collections/{collection}/records       — List records (filter, sort, page, expand)
GET    /api/collections/{collection}/records/{id}  — View record
POST   /api/collections/{collection}/records       — Create record
PATCH  /api/collections/{collection}/records/{id}  — Update record
DELETE /api/collections/{collection}/records/{id}  — Delete record
```

### 7.2 Dynamic Router

The router is rebuilt when collections change. A `RwLock<Router>` (or a similar pattern) allows hot-swapping routes without restarting the server.

### 7.3 Filtering and Expansion

Query parameters follow Pocketbase conventions:

- `?filter=(field='value' && field2>100)`
- `?sort=-created,title`
- `?page=1&perPage=20`
- `?expand=relation_field`
- `?fields=id,title,created`

A filter parser converts the filter string into a safe parameterized SQL WHERE clause.

---

## 8. Realtime (SSE)

Server-Sent Events for live record subscriptions:

```
GET /api/realtime   — SSE connection
```

Clients subscribe to topics like `collections/{name}/records` or `collections/{name}/records/{id}`. An in-memory `EventBus` (using `tokio::broadcast`) fans out domain events to connected SSE streams.

---

## 9. Authentication Architecture

### 9.1 Auth Collections

Auth-type collections extend base collections with built-in auth fields (`email`, `password`, `verified`, `tokenKey`, etc.) — mirroring Pocketbase.

### 9.2 Auth Flow

```
Client → POST /api/collections/{auth_collection}/auth-with-password
       → POST /api/collections/{auth_collection}/auth-with-oauth2
       → POST /api/collections/{auth_collection}/auth-with-otp
       → POST /api/collections/{auth_collection}/auth-refresh
```

Returns a JWT token. Subsequent requests pass `Authorization: Bearer <token>`.

### 9.3 Superadmins

A special `_superusers` collection (like Pocketbase's `_superusers`). Superadmin tokens grant access to admin-only endpoints (schema CRUD, backups, settings).

### 9.4 Access Rule Evaluation

Every CRUD request evaluates the collection's access rules. Rules are filter expressions that can reference `@request.auth.*` (the authenticated user). If the rule evaluates to an empty result set, access is denied.

---

## 10. Testing Strategy

### 10.1 Unit Tests

Every crate has `#[cfg(test)]` modules co-located with source. Domain logic in `zerobase-core` is pure and trivially testable.

### 10.2 Integration Tests

The `tests/` directory at the workspace root contains cross-crate integration tests:

- **API tests** — spin up an in-memory axum server with `axum::test`, make HTTP requests, assert responses.
- **Database tests** — use a temporary SQLite database (`:memory:` or tempfile) for each test.
- **Auth flow tests** — end-to-end auth with mock OAuth providers.

### 10.3 Test Utilities

A shared `test-utils` module (or crate) providing:
- `TestApp` — boots a full application stack with in-memory DB
- `TestClient` — wraps `reqwest` for ergonomic API testing
- Factory functions for creating test collections, records, users

### 10.4 CI

All tests run on every PR. `cargo test --workspace` is the single command.

---

## 11. Observability

- **Tracing** — `tracing` + `tracing-subscriber` for structured logging. Log level controlled via config.
- **Request logging** — `tower-http::trace` middleware on all routes.
- **Health check** — `GET /api/health` returning status and version.

---

## 12. Key Crate Choices

| Purpose | Crate | Rationale |
|---------|-------|-----------|
| HTTP framework | `axum` | Required by project spec |
| Async runtime | `tokio` | axum's runtime, most popular |
| SQLite | `rusqlite` + `r2d2` | Mature, sync (fits SQLite's single-writer model); `r2d2` for connection pooling |
| Serialization | `serde` + `serde_json` | Standard |
| Error derivation | `thiserror` | Ergonomic error enums |
| Config | `config` + `clap` | Layered config + CLI |
| JWT | `jsonwebtoken` | Most downloaded JWT crate |
| Password hashing | `argon2` | Recommended by OWASP |
| OAuth2 | `oauth2` | Well-maintained, protocol-level |
| WebAuthn/Passkeys | `webauthn-rs` | Reference implementation |
| S3 storage | `rust-s3` | Lightweight, well-maintained |
| Image thumbnails | `image` | Standard image processing |
| Tracing | `tracing` | Structured logging standard |
| HTTP middleware | `tower` + `tower-http` | axum ecosystem |
| UUID | `uuid` | ID generation |
| Chrono | `chrono` | Timestamp handling |
| Testing | `reqwest` + `wiremock` | HTTP client + mock servers |

---

## 13. Directory Layout (Full)

```
zerobase/
├── Cargo.toml                          # [workspace]
├── Cargo.lock
├── config/
│   ├── base.toml
│   ├── development.toml
│   ├── testing.toml
│   └── production.toml
├── crates/
│   ├── zerobase-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── collection.rs           # Collection, CollectionType, Field, FieldType
│   │       ├── record.rs               # Record, RecordId, typed values
│   │       ├── rules.rs                # Access rule types and parsing
│   │       ├── events.rs               # Domain events
│   │       ├── error.rs                # DomainError
│   │       └── types.rs                # Shared newtypes (CollectionId, RecordId)
│   ├── zerobase-db/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── pool.rs                 # Connection pool setup
│   │       ├── migrations/             # SQL migration files
│   │       │   └── mod.rs
│   │       ├── schema_repo.rs          # SchemaRepository impl
│   │       ├── record_repo.rs          # RecordRepository impl
│   │       ├── query_builder.rs        # Dynamic SQL generation
│   │       ├── filter_parser.rs        # Pocketbase filter syntax → SQL
│   │       └── error.rs                # DbError
│   ├── zerobase-auth/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── manager.rs              # AuthManager — registry of methods/providers
│   │       ├── token.rs                # JWT creation, validation, refresh
│   │       ├── password.rs             # Password auth (AuthMethod impl)
│   │       ├── oauth/                  # OAuth2 providers
│   │       │   ├── mod.rs
│   │       │   ├── google.rs
│   │       │   └── microsoft.rs
│   │       ├── otp.rs                  # OTP via email (AuthMethod impl)
│   │       ├── passkey.rs              # WebAuthn (AuthMethod impl)
│   │       ├── mfa.rs                  # MFA orchestration
│   │       ├── superuser.rs            # Superadmin logic
│   │       └── error.rs                # AuthError
│   ├── zerobase-files/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── storage.rs              # FileStorage trait
│   │       ├── local.rs                # LocalFileStorage impl
│   │       ├── s3.rs                   # S3FileStorage impl
│   │       ├── thumbnail.rs            # Image thumbnail generation
│   │       ├── token.rs                # File access tokens
│   │       └── error.rs               # StorageError
│   ├── zerobase-api/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── router.rs              # Dynamic route generation
│   │       ├── handlers/
│   │       │   ├── mod.rs
│   │       │   ├── records.rs         # CRUD handlers
│   │       │   ├── auth.rs            # Auth endpoints
│   │       │   ├── files.rs           # File upload/download
│   │       │   ├── realtime.rs        # SSE endpoint
│   │       │   └── health.rs          # Health check
│   │       ├── middleware/
│   │       │   ├── mod.rs
│   │       │   ├── auth.rs            # JWT extraction + validation
│   │       │   ├── cors.rs            # CORS configuration
│   │       │   └── logging.rs         # Request/response logging
│   │       ├── extractors.rs          # Custom axum extractors
│   │       ├── pagination.rs          # Pagination types
│   │       ├── filters.rs             # Query parameter parsing
│   │       ├── error.rs               # ApiError + IntoResponse
│   │       └── state.rs               # AppState definition
│   ├── zerobase-admin/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── routes.rs             # Admin-only routes (schema CRUD, backups, settings)
│   │       ├── handlers/
│   │       │   ├── mod.rs
│   │       │   ├── collections.rs    # Collection management
│   │       │   ├── settings.rs       # Server settings
│   │       │   ├── backups.rs        # Database backups
│   │       │   └── logs.rs           # Server logs
│   │       └── static_assets.rs      # Serve AstroJS build
│   └── zerobase-server/
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs               # Entrypoint
│           ├── config.rs             # AppConfig + layered loading
│           ├── startup.rs            # Build AppState, Router, run server
│           └── cli.rs                # CLI argument parsing
├── tests/
│   ├── common/
│   │   └── mod.rs                    # TestApp, TestClient, factories
│   ├── api_records_test.rs
│   ├── api_auth_test.rs
│   ├── api_files_test.rs
│   ├── api_realtime_test.rs
│   └── api_admin_test.rs
├── frontend/                          # AstroJS admin dashboard
│   ├── package.json
│   ├── astro.config.mjs
│   └── src/
├── docs/
│   └── architecture.md               # This document
└── README.md
```

---

## 14. Design Principles

1. **Type safety over runtime checks.** Use Rust's type system (enums, newtypes, sealed traits) to make invalid states unrepresentable.
2. **Trait-based extensibility.** New auth methods, storage backends, and field types are added by implementing traits — never by modifying existing code.
3. **No duplication.** Shared logic lives in `zerobase-core`. Crates depend on it; they don't reinvent it.
4. **Testability by construction.** All I/O goes through traits. Tests inject mocks or in-memory implementations. No hidden global state.
5. **Pocketbase API compatibility.** The REST API mirrors Pocketbase's URL structure and JSON format so that existing Pocketbase client SDKs can work with Zerobase.
6. **Single binary deployment.** `cargo build --release` produces one binary. The AstroJS frontend is embedded at build time.
