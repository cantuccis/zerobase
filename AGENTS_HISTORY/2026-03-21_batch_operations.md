# Batch/Bulk Record Operations

**Task ID:** `fhm75mgpz9cynpm`
**Date:** 2026-03-21

## Summary

Implemented the `POST /api/batch` endpoint for executing multiple record operations (create, update, delete) atomically within a single request.

## Changes

### New Files
- `crates/zerobase-api/src/handlers/batch.rs` — Batch handler with URL parsing, rule enforcement, operation execution, and best-effort rollback
- `crates/zerobase-api/tests/batch_endpoints.rs` — 22 integration tests

### Modified Files
- `crates/zerobase-api/src/handlers/mod.rs` — Added `pub mod batch;`
- `crates/zerobase-api/src/lib.rs` — Added `BatchState` re-export and `batch_routes()` function

## Design Decisions

- **Service-layer execution:** Operations go through `RecordService` (not raw DB transactions) to maintain abstraction boundaries. Rollback is best-effort: created records are deleted, but updates/deletes cannot be fully reversed.
- **URL-based routing:** Each batch operation specifies a method + URL path (matching PocketBase's batch API format), parsed to determine collection name, record ID, and operation type.
- **MAX_BATCH_SIZE = 50:** Prevents excessively large batch requests.
- **Rule enforcement:** Each operation checks the collection's API rules (create_rule, update_rule, delete_rule) individually, with superuser bypass.

## Test Coverage

22 tests covering: single/multiple creates, update, delete, mixed operations, rollback on failure, empty/oversized requests, invalid methods/URLs, nonexistent collections/records, missing body validation, locked rules, collection metadata in response, cross-collection operations, case-insensitive methods.

## Technical Notes

- Record IDs in tests must be exactly 15 characters to pass the `id` field validation (min_length=15, max_length=15).
- The `enforce_rule_no_record` function in `records.rs` has an existing unused warning (pre-existing, not related to this work).
