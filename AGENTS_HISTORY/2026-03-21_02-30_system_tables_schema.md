# System Tables Schema Implementation

**Date:** 2026-03-21 02:30
**Task:** Implement system tables schema (lk92yys4b0udssd)

## Summary

Verified and enhanced the system tables schema implementation. The core system tables (`_collections`, `_fields`, `_settings`, `_superusers`, `_migrations`) were already created via the migration system (v1). Added 25 additional tests to thoroughly verify column types, NOT NULL constraints, default values, primary keys, foreign key integrity, auto-set timestamps, and cross-table integrity.

## System Tables

### `_collections`
- `id` (TEXT PK NOT NULL), `name` (TEXT NOT NULL UNIQUE), `type` (TEXT NOT NULL, CHECK base/auth/view, default 'base'), `system` (INTEGER NOT NULL, default 0), `list_rule`/`view_rule`/`create_rule`/`update_rule`/`delete_rule` (TEXT, nullable = locked), `created`/`updated` (TEXT NOT NULL, auto-set datetime)
- Indexes: `idx_collections_name`, `idx_collections_type`

### `_fields`
- `id` (TEXT PK NOT NULL), `collection_id` (TEXT NOT NULL, FK -> _collections ON DELETE CASCADE), `name` (TEXT NOT NULL), `type` (TEXT NOT NULL), `required` (INTEGER NOT NULL, default 0), `unique_field` (INTEGER NOT NULL, default 0), `options` (TEXT NOT NULL, default '{}'), `sort_order` (INTEGER NOT NULL, default 0), `created`/`updated` (TEXT NOT NULL, auto-set datetime)
- UNIQUE constraint on (collection_id, name)
- Indexes: `idx_fields_collection`, `idx_fields_type`

### `_settings`
- `key` (TEXT PK NOT NULL), `value` (TEXT NOT NULL, default '{}'), `updated` (TEXT NOT NULL, auto-set datetime)

### `_superusers`
- `id` (TEXT PK NOT NULL), `email` (TEXT NOT NULL UNIQUE), `password` (TEXT NOT NULL), `created`/`updated` (TEXT NOT NULL, auto-set datetime)
- Index: `idx_superusers_email`

### `_migrations`
- `version` (INTEGER PK), `name` (TEXT NOT NULL), `applied_at` (TEXT NOT NULL, auto-set datetime)

## Test Results

**112 total tests pass** (25 new tests added in this task):
- Column type verification (TEXT PK, INTEGER, etc.)
- NOT NULL constraint verification
- Default value verification (type='base', system=0, required=0, options='{}', etc.)
- Nullable rule columns (locked by default)
- Auto-set timestamps on all tables
- Foreign key rejection of invalid collection_id
- Cascade delete correctness across collections
- Full schema round-trip (collection + fields + settings + superuser)
- Total table count verification (exactly 5)
- Total index count verification (all 5 explicit indexes present)

## Files Modified

- `crates/zerobase-db/src/migrations/system.rs` — Added 25 new structural verification tests
