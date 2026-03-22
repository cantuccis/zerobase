# Docker Build Configuration

**Date:** 2026-03-21
**Task ID:** 1jb7jw04p1pto0l

## Summary

Created a complete Docker build configuration for Zerobase with multi-stage builds, development compose file, and automated test script.

## Architecture

### Multi-stage Dockerfile (3 stages)

1. **frontend-builder** (node:22-alpine): Builds the AstroJS admin dashboard using pnpm
2. **rust-builder** (rust:1-alpine): Compiles the Rust binary with musl for static linking, embeds frontend assets via rust-embed
3. **runtime** (alpine:3.21): Minimal runtime with only ca-certificates, non-root user, health check

### Key design decisions
- Uses `rust:1-alpine` (musl) for smaller, statically-linked binary
- No OpenSSL dependency (project uses rustls everywhere)
- Dependency caching via dummy source files trick
- Git init in builder for build.rs ZEROBASE_GIT_HASH resolution
- Binary is stripped to reduce size
- Non-root user (uid 1001) in runtime image
- Health check via wget to `/api/health`
- Data directory exposed as a volume at `/app/zerobase_data`

### Expected image size
- Alpine base: ~7 MB
- ca-certificates: ~1 MB
- Stripped zerobase binary: ~20-25 MB (musl, stripped)
- **Total: ~30-35 MB** (well under the 50 MB target)

## Files Created

| File | Purpose |
|------|---------|
| `Dockerfile` | Multi-stage build: frontend + Rust + minimal runtime |
| `docker-compose.yml` | Production deployment with named volume |
| `docker-compose.dev.yml` | Development deployment with bind mount |
| `.dockerignore` | Excludes target/, node_modules/, .git/, docs/ etc. |
| `scripts/docker-test.sh` | Automated test script verifying build, size, startup, dashboard, shutdown |

## Test Script (scripts/docker-test.sh)

Validates 5 criteria:
1. Docker image builds successfully
2. Image size is under 50 MB
3. Container starts and `/api/health` responds with `"ok"`
4. Admin dashboard served at `/_/` returns HTTP 200
5. Container stops gracefully with exit code 0

Usage:
```bash
./scripts/docker-test.sh            # full build + test
./scripts/docker-test.sh --no-build # test existing image
```

## Notes

- Docker daemon was not accessible in this environment (requires sudo or docker group membership), so build was not tested live
- All referenced paths in Dockerfile were validated to exist
- YAML syntax of both compose files was validated
- The project uses `rusqlite` with `bundled` feature (compiles SQLite from C source, works on musl)
- All TLS uses rustls (no OpenSSL dependency)
