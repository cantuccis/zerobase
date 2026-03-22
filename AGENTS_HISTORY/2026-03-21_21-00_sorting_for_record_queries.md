# Implement Sorting for Record Queries

**Date:** 2026-03-21 21:00
**Task ID:** gb8ti6koa1xev4o

## Summary

Implemented PocketBase-style sort parameter parsing and validation for record queries. The sort parameter supports comma-separated field names with optional `-` prefix for descending order (ascending is default). Sort fields are validated against the collection schema (system fields + user-defined fields) before query execution.

## What Was Done

### 1. Sort Parameter Parser (`parse_sort`)
- Parses PocketBase-style sort strings (e.g., `-created,title,+views`)
- `-` prefix = descending, `+` prefix or no prefix = ascending
- Validates field name characters (alphanumeric + underscore only)
- Rejects malformed input: empty segments, trailing commas, lone prefixes, special characters

### 2. Sort Field Validation (`validate_sort_fields`)
- Validates sort field names against the collection schema
- Checks both system fields (id, created, updated) and user-defined fields
- Auth collections also accept auth-specific system fields (email, verified, etc.)
- Returns descriptive 400 validation error for unknown fields

### 3. Collection `has_field` Method
- New method on `Collection` struct to check if a field name exists
- Checks system fields (type-aware: base vs auth) and user-defined fields

### 4. Service Integration
- `RecordService::list_records` now validates sort fields before delegating to the repository
- Invalid sort fields produce a 400 error before any DB query is executed

### 5. DB Layer Integration Tests
- Descending sort test
- Multi-field sort test (secondary sort within same primary values)
- Default sort (created DESC when no sort specified) test

## Tests Added

- **16 parse_sort tests**: empty, whitespace, single asc/desc, explicit `+`, multi-field, mixed directions, whitespace trimming, empty segments, trailing comma, lone `-`/`+`, invalid chars, spaces, underscores, alphanumeric
- **6 validate_sort_fields tests**: user fields, system fields, empty sort, unknown field, partial unknown, auth system fields
- **4 list_records integration tests**: valid sort, system field sort, invalid field rejected, multi-field sort
- **4 has_field tests**: system fields, user fields, unknown, auth system fields
- **4 DB integration tests**: descending, multi-field, default sort, ascending (existing)
- **1 doc test**: `parse_sort` example in documentation

**Total new tests: 35**

## Files Modified

- `crates/zerobase-core/src/schema/collection.rs` - Added `has_field` method + 4 tests
- `crates/zerobase-core/src/services/record_service.rs` - Added `parse_sort`, `validate_sort_fields`, `Display` for `SortDirection`, sort validation in `list_records` + 26 tests
- `crates/zerobase-core/src/services/mod.rs` - Re-exported `parse_sort` and `validate_sort_fields`
- `crates/zerobase-db/src/record_repo.rs` - Added 4 DB integration tests for sorting
