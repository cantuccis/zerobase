# JSON Field Type Implementation

**Date:** 2026-03-21 08:00
**Task ID:** m7n73ir6kqjebqy

## Summary

Enhanced the existing JSON field type with optional JSON Schema validation support. The JSON field already existed with `max_size` support; this task added the ability to define a JSON Schema that validates incoming values at record creation/update time.

## Changes Made

### Added Dependencies
- Added `jsonschema = "0.45"` to workspace `Cargo.toml`
- Added `jsonschema = { workspace = true }` to `zerobase-core/Cargo.toml`

### Enhanced `JsonOptions` struct
- Added optional `schema: Option<serde_json::Value>` field for JSON Schema definitions
- Implemented `validate()` method for option-level validation (checks schema is a valid JSON object and compiles correctly)
- Enhanced `validate_value()` to validate record values against the provided schema after size checks
- Updated `FieldType::validate_options()` match arm to call `opts.validate()` instead of `Ok(())`

### Tests Added (14 new tests)
- Schema validates matching objects
- Schema rejects missing required properties
- Schema rejects wrong types
- Schema accepts array schemas with item validation
- No schema accepts anything
- Rejects non-object schema in options validation
- Accepts valid schema in options validation
- Accepts absent schema
- Both max_size and schema constraints apply together
- Enum constraint validation
- Nested object validation with patterns
- Serde round-trip with schema
- Serde round-trip without schema

## Files Modified

1. `Cargo.toml` - Added jsonschema workspace dependency
2. `crates/zerobase-core/Cargo.toml` - Added jsonschema dependency
3. `crates/zerobase-core/src/schema/field.rs` - Enhanced JsonOptions with schema validation + 14 new tests
4. `crates/zerobase-core/src/schema/record_validator.rs` - Updated test to use `..Default::default()` for JsonOptions

## Test Results

- 117 field tests passed
- 65 record_validator tests passed
- Full workspace test suite passed
