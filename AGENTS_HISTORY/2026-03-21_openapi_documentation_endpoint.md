# Auto-Generated OpenAPI Documentation Endpoint

**Task ID:** z5kvtkaoe7l3zp8
**Date:** 2026-03-21
**Status:** Complete

## Summary

Implemented auto-generated API documentation endpoint that dynamically generates an OpenAPI 3.1.0 specification from registered collections and serves Swagger UI at `/_/api/docs`.

## Changes Made

### New Files
- `crates/zerobase-api/src/handlers/openapi.rs` — OpenAPI spec generation handler module

### Modified Files
- `crates/zerobase-api/src/handlers/mod.rs` — Added `pub mod openapi;`
- `crates/zerobase-api/src/lib.rs` — Added `openapi_routes()` function
- `crates/zerobase-server/src/main.rs` — Wired openapi routes into the application router

## Technical Details

### Architecture
- **Dynamic generation:** The OpenAPI spec is regenerated on every request to `/_/api/docs/openapi.json` by querying the current collection state via `CollectionService::list_collections()`. This means the spec always reflects the latest schema.
- **No new dependencies:** Uses `serde_json` for JSON construction — no OpenAPI-specific crates required.
- **Swagger UI:** Served as an embedded HTML page at `/_/api/docs` that loads the spec from the JSON endpoint. Uses swagger-ui-dist from CDN.

### Endpoints
- `GET /_/api/docs` — Swagger UI HTML page
- `GET /_/api/docs/openapi.json` — OpenAPI 3.1.0 JSON specification

### Spec Coverage
- Health check endpoint
- Collection management CRUD (superuser)
- Per-collection record CRUD (dynamic, based on registered collections)
- Auth endpoints (only for Auth-type collections): auth-with-password, auth-refresh, request-verification, confirm-verification, request-password-reset, confirm-password-reset, request-email-change, confirm-email-change, request-otp, auth-with-otp, MFA endpoints
- Superuser auth
- File serving and token endpoints
- Settings management
- Realtime SSE
- Batch operations
- Backup management
- Log queries

### Field Type Mapping
All 14 field types are mapped to OpenAPI schema types:
- Text, Email, Url, Editor → `string`
- Number → `number`
- Bool → `boolean`
- DateTime, AutoDate → `string` (format: date-time)
- Select → `string` (with enum)
- MultiSelect → `array` of `string`
- File → `string`
- Relation → `string`
- Json → `object`
- Password → `string` (format: password)

### Tests
20 unit tests covering:
- Valid spec structure (openapi version, info, paths, components)
- Endpoint inclusion for all route groups
- Schema generation per collection with field mapping
- Auth-specific endpoints only for Auth collections
- Tag generation from collection names
- Empty collections edge case
- Select field enum values
- Multi-collection support

## Acceptance Criteria
- [x] OpenAPI spec generated for all endpoints
- [x] Swagger UI renders at `/_/api/docs`
- [x] Spec updates dynamically when collections change
- [x] Tests pass (20/20)
