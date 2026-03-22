# DateTime Field Type Enhancement

**Date:** 2026-03-21 10:00
**Task ID:** nu3wjmyl8y2j0uy

## Summary

Enhanced the existing DateTime field type with timezone handling modes, date-only support, options validation, value normalization for storage, and comparison utilities. Added 48 comprehensive tests covering all modes and edge cases.

## Changes Made

### Modified Files

1. **`crates/zerobase-core/src/schema/field.rs`**
   - Added `DateTimeMode` enum with three modes: `UtcOnly` (default), `TimezoneAware`, `DateOnly`
   - Enhanced `DateTimeOptions` with `mode` field
   - Added `DateTimeOptions::validate()` for options validation (invalid min/max strings, min > max)
   - Enhanced `DateTimeOptions::validate_value()` to enforce mode-specific constraints
   - Added `DateTimeOptions::prepare_value()` for storage normalization (UTC conversion, date-only formatting)
   - Made `parse_datetime()` public for reuse
   - Added `compare_datetimes()` utility for datetime comparison operations
   - Replaced 10 old tests with 48 comprehensive tests covering:
     - UTC-only mode: RFC 3339, naive datetime, date-only, offset conversion, rejection of invalid formats
     - Timezone-aware mode: offset acceptance, naive rejection
     - Date-only mode: date acceptance, time rejection
     - Range checks across all modes, boundary inclusivity
     - Options validation (invalid min/max, min > max, min == max)
     - Value preparation/normalization in all modes
     - `parse_datetime` function edge cases
     - `compare_datetimes` function (equal, less, greater, cross-format, with offset, invalid)
     - Serialization round-trips for all modes

2. **`crates/zerobase-core/src/schema/mod.rs`**
   - Added exports for `DateTimeMode`, `parse_datetime`, `compare_datetimes`

3. **`crates/zerobase-core/src/schema/record_validator.rs`**
   - Updated test to use `..Default::default()` for new `mode` field

### `FieldType` Integration
   - Updated `validate_options()` to call `DateTimeOptions::validate()` (was no-op before)
   - Updated `prepare_value()` to call `DateTimeOptions::prepare_value()` for storage normalization

## Test Results

- 48 DateTime-specific tests pass
- 384 total tests in `zerobase-core` pass
- Full workspace tests pass
