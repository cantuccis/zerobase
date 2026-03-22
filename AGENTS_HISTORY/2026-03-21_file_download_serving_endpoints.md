# File Download and Serving Endpoints

**Task ID:** uzrua3epi1mss4y
**Date:** 2026-03-21
**Status:** Complete

## Summary

Implemented PocketBase-compatible file download and serving endpoints:

- `GET /api/files/token` — generates short-lived file access tokens (120s JWT)
- `GET /api/files/:collectionId/:recordId/:filename` — serves files with correct Content-Type

## Changes

### New Files
- `crates/zerobase-api/src/handlers/files.rs` — Handler module with `FileState<S>`, `request_file_token()`, `serve_file()`, and `check_file_protected()` helper
- `crates/zerobase-api/tests/file_download_endpoints.rs` — 15 integration tests covering auth, public/protected files, MIME types, 404s, Content-Disposition, Cache-Control

### Modified Files
- `crates/zerobase-api/src/handlers/mod.rs` — Added `pub mod files;`
- `crates/zerobase-api/src/lib.rs` — Added `FileState` export and `file_routes()` function

## Design Decisions

- Protected file detection checks all file fields in a collection; if any has `protected: true`, a token is required
- Unknown/deleted collections default to treating files as public (matches PocketBase behavior)
- Manual `Clone` impl on `FileState<S>` to avoid requiring `S: Clone` (only `Arc`s need cloning)
- Content-Disposition defaults to `inline`, switches to `attachment` with `?download=true`
- Cache-Control: `max-age=604800` (7 days)
- MIME type falls back to `application/octet-stream` if not stored

## Test Results

All 15 tests pass with no warnings.
