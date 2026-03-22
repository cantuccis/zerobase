# Migration Guide: PocketBase to Zerobase

This guide helps you migrate an existing PocketBase application to Zerobase. Zerobase is designed to be API-compatible with PocketBase, making migration straightforward.

---

## Table of Contents

- [Overview](#overview)
- [Compatibility Summary](#compatibility-summary)
- [Step-by-Step Migration](#step-by-step-migration)
- [API Differences](#api-differences)
- [Configuration Mapping](#configuration-mapping)
- [Data Migration](#data-migration)
- [Authentication Migration](#authentication-migration)
- [Client SDK Compatibility](#client-sdk-compatibility)
- [Feature Parity](#feature-parity)

---

## Overview

Zerobase follows PocketBase's core design principles:

- Single-binary deployment with embedded SQLite
- Auto-generated REST API per collection
- PocketBase-compatible filter/sort syntax
- Same authentication patterns (password, OAuth2, OTP, MFA)
- Same admin dashboard concept at `/_/`
- Same realtime subscription model via SSE

The migration primarily involves:
1. Exporting your PocketBase collection schemas
2. Importing them into Zerobase
3. Migrating your data
4. Updating configuration
5. Testing your client application

---

## Compatibility Summary

| Feature | PocketBase | Zerobase | Notes |
|---------|-----------|----------|-------|
| Collection CRUD API | `/api/collections/*/records` | `/api/collections/*/records` | Same URL structure |
| Filter syntax | `(field='value')` | `(field='value')` | Compatible syntax |
| Sort syntax | `-created,title` | `-created,title` | Identical |
| Expand relations | `?expand=field` | `?expand=field` | Identical |
| Partial fields | `?fields=id,name` | `?fields=id,name` | Identical |
| Pagination | `?page=1&perPage=30` | `?page=1&perPage=30` | Identical |
| Auth endpoints | `/api/collections/*/auth-with-password` | `/api/collections/*/auth-with-password` | Same paths |
| SSE realtime | `GET /api/realtime` | `GET /api/realtime` | Same protocol |
| File serving | `/api/files/:col/:rec/:file` | `/api/files/:col/:rec/:file` | Same paths |
| Thumbnails | `?thumb=WxH` | `?thumb=WxH` | Same format |
| Admin panel | `/_/` | `/_/` | Same path |
| Config file | None (flags/env) | `zerobase.toml` | Different config system |
| Language | Go | Rust | Different runtime |
| Extensibility | Go hooks / JS hooks | Rust (trait-based) | Different extension model |

---

## Step-by-Step Migration

### Step 1: Install Zerobase

Download the Zerobase binary for your platform from the releases page, or build from source:

```bash
# From pre-built binary
curl -LO https://github.com/<org>/zerobase/releases/latest/download/zerobase-linux-amd64.tar.gz
tar xzf zerobase-linux-amd64.tar.gz

# Or build from source
git clone <repo-url> && cd zerobase
cd frontend && pnpm install && pnpm build && cd ..
cargo build --release
```

### Step 2: Export PocketBase Schemas

From your running PocketBase instance, export all collection schemas:

```bash
# Via the PocketBase API
curl http://localhost:8090/api/collections \
  -H "Authorization: Bearer $PB_ADMIN_TOKEN" \
  > pb_collections.json
```

Or use the PocketBase admin panel: **Settings > Export collections**.

### Step 3: Configure Zerobase

Create a `zerobase.toml` configuration file:

```toml
[server]
host = "127.0.0.1"
port = 8090

[database]
path = "zerobase_data/data.db"

[auth]
token_secret = "GENERATE_WITH_openssl_rand_hex_32"

[storage]
backend = "local"
local_path = "zerobase_data/storage"

[smtp]
enabled = true
host = "your-smtp-host"
port = 587
username = "your-username"
password = "your-password"
sender_address = "noreply@yourapp.com"
```

### Step 4: Start Zerobase and Create Superuser

```bash
# Run migrations (automatic on first start)
./zerobase migrate

# Create your admin account
./zerobase superuser create --email admin@example.com --password YourSecurePassword

# Start the server
./zerobase serve
```

### Step 5: Import Collection Schemas

Use the Zerobase admin panel or API to import your PocketBase collection schemas:

```bash
# Via API (superuser auth required)
ADMIN_TOKEN=$(curl -s -X POST http://localhost:8090/_/api/admins/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity":"admin@example.com","password":"YourSecurePassword"}' | jq -r '.token')

curl -X PUT http://localhost:8090/api/collections/import \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d @pb_collections.json
```

### Step 6: Migrate Data

See the [Data Migration](#data-migration) section below for strategies to move your records.

### Step 7: Migrate Files

Copy your PocketBase storage directory to the Zerobase storage directory:

```bash
cp -r pb_data/storage/* zerobase_data/storage/
```

File paths follow the same `<collection_id>/<record_id>/<filename>` structure.

### Step 8: Test Your Application

Point your client application to the Zerobase instance and run your test suite. The API should be compatible with PocketBase client SDKs.

---

## API Differences

### Identical Endpoints

These endpoints work identically between PocketBase and Zerobase:

- Record CRUD (`/api/collections/:collection/records`)
- Auth with password (`auth-with-password`)
- Auth refresh (`auth-refresh`)
- Email verification (`request-verification`, `confirm-verification`)
- Password reset (`request-password-reset`, `confirm-password-reset`)
- Email change (`request-email-change`, `confirm-email-change`)
- OTP (`request-otp`, `auth-with-otp`)
- MFA (`request-mfa-setup`, `confirm-mfa`, `auth-with-mfa`)
- Files (`/api/files/token`, `/api/files/:col/:rec/:file`)
- Realtime SSE (`/api/realtime`)
- Batch operations (`/api/batch`)

### Minor Differences

| Feature | PocketBase | Zerobase |
|---------|-----------|----------|
| Admin auth path | `POST /api/admins/auth-with-password` | `POST /_/api/admins/auth-with-password` |
| Config file | None | `zerobase.toml` |
| CLI | `pocketbase serve` | `zerobase serve` |
| Admin create | `pocketbase admin create` | `zerobase superuser create` |

### Unsupported PocketBase Features

The following PocketBase features have different implementations in Zerobase:

| PocketBase Feature | Zerobase Alternative |
|-------------------|---------------------|
| Go hooks (`OnRecordCreate`, etc.) | Rust trait-based hooks |
| JS hooks (`pb_hooks/*.pb.js`) | Not supported (use Rust) |
| `pb_migrations/` directory | System migrations in Rust |
| `--automigrate` flag | Not needed (auto-migration on startup) |

---

## Configuration Mapping

### PocketBase CLI Flags to Zerobase

| PocketBase | Zerobase CLI | Zerobase Config |
|-----------|-------------|----------------|
| `--http 0.0.0.0:8090` | `--host 0.0.0.0 --port 8090` | `server.host`, `server.port` |
| `--dir pb_data` | `--data-dir zerobase_data` | `database.path` |
| `--publicDir pb_public` | — | Static files are embedded |
| `--encryptionEnv PB_ENCRYPTION_KEY` | — | Not needed |

### PocketBase Environment Variables to Zerobase

| PocketBase Env | Zerobase Env |
|---------------|-------------|
| — | `ZEROBASE__AUTH__TOKEN_SECRET` (required) |
| — | `ZEROBASE__SERVER__HOST` |
| — | `ZEROBASE__SERVER__PORT` |
| — | `ZEROBASE__DATABASE__PATH` |
| — | `ZEROBASE__STORAGE__BACKEND` |

### SMTP Settings

PocketBase configures SMTP through the admin panel. Zerobase supports both config file and admin panel:

```toml
# zerobase.toml
[smtp]
enabled = true
host = "smtp.example.com"
port = 587
username = "user"
password = "pass"
sender_address = "noreply@example.com"
sender_name = "My App"
tls = true
```

---

## Data Migration

### Strategy 1: API-Based Migration (Recommended)

Export records from PocketBase and import into Zerobase via the API:

```bash
#!/bin/bash
# migrate_data.sh

PB_URL="http://localhost:8090"  # PocketBase
ZB_URL="http://localhost:8091"  # Zerobase

PB_TOKEN="your_pb_admin_token"
ZB_TOKEN="your_zb_admin_token"

# Get all collections
collections=$(curl -s "$PB_URL/api/collections" \
  -H "Authorization: Bearer $PB_TOKEN" | jq -r '.items[].name')

for collection in $collections; do
  echo "Migrating collection: $collection"

  page=1
  while true; do
    response=$(curl -s "$PB_URL/api/collections/$collection/records?page=$page&perPage=200" \
      -H "Authorization: Bearer $PB_TOKEN")

    items=$(echo "$response" | jq '.items')
    count=$(echo "$items" | jq 'length')

    if [ "$count" -eq 0 ]; then
      break
    fi

    # Import each record
    echo "$items" | jq -c '.[]' | while read -r record; do
      curl -s -X POST "$ZB_URL/api/collections/$collection/records" \
        -H "Authorization: Bearer $ZB_TOKEN" \
        -H "Content-Type: application/json" \
        -d "$record" > /dev/null
    done

    echo "  Migrated page $page ($count records)"
    page=$((page + 1))
  done
done
```

### Strategy 2: SQLite Direct Copy

Since both PocketBase and Zerobase use SQLite, you can directly copy user-created tables after setting up the Zerobase schema:

1. Set up Zerobase and import collection schemas (Step 5 above)
2. Stop both servers
3. Use `sqlite3` to copy data:

```bash
sqlite3 zerobase_data/data.db "ATTACH 'pb_data/data.db' AS pb;"
sqlite3 zerobase_data/data.db "INSERT INTO posts SELECT * FROM pb.posts;"
# Repeat for each collection
```

> **Warning:** Direct SQLite copy requires that table schemas match exactly. Verify column names and types before copying.

---

## Authentication Migration

### Password Hashes

Both PocketBase and Zerobase use bcrypt/Argon2 for password hashing. If migrating via API with the admin token, the password hashes are preserved in the record data.

### OAuth2 Identities

External auth identities (Google, GitHub, etc.) are stored in the `_external_auths` table. These need to be migrated along with the user records.

### JWT Tokens

Existing PocketBase JWT tokens will **not** work with Zerobase since the signing secrets are different. Users will need to re-authenticate after migration.

### Superuser Accounts

PocketBase admin accounts don't migrate automatically. Create new superuser accounts in Zerobase:

```bash
zerobase superuser create --email admin@example.com --password SecurePassword123
```

---

## Client SDK Compatibility

Zerobase aims for API compatibility with PocketBase client SDKs. The following SDKs should work with minimal or no changes:

- **PocketBase JavaScript SDK** — Compatible (same API structure)
- **PocketBase Dart SDK** — Compatible
- **Custom REST clients** — Compatible if using standard PocketBase API patterns

### Required Client Changes

1. **Base URL:** Update the PocketBase URL to your Zerobase URL
2. **Admin authentication:** Change admin auth endpoint if using direct admin API calls

```javascript
// Before (PocketBase)
const pb = new PocketBase('http://localhost:8090');

// After (Zerobase) — same URL structure
const pb = new PocketBase('http://localhost:8090');
// Works out of the box!
```

---

## Feature Parity

### Supported PocketBase Features

- Collections (base, auth, view types)
- All field types (text, number, bool, date, email, url, select, file, relation, json, editor, autodate)
- Access rules with filter expressions
- Email/password authentication
- OAuth2 authentication
- OTP (One-Time Password)
- MFA (TOTP)
- Passkeys/WebAuthn
- Email verification
- Password reset
- Email change
- File upload/download
- Thumbnail generation
- Relation expansion
- Full-text search
- Realtime SSE subscriptions
- Batch operations
- Backup/restore
- Settings management
- Request logging

### Zerobase Advantages over PocketBase

- **Rust performance** — Lower memory usage, faster response times
- **Stronger type safety** — Compile-time guarantees for schema operations
- **Trait-based extensibility** — Add auth methods, storage backends without modifying core code
- **Modular architecture** — 8 independent crates with clear boundaries
- **Cross-platform binaries** — Pre-built for Linux, macOS, and Windows (x86_64 and ARM64)

### Not Yet Supported

- Go framework mode (custom Go routes/hooks)
- JavaScript hooks (`pb_hooks/*.pb.js`)
- Some PocketBase-specific admin panel features

---

## Rollback Plan

If you need to rollback to PocketBase:

1. Stop Zerobase
2. If data was modified in Zerobase, export via the API and re-import into PocketBase
3. Restart PocketBase with the original `pb_data` directory

It's recommended to keep your PocketBase data directory intact until you've fully validated the migration.
