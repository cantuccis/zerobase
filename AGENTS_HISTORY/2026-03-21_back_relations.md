# Back-Relations Implementation

**Date:** 2026-03-21
**Task ID:** 8tblkknqeo1mat9
**Phase:** 8

## Summary

Enhanced the existing back-relation expansion system with pagination support, SQL-level query limits, and comprehensive test coverage. Back-relations allow expanding records from another collection that reference the current record using the `?expand=collectionName_via_fieldName` syntax.

## Changes Made

### 1. Pagination / Limit Support

- **`crates/zerobase-core/src/services/record_service.rs`**: Added `find_referencing_records_limited()` default method to `RecordRepository` trait with backward-compatible default implementation that delegates to `find_referencing_records()` and truncates results.

- **`crates/zerobase-core/src/services/expand.rs`**: Added `MAX_BACK_RELATION_EXPAND = 100` constant. Updated back-relation expansion to use `find_referencing_records_limited()` instead of the unlimited version.

- **`crates/zerobase-db/src/record_repo.rs`**: Overrode `find_referencing_records_limited()` in the `Database` implementation with an optimized version that pushes `LIMIT` into the SQL query for efficiency (avoids fetching unbounded rows).

### 2. New Unit Tests (expand.rs)

- `expand_back_relation_no_matches_excluded` — empty back-relations don't appear in expand map
- `expand_back_relation_via_multi_relation_field` — back-refs through JSON array fields work
- `expand_back_relation_wrong_collection_ignored` — silently ignores invalid targets
- `expand_combined_forward_and_back_relation` — forward + back-relation in same expand
- `expand_back_relation_limit_enforced` — verifies MAX_BACK_RELATION_EXPAND cap
- `expand_back_relation_collection_metadata_present` — collectionId/collectionName set
- `expand_back_relation_always_returns_array` — even single results are arrays
- `expand_records_batch_with_back_relations` — batch expand with back-relations
- `parse_back_relation_multiple_underscores_in_collection` — complex naming patterns

### 3. New Integration Tests (records_endpoints.rs)

- `expand_back_relation_on_list` — back-relations on list endpoint with per-author verification
- `expand_back_relation_includes_collection_metadata` — API response metadata check
- `expand_combined_forward_and_back_relation_on_view` — combined expand on view endpoint
- `expand_back_relation_no_matches_no_expand_field` — no expand field when empty

## Files Modified

- `crates/zerobase-core/src/services/record_service.rs` — Added `find_referencing_records_limited` trait method
- `crates/zerobase-core/src/services/expand.rs` — Added constant, pagination, 9 new unit tests
- `crates/zerobase-db/src/record_repo.rs` — Added optimized SQL LIMIT override
- `crates/zerobase-api/tests/records_endpoints.rs` — Added 4 new integration tests

## Test Results

All tests pass (0 failures across entire workspace).
