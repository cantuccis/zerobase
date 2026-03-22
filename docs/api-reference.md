# Zerobase API Reference

> Complete reference for all Zerobase REST API endpoints.

All endpoints return JSON. Authentication uses `Authorization: Bearer <token>` headers. Superuser-only endpoints require a superuser JWT.

---

## Table of Contents

- [Health](#health)
- [Records](#records)
- [Collections](#collections)
- [Authentication](#authentication)
- [Email Verification](#email-verification)
- [Password Reset](#password-reset)
- [Email Change](#email-change)
- [OTP (One-Time Password)](#otp-one-time-password)
- [MFA (Multi-Factor Authentication)](#mfa-multi-factor-authentication)
- [Passkeys (WebAuthn)](#passkeys-webauthn)
- [OAuth2](#oauth2)
- [External Auth Identities](#external-auth-identities)
- [Files](#files)
- [Batch Operations](#batch-operations)
- [Realtime (SSE)](#realtime-sse)
- [Admin Authentication](#admin-authentication)
- [Settings](#settings)
- [Backups](#backups)
- [Logs](#logs)
- [Admin Dashboard](#admin-dashboard)
- [Error Responses](#error-responses)
- [Query Parameters](#query-parameters)

---

## Health

### Health Check

```
GET /api/health
```

Returns server status and version information.

**Response** `200 OK`
```json
{
  "status": "ok"
}
```

---

## Records

All record endpoints operate on a specific collection by name.

### List Records

```
GET /api/collections/:collection/records
```

Returns a paginated list of records from the specified collection.

**Query Parameters:**

| Parameter  | Type   | Default | Description |
|------------|--------|---------|-------------|
| `page`     | int    | 1       | Page number |
| `perPage`  | int    | 30      | Items per page (max 500) |
| `sort`     | string | —       | Comma-separated sort fields. Prefix with `-` for descending |
| `filter`   | string | —       | PocketBase-compatible filter expression |
| `expand`   | string | —       | Comma-separated relation fields to expand |
| `fields`   | string | —       | Comma-separated fields to return (partial response) |
| `search`   | string | —       | Full-text search query |

**Response** `200 OK`
```json
{
  "page": 1,
  "perPage": 30,
  "totalPages": 1,
  "totalItems": 2,
  "items": [
    {
      "id": "abc123def456789",
      "collectionId": "col_id",
      "collectionName": "posts",
      "created": "2025-01-15T10:30:00Z",
      "updated": "2025-01-15T10:30:00Z",
      "title": "Hello World",
      "content": "..."
    }
  ]
}
```

### View Record

```
GET /api/collections/:collection/records/:id
```

Returns a single record by ID.

**Query Parameters:**

| Parameter | Type   | Description |
|-----------|--------|-------------|
| `expand`  | string | Comma-separated relation fields to expand |
| `fields`  | string | Comma-separated fields to return |

**Response** `200 OK`
```json
{
  "id": "abc123def456789",
  "collectionId": "col_id",
  "collectionName": "posts",
  "created": "2025-01-15T10:30:00Z",
  "updated": "2025-01-15T10:30:00Z",
  "title": "Hello World",
  "content": "..."
}
```

### Create Record

```
POST /api/collections/:collection/records
```

Creates a new record. Supports both JSON and multipart/form-data (for file uploads).

**Request Body (JSON)**
```json
{
  "title": "Hello World",
  "content": "This is my first post."
}
```

**Request Body (multipart/form-data)** — Use for file uploads.

**Response** `200 OK` — Returns the created record.

### Update Record

```
PATCH /api/collections/:collection/records/:id
```

Updates an existing record. Only fields present in the body are modified.

**Request Body (JSON)**
```json
{
  "title": "Updated Title"
}
```

**Response** `200 OK` — Returns the updated record.

### Delete Record

```
DELETE /api/collections/:collection/records/:id
```

Deletes a record by ID.

**Response** `204 No Content`

### Count Records

```
GET /api/collections/:collection/records/count
```

Returns the total count of records matching an optional filter.

**Query Parameters:**

| Parameter | Type   | Description |
|-----------|--------|-------------|
| `filter`  | string | PocketBase-compatible filter expression |

**Response** `200 OK`
```json
{
  "totalItems": 42
}
```

---

## Collections

> Superuser-only endpoints for managing collection schemas.

### List Collections

```
GET /api/collections
```

**Response** `200 OK` — Returns all collections with their fields and rules.

### Create Collection

```
POST /api/collections
```

**Request Body**
```json
{
  "name": "posts",
  "type": "base",
  "fields": [
    {
      "name": "title",
      "type": "text",
      "required": true,
      "options": {
        "min": 1,
        "max": 200
      }
    },
    {
      "name": "content",
      "type": "editor"
    },
    {
      "name": "published",
      "type": "bool"
    }
  ],
  "listRule": "",
  "viewRule": "",
  "createRule": "@request.auth.id != ''",
  "updateRule": "@request.auth.id = id",
  "deleteRule": "@request.auth.id = id"
}
```

**Collection Types:**
- `base` — General-purpose data collection
- `auth` — Authentication-enabled collection with built-in user fields
- `view` — Read-only SQL view collection (requires `viewQuery`)

**Field Types:**

| Type | Description | Key Options |
|------|-------------|-------------|
| `text` | Plain text | `min`, `max`, `pattern` |
| `editor` | Rich text (HTML) | `min`, `max` |
| `number` | Numeric value | `min`, `max` |
| `bool` | Boolean | — |
| `email` | Email address | — |
| `url` | URL | — |
| `datetime` | ISO 8601 datetime | — |
| `select` | Single select | `values` (array of options) |
| `multiselect` | Multi select | `values`, `maxSelect` |
| `file` | File attachment | `maxSize`, `maxSelect`, `mimeTypes` |
| `relation` | Relation to another collection | `collectionId`, `cascadeDelete` |
| `json` | Arbitrary JSON | `maxSize` |
| `autodate` | Auto-set datetime | `onCreate`, `onUpdate` |

**Response** `200 OK` — Returns the created collection.

### View Collection

```
GET /api/collections/:id_or_name
```

**Response** `200 OK` — Returns the collection schema.

### Update Collection

```
PATCH /api/collections/:id_or_name
```

**Request Body** — Same structure as create. Only provided fields are updated.

**Response** `200 OK` — Returns the updated collection.

### Delete Collection

```
DELETE /api/collections/:id_or_name
```

**Response** `204 No Content`

### Export Collections

```
GET /api/collections/export
```

Exports all collection schemas as a JSON array.

**Response** `200 OK`
```json
[
  { "name": "posts", "type": "base", "fields": [...], ... },
  { "name": "users", "type": "auth", "fields": [...], ... }
]
```

### Import Collections

```
PUT /api/collections/import
```

Replaces all collections with the provided schemas. Use with caution.

**Request Body** — Array of collection definitions.

### List Indexes

```
GET /api/collections/:id_or_name/indexes
```

**Response** `200 OK` — Returns array of index definitions.

### Add Index

```
POST /api/collections/:id_or_name/indexes
```

**Request Body**
```json
{
  "columns": ["title"],
  "unique": false
}
```

### Remove Index

```
DELETE /api/collections/:id_or_name/indexes/:position
```

**Response** `204 No Content`

---

## Authentication

Authentication endpoints operate per auth-type collection (e.g., `users`).

### Login with Email/Password

```
POST /api/collections/:collection/auth-with-password
```

**Request Body**
```json
{
  "identity": "user@example.com",
  "password": "securepassword"
}
```

**Response** `200 OK`
```json
{
  "token": "eyJhbGciOiJIUzI1NiIs...",
  "record": {
    "id": "abc123def456789",
    "email": "user@example.com",
    "verified": true,
    "created": "2025-01-15T10:30:00Z",
    "updated": "2025-01-15T10:30:00Z"
  }
}
```

### Refresh Token

```
POST /api/collections/:collection/auth-refresh
```

Requires a valid `Authorization: Bearer <token>` header.

**Response** `200 OK`
```json
{
  "token": "eyJhbGciOiJIUzI1NiIs...",
  "record": { ... }
}
```

### List Auth Methods

```
GET /api/collections/:collection/auth-methods
```

Returns enabled authentication methods for the collection.

**Response** `200 OK`
```json
{
  "emailPassword": true,
  "authProviders": [
    {
      "name": "google",
      "displayName": "Google",
      "state": "...",
      "codeVerifier": "...",
      "codeChallenge": "...",
      "codeChallengeMethod": "S256",
      "authUrl": "https://accounts.google.com/o/oauth2/..."
    }
  ]
}
```

---

## Email Verification

### Request Verification Email

```
POST /api/collections/:collection/request-verification
```

**Request Body**
```json
{
  "email": "user@example.com"
}
```

**Response** `204 No Content`

### Confirm Verification

```
POST /api/collections/:collection/confirm-verification
```

**Request Body**
```json
{
  "token": "verification_token_from_email"
}
```

**Response** `204 No Content`

---

## Password Reset

### Request Password Reset

```
POST /api/collections/:collection/request-password-reset
```

**Request Body**
```json
{
  "email": "user@example.com"
}
```

**Response** `204 No Content`

### Confirm Password Reset

```
POST /api/collections/:collection/confirm-password-reset
```

**Request Body**
```json
{
  "token": "reset_token_from_email",
  "password": "newSecurePassword",
  "passwordConfirm": "newSecurePassword"
}
```

**Response** `204 No Content`

---

## Email Change

### Request Email Change

```
POST /api/collections/:collection/request-email-change
```

Requires authentication.

**Request Body**
```json
{
  "newEmail": "newemail@example.com"
}
```

**Response** `204 No Content`

### Confirm Email Change

```
POST /api/collections/:collection/confirm-email-change
```

**Request Body**
```json
{
  "token": "email_change_token"
}
```

**Response** `204 No Content`

---

## OTP (One-Time Password)

### Request OTP

```
POST /api/collections/:collection/request-otp
```

Sends a one-time password code via email.

**Request Body**
```json
{
  "email": "user@example.com"
}
```

**Response** `200 OK`
```json
{
  "otpId": "otp_reference_id"
}
```

### Authenticate with OTP

```
POST /api/collections/:collection/auth-with-otp
```

**Request Body**
```json
{
  "otpId": "otp_reference_id",
  "code": "123456"
}
```

**Response** `200 OK`
```json
{
  "token": "eyJhbGciOiJIUzI1NiIs...",
  "record": { ... }
}
```

---

## MFA (Multi-Factor Authentication)

### Request MFA Setup

```
POST /api/collections/:collection/records/:id/request-mfa-setup
```

Generates a TOTP secret and QR code for authenticator app setup. Requires authentication.

**Response** `200 OK`
```json
{
  "mfaId": "mfa_setup_id",
  "secret": "JBSWY3DPEHPK3PXP",
  "qrCode": "data:image/png;base64,..."
}
```

### Confirm MFA Setup

```
POST /api/collections/:collection/records/:id/confirm-mfa
```

Verifies the TOTP code and enables MFA for the user.

**Request Body**
```json
{
  "mfaId": "mfa_setup_id",
  "code": "123456"
}
```

**Response** `204 No Content`

### Authenticate with MFA

```
POST /api/collections/:collection/auth-with-mfa
```

Used when login returns a partial MFA token instead of a full auth token.

**Request Body**
```json
{
  "mfaToken": "partial_mfa_token",
  "code": "123456"
}
```

**Response** `200 OK`
```json
{
  "token": "eyJhbGciOiJIUzI1NiIs...",
  "record": { ... }
}
```

---

## Passkeys (WebAuthn)

### Begin Passkey Registration

```
POST /api/collections/:collection/request-passkey-register
```

Requires authentication. Returns WebAuthn creation options.

**Response** `200 OK`
```json
{
  "options": { ... }
}
```

### Complete Passkey Registration

```
POST /api/collections/:collection/confirm-passkey-register
```

**Request Body** — WebAuthn `AuthenticatorAttestationResponse` from the browser.

**Response** `204 No Content`

### Begin Passkey Authentication

```
POST /api/collections/:collection/auth-with-passkey-begin
```

Returns WebAuthn request options.

**Response** `200 OK`
```json
{
  "options": { ... }
}
```

### Complete Passkey Authentication

```
POST /api/collections/:collection/auth-with-passkey-finish
```

**Request Body** — WebAuthn `AuthenticatorAssertionResponse` from the browser.

**Response** `200 OK`
```json
{
  "token": "eyJhbGciOiJIUzI1NiIs...",
  "record": { ... }
}
```

---

## OAuth2

### Authenticate with OAuth2

```
POST /api/collections/:collection/auth-with-oauth2
```

Completes the OAuth2 flow after the user has been redirected back from the provider.

**Request Body**
```json
{
  "provider": "google",
  "code": "authorization_code",
  "codeVerifier": "pkce_code_verifier",
  "redirectUrl": "https://yourapp.com/callback"
}
```

**Response** `200 OK`
```json
{
  "token": "eyJhbGciOiJIUzI1NiIs...",
  "record": { ... },
  "meta": {
    "isNew": true
  }
}
```

**Built-in Providers:** Google, GitHub (extensible via provider registry)

---

## External Auth Identities

### List Linked Identities

```
GET /api/collections/:collection/records/:id/external-auths
```

Returns OAuth2 identities linked to a user record.

**Response** `200 OK`
```json
[
  {
    "id": "ext_auth_id",
    "provider": "google",
    "providerId": "google_user_id",
    "created": "2025-01-15T10:30:00Z"
  }
]
```

### Unlink Identity

```
DELETE /api/collections/:collection/records/:id/external-auths/:provider
```

**Response** `204 No Content`

---

## Files

### Generate File Token

```
GET /api/files/token
```

Requires authentication. Returns a short-lived token (2 minutes) for accessing protected files.

**Response** `200 OK`
```json
{
  "token": "file_access_token"
}
```

### Download File

```
GET /api/files/:collectionId/:recordId/:filename
```

Serves the file. For protected files, pass `?token=<file_token>` query parameter.

**Thumbnail Support:** Append `?thumb=WxH` to generate/serve thumbnails.

| Format | Description |
|--------|-------------|
| `100x100` | Crop to center |
| `100x100t` | Crop to top |
| `100x100b` | Crop to bottom |
| `100x100f` | Fit within bounds |

**Example:** `GET /api/files/col_id/rec_id/image.jpg?thumb=200x200f`

---

## Batch Operations

### Execute Batch

```
POST /api/batch
```

Executes multiple record operations atomically.

**Request Body**
```json
{
  "requests": [
    {
      "method": "POST",
      "url": "/api/collections/posts/records",
      "body": { "title": "First Post" }
    },
    {
      "method": "PATCH",
      "url": "/api/collections/posts/records/abc123",
      "body": { "title": "Updated Title" }
    },
    {
      "method": "DELETE",
      "url": "/api/collections/posts/records/def456"
    }
  ]
}
```

**Response** `200 OK` — Array of individual response objects.

---

## Realtime (SSE)

### Establish SSE Connection

```
GET /api/realtime
```

Opens a Server-Sent Events connection. The server immediately sends a `PB_CONNECT` event with a client ID.

**SSE Events:**

```
event: PB_CONNECT
data: {"clientId":"abc123"}

event: posts
data: {"action":"create","record":{...}}
```

### Set Subscriptions

```
POST /api/realtime
```

Sets the topics the SSE client is subscribed to. Replaces any previous subscriptions.

**Request Body**
```json
{
  "clientId": "abc123",
  "subscriptions": ["posts", "comments/rec_id"]
}
```

**Subscription Formats:**
- `"collection"` — All record changes in the collection
- `"collection/recordId"` — Changes to a specific record

**Keep-alive:** The server sends `: ping` comments every 30 seconds to keep the connection alive.

---

## Admin Authentication

### Superuser Login

```
POST /_/api/admins/auth-with-password
```

**Request Body**
```json
{
  "identity": "admin@example.com",
  "password": "adminpassword"
}
```

**Response** `200 OK`
```json
{
  "token": "eyJhbGciOiJIUzI1NiIs...",
  "admin": {
    "id": "superuser_id",
    "email": "admin@example.com"
  }
}
```

---

## Settings

> Superuser-only endpoints.

### Get All Settings

```
GET /api/settings
```

**Response** `200 OK` — Returns all configurable settings.

### Update Settings

```
PATCH /api/settings
```

**Request Body** — Partial settings object.
```json
{
  "meta": {
    "appName": "My App"
  }
}
```

**Response** `200 OK`

### Get Single Setting

```
GET /api/settings/:key
```

### Reset Setting to Default

```
DELETE /api/settings/:key
```

### Send Test Email

```
POST /api/settings/test-email
```

Sends a test email to verify SMTP configuration.

**Request Body**
```json
{
  "email": "test@example.com",
  "template": "verification"
}
```

---

## Backups

> Superuser-only endpoints.

### Create Backup

```
POST /_/api/backups
```

Creates a new SQLite database backup.

**Response** `200 OK`
```json
{
  "key": "backup_20250115_103000.db"
}
```

### List Backups

```
GET /_/api/backups
```

**Response** `200 OK`
```json
[
  {
    "key": "backup_20250115_103000.db",
    "size": 1048576,
    "modified": "2025-01-15T10:30:00Z"
  }
]
```

### Download Backup

```
GET /_/api/backups/:name
```

Returns the backup file as a download.

### Delete Backup

```
DELETE /_/api/backups/:name
```

**Response** `204 No Content`

### Restore from Backup

```
POST /_/api/backups/:name/restore
```

Restores the database from a backup file. **This replaces the current database.**

**Response** `204 No Content`

---

## Logs

> Superuser-only endpoints.

### List Logs

```
GET /_/api/logs
```

**Query Parameters:**

| Parameter | Type   | Description |
|-----------|--------|-------------|
| `page`    | int    | Page number |
| `perPage` | int    | Items per page |
| `filter`  | string | Filter expression |
| `sort`    | string | Sort fields |

**Response** `200 OK` — Paginated list of request logs.

### Log Statistics

```
GET /_/api/logs/stats
```

Returns aggregate statistics about request logs.

### View Log Entry

```
GET /_/api/logs/:id
```

---

## Admin Dashboard

```
GET /_/
```

Serves the embedded Astro admin dashboard SPA. All paths under `/_/` are handled by the SPA router.

---

## Error Responses

All errors follow a consistent JSON format:

```json
{
  "code": 400,
  "message": "Validation failed.",
  "data": {
    "title": {
      "code": "validation_required",
      "message": "Missing required value."
    }
  }
}
```

**HTTP Status Codes:**

| Code | Meaning |
|------|---------|
| 200  | Success |
| 204  | Success (no body) |
| 400  | Validation error / bad request |
| 401  | Authentication required or failed |
| 403  | Permission denied |
| 404  | Resource not found |
| 409  | Conflict (uniqueness violation) |
| 413  | Payload too large |
| 429  | Rate limit exceeded |
| 500  | Internal server error |

---

## Query Parameters

### Filter Syntax

Zerobase uses PocketBase-compatible filter expressions. Filters are passed as the `filter` query parameter.

**Operators:**

| Operator | Description |
|----------|-------------|
| `=`      | Equal |
| `!=`     | Not equal |
| `>`      | Greater than |
| `>=`     | Greater than or equal |
| `<`      | Less than |
| `<=`     | Less than or equal |
| `~`      | Like (contains) |
| `!~`     | Not like |

**Logical Operators:**

| Operator | Description |
|----------|-------------|
| `&&`     | AND |
| `\|\|`  | OR |

**Examples:**
```
?filter=(title='Hello' && published=true)
?filter=(created>'2025-01-01' || featured=true)
?filter=(status='active' && role!='admin')
?filter=(name~'john')
```

**Special Variables (in access rules):**

| Variable | Description |
|----------|-------------|
| `@request.auth.id` | Authenticated user's record ID |
| `@request.auth.*` | Any field on the authenticated user |
| `@request.data.*` | Incoming request body fields |
| `@request.query.*` | URL query parameters |
| `@request.headers.*` | Request headers |
| `@request.method` | HTTP method |

### Sort Syntax

Comma-separated field names. Prefix with `-` for descending order.

```
?sort=-created,title
?sort=name
```

### Expand Syntax

Comma-separated relation field names to auto-expand with full records.

```
?expand=author
?expand=author,comments
```

### Fields Syntax

Comma-separated field names for partial responses.

```
?fields=id,title,created
?fields=*,expand.author.name
```

---

## Access Rules

Each collection has per-operation access rules:

| Rule | Applied to |
|------|-----------|
| `listRule` | `GET .../records` |
| `viewRule` | `GET .../records/:id` |
| `createRule` | `POST .../records` |
| `updateRule` | `PATCH .../records/:id` |
| `deleteRule` | `DELETE .../records/:id` |
| `manageRule` | Full CRUD access for delegated administration |

**Rule Values:**
- **`null`** — Locked (superusers only)
- **`""`** — Open to everyone (including guests)
- **`"expression"`** — Conditional access using filter syntax

**Examples:**
```json
{
  "listRule": "",
  "viewRule": "",
  "createRule": "@request.auth.id != ''",
  "updateRule": "@request.auth.id = user",
  "deleteRule": null,
  "manageRule": "@request.auth.role = 'admin'"
}
```

---

## JWT Token Structure

Zerobase JWTs contain the following claims:

| Claim | Description |
|-------|-------------|
| `id` | User record ID |
| `collectionId` | Collection name (e.g., `users`, `_superusers`) |
| `type` | Token type (`auth`, `refresh`, `file`, `verification`, etc.) |
| `tokenKey` | Per-user invalidation key |
| `iat` | Issued at (Unix timestamp) |
| `exp` | Expiry (Unix timestamp) |

**Default token duration:** 14 days (configurable via `auth.token_duration_secs`).

Tokens are signed with HMAC-SHA256 using the `auth.token_secret` configuration value.
