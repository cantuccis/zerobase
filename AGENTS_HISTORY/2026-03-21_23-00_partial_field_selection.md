# Partial Field Selection Implementation

**Date:** 2026-03-21 23:00
**Task ID:** 7730hwb6da505am

## Summary

Implemented support for a `fields` query parameter that allows API consumers to request only specific fields in record responses, reducing payload size. The `id` field is always included regardless of the field list. Unknown fields are gracefully ignored (filtered out) rather than producing errors.

## Design Decisions

- **Post-query projection**: Fields are filtered at the service layer after the database query returns all columns. This keeps the DB layer simple and avoids complex column-validation at the SQL level.
- **Graceful handling of unknown fields**: Unknown fields are silently dropped (matching PocketBase behaviour) rather than returning errors. This makes the API more resilient to schema changes.
- **`id` always included**: The `id` field is always present in results even if not explicitly requested, ensuring records remain identifiable.
- **Input validation**: Field names are validated for safe characters (alphanumeric + underscore) to prevent injection.

## Changes Made

### zerobase-core (`crates/zerobase-core/src/services/record_service.rs`)
- Added `fields: Option<Vec<String>>` to `RecordQuery` struct
- Added `parse_fields()` function: parses comma-separated field names, validates characters, deduplicates, always includes `id`
- Added `validate_and_filter_fields()`: filters requested fields against collection schema, dropping unknowns
- Added `project_fields()`: projects a single record to only include specified fields
- Added `project_record_list()`: applies field projection to all items in a RecordList
- Updated `list_records()` to apply field projection when `fields` is set
- Added `get_record_with_fields()` method for single-record field projection
- Added 20+ unit tests covering parsing, validation, projection, and integration

### zerobase-core (`crates/zerobase-core/src/services/mod.rs`)
- Exported new public functions: `parse_fields`, `project_fields`, `project_record_list`, `validate_and_filter_fields`

### zerobase-api (`crates/zerobase-api/src/handlers/records.rs`)
- Added `fields: Option<String>` to `ListRecordsParams`
- Updated `into_record_query()` to parse the fields parameter
- Added 3 new handler tests: `list_params_with_fields`, `list_params_with_empty_fields_results_in_none`, `list_params_invalid_field_name_returns_error`

## Files Modified

1. `crates/zerobase-core/src/services/record_service.rs`
2. `crates/zerobase-core/src/services/mod.rs`
3. `crates/zerobase-api/src/handlers/records.rs`

## Test Results

All 1077 tests pass, including 23+ new tests for field selection functionality.
