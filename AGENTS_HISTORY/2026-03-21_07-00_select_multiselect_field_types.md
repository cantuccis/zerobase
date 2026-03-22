# Implement Select and MultiSelect Field Types

**Date:** 2026-03-21 07:00
**Task ID:** t35g0lsua4jgjgr

## Summary

Separated the existing `Select` field type (which handled both single and multi-select via `max_select`) into two distinct field types:

- **Select**: Single value from a predefined list, stored as plain TEXT in SQLite. Is text-like (empty string treated as "no value" for required checks).
- **MultiSelect**: Multiple values from a predefined list, stored as a JSON array in a TEXT column. Enforces `max_select` limit (0 = no limit), validates all values against allowed list, and rejects duplicate selections.

### Key Changes

1. **SelectOptions** simplified: removed `max_select` field (always single value)
2. **MultiSelectOptions** added: new struct with `values` and `max_select` fields
3. **Duplicate detection** added to MultiSelect value validation
4. **Select is now text-like**: empty string on required field triggers required error
5. All `FieldType` match arms updated (validate_options, validate_value, sql_type, is_text_like, type_name)
6. Serde serialization: Select serializes as `"select"`, MultiSelect as `"multiSelect"`
7. All existing tests updated, 20+ new tests added for both types

### Test Results

- 506 tests pass, 0 failures, 0 warnings

## Files Modified

- `crates/zerobase-core/src/schema/field.rs` — Split SelectOptions, added MultiSelectOptions struct and FieldType::MultiSelect variant with all implementations and tests
- `crates/zerobase-core/src/schema/mod.rs` — Added MultiSelectOptions to public exports
- `crates/zerobase-core/src/schema/record_validator.rs` — Updated select_field helper (removed max_select param), added multiselect_field helper, updated tests
- `crates/zerobase-core/src/schema/collection.rs` — Updated SelectOptions usage (removed max_select)
- `crates/zerobase-core/src/services/collection_service.rs` — Updated SelectOptions usage (removed max_select)
