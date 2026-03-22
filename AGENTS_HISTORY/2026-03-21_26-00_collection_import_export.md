# Collection Import/Export Implementation

**Date:** 2026-03-21
**Task ID:** 9awm2k0my8v6ljl

## Summary

Implemented admin endpoints for importing/exporting collection schemas as JSON. The feature supports exporting all non-system collections and importing collections (creating new or updating existing ones). All imported schemas are validated before any changes are applied, ensuring atomicity.

## What Was Done

### Service Layer (`zerobase-core`)
- Added `export_collections()` method to `CollectionService` — exports all non-system collections
- Added `import_collections()` method to `CollectionService` — creates or updates collections with upfront validation
  - System collections (names starting with `_`) are rejected
  - All collections are validated before any changes are applied (atomic validation)
  - Empty IDs are auto-assigned on import
  - Existing collections are updated, new ones are created

### API Layer (`zerobase-api`)
- Added `GET /api/collections/export` endpoint — returns `{ "collections": [...] }`
- Added `PUT /api/collections/import` endpoint — accepts `{ "collections": [...] }`, returns imported state
- Added `ExportCollectionsResponse` and `ImportCollectionsBody` request/response types
- Routes registered before wildcard `{id_or_name}` to avoid path conflicts
- All endpoints protected by superuser auth middleware

### Tests Added (25+ new tests)
**Service-level tests:**
- Export empty returns empty list
- Export excludes system collections
- Export returns all user collections
- Import creates new collections
- Import updates existing collections
- Import handles mixed create/update
- Import rejects system collections
- Import rejects invalid names
- Import rejects duplicate fields
- Import aborts entirely on any validation failure
- Import empty list succeeds
- Import assigns ID when empty
- Import preserves existing ID
- Export-then-import round trip

**Handler-level tests:**
- Export returns 200 with empty list
- Export returns non-system collections
- Export response contains `collections` key
- Import creates new collections returns 200
- Import updates existing collection
- Import rejects invalid collection returns 400
- Import rejects system collection returns 400
- Import empty list returns 200
- Import invalid JSON returns 400
- Import multiple collections

## Files Modified

- `crates/zerobase-core/src/services/collection_service.rs` — Added `export_collections()` and `import_collections()` methods + 14 unit tests
- `crates/zerobase-api/src/handlers/collections.rs` — Added import/export handlers, request/response types + 11 handler tests
- `crates/zerobase-api/src/lib.rs` — Registered new routes for `/api/collections/export` and `/api/collections/import`

## Test Results

All 1,179 tests pass (0 failures).
