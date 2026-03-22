# ── Stage 1: Build frontend ──────────────────────────────────────────────────
FROM node:22-alpine AS frontend-builder

RUN corepack enable && corepack prepare pnpm@latest --activate

WORKDIR /app/frontend

# Install dependencies first (better layer caching)
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

# Build the Astro frontend
COPY frontend/ ./
RUN pnpm run build


# ── Stage 2: Build Rust binary ───────────────────────────────────────────────
FROM rust:1-alpine AS rust-builder

# Install build dependencies for musl + SQLite bundled compilation
# (No openssl needed: all TLS uses rustls)
RUN apk add --no-cache musl-dev git

WORKDIR /app

# Cache dependency compilation: copy manifests first
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/zerobase-core/Cargo.toml crates/zerobase-core/Cargo.toml
COPY crates/zerobase-db/Cargo.toml   crates/zerobase-db/Cargo.toml
COPY crates/zerobase-auth/Cargo.toml crates/zerobase-auth/Cargo.toml
COPY crates/zerobase-api/Cargo.toml  crates/zerobase-api/Cargo.toml
COPY crates/zerobase-files/Cargo.toml crates/zerobase-files/Cargo.toml
COPY crates/zerobase-admin/Cargo.toml crates/zerobase-admin/Cargo.toml
COPY crates/zerobase-hooks/Cargo.toml crates/zerobase-hooks/Cargo.toml
COPY crates/zerobase-server/Cargo.toml crates/zerobase-server/Cargo.toml

# Create dummy source files so Cargo can resolve the workspace
RUN for crate in zerobase-core zerobase-db zerobase-auth zerobase-api \
        zerobase-files zerobase-admin zerobase-hooks zerobase-server; do \
      mkdir -p "crates/${crate}/src" && \
      echo "// dummy" > "crates/${crate}/src/lib.rs"; \
    done && \
    echo "fn main() {}" > crates/zerobase-server/src/main.rs

# Pre-build dependencies (this layer is cached unless Cargo.toml/lock change)
RUN cargo build --release --package zerobase-server 2>/dev/null || true

# Remove dummy artifacts so real source is compiled fresh
RUN rm -rf crates/*/src

# Copy real source code
COPY crates/ crates/
COPY sample.zerobase.toml ./

# Copy pre-built frontend assets for rust-embed
COPY --from=frontend-builder /app/frontend/dist/ frontend/dist/

# Initialise a git repo so the build script can resolve ZEROBASE_GIT_HASH
RUN git init && git add -A && git commit -m "docker build" --allow-empty

# Build the real binary
RUN cargo build --release --package zerobase-server && \
    strip target/release/zerobase && \
    cp target/release/zerobase /zerobase


# ── Stage 3: Minimal runtime image ──────────────────────────────────────────
FROM alpine:3.21 AS runtime

# Only ca-certificates needed at runtime (for HTTPS / S3 / OAuth2 / SMTP)
RUN apk add --no-cache ca-certificates && \
    addgroup -g 1001 -S zerobase && \
    adduser  -u 1001 -S zerobase -G zerobase

WORKDIR /app

# Copy the static binary
COPY --from=rust-builder /zerobase /app/zerobase

# Default data directory
RUN mkdir -p /app/zerobase_data && \
    chown -R zerobase:zerobase /app

USER zerobase

EXPOSE 8090

# Health check: hit the health endpoint
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD wget -qO- http://127.0.0.1:8090/api/health || exit 1

VOLUME ["/app/zerobase_data"]

ENTRYPOINT ["/app/zerobase"]
CMD ["serve", "--host", "0.0.0.0", "--port", "8090", "--data-dir", "/app/zerobase_data"]
