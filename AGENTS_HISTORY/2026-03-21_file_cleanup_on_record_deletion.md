# File Cleanup on Record Deletion

**Date:** 2026-03-21
**Task ID:** bw7mv7u8gfdixpw

## Summary

Implemented graceful file cleanup when records with file fields are deleted. Storage errors are now logged with `tracing::warn` instead of being silently swallowed, and they never block the record deletion response (HTTP 204). Added comprehensive tests covering the happy path, failure scenarios, and edge cases.

## Changes Made

### Modified Files

1. **`crates/zerobase-api/src/handlers/records.rs`**
   - Added `use tracing::warn;` import
   - `delete_record` handler: Replaced `let _ = ...` with `if let Err(e) = ...` + `warn!()` structured log for file cleanup failures
   - `update_record` handler: Same pattern for orphaned file cleanup on record updates

2. **`crates/zerobase-files/src/service.rs`**
   - Added `FailingDeleteStorage` mock that always errors on `delete`/`delete_prefix`
   - Added unit test `delete_record_files_propagates_storage_error` — verifies errors are propagated to callers
   - Added unit test `delete_file_propagates_storage_error` — verifies individual file deletion error propagation

3. **`crates/zerobase-api/tests/file_upload_endpoints.rs`**
   - Added `FailingDeleteStorage` mock for integration testing
   - Added `spawn_file_app_dyn` helper accepting `Arc<dyn FileStorage>`
   - Added integration test `delete_record_succeeds_even_when_file_cleanup_fails` — core acceptance criterion
   - Added integration test `delete_record_cleans_up_multiple_file_fields` — verifies cleanup across single-file and multi-file fields
   - Added integration test `delete_record_without_files_succeeds` — verifies no-op cleanup doesn't affect other records

## Test Results

- **zerobase-files unit tests:** 98 passed, 0 failed
- **file_upload_endpoints integration tests:** 9 passed, 0 failed (3 new)

## Acceptance Criteria

- [x] Files removed when record deleted
- [x] Storage errors don't block record deletion
- [x] Errors are logged (not silently swallowed)
- [x] Tests pass
