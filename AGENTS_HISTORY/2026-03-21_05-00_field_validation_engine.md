# Field Validation Engine

**Date:** 2026-03-21 05:00
**Task ID:** pggzzkhdrskvcwi
**Phase:** 3

## Summary

Implemented a comprehensive field validation engine that validates record data against collection field definitions. The engine collects ALL validation errors (not just the first) and returns structured error responses.

### Key Components Built

1. **RecordValidator** (`record_validator.rs`) - Full-record validation engine that:
   - Validates all fields in a record against collection field definitions
   - Collects ALL errors across all fields (non-fail-fast)
   - Detects unknown fields not in the schema
   - Supports both full validation (`validate`) and partial/PATCH validation (`validate_partial`)
   - Returns structured `ZerobaseError::Validation` with per-field error messages

2. **Enhanced DateTime validation** - Added min/max range checking to `DateTimeOptions`:
   - Validates dates fall within configured min/max bounds
   - Supports all three datetime formats (RFC 3339, naive datetime, date-only)
   - Extracted `parse_datetime()` helper for consistent parsing

3. **Enhanced Json validation** - Added `max_size` enforcement to `JsonOptions`:
   - Validates serialized JSON payload size against configured limit
   - 0 = no limit (default behavior)

4. **Empty string handling** for required text-like fields:
   - Required text-like fields (Text, Email, Url, DateTime, Editor, Password) reject empty strings
   - Non-required text-like fields treat empty strings as "no value" (skip type validation)
   - Added `FieldType::is_text_like()` helper method

### Test Coverage

**258 total tests passing** (up from ~210), including:
- RecordValidator: 40+ tests covering full/partial validation, multi-error collection, unknown fields, all field types
- DateTime min/max: 3 new range tests
- Json max_size: 3 new tests
- Empty string required: 4 new tests
- Realistic integration scenarios: blog post with 12 field types

## Files Modified

- `crates/zerobase-core/src/schema/record_validator.rs` - **NEW** - RecordValidator with 40+ tests
- `crates/zerobase-core/src/schema/field.rs` - Enhanced DateTime, Json, empty string handling, new tests
- `crates/zerobase-core/src/schema/mod.rs` - Added module registration and exports
- `crates/zerobase-core/src/lib.rs` - Added RecordValidator to crate root re-exports
