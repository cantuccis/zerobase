# Multi-Relation Add/Remove Modifiers

**Date:** 2026-03-21
**Task ID:** i47ylmaqvt76xiv
**Phase:** 8

## Summary

Implemented `+` and `-` modifier support for multi-relation fields in record update operations, matching PocketBase's behavior. When updating a record, users can now use `field+` to add relation IDs and `field-` to remove relation IDs without replacing the entire array.

### Features Implemented

- **Add modifier (`field+`)**: Appends one or more IDs to a multi-relation field, with automatic deduplication
- **Remove modifier (`field-`)**: Removes one or more IDs from a multi-relation field; removing a non-existent ID is a no-op
- **Combined modifiers**: Both `+` and `-` can be used on the same field in a single update
- **Mixed updates**: Modifiers can be combined with plain field updates in the same request
- **Validation**: Enforces `max_select` limits, rejects modifiers on non-relation or single-relation fields
- **Error handling**: Clear validation errors for unknown fields, non-relation fields, invalid value types

### Acceptance Criteria Met

- Add modifiers work correctly (single ID and array of IDs)
- Remove modifiers work correctly (single ID and array of IDs)
- Duplicates prevented on add
- Removing non-existent ID is a no-op
- max_select limits enforced
- 19 new tests pass, 113 existing tests unaffected

## Files Modified

- `crates/zerobase-core/src/services/record_service.rs`
  - Modified `update_record()` to process modifier keys before standard merge
  - Added `apply_relation_modifiers()` function
  - Added `extract_modifier_ids()` helper function
  - Added 19 tests: 14 integration tests via `RecordService` and 5 unit tests for `apply_relation_modifiers`

## Test Results

- 132 record_service tests pass (19 new + 113 existing)
- All API integration tests pass
