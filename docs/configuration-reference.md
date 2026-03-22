# Zerobase Configuration Reference

Zerobase reads configuration from multiple sources, merged in the following order of precedence (highest wins):

1. **Built-in defaults** — Sensible values for all settings
2. **Config file** — `zerobase.toml` in the working directory (or custom path via `ZEROBASE_CONFIG`)
3. **Environment variables** — `ZEROBASE__` prefix with `__` for nesting
4. **CLI flags** — Override everything

---

## Configuration File

By default, Zerobase looks for `zerobase.toml` in the current working directory. Override with:

```bash
ZEROBASE_CONFIG=/path/to/config.toml zerobase serve
```

### Full Configuration Reference

```toml
# ── Server ──────────────────────────────────────────────────────────────

[server]
# Network address to bind to.
host = "127.0.0.1"

# Port to listen on.
port = 8090

# Log output format: "json" (structured, for production) or "pretty" (human-readable).
log_format = "json"

# Maximum request body size in bytes for regular (non-file) requests.
# Default: 10 MiB (10485760)
body_limit = 10485760

# Maximum request body size in bytes for file upload (multipart) requests.
# Default: 100 MiB (104857600)
body_limit_upload = 104857600

# ── Database ────────────────────────────────────────────────────────────

[database]
# Path to the SQLite database file. Created automatically if it doesn't exist.
path = "zerobase_data/data.db"

# Number of read-only connections in the pool.
max_read_connections = 8

# Milliseconds to wait when the database is locked before returning SQLITE_BUSY.
busy_timeout_ms = 5000

# ── Auth ────────────────────────────────────────────────────────────────

[auth]
# REQUIRED — HMAC secret used to sign JWT tokens. Must be a long, random string.
# Generate one with: openssl rand -hex 32
token_secret = ""

# JWT token validity in seconds.
# Default: 14 days (1209600)
token_duration_secs = 1209600

# ── Storage ─────────────────────────────────────────────────────────────

[storage]
# Storage backend: "local" for filesystem, "s3" for S3-compatible object storage.
backend = "local"

# Directory for local file storage (when backend = "local").
local_path = "zerobase_data/storage"

# S3 configuration (when backend = "s3").
# [storage.s3]
# bucket = "my-bucket"
# region = "us-east-1"
# access_key = "AKIAIOSFODNN7EXAMPLE"
# secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
# endpoint = ""              # Custom endpoint for S3-compatible services (MinIO, DigitalOcean Spaces, etc.)
# force_path_style = false   # Set to true for MinIO or path-style URL access

# ── SMTP ────────────────────────────────────────────────────────────────

[smtp]
# Enable SMTP email sending. Required for verification, password reset, OTP, and email change.
enabled = false

# SMTP server hostname.
# host = "smtp.example.com"

# SMTP server port.
# port = 587

# SMTP authentication username.
# username = ""

# SMTP authentication password.
# password = ""

# Email address used as the "From" address.
# sender_address = "noreply@example.com"

# Display name used in the "From" field.
# sender_name = "Zerobase"

# Whether to use TLS for the SMTP connection.
# tls = true

# ── Logs ────────────────────────────────────────────────────────────────

[logs]
# Number of days to retain request logs before automatic cleanup.
retention_days = 7
```

---

## Environment Variables

All settings can be overridden via environment variables using the `ZEROBASE__` prefix with `__` (double underscore) as a separator for nested values.

### Environment Variable Mapping

| Setting | Environment Variable |
|---------|---------------------|
| `server.host` | `ZEROBASE__SERVER__HOST` |
| `server.port` | `ZEROBASE__SERVER__PORT` |
| `server.log_format` | `ZEROBASE__SERVER__LOG_FORMAT` |
| `server.body_limit` | `ZEROBASE__SERVER__BODY_LIMIT` |
| `server.body_limit_upload` | `ZEROBASE__SERVER__BODY_LIMIT_UPLOAD` |
| `database.path` | `ZEROBASE__DATABASE__PATH` |
| `database.max_read_connections` | `ZEROBASE__DATABASE__MAX_READ_CONNECTIONS` |
| `database.busy_timeout_ms` | `ZEROBASE__DATABASE__BUSY_TIMEOUT_MS` |
| `auth.token_secret` | `ZEROBASE__AUTH__TOKEN_SECRET` |
| `auth.token_duration_secs` | `ZEROBASE__AUTH__TOKEN_DURATION_SECS` |
| `storage.backend` | `ZEROBASE__STORAGE__BACKEND` |
| `storage.local_path` | `ZEROBASE__STORAGE__LOCAL_PATH` |
| `storage.s3.bucket` | `ZEROBASE__STORAGE__S3__BUCKET` |
| `storage.s3.region` | `ZEROBASE__STORAGE__S3__REGION` |
| `storage.s3.access_key` | `ZEROBASE__STORAGE__S3__ACCESS_KEY` |
| `storage.s3.secret_key` | `ZEROBASE__STORAGE__S3__SECRET_KEY` |
| `storage.s3.endpoint` | `ZEROBASE__STORAGE__S3__ENDPOINT` |
| `storage.s3.force_path_style` | `ZEROBASE__STORAGE__S3__FORCE_PATH_STYLE` |
| `smtp.enabled` | `ZEROBASE__SMTP__ENABLED` |
| `smtp.host` | `ZEROBASE__SMTP__HOST` |
| `smtp.port` | `ZEROBASE__SMTP__PORT` |
| `smtp.username` | `ZEROBASE__SMTP__USERNAME` |
| `smtp.password` | `ZEROBASE__SMTP__PASSWORD` |
| `smtp.sender_address` | `ZEROBASE__SMTP__SENDER_ADDRESS` |
| `smtp.sender_name` | `ZEROBASE__SMTP__SENDER_NAME` |
| `smtp.tls` | `ZEROBASE__SMTP__TLS` |
| `logs.retention_days` | `ZEROBASE__LOGS__RETENTION_DAYS` |

### Example

```bash
export ZEROBASE__AUTH__TOKEN_SECRET="$(openssl rand -hex 32)"
export ZEROBASE__SERVER__HOST=0.0.0.0
export ZEROBASE__SERVER__PORT=9090
export ZEROBASE__SMTP__ENABLED=true
export ZEROBASE__SMTP__HOST=smtp.gmail.com
export ZEROBASE__SMTP__PORT=587
export ZEROBASE__SMTP__USERNAME=myapp@gmail.com
export ZEROBASE__SMTP__PASSWORD=app_specific_password
export ZEROBASE__SMTP__SENDER_ADDRESS=noreply@myapp.com

zerobase serve
```

---

## CLI Reference

### `zerobase serve`

Start the HTTP server.

```bash
zerobase serve [OPTIONS]
```

| Flag | Short | Description |
|------|-------|-------------|
| `--host <HOST>` | | Bind address (overrides config) |
| `--port <PORT>` | `-p` | Listen port (overrides config) |
| `--data-dir <PATH>` | | Data directory for database and storage |
| `--log-format <FORMAT>` | | Log format: `json` or `pretty` |

**Examples:**
```bash
# Development mode with pretty logging
zerobase serve --log-format pretty

# Bind to all interfaces on port 9090
zerobase serve --host 0.0.0.0 --port 9090

# Custom data directory
zerobase serve --data-dir /var/lib/zerobase
```

### `zerobase migrate`

Run pending database migrations without starting the server.

```bash
zerobase migrate [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `--data-dir <PATH>` | Data directory containing the database |

### `zerobase superuser`

Manage superuser (admin) accounts.

```bash
# Create a new superuser
zerobase superuser create --email admin@example.com --password SecurePass123

# Update a superuser's credentials
zerobase superuser update --email admin@example.com --new-password NewPass456
zerobase superuser update --email admin@example.com --new-email newadmin@example.com

# Delete a superuser
zerobase superuser delete --email admin@example.com

# List all superusers
zerobase superuser list
```

### `zerobase version`

Print version information and exit.

```bash
zerobase version
```

---

## Data Directory Structure

When running, Zerobase creates the following directory structure:

```
zerobase_data/           # Default data directory (configurable)
├── data.db              # SQLite database file
├── data.db-wal          # WAL journal (if WAL mode enabled)
├── data.db-shm          # Shared memory (if WAL mode enabled)
├── storage/             # File storage root (local backend)
│   ├── <collection_id>/
│   │   └── <record_id>/
│   │       ├── <random>_<filename>
│   │       └── thumbs/  # Generated thumbnails
└── backups/             # Database backups
    └── backup_*.db
```

---

## Security Considerations

### Token Secret

The `auth.token_secret` is the most critical security setting. It is used to sign all JWTs.

- **Must be set** before running in production
- Use at least 32 bytes of randomness: `openssl rand -hex 32`
- Rotating the secret invalidates all existing tokens
- Never commit the secret to version control

### SMTP Password

The SMTP password is stored as a `secrecy::SecretString` in memory and is never logged.

### Body Limits

- Regular requests: 10 MiB default (configurable via `server.body_limit`)
- File uploads: 100 MiB default (configurable via `server.body_limit_upload`)

### Rate Limiting

Zerobase includes built-in per-IP rate limiting for API endpoints. This is configured via the settings API at runtime.

### CORS

CORS is configurable via the settings API. By default, all origins are restricted.
