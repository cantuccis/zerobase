# Authentication System Architecture

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build an extensible authentication system for Zerobase that mirrors PocketBase's auth capabilities — password auth, OTP, OAuth2 providers, MFA — using trait-based abstractions in Rust.

**Architecture:** The auth system is built around two core trait hierarchies: `AuthMethod` for direct authentication strategies (password, OTP, passkeys) and `AuthProvider` for external identity providers (Google, Microsoft, etc.). A `TokenService` handles stateless JWT lifecycle. Auth collections are a special collection type (`CollectionType::Auth`) with implicit system fields for identity management.

**Tech Stack:** `jsonwebtoken` (JWT), `argon2` (password hashing), `oauth2` (OAuth2 flows), `secrecy` (secret handling), `chrono` (timestamps)

---

## Table of Contents

1. [Overview & Design Principles](#1-overview--design-principles)
2. [Auth Collection Type](#2-auth-collection-type)
3. [Core Traits](#3-core-traits)
4. [JWT Token Structure](#4-jwt-token-structure)
5. [Password Authentication Flow](#5-password-authentication-flow)
6. [OTP Authentication Flow](#6-otp-authentication-flow)
7. [OAuth2 Authentication Flow](#7-oauth2-authentication-flow)
8. [Superuser Authentication](#8-superuser-authentication)
9. [MFA (Multi-Factor Authentication)](#9-mfa-multi-factor-authentication)
10. [API Endpoints](#10-api-endpoints)
11. [Middleware Integration](#11-middleware-integration)
12. [Extensibility Points](#12-extensibility-points)
13. [Security Considerations](#13-security-considerations)
14. [Data Model](#14-data-model)

---

## 1. Overview & Design Principles

### Principles

- **Trait-driven extensibility**: All auth methods and providers implement well-defined traits. Adding a new auth method (e.g., passkeys) means implementing a trait, not modifying existing code.
- **Stateless tokens**: JWT-based auth with no server-side session storage. Token validity is verified via signature + expiry + `tokenKey` rotation.
- **Collection-scoped auth**: Each auth collection is independent. A `users` collection and a `staff` collection each have their own identity namespace, auth settings, and tokens.
- **PocketBase compatibility**: API endpoints, token structure, and auth flows match PocketBase semantics.
- **Defense in depth**: Argon2id for passwords, HMAC-SHA256 for JWTs, per-user `tokenKey` for instant invalidation, constant-time comparisons.

### Crate Responsibilities

| Crate | Auth Responsibility |
|---|---|
| `zerobase-core` | Auth collection types, `AuthOptions`, field definitions, error types |
| `zerobase-auth` | Trait definitions, implementations (password, OTP, OAuth2), token service, password hashing |
| `zerobase-db` | Auth record persistence, `_superusers` table, `_externalAuths` table, OTP storage |
| `zerobase-api` | Auth endpoints, middleware (JWT extraction, superuser guard), request routing |

---

## 2. Auth Collection Type

Auth collections extend base collections with implicit system fields that are always present (never defined by the user). These match PocketBase exactly.

### System Fields (implicit, not in `fields` vec)

| Field | SQLite Type | Description |
|---|---|---|
| `id` | `TEXT PRIMARY KEY` | 15-char alphanumeric ID |
| `email` | `TEXT` | User's email address (identity field) |
| `emailVisibility` | `INTEGER` (bool) | Whether email is visible in API responses |
| `verified` | `INTEGER` (bool) | Whether the email is verified |
| `password` | `TEXT` | Argon2id password hash |
| `tokenKey` | `TEXT` | Random key included in JWT — rotate to invalidate all tokens |
| `created` | `TEXT` | ISO 8601 timestamp |
| `updated` | `TEXT` | ISO 8601 timestamp |

### AuthOptions (existing in `collection.rs`)

```rust
pub struct AuthOptions {
    pub allow_email_auth: bool,        // Enable password auth
    pub allow_oauth2_auth: bool,       // Enable OAuth2 providers
    pub allow_otp_auth: bool,          // Enable OTP via email
    pub require_email: bool,           // Require email before auth
    pub min_password_length: u32,      // Min 8 by default
    pub identity_fields: Vec<String>,  // Default: ["email"]
    pub manage_rule: Option<String>,   // Who can manage other users
}
```

### DDL for Auth Collection Tables

When creating an auth collection (e.g., `users`), the schema repo must create the table with both system fields and user-defined fields:

```sql
CREATE TABLE users (
    id               TEXT PRIMARY KEY NOT NULL,
    email            TEXT NOT NULL DEFAULT '',
    emailVisibility  INTEGER NOT NULL DEFAULT 0,
    verified         INTEGER NOT NULL DEFAULT 0,
    password         TEXT NOT NULL DEFAULT '',
    tokenKey         TEXT NOT NULL DEFAULT '',
    created          TEXT NOT NULL DEFAULT (datetime('now')),
    updated          TEXT NOT NULL DEFAULT (datetime('now'))
    -- user-defined fields follow
);

CREATE UNIQUE INDEX idx_users_email ON users(email) WHERE email != '';
CREATE INDEX idx_users_tokenKey ON users(tokenKey);
```

---

## 3. Core Traits

### 3.1 AuthMethod Trait

`AuthMethod` represents a direct authentication strategy where the user provides credentials directly to Zerobase. Each implementation handles one type of credential.

```rust
// crates/zerobase-auth/src/method.rs

use async_trait::async_trait;
use zerobase_core::error::Result;
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Credentials provided by the user for authentication.
/// The shape varies by method — password auth sends {identity, password},
/// OTP sends {otpId, password}, etc.
pub type AuthCredentials = HashMap<String, JsonValue>;

/// Result of a successful authentication — the authenticated record's data.
pub struct AuthResult {
    /// The authenticated user's record ID.
    pub record_id: String,
    /// The collection name this user belongs to.
    pub collection_name: String,
    /// The user's record data (for embedding in token/response).
    pub record: HashMap<String, JsonValue>,
}

/// A direct authentication method (password, OTP, passkeys).
///
/// Each implementation knows how to:
/// 1. Validate that the credentials have the right shape.
/// 2. Authenticate against stored data (DB lookup + verification).
/// 3. Report its method name for logging/configuration.
#[async_trait]
pub trait AuthMethod: Send + Sync {
    /// Unique identifier for this method (e.g., "password", "otp", "passkey").
    fn method_name(&self) -> &'static str;

    /// Authenticate a user in the given collection using the provided credentials.
    ///
    /// Returns `AuthResult` on success, `ZerobaseError::Auth` on failure.
    /// Must NOT leak whether the identity exists (timing-safe).
    async fn authenticate(
        &self,
        collection: &str,
        credentials: &AuthCredentials,
    ) -> Result<AuthResult>;

    /// Validate that the credentials contain the required fields for this method.
    /// Called before `authenticate` to provide early validation errors.
    fn validate_credentials(&self, credentials: &AuthCredentials) -> Result<()>;
}
```

### 3.2 AuthProvider Trait

`AuthProvider` represents external OAuth2 identity providers. The trait abstracts the OAuth2 authorization code flow.

```rust
// crates/zerobase-auth/src/provider.rs

use async_trait::async_trait;
use zerobase_core::error::Result;

/// User info retrieved from an external OAuth2 provider.
pub struct ExternalAuthUser {
    /// Provider-assigned user ID.
    pub provider_id: String,
    /// Provider name (e.g., "google", "microsoft").
    pub provider: String,
    /// User's email from the provider.
    pub email: String,
    /// User's display name (if available).
    pub name: Option<String>,
    /// URL to the user's avatar (if available).
    pub avatar_url: Option<String>,
    /// Raw metadata from the provider's userinfo endpoint.
    pub raw: serde_json::Value,
}

/// Configuration for an OAuth2 provider instance.
pub struct ProviderConfig {
    /// Provider name (e.g., "google").
    pub name: String,
    /// OAuth2 client ID.
    pub client_id: String,
    /// OAuth2 client secret.
    pub client_secret: secrecy::SecretString,
    /// Authorization endpoint URL.
    pub auth_url: String,
    /// Token endpoint URL.
    pub token_url: String,
    /// Userinfo endpoint URL.
    pub userinfo_url: String,
    /// OAuth2 scopes to request.
    pub scopes: Vec<String>,
    /// Redirect URL after authorization.
    pub redirect_url: String,
}

/// An external OAuth2 authentication provider.
///
/// Implementors handle the specifics of each provider's OAuth2 flow
/// and user info endpoint format.
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// Provider identifier (e.g., "google", "microsoft", "github").
    fn provider_name(&self) -> &'static str;

    /// Generate the authorization URL that the client should redirect to.
    /// Returns (url, state, code_verifier) for PKCE.
    fn authorization_url(&self) -> Result<(String, String, String)>;

    /// Exchange an authorization code for user information.
    ///
    /// This performs:
    /// 1. Code → token exchange
    /// 2. Token → userinfo fetch
    /// 3. Parse into ExternalAuthUser
    async fn authenticate(
        &self,
        code: &str,
        state: &str,
        code_verifier: &str,
    ) -> Result<ExternalAuthUser>;
}
```

### 3.3 TokenService Trait

`TokenService` manages the JWT token lifecycle. It's a trait to allow testing with mock implementations.

```rust
// crates/zerobase-auth/src/token.rs

use zerobase_core::error::Result;

/// Claims embedded in a JWT token.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TokenClaims {
    /// Subject — the record ID of the authenticated user.
    pub sub: String,
    /// Collection ID that the user belongs to.
    #[serde(rename = "collectionId")]
    pub collection_id: String,
    /// Collection name.
    #[serde(rename = "collectionName")]
    pub collection_name: String,
    /// Token type: "auth" for users, "admin" for superusers.
    #[serde(rename = "type")]
    pub token_type: String,
    /// Issued at (Unix timestamp).
    pub iat: i64,
    /// Expiration (Unix timestamp).
    pub exp: i64,
    /// Refresh window start (optional — tokens issued before this are invalid).
    /// This corresponds to the user's `tokenKey` — if the tokenKey changes,
    /// all previously issued tokens become invalid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_key: Option<String>,
}

/// Manages JWT token creation, validation, and refresh.
pub trait TokenService: Send + Sync {
    /// Generate a new JWT for the given claims.
    fn generate(&self, claims: &TokenClaims) -> Result<String>;

    /// Validate a JWT and return its claims.
    ///
    /// Checks:
    /// 1. Signature is valid (HMAC-SHA256).
    /// 2. Token is not expired.
    /// 3. Required claims are present.
    ///
    /// Does NOT check `tokenKey` — the caller must verify that
    /// the token's `token_key` matches the user's current `tokenKey`.
    fn validate(&self, token: &str) -> Result<TokenClaims>;

    /// Refresh a token — validate the existing token, then issue
    /// a new one with a fresh `exp`. The caller must verify the
    /// user still exists and the `tokenKey` hasn't changed.
    fn refresh(&self, token: &str) -> Result<String>;
}
```

### 3.4 PasswordHasher Trait

Abstracts password hashing for testability.

```rust
// crates/zerobase-auth/src/password.rs

use zerobase_core::error::Result;

/// Password hashing and verification.
///
/// Default implementation uses Argon2id. Trait exists for testability.
pub trait PasswordHasher: Send + Sync {
    /// Hash a plaintext password. Returns the encoded hash string.
    fn hash(&self, password: &str) -> Result<String>;

    /// Verify a plaintext password against a stored hash.
    /// Returns `Ok(true)` if match, `Ok(false)` if not.
    /// Must be constant-time to prevent timing attacks.
    fn verify(&self, password: &str, hash: &str) -> Result<bool>;
}
```

---

## 4. JWT Token Structure

### Token Format

Zerobase uses **HS256** (HMAC-SHA256) JWTs, matching PocketBase's approach.

**Header:**
```json
{
  "alg": "HS256",
  "typ": "JWT"
}
```

**Payload (user auth token):**
```json
{
  "sub": "abc123def456gh7",
  "collectionId": "xyz789abc012de3",
  "collectionName": "users",
  "type": "auth",
  "iat": 1711000000,
  "exp": 1712209600,
  "tokenKey": "a1b2c3d4e5f6g7h8"
}
```

**Payload (superuser auth token):**
```json
{
  "sub": "admin_id_here",
  "collectionId": "_superusers",
  "collectionName": "_superusers",
  "type": "admin",
  "iat": 1711000000,
  "exp": 1712209600
}
```

### Token Signing

- **Secret**: `auth.token_secret` from configuration (`SecretString`).
- **Algorithm**: HS256 (HMAC-SHA256).
- **Default duration**: 14 days (1,209,600 seconds), configurable via `auth.token_duration_secs`.

### Token Validation Pipeline

```
Request → Extract Bearer token → Decode JWT → Verify signature → Check expiry
  → Resolve collection from claims → Lookup user record → Compare tokenKey
  → Build AuthInfo
```

The `tokenKey` check is critical: when a user changes their password or an admin invalidates sessions, the `tokenKey` field on the record is rotated. Any JWT issued before the rotation will have the old `tokenKey` and be rejected.

---

## 5. Password Authentication Flow

### Registration (Create Auth Record)

```
POST /api/collections/{collection}/records
{
  "email": "user@example.com",
  "password": "securepassword123",
  "passwordConfirm": "securepassword123",
  "name": "John Doe"
}
```

**Server flow:**
1. Validate `password` meets `min_password_length` from `AuthOptions`.
2. Validate `password == passwordConfirm`.
3. Hash password with Argon2id → store in `password` column.
4. Generate random `tokenKey` (16 chars alphanumeric).
5. Set `verified = false`, `emailVisibility = false`.
6. Create record with system fields + user fields.
7. (If SMTP enabled) Send verification email.
8. Return record (password field excluded from response).

### Login (Password Auth)

```
POST /api/collections/{collection}/auth-with-password
{
  "identity": "user@example.com",
  "password": "securepassword123"
}
```

**Server flow:**
1. Check `allow_email_auth` is enabled in collection's `AuthOptions`.
2. Look up record by identity field(s) — iterate `identity_fields` (default: `["email"]`).
3. If not found → return generic "invalid credentials" (don't reveal whether identity exists).
4. Verify password against stored Argon2id hash (constant-time).
5. If mismatch → return generic "invalid credentials".
6. If `require_email` and `verified == false` → return error.
7. Generate JWT with user's `tokenKey` embedded.
8. Return `{ token, record }`.

### Response Format

```json
{
  "token": "eyJhbGciOiJIUzI1NiIs...",
  "record": {
    "id": "abc123def456gh7",
    "collectionId": "xyz789abc012de3",
    "collectionName": "users",
    "email": "user@example.com",
    "verified": true,
    "name": "John Doe",
    "created": "2024-01-15 10:00:00.000Z",
    "updated": "2024-01-15 10:00:00.000Z"
  }
}
```

---

## 6. OTP Authentication Flow

OTP (One-Time Password) provides passwordless email authentication.

### Step 1: Request OTP

```
POST /api/collections/{collection}/request-otp
{
  "email": "user@example.com"
}
```

**Server flow:**
1. Check `allow_otp_auth` is enabled.
2. Look up user by email. If not found, create a new record (PocketBase behavior).
3. Generate a random OTP code (e.g., 6-digit numeric or 8-char alphanumeric).
4. Store OTP in `_otps` table: `{ id, collectionId, recordId, password (hashed), sentTo, created }`.
5. Send OTP via email (SMTP).
6. Return `{ otpId }`.

### Step 2: Verify OTP

```
POST /api/collections/{collection}/auth-with-otp
{
  "otpId": "otp_abc123",
  "password": "123456"
}
```

**Server flow:**
1. Look up OTP record by `otpId`.
2. Check OTP is not expired (default: 5 minutes).
3. Verify OTP password hash (constant-time).
4. Delete the OTP record (single-use).
5. Mark user as `verified = true` if they used the email that matches.
6. Generate JWT and return `{ token, record }`.

### OTP Storage Table

```sql
CREATE TABLE _otps (
    id            TEXT PRIMARY KEY NOT NULL,
    collection_id TEXT NOT NULL,
    record_id     TEXT NOT NULL,
    password      TEXT NOT NULL,  -- Argon2id hash of OTP code
    sent_to       TEXT NOT NULL,  -- Email address OTP was sent to
    created       TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_otps_record ON _otps(collection_id, record_id);
```

---

## 7. OAuth2 Authentication Flow

### Step 1: List Available Providers

```
GET /api/collections/{collection}/auth-methods
```

**Response:**
```json
{
  "password": { "enabled": true, "identityFields": ["email"] },
  "otp": { "enabled": true },
  "oauth2": {
    "enabled": true,
    "providers": [
      { "name": "google", "displayName": "Google", "state": "...", "codeVerifier": "...", "codeChallenge": "...", "codeChallengeMethod": "S256", "authURL": "https://accounts.google.com/o/oauth2/v2/auth?..." },
      { "name": "microsoft", "displayName": "Microsoft", "state": "...", "codeVerifier": "...", "codeChallenge": "...", "codeChallengeMethod": "S256", "authURL": "https://login.microsoftonline.com/..." }
    ]
  }
}
```

### Step 2: Client Redirects to Provider

The client opens the `authURL` in a browser. After the user authorizes, the provider redirects back with a `code` and `state`.

### Step 3: Exchange Code for Auth

```
POST /api/collections/{collection}/auth-with-oauth2
{
  "provider": "google",
  "code": "authorization_code_here",
  "state": "state_from_step_1",
  "codeVerifier": "code_verifier_from_step_1",
  "redirectURL": "https://myapp.com/auth/callback"
}
```

**Server flow:**
1. Look up provider configuration.
2. Exchange `code` → access token (via provider's token endpoint).
3. Fetch user info from provider's userinfo endpoint.
4. Look up `_externalAuths` for existing link: `(provider, providerId)`.
5. If linked → authenticate as the linked user.
6. If not linked → look up by email.
   - If email matches existing user → link the provider and authenticate.
   - If no match → create new user record, link provider, authenticate.
7. Generate JWT and return `{ token, record, meta: { ... } }`.

### External Auth Storage Table

```sql
CREATE TABLE _externalAuths (
    id            TEXT PRIMARY KEY NOT NULL,
    collection_id TEXT NOT NULL,
    record_id     TEXT NOT NULL,
    provider      TEXT NOT NULL,
    provider_id   TEXT NOT NULL,
    created       TEXT NOT NULL DEFAULT (datetime('now')),
    updated       TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (collection_id, provider, provider_id)
);

CREATE INDEX idx_external_auths_record ON _externalAuths(collection_id, record_id);
CREATE INDEX idx_external_auths_provider ON _externalAuths(provider, provider_id);
```

### Provider Implementations

**Google:**
- Auth URL: `https://accounts.google.com/o/oauth2/v2/auth`
- Token URL: `https://oauth2.googleapis.com/token`
- Userinfo URL: `https://www.googleapis.com/oauth2/v3/userinfo`
- Scopes: `openid email profile`

**Microsoft:**
- Auth URL: `https://login.microsoftonline.com/common/oauth2/v2.0/authorize`
- Token URL: `https://login.microsoftonline.com/common/oauth2/v2.0/token`
- Userinfo URL: `https://graph.microsoft.com/v1.0/me`
- Scopes: `openid email profile User.Read`

---

## 8. Superuser Authentication

Superusers are stored in the `_superusers` system table (not a regular auth collection). They have elevated privileges across the entire system.

### Superuser Login

```
POST /api/admins/auth-with-password
{
  "identity": "admin@example.com",
  "password": "adminpassword123"
}
```

**Server flow:**
1. Look up superuser by email in `_superusers` table.
2. Verify password against Argon2id hash.
3. Generate JWT with `type: "admin"` and `collectionName: "_superusers"`.
4. Return `{ token, admin }`.

### Superuser Token

Superuser tokens have `type: "admin"` in the claims. The middleware checks this to distinguish superuser requests from regular user requests.

### Initial Superuser Creation

On first run (when `_superusers` is empty), the server should prompt for or accept the initial superuser credentials via:
- CLI flags: `--admin-email`, `--admin-password`
- Environment: `ZEROBASE__ADMIN_EMAIL`, `ZEROBASE__ADMIN_PASSWORD`
- Interactive prompt (if running in a terminal)

---

## 9. MFA (Multi-Factor Authentication)

MFA is implemented as a per-collection option that requires a second factor after primary authentication.

### MFA Options (extend AuthOptions)

```rust
pub struct MfaOptions {
    /// Enable MFA for this collection.
    pub enabled: bool,
    /// Grace period in seconds — how long a partial auth (first factor only) is valid.
    pub duration_secs: u64,
    /// Minimum required auth methods to consider MFA complete.
    /// Default: 2 (primary + one additional).
    pub required_methods: u32,
}
```

### MFA Flow

1. User authenticates with primary method (e.g., password).
2. If MFA is enabled, server returns a **partial auth token** instead of a full token:
   ```json
   { "mfaId": "mfa_abc123", "methods": ["password"] }
   ```
3. Client presents the second factor (e.g., OTP):
   ```
   POST /api/collections/{collection}/auth-with-otp
   { "otpId": "...", "password": "123456", "mfaId": "mfa_abc123" }
   ```
4. Server verifies the second factor and checks that `mfaId` is valid and not expired.
5. If `methods.len() >= required_methods`, issue the full JWT.

### MFA Storage Table

```sql
CREATE TABLE _mfas (
    id            TEXT PRIMARY KEY NOT NULL,
    collection_id TEXT NOT NULL,
    record_id     TEXT NOT NULL,
    method        TEXT NOT NULL,  -- Comma-separated completed methods
    created       TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_mfas_record ON _mfas(collection_id, record_id);
```

---

## 10. API Endpoints

All auth endpoints are scoped under `/api/collections/{collection}/` for auth collections and `/api/admins/` for superusers.

### Auth Collection Endpoints

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/collections/{collection}/auth-with-password` | Password login |
| `POST` | `/api/collections/{collection}/auth-with-otp` | OTP verification |
| `POST` | `/api/collections/{collection}/request-otp` | Request OTP code |
| `POST` | `/api/collections/{collection}/auth-with-oauth2` | OAuth2 code exchange |
| `GET` | `/api/collections/{collection}/auth-methods` | List available auth methods |
| `POST` | `/api/collections/{collection}/auth-refresh` | Refresh auth token |
| `POST` | `/api/collections/{collection}/request-verification` | Request email verification |
| `POST` | `/api/collections/{collection}/confirm-verification` | Confirm email verification |
| `POST` | `/api/collections/{collection}/request-password-reset` | Request password reset |
| `POST` | `/api/collections/{collection}/confirm-password-reset` | Confirm password reset |
| `POST` | `/api/collections/{collection}/request-email-change` | Request email change |
| `POST` | `/api/collections/{collection}/confirm-email-change` | Confirm email change |
| `GET` | `/api/collections/{collection}/records/{id}/external-auths` | List linked OAuth2 providers |
| `DELETE` | `/api/collections/{collection}/records/{id}/external-auths/{provider}` | Unlink OAuth2 provider |

### Superuser Endpoints

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/admins/auth-with-password` | Superuser login |
| `POST` | `/api/admins/auth-refresh` | Refresh superuser token |
| `GET` | `/api/admins` | List superusers (superuser only) |
| `POST` | `/api/admins` | Create superuser (superuser only) |
| `GET` | `/api/admins/{id}` | View superuser (superuser only) |
| `PATCH` | `/api/admins/{id}` | Update superuser (superuser only) |
| `DELETE` | `/api/admins/{id}` | Delete superuser (superuser only) |

---

## 11. Middleware Integration

### Updated Auth Extraction (replacing placeholder)

The current placeholder in `auth_context.rs` will be replaced with proper JWT validation:

```rust
// Pseudocode for the updated AuthInfo extractor

impl<S: Send + Sync> FromRequestParts<S> for AuthInfo {
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let token = extract_bearer_token(parts);

        if token.is_none() {
            return Ok(AuthInfo::anonymous());
        }

        let token = token.unwrap();
        let token_service = get_token_service(state);

        // 1. Decode and validate JWT
        let claims = match token_service.validate(&token) {
            Ok(claims) => claims,
            Err(_) => return Ok(AuthInfo::anonymous()), // Invalid token = anonymous
        };

        // 2. Check token type
        if claims.token_type == "admin" {
            // Superuser token — verify superuser still exists
            let superuser = db.find_superuser(&claims.sub);
            if superuser.is_some() {
                return Ok(AuthInfo::superuser());
            }
            return Ok(AuthInfo::anonymous());
        }

        // 3. User token — resolve collection and verify tokenKey
        let record = db.find_record(&claims.collection_name, &claims.sub);
        if let Some(record) = record {
            let stored_token_key = record.get("tokenKey");
            if claims.token_key == stored_token_key {
                return Ok(AuthInfo::authenticated(record));
            }
        }

        Ok(AuthInfo::anonymous())
    }
}
```

### Require Superuser Middleware (replacing placeholder)

```rust
// Updated to check JWT claims instead of just header presence
pub async fn require_superuser(
    auth: AuthInfo,
    request: Request,
    next: Next,
) -> Response {
    if !auth.is_superuser {
        return (StatusCode::UNAUTHORIZED, Json(error_body)).into_response();
    }
    next.run(request).await
}
```

---

## 12. Extensibility Points

### Adding a New Auth Method

1. **Create implementation** in `crates/zerobase-auth/src/methods/`:
   ```rust
   pub struct PasskeyAuth { /* WebAuthn state */ }

   #[async_trait]
   impl AuthMethod for PasskeyAuth {
       fn method_name(&self) -> &'static str { "passkey" }
       async fn authenticate(&self, collection: &str, credentials: &AuthCredentials) -> Result<AuthResult> { ... }
       fn validate_credentials(&self, credentials: &AuthCredentials) -> Result<()> { ... }
   }
   ```

2. **Register in AuthService** (the orchestrator):
   ```rust
   auth_service.register_method(Box::new(PasskeyAuth::new()));
   ```

3. **Add API endpoint** in `crates/zerobase-api/`:
   ```rust
   .route("/api/collections/:collection/auth-with-passkey", post(auth_with_passkey))
   ```

4. **Extend AuthOptions** if needed:
   ```rust
   pub allow_passkey_auth: bool,
   ```

### Adding a New OAuth2 Provider

1. **Create implementation** in `crates/zerobase-auth/src/providers/`:
   ```rust
   pub struct GitHubProvider { config: ProviderConfig }

   #[async_trait]
   impl AuthProvider for GitHubProvider {
       fn provider_name(&self) -> &'static str { "github" }
       // ... implement authorization_url() and authenticate()
   }
   ```

2. **Register in provider registry**:
   ```rust
   provider_registry.register(Box::new(GitHubProvider::new(config)));
   ```

No changes needed to API endpoints — the generic OAuth2 endpoints handle all providers.

### Provider Configuration (stored in `_settings`)

OAuth2 providers are configured via the settings API:

```json
{
  "key": "oauth2_providers",
  "value": {
    "google": {
      "enabled": true,
      "clientId": "...",
      "clientSecret": "...",
      "redirectURL": "https://myapp.com/auth/callback"
    },
    "microsoft": {
      "enabled": true,
      "clientId": "...",
      "clientSecret": "...",
      "redirectURL": "https://myapp.com/auth/callback"
    }
  }
}
```

---

## 13. Security Considerations

### Password Storage
- **Algorithm**: Argon2id (memory-hard, side-channel resistant).
- **Parameters**: Default from the `argon2` crate (19 MiB memory, 2 iterations, 1 parallelism).
- Passwords are NEVER stored in plaintext, logged, or returned in API responses.

### Token Security
- **Signing**: HMAC-SHA256 with a server secret (`auth.token_secret`).
- **Expiry**: Default 14 days, configurable.
- **Revocation**: Rotating a user's `tokenKey` invalidates ALL existing tokens for that user instantly.
- Tokens should be transmitted only over HTTPS in production.

### Rate Limiting (future)
- Auth endpoints should be rate-limited to prevent brute-force attacks.
- Consider per-IP and per-identity rate limiting.

### OTP Security
- OTP codes are hashed before storage (Argon2id).
- OTPs expire after 5 minutes.
- OTPs are single-use (deleted after verification).
- Failed OTP attempts should be rate-limited.

### OAuth2 Security
- PKCE (Proof Key for Code Exchange) is mandatory for all OAuth2 flows.
- State parameter is validated to prevent CSRF attacks.
- Provider secrets are stored encrypted in `_settings`.

### Timing Attacks
- Password verification uses constant-time comparison (provided by Argon2).
- Identity lookups should NOT short-circuit on "user not found" — always perform a dummy hash comparison.

---

## 14. Data Model

### Entity Relationship Summary

```
┌─────────────────────┐
│    _superusers      │
│─────────────────────│
│ id (PK)             │
│ email (UNIQUE)      │
│ password (argon2)   │
│ created, updated    │
└─────────────────────┘

┌─────────────────────┐      ┌─────────────────────┐
│  Auth Collection    │      │   _externalAuths    │
│  (e.g., "users")   │──1:N─│─────────────────────│
│─────────────────────│      │ id (PK)             │
│ id (PK)             │      │ collection_id       │
│ email               │      │ record_id (FK)      │
│ emailVisibility     │      │ provider            │
│ verified            │      │ provider_id         │
│ password (argon2)   │      │ created, updated    │
│ tokenKey            │      └─────────────────────┘
│ created, updated    │
│ [user fields...]    │      ┌─────────────────────┐
│                     │──1:N─│      _otps           │
└─────────────────────┘      │─────────────────────│
                              │ id (PK)             │
                              │ collection_id       │
                              │ record_id           │
                              │ password (argon2)   │
                              │ sent_to             │
                              │ created             │
                              └─────────────────────┘

                              ┌─────────────────────┐
                              │      _mfas           │
                              │─────────────────────│
                              │ id (PK)             │
                              │ collection_id       │
                              │ record_id           │
                              │ method              │
                              │ created             │
                              └─────────────────────┘
```

### Module Structure (target)

```
crates/zerobase-auth/src/
├── lib.rs                    # Re-exports, AuthService orchestrator
├── method.rs                 # AuthMethod trait definition
├── provider.rs               # AuthProvider trait definition
├── token.rs                  # TokenService trait + HmacTokenService impl
├── password.rs               # PasswordHasher trait + Argon2Hasher impl
├── methods/
│   ├── mod.rs
│   ├── password_auth.rs      # PasswordAuth (AuthMethod impl)
│   └── otp_auth.rs           # OtpAuth (AuthMethod impl)
├── providers/
│   ├── mod.rs
│   ├── google.rs             # GoogleProvider (AuthProvider impl)
│   └── microsoft.rs          # MicrosoftProvider (AuthProvider impl)
└── service.rs                # AuthService — orchestrates methods, providers, tokens
```

### AuthService (Orchestrator)

```rust
/// Central auth service that dispatches to registered methods and providers.
pub struct AuthService<R: AuthRepository, T: TokenService, H: PasswordHasher> {
    repo: R,
    token_service: T,
    hasher: H,
    methods: HashMap<String, Box<dyn AuthMethod>>,
    providers: HashMap<String, Box<dyn AuthProvider>>,
}

impl<R, T, H> AuthService<R, T, H>
where
    R: AuthRepository,
    T: TokenService,
    H: PasswordHasher,
{
    pub fn register_method(&mut self, method: Box<dyn AuthMethod>) { ... }
    pub fn register_provider(&mut self, provider: Box<dyn AuthProvider>) { ... }

    pub async fn authenticate_with_password(&self, collection: &str, identity: &str, password: &str) -> Result<AuthResponse> { ... }
    pub async fn authenticate_with_otp(&self, collection: &str, otp_id: &str, code: &str) -> Result<AuthResponse> { ... }
    pub async fn authenticate_with_oauth2(&self, collection: &str, provider: &str, code: &str, state: &str, verifier: &str) -> Result<AuthResponse> { ... }

    pub fn refresh_token(&self, token: &str) -> Result<AuthResponse> { ... }
}
```

### AuthRepository Trait (DB interface for auth operations)

```rust
/// Database operations specific to authentication.
pub trait AuthRepository: Send + Sync {
    fn find_by_identity(&self, collection: &str, identity_field: &str, value: &str) -> Result<Option<HashMap<String, JsonValue>>>;
    fn find_superuser_by_email(&self, email: &str) -> Result<Option<HashMap<String, JsonValue>>>;
    fn update_token_key(&self, collection: &str, record_id: &str, new_key: &str) -> Result<()>;
    fn update_password(&self, collection: &str, record_id: &str, hash: &str) -> Result<()>;
    fn set_verified(&self, collection: &str, record_id: &str) -> Result<()>;

    // OTP
    fn create_otp(&self, collection_id: &str, record_id: &str, password_hash: &str, sent_to: &str) -> Result<String>;
    fn find_otp(&self, otp_id: &str) -> Result<Option<OtpRecord>>;
    fn delete_otp(&self, otp_id: &str) -> Result<()>;
    fn delete_expired_otps(&self, max_age_secs: u64) -> Result<u64>;

    // External auths
    fn find_external_auth(&self, provider: &str, provider_id: &str) -> Result<Option<ExternalAuthRecord>>;
    fn create_external_auth(&self, auth: &ExternalAuthRecord) -> Result<()>;
    fn delete_external_auth(&self, id: &str) -> Result<()>;
    fn list_external_auths(&self, collection_id: &str, record_id: &str) -> Result<Vec<ExternalAuthRecord>>;

    // MFA
    fn create_mfa(&self, collection_id: &str, record_id: &str, method: &str) -> Result<String>;
    fn find_mfa(&self, mfa_id: &str) -> Result<Option<MfaRecord>>;
    fn update_mfa_method(&self, mfa_id: &str, method: &str) -> Result<()>;
    fn delete_mfa(&self, mfa_id: &str) -> Result<()>;
}
```

---

## Summary

This architecture provides:

1. **Complete PocketBase auth parity**: Password, OTP, OAuth2, MFA, superusers, email verification, password reset.
2. **Trait-based extensibility**: New auth methods and OAuth2 providers can be added by implementing a trait — no modification of existing code.
3. **Stateless JWT auth**: No server-side sessions. `tokenKey` rotation provides instant invalidation.
4. **Defense in depth**: Argon2id passwords, HMAC-SHA256 tokens, PKCE for OAuth2, constant-time comparisons, single-use OTPs.
5. **Clean separation**: Core types in `zerobase-core`, auth logic in `zerobase-auth`, persistence in `zerobase-db`, HTTP in `zerobase-api`.
