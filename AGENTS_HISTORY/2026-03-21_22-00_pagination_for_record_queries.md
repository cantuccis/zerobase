# Implement Pagination for Record Queries

**Date:** 2026-03-21 22:00
**Task ID:** 2dbg52v111r5tay
**Phase:** 4

## Summary

Implemented full pagination support for record queries with `page` and `perPage` parameters, matching PocketBase behavior. Updated the default `perPage` from 20 to 30, enforced max `perPage` of 500, and ensured pagination metadata (`page`, `perPage`, `totalPages`, `totalItems`) is accurately returned. Added a serializable `RecordList` with camelCase JSON output and an API handler for listing records with query parameters. Wrote 22 new tests covering all pagination edge cases.

## Changes Made

### Core Crate (`zerobase-core`)

- **`crates/zerobase-core/src/services/record_service.rs`**
  - Added `DEFAULT_PER_PAGE = 30` and `MAX_PER_PAGE = 500` constants
  - Added `#[derive(serde::Serialize)]` with `#[serde(rename_all = "camelCase")]` to `RecordList` for JSON API responses
  - Updated `list_records` to use the new constants
  - Fixed mock `find_many` to return `total_pages=1` for empty collections (matching real DB behavior)
  - Added 6 new service-level pagination tests:
    - `list_records_default_per_page_is_30`
    - `list_records_max_per_page_is_500`
    - `list_records_page_beyond_last_returns_empty`
    - `list_records_empty_collection_returns_metadata`
    - `list_records_per_page_at_boundary_500`
    - `record_list_serializes_to_camel_case_json`

- **`crates/zerobase-core/src/services/mod.rs`**
  - Exported `RecordList`, `RecordQuery`, `SortDirection`, `DEFAULT_PER_PAGE`, `MAX_PER_PAGE`

### DB Crate (`zerobase-db`)

- **`crates/zerobase-db/src/record_repo.rs`**
  - Added 12 new integration tests against real SQLite:
    - `find_many_page_beyond_last_returns_empty_items`
    - `find_many_last_page_partial_results`
    - `find_many_page_zero_clamps_to_one`
    - `find_many_per_page_zero_clamps_to_one`
    - `find_many_per_page_clamped_to_500`
    - `find_many_single_item_single_page`
    - `find_many_exact_page_boundary`
    - `find_many_total_pages_is_1_for_empty_table`
    - `find_many_with_filter_pagination_metadata_reflects_filtered_count`
    - `find_many_large_page_number_returns_empty`
    - `find_many_per_page_equals_total_items`
    - `find_many_per_page_1_creates_many_pages`

### API Crate (`zerobase-api`)

- **`crates/zerobase-api/src/handlers/mod.rs`** (new)
  - Handler module declaration

- **`crates/zerobase-api/src/handlers/records.rs`** (new)
  - `ListRecordsParams` struct for deserializing camelCase query params (`page`, `perPage`, `sort`, `filter`)
  - `list_records` handler for `GET /api/collections/:name/records`
  - `error_response` helper for converting `ZerobaseError` to HTTP responses
  - 4 unit tests for param parsing

- **`crates/zerobase-api/src/lib.rs`**
  - Added `pub mod handlers`

## Test Results

- **1022+ tests passing** across all lib tests
- 22 new tests added (12 DB integration + 6 service + 4 API handler)
- Pre-existing tracing integration test failure is unrelated to this change

## Files Modified

1. `crates/zerobase-core/src/services/record_service.rs`
2. `crates/zerobase-core/src/services/mod.rs`
3. `crates/zerobase-db/src/record_repo.rs`
4. `crates/zerobase-api/src/lib.rs`
5. `crates/zerobase-api/src/handlers/mod.rs` (new)
6. `crates/zerobase-api/src/handlers/records.rs` (new)
