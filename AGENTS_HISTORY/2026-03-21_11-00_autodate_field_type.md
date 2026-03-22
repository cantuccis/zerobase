# AutoDate Field Type Implementation

**Date:** 2026-03-21 11:00 UTC
**Task ID:** kx5dzvsgv5lqm7u

## Summary

Implemented the AutoDate field type that auto-populates with current UTC datetime on record creation and/or update. The field is configurable via `on_create` and `on_update` boolean options. Manual values are always overridden by the system-generated timestamp.

## Changes Made

### `crates/zerobase-core/src/schema/field.rs`
- Added `validate_value()` method to `AutoDateOptions` — validates that stored values are valid datetime strings
- Added `now_utc()` static method to `AutoDateOptions` — generates current UTC datetime in `YYYY-MM-DD HH:MM:SS` format
- Updated `FieldType::validate_value()` to delegate to `AutoDateOptions::validate_value()` instead of returning `Ok(())` unconditionally
- Added 12 new tests for AutoDate:
  - `autodate_accepts_update_only`
  - `autodate_validate_value_accepts_valid_datetime`
  - `autodate_validate_value_accepts_date_only`
  - `autodate_validate_value_accepts_rfc3339`
  - `autodate_validate_value_rejects_non_string`
  - `autodate_validate_value_rejects_invalid_format`
  - `autodate_validate_value_rejects_boolean`
  - `autodate_now_utc_produces_valid_datetime`
  - `autodate_serialization_roundtrip`
  - `autodate_deserialization_defaults`
  - `autodate_field_type_validate_value_delegates`

### `crates/zerobase-core/src/schema/record_validator.rs`
- Added `OperationContext` enum with `Create` and `Update` variants
- Added `apply_auto_dates()` method — injects current timestamp into AutoDate fields based on operation context
- Added `validate_and_prepare_with_context()` method — combines auto-date injection, sanitization, and validation in one call
- Added 17 new tests:
  - `apply_auto_dates_injects_on_create`
  - `apply_auto_dates_skips_on_create_when_update_only`
  - `apply_auto_dates_injects_on_update`
  - `apply_auto_dates_skips_on_update_when_create_only`
  - `apply_auto_dates_both_on_create`
  - `apply_auto_dates_both_on_update`
  - `apply_auto_dates_overrides_manual_value`
  - `apply_auto_dates_ignores_non_autodate_fields`
  - `apply_auto_dates_noop_on_non_object`
  - `validate_and_prepare_with_context_injects_autodate_on_create`
  - `validate_and_prepare_with_context_injects_autodate_on_update`
  - `validate_and_prepare_with_context_overrides_manual_autodate`
  - `validate_and_prepare_with_context_does_not_set_update_only_on_create`
  - `validate_and_prepare_with_context_does_not_set_create_only_on_update`
  - `validate_and_prepare_with_context_rejects_manual_autodate_not_in_schema`
  - `validate_and_prepare_with_context_autodate_not_flagged_as_unknown`
  - `validate_and_prepare_with_context_multiple_autodate_fields`
  - `validate_and_prepare_with_context_still_validates_other_fields`

### `crates/zerobase-core/src/schema/mod.rs`
- Exported `OperationContext` from the schema module

## Test Results

All 413 tests in zerobase-core pass. Full workspace test suite is green.

## Files Modified
- `crates/zerobase-core/src/schema/field.rs`
- `crates/zerobase-core/src/schema/record_validator.rs`
- `crates/zerobase-core/src/schema/mod.rs`
