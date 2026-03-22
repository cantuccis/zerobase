# Record Count Endpoint

**Date:** 2026-03-21
**Task ID:** u30crvtm5rzouna
**Phase:** 4

## Summary

Implemented `GET /api/collections/:collection/records/count` endpoint that returns the total record count for a collection with optional filter support. The endpoint is useful for dashboard widgets and statistics displays.

### What was done

1. **Service layer** (`RecordService::count_records`): Added a new method that verifies the collection exists and delegates to the repository's `count` method with optional filter.

2. **HTTP handler** (`count_records`): New axum handler that accepts an optional `filter` query parameter and returns `{ "totalItems": N }`.

3. **Route registration**: Registered the `/api/collections/{collection_name}/records/count` route before the `{id}` wildcard to prevent "count" from being captured as a record ID.

4. **Tests**: Added 9 new tests total:
   - 3 unit tests in `record_service.rs` (empty collection, after inserts, unknown collection error)
   - 6 integration tests in `records_endpoints.rs` (empty collection, correct total, with filter, 404 for unknown collection, without filter returns all, filter matches none)

5. **Mock improvement**: Enhanced `MockRecordRepo::count` to support basic `field = "value"` filtering for more realistic integration tests.

### Response format

```json
{
  "totalItems": 42
}
```

### Access rules

The endpoint verifies the collection exists (returns 404 for unknown collections). Full access rule enforcement will be added when the rule evaluation engine is implemented (currently the same status as all other record endpoints).

## Files Modified

- `crates/zerobase-core/src/services/record_service.rs` — Added `count_records` method + 3 unit tests
- `crates/zerobase-api/src/handlers/records.rs` — Added `CountRecordsParams` struct + `count_records` handler
- `crates/zerobase-api/src/lib.rs` — Registered count route
- `crates/zerobase-api/tests/records_endpoints.rs` — Enhanced mock count + 6 integration tests

## Test Results

All 1,189 tests passing (0 failures).
