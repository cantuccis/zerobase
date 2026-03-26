# Replace panic!/assert! with Result in sanitize_table_name

**Date:** 2026-03-22
**Task ID:** 8tstwsur4l8so1c
**Phase:** 14

## Summary

Replaced `assert!` macros in `sanitize_table_name` with proper `Result` error handling to prevent a single malformed collection name from crashing the entire server. This is a defense-in-depth improvement — inputs are validated at the schema layer, but the function no longer panics if invalid data somehow reaches it.

## Changes Made

### `crates/zerobase-db/src/record_repo.rs`

1. **Changed function signature**: `fn sanitize_table_name(name: &str) -> &str` → `fn sanitize_table_name(name: &str) -> std::result::Result<&str, RecordRepoError>`

2. **Replaced `assert!` macros** with `if` checks that return `Err(RecordRepoError::Database { ... })` for:
   - Empty table name
   - Forbidden characters: `"`, `;`, `\0`, `\\`, `\n`, `\r`

3. **Updated all 10 call sites** to propagate errors with `?`:
   - `find_one` (line 30)
   - `find_many` (line 61)
   - `insert` (line 188)
   - `update` (line 222)
   - `delete` (line 260)
   - `count` (line 279)
   - `find_referencing_records` (lines 312-313)
   - `find_referencing_records_limited` (lines 357-358)

4. **Updated 7 existing tests** from `#[should_panic]` to proper `Result` assertions using `unwrap_err()` and pattern matching.

5. **Added 1 new test**: `sanitize_table_name_rejects_carriage_return` to cover `\r` which was guarded but not previously tested.

## Files Modified

- `crates/zerobase-db/src/record_repo.rs`

## Test Results

- All 8 `sanitize_table_name` tests pass
- All 395+ unit tests in `zerobase-db` pass
- All 22 integration tests pass
- Full workspace builds successfully
