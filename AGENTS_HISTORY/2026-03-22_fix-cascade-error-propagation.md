# Fix Silent Cascade Delete/Update Error Handling

**Date:** 2026-03-22
**Task ID:** c4ajmx1k6gpmd43

## Summary

Fixed silent error handling in `record_service.rs` where cascade delete and cascade set-null operations discarded `Result` values using `let _ =`. This could leave the database in an inconsistent state: the parent record would be deleted while referencing records retained stale foreign keys or orphaned references.

## Changes Made

### Bug Fix

**File:** `crates/zerobase-core/src/services/record_service.rs`

1. **Line 1164 (cascade delete):** Replaced `let _ = self.delete_record(ref_collection_name, ref_id);` with `self.delete_record(ref_collection_name, ref_id)?;` — cascade delete errors now propagate, preventing the parent delete from proceeding.

2. **Line 1194 (cascade set-null):** Replaced `let _ = self.repo.update(ref_collection_name, ref_id, &update_data);` with `self.repo.update(ref_collection_name, ref_id, &update_data)?;` — set-null update errors now propagate, preventing the parent delete from proceeding.

### Integration Tests Added

Added 3 new integration tests with a `FailingMockRecordRepo` that can simulate failures on specific collections:

- `cascade_delete_failure_propagates_error` — Verifies that when a cascade delete fails, the error is surfaced (500 status) and the parent record remains intact.
- `set_null_failure_propagates_error` — Verifies that when a set-null update fails, the error is surfaced and the parent record + referencing record are unchanged.
- `cascade_delete_failure_does_not_leave_partial_state` — Verifies that with multiple referencing records, a cascade failure leaves all records (parent + children) intact.

## Files Modified

- `crates/zerobase-core/src/services/record_service.rs`

## Test Results

All 138 record_service tests pass, including the 3 new ones.
