# File Upload in Record Create/Update/Delete Endpoints

**Date:** 2026-03-21
**Task ID:** wrrkpyk6fmtanwp
**Phase:** 7

## Summary

Extended record create, update, and delete endpoints to handle file uploads via `multipart/form-data`, integrating with the existing `FileService` for validation, storage, and cleanup.

## Changes Made

### `crates/zerobase-api/src/handlers/records.rs`
- Added `RecordState<R, S>` struct combining `RecordService` + optional `FileService` (replaces bare `Arc<RecordService>` as handler state)
- `create_record`: now accepts both `application/json` and `multipart/form-data`. When multipart, extracts files via `extract_multipart()`, creates the record, uploads files via `FileService::process_uploads()`, then updates the record with generated filenames.
- `update_record`: same dual content-type support. Uploads new files, merges filenames, cleans up old files that are no longer referenced.
- `delete_record`: after deleting the record, calls `FileService::delete_record_files()` to clean up all associated files.
- All read-only handlers (`list_records`, `view_record`, `count_records`) updated to use `RecordState` instead of `Arc<RecordService>`.
- Added `merge_file_names()` helper: merges uploaded file names into record data (string for single-file fields, array for multi-file fields based on `max_select`).
- Added `collect_existing_filenames()` helper: collects filenames from file-type fields in a record for cleanup comparison.

### `crates/zerobase-api/src/handlers/multipart.rs` (created in prior step)
- Multipart extraction module that separates file uploads from regular form fields.

### `crates/zerobase-api/src/lib.rs`
- `record_routes()`: preserved backward compatibility, delegates to `record_routes_with_files(service, None)`.
- Added `record_routes_with_files()`: accepts optional `FileService` to enable file upload support.
- Re-exported `RecordState` for external use.

### `crates/zerobase-api/tests/file_upload_endpoints.rs` (new)
- Integration tests with `MemoryStorage`, `MockRecordRepo`, and `MockSchemaLookup`.
- Tests: JSON-only create still works, multipart file upload on create, file size validation, multi-file gallery upload, file cleanup on delete, file upload on update.

## Architecture Decisions

- **Dual content-type support**: Handlers inspect `Content-Type` header to dispatch between JSON body parsing and multipart extraction. This avoids separate endpoints.
- **Create-then-update for files**: On create with files, the record is created first (to get its ID for the storage key), then files are uploaded, then the record is updated with filenames. This matches PocketBase's approach.
- **Optional FileService**: The `RecordState` holds `Option<Arc<FileService>>`, so file support is opt-in. When `None`, file parts in multipart requests are silently ignored and the system behaves as before.
- **Old file cleanup on update**: When a record is updated with new file uploads, old filenames that are no longer present in the updated record are deleted from storage.

## Test Results

All 6 new file upload tests pass. All existing workspace tests continue to pass.
