# Relations & Expansion Integration Tests

**Task ID:** fgn6m585pjiuzgy
**Date:** 2026-03-21
**Status:** Complete

## Summary

Created comprehensive integration test suite for relations and expansion at
`crates/zerobase-api/tests/relations_expansion_integration.rs`.

## Test Coverage (38 tests)

### 1. Forward Relation Expansion (6 tests)
- Single relation expand on view
- Multi-relation expand returns array
- Multiple fields expanded simultaneously
- Expand on list applies to all items
- Missing reference is skipped gracefully
- Null relation is skipped

### 2. Back-Relation Expansion (7 tests)
- `collection_via_field` pattern
- Single match returns array
- No matches yields no expand
- Back-relation on list expands per item
- Back-relation includes collection metadata
- Back-relation via multi-relation field
- Wrong collection is silently ignored

### 3. Nested Expansion (4 tests)
- Two-level forward expansion (dot notation)
- Back-relation with forward expansion
- Combined forward and back-relation
- Complex three-field nested expansion

### 4. Circular Reference & Depth Limits (4 tests)
- Depth limit enforcement (MAX_EXPAND_DEPTH=6)
- Nonexistent field silently ignored
- Non-relation field silently ignored
- No expand param returns no expand field

### 5. Multi-Relation Modifiers (8 tests)
- `field+` append single ID
- `field+` append array of IDs
- `field+` deduplicates existing IDs
- `field-` remove single ID
- `field-` remove array of IDs
- `field-` nonexistent ID is noop
- Combined `field+` and `field-` in same update
- Modifier on single-relation field returns error

### 6. Cascade Delete Behaviors (7 tests)
- Cascade removes referencing records
- SetNull clears relation field
- Restrict prevents delete when references exist
- Restrict allows delete when no references
- NoAction leaves dangling references
- SetNull on multi-relation removes ID from array
- Recursive cascade across three levels

### 7. Expansion Collection Metadata (2 tests)
- Forward expanded records include collectionId/collectionName
- Multi-relation expanded records include metadata

## Technical Notes

- Uses in-memory `MockRecordRepo` and `MockSchemaLookup` with full trait implementations
- Enhanced `MockSchemaLookup` implements `get_collection_by_id` and `list_all_collections` (required for cascade delete via `process_on_delete_actions`)
- All test IDs are exactly 15 characters to satisfy PocketBase ID validation
- Tests spawn isolated HTTP servers on random ports per test
- Fixed initial ID length bug (16 chars exceeded 15-char max, causing 400 errors on write paths)

## Files Modified

- `crates/zerobase-api/tests/relations_expansion_integration.rs` (new, ~1400 lines)
