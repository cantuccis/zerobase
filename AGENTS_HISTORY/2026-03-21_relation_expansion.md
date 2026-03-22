# Relation Expansion (`?expand=`) Support

**Task ID:** x3dcj00a2ssqcdz
**Date:** 2026-03-21
**Phase:** 8 — Relation Expansion in API Responses

## Summary

Implemented PocketBase-compatible relation expansion for record API endpoints. When clients pass `?expand=fieldName`, related records are fetched and nested inline in the response under an `expand` key.

## Changes

### New Files
- **`crates/zerobase-core/src/services/expand.rs`** (~700 lines)
  - `parse_expand()` — parses comma-separated expand strings into `ExpandPath` structs
  - `expand_record()` — recursively resolves forward and back relations for a single record
  - `expand_records()` — batch wrapper for list endpoints
  - `ExpandPath` struct with dot-notation segment navigation
  - `ExpandKind` enum: `Forward` (field → collection) and `BackRelation` (`<collection>_via_<field>`)
  - Circular reference detection via `HashSet<(collection, id)>` visited set
  - `MAX_EXPAND_DEPTH = 6` constant
  - 25 unit tests

### Modified Files
- **`crates/zerobase-core/src/services/mod.rs`** — added `pub mod expand` and re-exports
- **`crates/zerobase-core/src/services/record_service.rs`** — added `repo()` and `schema()` accessor methods
- **`crates/zerobase-api/src/handlers/records.rs`**:
  - Added `expand: Option<String>` to `ListRecordsParams` and `FieldsParam`
  - Updated `list_records` handler to parse expand and call `expand_records()`
  - Updated `view_record` handler to parse expand and call `expand_record()`
- **`crates/zerobase-api/tests/records_endpoints.rs`**:
  - Added `with_multi_records()` constructor to `MockRecordRepo`
  - Updated `find_referencing_records()` mock to actually search records
  - Added 5 integration tests for expand functionality

## Features
- Single relation expansion: `?expand=author`
- Multi-relation expansion: `?expand=author,category`
- Nested dot-notation: `?expand=author.profile`
- Back-relation expansion: `?expand=comments_via_post`
- Depth limit enforcement (max 6 levels)
- Circular reference protection
- Non-existent fields gracefully ignored

## Test Coverage
- 25 unit tests in `expand.rs`
- 5 integration tests in `records_endpoints.rs`
- All existing workspace tests continue to pass
