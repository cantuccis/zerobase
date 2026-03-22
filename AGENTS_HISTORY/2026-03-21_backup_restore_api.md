# Backup and Restore API

**Date**: 2026-03-21
**Task ID**: a0k6x1nkdmzcy6j
**Status**: Completed

## Summary

Implemented full backup and restore functionality for Zerobase, allowing superusers to create, list, download, delete, and restore database backups via admin API endpoints.

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/_/api/backups` | Create a new backup (optional `name` in body) |
| `GET` | `/_/api/backups` | List all backups (newest first) |
| `GET` | `/_/api/backups/:name` | Download a backup file |
| `DELETE` | `/_/api/backups/:name` | Delete a backup |
| `POST` | `/_/api/backups/:name/restore` | Restore database from backup |

All endpoints require superuser authentication (`require_superuser` middleware).

## Architecture

Follows the established repository-trait pattern:

1. **`zerobase-core/src/services/backup_service.rs`** — `BackupRepository` trait, `BackupService<R>`, DTOs (`BackupInfo`, `CreateBackupRequest`), name validation (rejects path traversal, invalid extensions, null bytes), auto-name generation (`pb_backup_<timestamp>.db`), error mapping to `ZerobaseError`.

2. **`zerobase-db/src/backup_repo.rs`** — `BackupRepository` implementation for `Database` using SQLite's online backup API (`rusqlite::backup::Backup`). Backups stored in `pb_backups/` directory alongside the database file. Restore uses the backup API in reverse, writing into the live write connection.

3. **`zerobase-api/src/handlers/backups.rs`** — Axum HTTP handlers for all 5 endpoints. File downloads use `tokio::fs::read` with `Content-Disposition` header.

4. **`zerobase-api/src/lib.rs`** — `backup_routes()` function following the same pattern as `settings_routes()`, `collection_routes()`, etc.

## Changes

### New files
- `crates/zerobase-core/src/services/backup_service.rs` — Service + trait + DTOs + 23 unit tests
- `crates/zerobase-db/src/backup_repo.rs` — SQLite backup implementation + 12 unit tests
- `crates/zerobase-api/src/handlers/backups.rs` — HTTP handlers
- `crates/zerobase-api/tests/backup_endpoints.rs` — 18 integration tests

### Modified files
- `Cargo.toml` — Added `backup` feature to `rusqlite`
- `crates/zerobase-core/src/services/mod.rs` — Added `pub mod backup_service` + re-export
- `crates/zerobase-core/src/lib.rs` — Added `pub use services::BackupService`
- `crates/zerobase-db/src/lib.rs` — Added `pub mod backup_repo`
- `crates/zerobase-db/src/pool.rs` — Added `db_path: Option<PathBuf>` field to `Database`, made `write_conn` `pub(crate)`
- `crates/zerobase-api/src/handlers/mod.rs` — Added `pub mod backups`
- `crates/zerobase-api/src/lib.rs` — Added `backup_routes()` function + imports

## Tests

- **23** unit tests in `backup_service.rs` (name validation, CRUD operations, error mapping)
- **12** unit tests in `backup_repo.rs` (SQLite backup/restore, file operations, edge cases)
- **18** integration tests in `backup_endpoints.rs` (full HTTP stack with mock repo, auth enforcement, lifecycle)

All tests pass. Full workspace compilation succeeds with 0 errors.

## Key Design Decisions

- **SQLite backup API**: Uses `rusqlite::backup::Backup` for consistent, hot backups without locking readers.
- **Backup directory**: `pb_backups/` alongside the database file, matching PocketBase convention.
- **Name validation**: Prevents path traversal (`..`, `/`, `\`), null bytes, overly long names, and requires `.db` or `.zip` extension.
- **In-memory databases**: Gracefully rejected (backup/restore not supported).
- **Restore via write connection**: Restores directly into the live write connection using the backup API in reverse, preserving the connection pool.
