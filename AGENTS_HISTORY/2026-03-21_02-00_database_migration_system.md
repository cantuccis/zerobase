# Database Migration System Implementation

**Date:** 2026-03-21 02:00
**Task ID:** bmvrnwg0rl30so4

## Summary

Implemented a database migration system that tracks applied migrations in a `_migrations` table. Migrations are Rust functions (not SQL files) for type safety, with support for both raw SQL and Rust function migrations. The system includes an initial migration that creates all system tables.

## What was done

### 1. Enhanced Migration struct (`migrations/mod.rs`)
- Added `MigrationAction` enum supporting both `Sql(&'static str)` and `Function(fn(&Connection) -> Result<()>)` variants
- Added convenience constructors `Migration::sql()` and `Migration::function()`
- Added `applied_migrations()` function to list all applied migrations
- Implemented `Debug` for `MigrationAction`
- Maintained full backward compatibility with existing migration runner

### 2. Created system migrations (`migrations/system.rs`)
- Initial migration (v1: `create_system_tables`) creates all five system tables:
  - `_migrations` — tracked automatically by the runner
  - `_collections` — collection metadata with type CHECK constraint (base/auth/view), access rules, unique name
  - `_fields` — field definitions with CASCADE DELETE on collection, unique (collection_id, name) constraint
  - `_settings` — key-value application settings with JSON values
  - `_superusers` — admin accounts with unique email constraint
- All tables have appropriate indexes for query performance
- Foreign key relationships enforced (fields → collections with CASCADE DELETE)

### 3. Added Database integration methods (`pool.rs`)
- `Database::run_system_migrations()` — runs built-in system migrations
- `Database::run_migrations()` — runs arbitrary migration lists
- `Database::migration_version()` — returns current schema version

### 4. Comprehensive test coverage (42 new tests, 87 total in zerobase-db)
- Migration runner: SQL migrations, function migrations, mixed, ordering, idempotency, error handling, rollback
- System tables: column presence, constraints (CHECK, UNIQUE, FK CASCADE), indexes, default values
- Integration: Database pool + migrations working together
- Edge cases: empty lists, duplicate versions, debug formatting

## Files Modified
- `crates/zerobase-db/src/migrations/mod.rs` — Enhanced with MigrationAction enum and function migration support
- `crates/zerobase-db/src/pool.rs` — Added migration convenience methods to Database

## Files Created
- `crates/zerobase-db/src/migrations/system.rs` — System tables migration (v1)

## Test Results
- **87 tests pass** in `zerobase-db` crate
- **156 tests pass** across the full workspace
- All tests are idempotent and use in-memory databases
