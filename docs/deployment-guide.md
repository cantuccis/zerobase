# Zerobase Deployment Guide

Zerobase is a single-binary application with an embedded SQLite database. No external database server or runtime dependencies are required.

---

## Table of Contents

- [Prerequisites](#prerequisites)
- [Building from Source](#building-from-source)
- [Pre-built Binaries](#pre-built-binaries)
- [Bare Metal Deployment](#bare-metal-deployment)
- [Docker Deployment](#docker-deployment)
- [systemd Service](#systemd-service)
- [Reverse Proxy Configuration](#reverse-proxy-configuration)
- [S3 Storage Configuration](#s3-storage-configuration)
- [SMTP Configuration](#smtp-configuration)
- [Backups](#backups)
- [Monitoring](#monitoring)
- [Upgrading](#upgrading)
- [Troubleshooting](#troubleshooting)

---

## Prerequisites

### Building from Source
- **Rust** stable toolchain (1.75+, managed via `rust-toolchain.toml`)
- **Node.js** >= 22.12 and **pnpm** (for building the admin frontend)
- **C compiler** (for SQLite bundled compilation)

### Running Pre-built Binaries
- No runtime dependencies beyond a standard Linux/macOS/Windows installation

---

## Building from Source

### 1. Clone and Build

```bash
git clone <repo-url> && cd zerobase

# Build the admin frontend (embedded in the binary)
cd frontend && pnpm install && pnpm build && cd ..

# Build the release binary
cargo build --release
```

The binary is at `target/release/zerobase`.

### 2. Cross-Compilation

Zerobase supports cross-compilation via the `cross` tool:

```bash
cargo install cross --locked

# Linux ARM64 (Raspberry Pi, AWS Graviton)
cross build --release --target aarch64-unknown-linux-gnu --package zerobase-server

# Windows
cross build --release --target x86_64-pc-windows-gnu --package zerobase-server
```

**Supported targets:**
| Target | OS | Architecture |
|--------|----|-------------|
| `x86_64-unknown-linux-gnu` | Linux | x86_64 |
| `aarch64-unknown-linux-gnu` | Linux | ARM64 |
| `x86_64-apple-darwin` | macOS | Intel |
| `aarch64-apple-darwin` | macOS | Apple Silicon |
| `x86_64-pc-windows-gnu` | Windows | x86_64 |

---

## Pre-built Binaries

Pre-built binaries for all supported platforms are available on the [GitHub Releases](../../releases) page. Each release includes:

- `.tar.gz` archives for Linux and macOS
- `.zip` archives for Windows
- SHA256 checksums

```bash
# Download and extract (Linux x86_64 example)
curl -LO https://github.com/<org>/zerobase/releases/download/v0.1.0/zerobase-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
tar xzf zerobase-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
chmod +x zerobase

# Verify
./zerobase version
```

---

## Bare Metal Deployment

### 1. Prepare the Server

```bash
# Create a dedicated user
sudo useradd -r -s /bin/false zerobase

# Create directories
sudo mkdir -p /opt/zerobase /var/lib/zerobase
sudo chown zerobase:zerobase /var/lib/zerobase
```

### 2. Install the Binary

```bash
sudo cp zerobase /opt/zerobase/zerobase
sudo chmod +x /opt/zerobase/zerobase
```

### 3. Create Configuration

```bash
sudo tee /opt/zerobase/zerobase.toml > /dev/null << 'EOF'
[server]
host = "127.0.0.1"
port = 8090
log_format = "json"

[database]
path = "/var/lib/zerobase/data.db"

[auth]
token_secret = "REPLACE_WITH_OPENSSL_RAND_HEX_32"

[storage]
backend = "local"
local_path = "/var/lib/zerobase/storage"

[smtp]
enabled = true
host = "smtp.example.com"
port = 587
username = "noreply@example.com"
password = "smtp_password"
sender_address = "noreply@example.com"
sender_name = "My App"
tls = true

[logs]
retention_days = 30
EOF

sudo chown zerobase:zerobase /opt/zerobase/zerobase.toml
sudo chmod 600 /opt/zerobase/zerobase.toml
```

### 4. Create Initial Superuser

```bash
cd /opt/zerobase
sudo -u zerobase ZEROBASE_CONFIG=/opt/zerobase/zerobase.toml \
  ./zerobase superuser create --email admin@example.com --password YourSecurePassword
```

### 5. Start the Server

```bash
cd /opt/zerobase
sudo -u zerobase ZEROBASE_CONFIG=/opt/zerobase/zerobase.toml ./zerobase serve
```

---

## Docker Deployment

### Dockerfile

Create a `Dockerfile` in your project:

```dockerfile
# ── Build stage ─────────────────────────────────────────────────────
FROM node:22-slim AS frontend
WORKDIR /app/frontend
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN corepack enable && pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm build

FROM rust:1.75-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
COPY --from=frontend /app/frontend/dist crates/zerobase-admin/public
RUN cargo build --release --package zerobase-server

# ── Runtime stage ───────────────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

RUN useradd -r -s /bin/false zerobase
WORKDIR /app

COPY --from=builder /app/target/release/zerobase /app/zerobase

RUN mkdir -p /data && chown zerobase:zerobase /data
USER zerobase

ENV ZEROBASE__SERVER__HOST=0.0.0.0
ENV ZEROBASE__SERVER__PORT=8090
ENV ZEROBASE__DATABASE__PATH=/data/data.db
ENV ZEROBASE__STORAGE__LOCAL_PATH=/data/storage

EXPOSE 8090
VOLUME ["/data"]

ENTRYPOINT ["/app/zerobase"]
CMD ["serve"]
```

### Docker Compose

```yaml
# docker-compose.yml
services:
  zerobase:
    build: .
    ports:
      - "8090:8090"
    volumes:
      - zerobase_data:/data
    environment:
      ZEROBASE__AUTH__TOKEN_SECRET: "${ZEROBASE_TOKEN_SECRET}"
      ZEROBASE__SMTP__ENABLED: "true"
      ZEROBASE__SMTP__HOST: "smtp.example.com"
      ZEROBASE__SMTP__PORT: "587"
      ZEROBASE__SMTP__USERNAME: "noreply@example.com"
      ZEROBASE__SMTP__PASSWORD: "${SMTP_PASSWORD}"
      ZEROBASE__SMTP__SENDER_ADDRESS: "noreply@example.com"
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "wget", "-q", "--spider", "http://localhost:8090/api/health"]
      interval: 30s
      timeout: 5s
      retries: 3

volumes:
  zerobase_data:
```

### Docker Commands

```bash
# Build and start
docker compose up -d

# Create superuser
docker compose exec zerobase ./zerobase superuser create \
  --email admin@example.com --password SecurePassword123

# View logs
docker compose logs -f zerobase

# Backup
docker compose exec zerobase ./zerobase backup create
docker cp "$(docker compose ps -q zerobase)":/data/backups/ ./backups/
```

---

## systemd Service

Create `/etc/systemd/system/zerobase.service`:

```ini
[Unit]
Description=Zerobase Backend-as-a-Service
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=zerobase
Group=zerobase
WorkingDirectory=/opt/zerobase
ExecStart=/opt/zerobase/zerobase serve
Environment=ZEROBASE_CONFIG=/opt/zerobase/zerobase.toml
Restart=on-failure
RestartSec=5

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/zerobase

# Limits
LimitNOFILE=65536

# Logging
StandardOutput=journal
StandardError=journal
SyslogIdentifier=zerobase

[Install]
WantedBy=multi-user.target
```

### Enable and Start

```bash
sudo systemctl daemon-reload
sudo systemctl enable zerobase
sudo systemctl start zerobase

# Check status
sudo systemctl status zerobase

# View logs
sudo journalctl -u zerobase -f
```

---

## Reverse Proxy Configuration

Zerobase should be placed behind a reverse proxy for TLS termination and production use.

### Nginx

```nginx
server {
    listen 443 ssl http2;
    server_name api.example.com;

    ssl_certificate /etc/letsencrypt/live/api.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/api.example.com/privkey.pem;

    client_max_body_size 100M;

    location / {
        proxy_pass http://127.0.0.1:8090;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # SSE support
        proxy_buffering off;
        proxy_cache off;
        proxy_read_timeout 3600s;
    }
}

server {
    listen 80;
    server_name api.example.com;
    return 301 https://$server_name$request_uri;
}
```

### Caddy

```caddyfile
api.example.com {
    reverse_proxy localhost:8090
}
```

Caddy automatically provisions TLS certificates via Let's Encrypt.

---

## S3 Storage Configuration

For production deployments that need scalable file storage:

### AWS S3

```toml
[storage]
backend = "s3"

[storage.s3]
bucket = "my-zerobase-files"
region = "us-east-1"
access_key = "AKIAIOSFODNN7EXAMPLE"
secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
```

### MinIO (Self-Hosted)

```toml
[storage]
backend = "s3"

[storage.s3]
bucket = "zerobase"
region = "us-east-1"
access_key = "minioadmin"
secret_key = "minioadmin"
endpoint = "http://localhost:9000"
force_path_style = true
```

### DigitalOcean Spaces

```toml
[storage]
backend = "s3"

[storage.s3]
bucket = "my-space"
region = "nyc3"
access_key = "DO_SPACES_KEY"
secret_key = "DO_SPACES_SECRET"
endpoint = "https://nyc3.digitaloceanspaces.com"
```

---

## SMTP Configuration

SMTP is required for email verification, password reset, OTP, and email change flows.

### Gmail

```toml
[smtp]
enabled = true
host = "smtp.gmail.com"
port = 587
username = "yourapp@gmail.com"
password = "app_specific_password"
sender_address = "noreply@yourapp.com"
sender_name = "My App"
tls = true
```

### Amazon SES

```toml
[smtp]
enabled = true
host = "email-smtp.us-east-1.amazonaws.com"
port = 587
username = "SES_SMTP_USERNAME"
password = "SES_SMTP_PASSWORD"
sender_address = "noreply@yourapp.com"
sender_name = "My App"
tls = true
```

### Resend

```toml
[smtp]
enabled = true
host = "smtp.resend.com"
port = 465
username = "resend"
password = "re_your_api_key"
sender_address = "noreply@yourapp.com"
sender_name = "My App"
tls = true
```

---

## Backups

### Automatic Backups via API

```bash
# Create a backup (requires superuser token)
curl -X POST http://localhost:8090/_/api/backups \
  -H "Authorization: Bearer $ADMIN_TOKEN"

# List backups
curl http://localhost:8090/_/api/backups \
  -H "Authorization: Bearer $ADMIN_TOKEN"

# Download a backup
curl -o backup.db http://localhost:8090/_/api/backups/backup_20250115.db \
  -H "Authorization: Bearer $ADMIN_TOKEN"
```

### Manual SQLite Backup

```bash
# Stop the server first for a consistent backup
sqlite3 /var/lib/zerobase/data.db ".backup /backups/zerobase_$(date +%Y%m%d).db"
```

### Automated Backup Script

```bash
#!/bin/bash
# /opt/zerobase/backup.sh
BACKUP_DIR="/backups/zerobase"
RETENTION_DAYS=30

mkdir -p "$BACKUP_DIR"

# Login and get token
TOKEN=$(curl -s -X POST http://localhost:8090/_/api/admins/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity":"admin@example.com","password":"AdminPassword"}' | jq -r '.token')

# Create backup via API
curl -s -X POST http://localhost:8090/_/api/backups \
  -H "Authorization: Bearer $TOKEN"

# Clean up old backups
find "$BACKUP_DIR" -name "*.db" -mtime +$RETENTION_DAYS -delete
```

Add to crontab:
```bash
# Daily backup at 3 AM
0 3 * * * /opt/zerobase/backup.sh
```

---

## Monitoring

### Health Check

```bash
curl http://localhost:8090/api/health
```

### Log Analysis

Zerobase outputs structured JSON logs when `log_format = "json"`. These can be ingested by any log aggregation tool (ELK, Loki, Datadog, etc.).

```bash
# View logs with jq formatting
journalctl -u zerobase --output cat | jq .

# Filter errors
journalctl -u zerobase --output cat | jq 'select(.level == "ERROR")'
```

### Request Logs

Request logs are stored in the database and accessible via the API:

```bash
# Get log statistics
curl http://localhost:8090/_/api/logs/stats \
  -H "Authorization: Bearer $ADMIN_TOKEN"
```

---

## Upgrading

### Upgrading from a Previous Version

1. **Back up your data** before upgrading:
   ```bash
   sqlite3 /var/lib/zerobase/data.db ".backup /backups/pre-upgrade.db"
   ```

2. **Stop the service:**
   ```bash
   sudo systemctl stop zerobase
   ```

3. **Replace the binary:**
   ```bash
   sudo cp zerobase-new /opt/zerobase/zerobase
   sudo chmod +x /opt/zerobase/zerobase
   ```

4. **Run migrations** (applied automatically on startup, but can be run separately):
   ```bash
   sudo -u zerobase ZEROBASE_CONFIG=/opt/zerobase/zerobase.toml \
     /opt/zerobase/zerobase migrate
   ```

5. **Start the service:**
   ```bash
   sudo systemctl start zerobase
   ```

6. **Verify:**
   ```bash
   curl http://localhost:8090/api/health
   ```

### Docker Upgrade

```bash
docker compose pull    # If using a registry
docker compose build   # If building locally
docker compose up -d
```

---

## Troubleshooting

### Common Issues

**"token_secret is required"**
Set `auth.token_secret` in `zerobase.toml` or via `ZEROBASE__AUTH__TOKEN_SECRET`.

**"address already in use"**
Another process is using the port. Check with `lsof -i :8090` or change the port.

**"database is locked"**
Increase `database.busy_timeout_ms` or ensure only one Zerobase instance accesses the database.

**Permission denied on data directory**
Ensure the Zerobase user has read/write access to the data directory:
```bash
sudo chown -R zerobase:zerobase /var/lib/zerobase
```

**SMTP connection failed**
- Verify SMTP credentials and host/port
- Check if TLS is required (`smtp.tls = true`)
- Test with `POST /api/settings/test-email`

### Debug Logging

Enable verbose logging for troubleshooting:

```bash
RUST_LOG=debug zerobase serve --log-format pretty
```

### Database Recovery

If the database becomes corrupted:

```bash
# Check integrity
sqlite3 /var/lib/zerobase/data.db "PRAGMA integrity_check;"

# Restore from backup
sudo systemctl stop zerobase
cp /backups/latest.db /var/lib/zerobase/data.db
sudo systemctl start zerobase
```
