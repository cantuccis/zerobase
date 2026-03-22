# Embed Admin Dashboard Static Files in Rust Binary

**Date:** 2026-03-21
**Task ID:** 0t8uhnxt7habauq

## Summary

Implemented embedding of the AstroJS admin dashboard static files into the Rust binary using `rust-embed`. The admin dashboard is served at `/_/` matching PocketBase convention. Includes SPA fallback routing, correct MIME types, intelligent caching, and a dev proxy mode for hot-reload during development.

## What Was Done

1. **Added dependencies**: `rust-embed` (v8 with mime-guess), `mime_guess`, `mime`, `http`, `hyper`, `hyper-util`, `http-body-util` to workspace; `rust-embed`, `mime_guess`, `mime`, `http`, `tokio`, and optional `reqwest` to `zerobase-admin`.

2. **Created `dashboard.rs` module** in `zerobase-admin` with:
   - `DashboardAssets` struct using `rust-embed` to embed `frontend/dist/` at compile time
   - `dashboard_routes()` — returns an axum Router handling all `/_/` paths
   - Exact file matching with directory index fallback (`path/index.html`)
   - SPA fallback to `index.html` for client-side routing (dynamic routes)
   - Correct MIME type detection via `mime_guess`
   - Intelligent cache headers: immutable for hashed `_astro/` assets, must-revalidate for HTML
   - Dev proxy mode (behind `dev-proxy` feature flag) that forwards requests to AstroJS dev server

3. **Integrated into server**: Merged `dashboard_routes()` into the main axum router in `zerobase-server/src/main.rs`.

4. **Wrote comprehensive tests** (22 total):
   - 12 unit tests: asset embedding verification, MIME types, cache headers, 404 handling
   - 10 integration tests: full HTTP server tests for root, login, JS/CSS serving, SPA fallback, favicon, settings, cache headers, path isolation

## Files Modified

- `Cargo.toml` (workspace) — added rust-embed, mime_guess, mime, http, hyper, hyper-util, http-body-util
- `crates/zerobase-admin/Cargo.toml` — added dependencies and feature flags
- `crates/zerobase-admin/src/lib.rs` — added `dashboard` module
- `crates/zerobase-admin/src/dashboard.rs` — **new** — embedded file serving implementation
- `crates/zerobase-admin/tests/dashboard_serving.rs` — **new** — integration tests
- `crates/zerobase-server/src/main.rs` — merged dashboard routes into server

## Test Results

All 22 tests pass:
- 12 unit tests (embedded assets, MIME types, cache control, 404)
- 10 integration tests (HTTP serving, SPA routing, MIME types, caching)
