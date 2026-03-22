# Database Indexing for Custom Fields

**Task ID:** `ulucf48cjl3ngj8`
**Date:** 2026-03-21
**Status:** Completed

## Summary

Implemented full database indexing support for custom fields, including single and composite indexes with optional sort directions, auto-indexing of fields referenced in access rules, and dedicated index management API endpoints.

## Changes Made

### Core Layer (`zerobase-core`)

**`crates/zerobase-core/src/schema/collection.rs`**
- Added `IndexSortDirection` enum (Asc/Desc) with SQL generation
- Added `IndexColumn` struct with name and direction
- Enhanced `IndexSpec` with dual-mode support: simple `columns: Vec<String>` for backwards compatibility and rich `index_columns: Vec<IndexColumn>` for sort directions
- Added helper methods: `new()`, `unique()`, `with_columns()`, `effective_column_names()`, `effective_index_columns()`, `generate_name()`
- Refactored index validation into shared `validate_indexes()` method
- Added tests for composite indexes, unique indexes, sort directions, and name generation

**`crates/zerobase-core/src/schema/mod.rs`**
- Exported new types: `IndexColumn`, `IndexSortDirection`

**`crates/zerobase-core/src/schema/rules.rs`**
- Added `referenced_fields()` method to `ApiRules` for extracting field names from rule expressions
- Added `extract_field_names()` helper that parses filter expressions to find field identifiers (skips `@`-macros, string literals, keywords)
- Added comprehensive tests for field extraction from various filter patterns

**`crates/zerobase-core/src/services/collection_service.rs`**
- Added `IndexColumnSortDto`, `IndexColumnDto` types for DTO layer
- Updated `IndexDto` with `index_columns: Vec<IndexColumnDto>` field
- Updated `collection_to_schema()` to preserve sort direction info and auto-generate indexes for fields referenced in access rules
- Updated `schema_to_collection()` to reconstruct rich `IndexSpec` from DTOs
- Added tests for auto-indexing from rules, duplicate prevention, and sort direction round-trips

### DB Layer (`zerobase-db`)

**`crates/zerobase-db/src/lib.rs`**
- Added `IndexColumnSort` enum (Asc/Desc) with SQL generation
- Added `IndexColumnDef` struct for column-level sort direction
- Enhanced `IndexDef` with `index_columns: Vec<IndexColumnDef>` field
- Added `column_exprs()` and `column_names()` helper methods to `IndexDef`

**`crates/zerobase-db/src/schema_repo.rs`**
- Updated `create_index()` to use `column_exprs()` for SQL generation (supports sort directions)
- Updated `load_indexes()` to parse sort directions from `sqlite_master` SQL
- Added all `IndexColumnDef`/`IndexColumnSort` imports
- Added all `index_columns: vec![]` fields to existing `IndexDef` struct literals
- Added comprehensive tests:
  - DESC sort direction index creation and round-trip
  - Composite indexes with mixed ASC/DESC directions
  - Index survival through table rebuild (ALTER TABLE)
  - Index improves query performance (EXPLAIN QUERY PLAN verification)
  - Index drop on collection update

### API Layer (`zerobase-api`)

**`crates/zerobase-api/src/handlers/collections.rs`**
- Added `GET /api/collections/:id_or_name/indexes` — list all indexes
- Added `POST /api/collections/:id_or_name/indexes` — add an index
- Added `DELETE /api/collections/:id_or_name/indexes/:index_pos` — remove an index by position

**`crates/zerobase-api/src/lib.rs`**
- Registered index management routes in `collection_routes()`

## Test Results

All 1,207 workspace tests pass (763 core + 313 db + 131 others).

New tests added:
- 6 tests for IndexSpec enhancements (composite, unique, sort directions, name generation)
- 6 tests for field extraction from access rules
- 4 tests for auto-indexing in collection service
- 5 tests for DB-layer index operations (sort directions, composite, rebuild, performance, drop)

## Architecture Decisions

1. **Backwards-compatible IndexSpec**: Both simple `columns` and rich `index_columns` are supported. The `effective_*` methods transparently resolve the active representation.
2. **Auto-indexing from rules**: When a collection has filter rules referencing fields, single-column indexes are automatically created for those fields (unless already indexed).
3. **Sort direction support**: Full ASC/DESC support flows through all layers (core -> DTO -> DB -> SQLite DDL).
4. **Index management via API**: Dedicated endpoints allow adding/removing individual indexes without full collection updates.
