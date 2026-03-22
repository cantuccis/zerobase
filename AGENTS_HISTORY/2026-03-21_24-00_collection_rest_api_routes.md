# Collection REST API Routes

**Date:** 2026-03-21 24:00
**Task ID:** q77ukixnhwjebrd
**Phase:** 4

## Summary

Implemented REST API routes for collections management, wired to `CollectionService` with proper HTTP status codes. All endpoints are protected by a superuser auth placeholder middleware. Comprehensive unit and integration tests were written.

## Endpoints Created

| Method | Path | Description | Status Codes |
|--------|------|-------------|-------------|
| GET | `/api/collections` | List all collections | 200, 401 |
| POST | `/api/collections` | Create a new collection | 200, 400, 401, 409 |
| GET | `/api/collections/:id_or_name` | View a single collection | 200, 401, 404 |
| PATCH | `/api/collections/:id_or_name` | Update a collection | 200, 400, 401, 403, 404 |
| DELETE | `/api/collections/:id_or_name` | Delete a collection | 204, 401, 403, 404 |

## Key Decisions

- **Response format**: List endpoint returns PocketBase-compatible paginated response with `page`, `perPage`, `totalItems`, `totalPages`, `items` fields.
- **Auth middleware**: Placeholder `require_superuser` middleware checks for `Authorization` header presence. Will be replaced with proper JWT validation later.
- **PATCH semantics**: Merge-style updates - only provided fields overwrite existing values.
- **`collection_routes()` function**: Returns a composable `Router` that the server can merge into its main router, keeping the API layer modular.

## Tests

- **25 unit tests** in handler and middleware modules (in-process, using axum's `oneshot`)
- **21 integration tests** spawning full HTTP server on random ports with `reqwest` client
- Tests cover: all CRUD operations, auth enforcement, error responses, system collection protection, PocketBase response format

## Files Modified

- `crates/zerobase-api/src/handlers/collections.rs` (NEW) - Collection CRUD handlers + unit tests
- `crates/zerobase-api/src/handlers/mod.rs` - Added `collections` module
- `crates/zerobase-api/src/middleware/require_superuser.rs` (NEW) - Auth placeholder middleware + unit tests
- `crates/zerobase-api/src/middleware/mod.rs` - Added `require_superuser` module
- `crates/zerobase-api/src/lib.rs` - Added `collection_routes()` public function
- `crates/zerobase-api/tests/collections_endpoints.rs` (NEW) - Integration tests
