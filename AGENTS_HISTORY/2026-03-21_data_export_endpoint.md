# Data Export Endpoint

**Date**: 2026-03-21
**Task ID**: d508ju8oij8fuyx
**Status**: Complete

## Summary

Implemented `GET /_/api/collections/:collection/export` — a superuser-only endpoint that exports all records from a collection as JSON or CSV.

## Changes Made

### New Files
- `crates/zerobase-api/src/handlers/export.rs` — Core export handler with:
  - `ExportFormat` enum (JSON/CSV) with serde deserialization
  - `ExportParams` query parameters (format, filter, sort)
  - `ExportState` shared state wrapping `RecordService`
  - `export_records` handler with collection validation, paginated fetching, and format-specific response building
  - `build_export_headers` — ordered column list from schema (system fields first, auth fields for auth collections, user fields minus passwords)
  - `fetch_all_records` — paginated iteration (500 records/page, max 100K pages)
  - `build_json_response` — incremental JSON array serialization
  - `build_csv_response` — RFC 4180 CSV with proper header/data rows
  - `value_to_csv_cell` — JSON value to CSV cell conversion (complex types serialized as JSON strings)
  - 27 unit tests covering all helpers and edge cases
- `crates/zerobase-api/tests/export_endpoints.rs` — 8 integration tests covering:
  - JSON export (empty + with records)
  - CSV export (empty + with records)
  - Superuser auth enforcement (anonymous + regular user rejected)
  - Non-existent collection 404
  - Filter parameter application
  - Auth collection CSV headers (email, verified, emailVisibility)

### Modified Files
- `Cargo.toml` (workspace root) — Added `csv = "1"` workspace dependency
- `crates/zerobase-api/Cargo.toml` — Added `csv = { workspace = true }`
- `crates/zerobase-api/src/handlers/mod.rs` — Added `pub mod export;`
- `crates/zerobase-api/src/lib.rs` — Added `ExportState` re-export and `export_routes()` function
- `crates/zerobase-server/src/main.rs` — Wired export routes into the server

## Technical Decisions
- Used Vec-based buffering instead of async streaming (simpler, avoids `async_stream` dependency)
- Page size of 500 records balances memory usage with query overhead
- Password fields excluded from export headers for security
- Complex JSON values (arrays/objects) serialized as JSON strings in CSV cells

## Test Results
- 27 unit tests: all passing
- 8 integration tests: all passing
