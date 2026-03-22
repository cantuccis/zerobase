# File Field Type Metadata Implementation

**Date:** 2026-03-21 17:00
**Task ID:** cao4bvdzlfp02zz

## Summary

Enhanced the existing `FileOptions` struct with comprehensive validation for MIME types, thumbnail size specifications, and file metadata value validation. Added 31 new tests covering all aspects of the File field type.

## Changes Made

### Enhanced `FileOptions::validate()` (options validation)
- Added MIME type format validation using a regex (`type/subtype` pattern)
- Added thumbnail size specification validation:
  - Format: `WxH` with optional suffix `t` (top), `b` (bottom), `f` (fit/force)
  - At least one dimension must be non-zero
  - Examples: `100x100`, `200x200f`, `50x0t`

### Added `FileOptions::validate_value()` (record value validation)
- **Single file** (`max_select == 1`): validates value is a non-empty string filename
- **Multiple files** (`max_select > 1`): validates value is an array of non-empty string filenames
  - Enforces `max_select` limit
  - Rejects duplicate filenames
  - Validates each element is a non-empty string

### Connected File validation to `FieldType::validate_value()`
- Changed from `Ok(())` bypass to calling `opts.validate_value()` for proper metadata validation

### Added static regex patterns
- `THUMB_SIZE_RE`: validates thumbnail size format `WxH[t|b|f]`
- `MIME_TYPE_RE`: validates MIME type format `type/subtype`

## Tests Added (31 new tests)

- Options validation: defaults, max_select, MIME types, thumb sizes, suffixes, full config
- Single file value: accepts filename, rejects empty, rejects non-string
- Multi file value: accepts arrays, empty arrays, max_select enforcement, duplicate detection
- Field-level: required/optional null handling, value pass-through
- Serde: round-trip, defaults, empty vec omission
- Metadata: SQL type, type name, is_text_like

## Files Modified

- `crates/zerobase-core/src/schema/field.rs`
  - Enhanced `FileOptions::validate()` with MIME type and thumb validation
  - Added `FileOptions::validate_value()` for filename metadata validation
  - Added `THUMB_SIZE_RE` and `MIME_TYPE_RE` static regexes
  - Updated `FieldType::validate_value()` to call File validation
  - Added 31 comprehensive tests

## Test Results

- All 31 new file field tests pass
- All 621 total tests in zerobase-core pass
