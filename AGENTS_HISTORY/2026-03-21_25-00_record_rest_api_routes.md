# Record REST API Routes

**Task ID:** 5q8hgmq13rx694q
**Date:** 2026-03-21
**Status:** Completed

## Summary

Implemented full CRUD REST API routes for records within collections, following PocketBase-compatible URL patterns and response shapes.

## Changes Made

### 1. Record CRUD Handlers (`crates/zerobase-api/src/handlers/records.rs`)

Added four new handler functions alongside the existing `list_records`:

- **`view_record`** — `GET /api/collections/:collection_name/records/:id` with optional `fields` query param for field projection
- **`create_record`** — `POST /api/collections/:collection_name/records` accepting JSON body, returns created record with generated system fields
- **`update_record`** — `PATCH /api/collections/:collection_name/records/:id` with PATCH semantics (partial update)
- **`delete_record`** — `DELETE /api/collections/:collection_name/records/:id` returning 204 No Content

Also added `FieldsParam` query parameter struct for the view endpoint's field projection support.

### 2. Route Wiring (`crates/zerobase-api/src/lib.rs`)

Added `record_routes()` public function that builds an axum `Router` with all five record endpoints wired to the `RecordService`. Uses axum 0.8 method routing (`.post()`, `.patch()`, `.delete()` chained on route definitions).

### 3. Integration Tests (`crates/zerobase-api/tests/records_endpoints.rs`)

Created comprehensive integration test suite with 28 tests covering:

- **List**: empty collection, seeded records, pagination shape, sorting, filtering, invalid sort error, nonexistent collection
- **View**: success, field projection, nonexistent record, nonexistent collection
- **Create**: success, appears in list, invalid body validation, nonexistent collection
- **Update**: success, preserves unmodified fields, cannot change ID, nonexistent record, nonexistent collection
- **Delete**: success (204), disappears from list, nonexistent record, nonexistent collection
- **Error responses**: proper `code`/`message` shape
- **Full lifecycle**: create → view → update → delete → verify gone

Tests use mock `RecordRepository` and `SchemaLookup` implementations with in-memory storage, spawning isolated axum servers on random ports per test.

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/collections/:collection/records` | List with pagination, sort, filter, fields |
| POST | `/api/collections/:collection/records` | Create a record |
| GET | `/api/collections/:collection/records/:id` | View a record (with optional field projection) |
| PATCH | `/api/collections/:collection/records/:id` | Partial update a record |
| DELETE | `/api/collections/:collection/records/:id` | Delete a record |

## Test Results

All 28 integration tests pass: `cargo test -p zerobase-api --test records_endpoints`
