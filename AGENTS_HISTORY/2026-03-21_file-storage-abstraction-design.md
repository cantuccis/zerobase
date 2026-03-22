# File Storage Abstraction Design

**Date:** 2026-03-21
**Task ID:** qjqi1xyrd16a9nf
**Phase:** 7

## Summary

Designed and implemented the trait-based file storage abstraction for Zerobase. This includes:

1. **FileStorage trait** defined in `zerobase-core` with async methods: `upload`, `download`, `delete`, `exists`, `generate_url`, and `delete_prefix`
2. **LocalFileStorage** — fully implemented local filesystem backend using `tokio::fs`
3. **S3FileStorage** — stub implementation with planned design for S3-compatible storage
4. **FileService** — high-level service handling upload validation, filename generation, and record integration
5. **Thumbnail support** — helper types (`ThumbSize`, `ThumbMode`) and key generation for thumbnails
6. **File metadata types** — `FileMetadata`, `FileDownload`, `FileUpload`, `StorageError`
7. **Error integration** — `StorageError` converts to `ZerobaseError` with correct HTTP status codes
8. **Design document** — comprehensive plan covering local/S3 implementations, record attachment flow, protected files, and thumbnails

All code compiles and all 18 tests pass.

## Files Modified

- `crates/zerobase-core/src/storage.rs` — **NEW** — FileStorage trait, StorageError, supporting types, helper functions, tests
- `crates/zerobase-core/src/lib.rs` — Added `storage` module and re-exports
- `crates/zerobase-files/src/lib.rs` — Updated with module declarations and re-exports
- `crates/zerobase-files/src/local.rs` — **NEW** — LocalFileStorage implementation with tests
- `crates/zerobase-files/src/s3.rs` — **NEW** — S3FileStorage stub with planned design
- `crates/zerobase-files/src/service.rs` — **NEW** — FileService with upload validation, tests using in-memory mock
- `crates/zerobase-files/src/thumb.rs` — **NEW** — Thumbnail helpers and key generation
- `crates/zerobase-files/Cargo.toml` — Added `async-trait` and `tempfile` dependencies
- `docs/plans/2026-03-21-file-storage-abstraction.md` — **NEW** — Full design document
