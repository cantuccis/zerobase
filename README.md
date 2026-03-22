# Zerobase

A single-binary Backend-as-a-Service (BaaS) built with Rust and inspired by [PocketBase](https://pocketbase.io). Zerobase bundles an embedded SQLite database, auto-generated REST API, built-in authentication, realtime subscriptions, file storage, and an admin dashboard into one self-contained executable.

## Features

- **Embedded SQLite** with auto-generated CRUD REST API
- **Authentication** — email/password, OAuth2, OTP, MFA (TOTP), and passkeys (WebAuthn)
- **Realtime subscriptions** via Server-Sent Events (SSE)
- **File storage** — local filesystem or S3-compatible backends with thumbnail generation
- **Access rules** — per-collection row-level permissions with PocketBase-compatible filter syntax
- **Batch operations** — execute multiple record operations atomically
- **Admin dashboard** — Astro + React SPA embedded in the binary
- **Backup/restore** — database backup management via API and CLI
- **PocketBase-compatible API** shape, filter/sort syntax, and conventions

## Quick Start

### 1. Install

**From source:**
```bash
git clone <repo-url> && cd zerobase
cd frontend && pnpm install && pnpm build && cd ..
cargo build --release
```

**From pre-built binaries:** Download from the [Releases](../../releases) page.

### 2. Configure

Generate a token secret and create a minimal config:

```bash
cat > zerobase.toml << EOF
[auth]
token_secret = "$(openssl rand -hex 32)"
EOF
```

Or set via environment variable:
```bash
export ZEROBASE__AUTH__TOKEN_SECRET="$(openssl rand -hex 32)"
```

### 3. Create a Superuser

```bash
./zerobase superuser create --email admin@example.com --password YourSecurePassword
```

### 4. Start the Server

```bash
./zerobase serve
```

The API is available at `http://localhost:8090` and the admin dashboard at `http://localhost:8090/_/`.

### 5. Create Your First Collection

Login to the admin dashboard or use the API:

```bash
# Authenticate as superuser
TOKEN=$(curl -s -X POST http://localhost:8090/_/api/admins/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity":"admin@example.com","password":"YourSecurePassword"}' | jq -r '.token')

# Create a "posts" collection
curl -X POST http://localhost:8090/api/collections \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "posts",
    "type": "base",
    "fields": [
      {"name": "title", "type": "text", "required": true},
      {"name": "content", "type": "editor"},
      {"name": "published", "type": "bool"}
    ],
    "listRule": "",
    "viewRule": "",
    "createRule": "@request.auth.id != '\'''\''",
    "updateRule": "@request.auth.id != '\'''\''",
    "deleteRule": null
  }'
```

### 6. Use the Auto-Generated API

```bash
# Create a record
curl -X POST http://localhost:8090/api/collections/posts/records \
  -H "Content-Type: application/json" \
  -d '{"title": "Hello World", "content": "My first post!", "published": true}'

# List records with filtering
curl "http://localhost:8090/api/collections/posts/records?filter=(published=true)&sort=-created"

# View a single record
curl http://localhost:8090/api/collections/posts/records/RECORD_ID
```

## Tech Stack

**Backend (Rust):** Axum, rusqlite (bundled SQLite), jsonwebtoken, Argon2id, webauthn-rs, tracing, lettre, rust-s3

**Frontend (Admin UI):** Astro 6, React 19, Tailwind CSS 4, TypeScript

## Project Structure

```
zerobase/
├── crates/
│   ├── zerobase-core/       # Domain logic, schemas, validation, services, hooks
│   ├── zerobase-db/         # SQLite, migrations, repositories, query builder
│   ├── zerobase-auth/       # Password hashing, JWT, OAuth2, OTP, MFA, passkeys
│   ├── zerobase-api/        # Axum HTTP handlers, middleware, SSE realtime
│   ├── zerobase-files/      # Local/S3 file storage, thumbnails
│   ├── zerobase-admin/      # Embedded admin dashboard serving
│   ├── zerobase-hooks/      # Hook system for extensibility
│   └── zerobase-server/     # Binary entry point, CLI, composition root
├── frontend/                # Astro admin dashboard
├── docs/                    # Documentation
│   ├── architecture.md      # Architecture overview
│   ├── api-reference.md     # Complete API reference
│   ├── configuration-reference.md  # All configuration options
│   ├── deployment-guide.md  # Docker, systemd, bare metal deployment
│   └── migration-from-pocketbase.md  # PocketBase migration guide
└── .github/workflows/       # CI pipeline
```

## Prerequisites

- **Rust** stable (managed via `rust-toolchain.toml`)
- **Node.js** >= 22.12 and **pnpm** (for the admin frontend)

## Configuration

Zerobase reads configuration from (in order of precedence):

1. Built-in defaults
2. `zerobase.toml` in the working directory (or path in `ZEROBASE_CONFIG`)
3. Environment variables with `ZEROBASE__` prefix (`__` for nesting)
4. CLI flags

The only required setting is `auth.token_secret` (or `ZEROBASE__AUTH__TOKEN_SECRET`).

| Setting | Default | Description |
|---|---|---|
| `server.host` | `127.0.0.1` | Bind address |
| `server.port` | `8090` | Listen port |
| `server.log_format` | `json` | Log format (`json` or `pretty`) |
| `database.path` | `zerobase_data/data.db` | SQLite database path |
| `storage.backend` | `local` | File storage backend (`local` or `s3`) |
| `storage.local_path` | `zerobase_data/storage` | Local storage directory |
| `auth.token_secret` | — | **Required.** JWT signing secret |
| `auth.token_duration_secs` | `1209600` | Token validity (14 days) |
| `smtp.enabled` | `false` | Enable email sending |

See [docs/configuration-reference.md](docs/configuration-reference.md) for the full reference.

## CLI Reference

```bash
zerobase serve [OPTIONS]          # Start the server
  --host <HOST>                   # Bind address
  --port, -p <PORT>               # Listen port
  --data-dir <PATH>               # Data directory
  --log-format <FORMAT>           # json or pretty

zerobase migrate [OPTIONS]        # Run database migrations
  --data-dir <PATH>               # Data directory

zerobase superuser create         # Create a superuser account
  --email <EMAIL>
  --password <PASSWORD>
zerobase superuser update         # Update a superuser
  --email <EMAIL>
  [--new-email <EMAIL>]
  [--new-password <PASSWORD>]
zerobase superuser delete         # Delete a superuser
  --email <EMAIL>
zerobase superuser list           # List all superusers

zerobase version                  # Print version
```

## API Overview

All collections automatically get REST endpoints:

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/collections/:col/records` | List records (filter, sort, paginate, expand) |
| `POST` | `/api/collections/:col/records` | Create record |
| `GET` | `/api/collections/:col/records/:id` | View record |
| `PATCH` | `/api/collections/:col/records/:id` | Update record |
| `DELETE` | `/api/collections/:col/records/:id` | Delete record |

**Authentication** (per auth collection):

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `.../auth-with-password` | Email/password login |
| `POST` | `.../auth-with-otp` | OTP login |
| `POST` | `.../auth-with-mfa` | MFA verification |
| `POST` | `.../auth-with-passkey-finish` | Passkey login |
| `POST` | `.../auth-with-oauth2` | OAuth2 login |
| `POST` | `.../auth-refresh` | Refresh token |

**Admin & system endpoints:**

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/health` | Health check |
| `GET` | `/api/realtime` | SSE connection |
| `POST` | `/api/batch` | Batch operations |
| `GET/POST` | `/api/collections` | Schema management (superuser) |
| `GET/PATCH` | `/api/settings` | Settings management (superuser) |
| `POST` | `/_/api/backups` | Backup management (superuser) |

See [docs/api-reference.md](docs/api-reference.md) for the complete API reference.

## Documentation

- [Architecture Overview](docs/architecture.md) — Crate organization, layered architecture, key abstractions
- [API Reference](docs/api-reference.md) — Complete endpoint documentation with examples
- [Configuration Reference](docs/configuration-reference.md) — All config options, env vars, CLI flags
- [Deployment Guide](docs/deployment-guide.md) — Docker, systemd, bare metal, reverse proxy, backups
- [Migration from PocketBase](docs/migration-from-pocketbase.md) — Step-by-step migration guide

## Frontend Development

The admin dashboard can be developed independently:

```bash
cd frontend
pnpm dev            # Dev server on port 4321
pnpm build          # Production build (output embedded in binary)
pnpm preview        # Preview production build
pnpm test           # Run unit tests (Vitest)
pnpm test:e2e       # Run E2E tests (Playwright)
pnpm check          # Astro type checking
```

## Testing

```bash
cargo test --workspace       # All backend tests (141+ test files)
cd frontend && pnpm test     # Frontend unit tests
```

## CI

The GitHub Actions workflow runs on every push to `main` and on pull requests:

- `cargo fmt --all --check` — Format check
- `cargo clippy --workspace --all-targets -- -D warnings` — Lint
- `cargo test --workspace` — Test suite
- `cargo build --workspace --release` — Release build
- Smoke test — Binary starts and serves dashboard

Cross-compilation builds for Linux (x86_64, ARM64), macOS (Intel, Apple Silicon), and Windows are produced on main branch pushes and tags.

## License

MIT
