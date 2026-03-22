# Schema Alteration (Add/Remove/Modify Fields)

**Date:** 2026-03-21 04:00
**Task ID:** e6sxmxq7txiohs9

## Summary

Implemented comprehensive schema alteration support for Zerobase collections, including field renaming, type changes with data migration, and constraint modifications. Added the `alter_collection` method to the `SchemaRepository` trait and a full implementation using SQLite's table rebuild strategy.

## What Was Done

### 1. SchemaAlteration Type (`lib.rs`)
- Added `SchemaAlteration` struct with rename mappings (`Vec<(old_name, new_name)>`)
- Added `alter_collection` method to `SchemaRepository` trait with full documentation

### 2. alter_collection Implementation (`schema_repo.rs`)
- `alter_collection_impl`: Validates rename mappings, updates metadata, rebuilds table
- `build_copy_expressions`: Builds column mapping for data copy step, handling renames and type changes
- `build_cast_expression`: Generates safe SQL CAST expressions with graceful handling of incompatible values (NULL fallback)
- `alter_user_table_with_renames`: Rename-aware table rebuild using SQLite's 12-step process

### 3. Bug Fixes to Existing Code
- Fixed `alter_user_table` to properly copy auth system columns (email, password, etc.) during rebuild
- Fixed `alter_user_table` to recreate system indexes (created, email, tokenKey) after rebuild

### 4. Type Conversion Support
- TEXT → INTEGER: Converts numeric text, NULL for non-numeric
- TEXT → REAL: Converts numeric text (including decimals), NULL for non-numeric
- INTEGER → TEXT: Always succeeds via CAST
- REAL → TEXT: Always succeeds via CAST
- INTEGER → REAL: Always succeeds via CAST
- Any → unknown type: Pass-through CAST

### 5. Tests (32 new tests, 181 total)
- **Field renaming**: single, multiple, rename+add, rename+remove, rename with collection rename
- **Validation**: nonexistent source field, missing target in schema, nonexistent collection, name conflicts
- **Type changes**: TEXT→REAL, TEXT→INTEGER, REAL→TEXT, INTEGER→TEXT, INTEGER→REAL, rename+type change
- **Constraint changes**: add/remove NOT NULL, add UNIQUE, add DEFAULT
- **Auth collections**: preserves system columns, rename user fields, preserves email index
- **Data preservation**: multiple rows (100), NULL values, timestamps
- **Indexes**: preserves user indexes, adds new indexes
- **Edge cases**: no-op alteration, add-only, remove-only, column reorder, complex multi-operation

## Files Modified

- `crates/zerobase-db/src/lib.rs` — Added `SchemaAlteration` struct and `alter_collection` to `SchemaRepository` trait
- `crates/zerobase-db/src/schema_repo.rs` — Added implementation + 32 tests, fixed auth column handling in existing rebuild
