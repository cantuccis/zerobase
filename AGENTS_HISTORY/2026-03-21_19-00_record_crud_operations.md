# Record CRUD Operations

**Date:** 2026-03-21
**Task ID:** jswcb0y10wwcki3

## Summary

Implemented the RecordService in zerobase-core and the RecordRepository SQLite implementation in zerobase-db for full CRUD operations on dynamic collection tables.

## Changes

### New Files
- `crates/zerobase-core/src/services/record_service.rs` — RecordService with create, get, list, update, delete operations

### Modified Files
- `crates/zerobase-core/src/services/mod.rs` — Added record_service module
- `crates/zerobase-core/src/lib.rs` — Re-exported RecordService
- `crates/zerobase-db/src/record_repo.rs` — SQLite RecordRepository implementation + integration tests
- `crates/zerobase-db/src/lib.rs` — Added record_repo module

## Architecture

### Core Layer (`record_service.rs`)
- **RecordRepository trait** — Persistence contract (find_one, find_many, insert, update, delete, count)
- **RecordRepoError** — Repository-level error enum with NotFound/Conflict/Database variants, converts to ZerobaseError
- **SchemaLookup trait** — Decouples record service from collection service for schema access
- **RecordService<R, S>** — Generic service for testability with mocks
  - `create_record` — Generates ID, injects timestamps via AutoDate, validates against schema, persists
  - `get_record` — Verifies collection exists, delegates to repo
  - `list_records` — Normalizes pagination (page 0→1, per_page 0→20, max 500), delegates to repo
  - `update_record` — PATCH semantics (merge with existing), prevents id modification, refreshes updated timestamp
  - `delete_record` — Returns 404 if record doesn't exist

### DB Layer (`record_repo.rs`)
- Implements RecordRepository for Database using parameterized SQL
- Value conversion helpers: sqlite_value_to_json (with JSON object/array auto-parsing), json_to_sqlite_value
- Error conversion: db_err_to_repo, sqlite_err_to_repo (detects unique/conflict errors)
- Default sort by `created DESC` when no sort specified

## Tests

### Unit Tests (21 in zerobase-core)
- Create: ID/timestamp generation, required field validation, unknown field rejection, type validation, non-object rejection, unknown collection
- Get: returns created record, not found error
- List: pagination, default normalization, per_page clamping
- Update: field modification, unmodified field preservation, timestamp refresh, id change prevention, not found, type validation
- Delete: removes record, not found, unknown collection
- Multiple records have unique IDs

### Integration Tests (19 in zerobase-db)
- 7 unit tests for value conversion helpers
- 12 integration tests against real in-memory SQLite:
  - insert + find_one roundtrip
  - find_one not found
  - find_many pagination, empty table, sort by column
  - update modify + not found
  - delete remove + not found
  - count correctness
  - full CRUD lifecycle
  - JSON object field roundtrip through SQLite
  - RecordService auto-timestamp injection end-to-end

**Full workspace: 922 tests, 0 failures.**
