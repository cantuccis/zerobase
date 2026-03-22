# Subscription Management for Realtime SSE

**Date:** 2026-03-21
**Task:** Implement subscription management (POST /api/realtime)

## Summary

Implemented per-client subscription tracking and the `POST /api/realtime` endpoint for managing SSE subscriptions. This follows PocketBase's set-based subscription model where each POST call replaces the client's entire subscription set.

## What Was Done

### Core Changes

1. **Per-client subscription state**: Added `HashSet<String>` subscriptions field to `ClientInfo`, stored alongside existing connection metadata in the `RealtimeHub`.

2. **Subscription management methods on `RealtimeHub`**:
   - `set_subscriptions(client_id, subscriptions)` — replaces the full subscription set, validates topic format
   - `get_subscriptions(client_id)` — returns current subscriptions for a client
   - `is_subscribed(client_id, topic)` — checks subscription match, including collection-level matching (subscribing to `"posts"` also matches `"posts/rec_123"`)

3. **Topic validation**: Topics must match `<collection>` or `<collection>/<recordId>` format with alphanumeric/underscore characters only.

4. **POST /api/realtime handler**: Accepts JSON `{ "clientId": "...", "subscriptions": [...] }`, validates input, and returns structured response. Error cases:
   - 400: missing/empty clientId, missing/invalid subscriptions, invalid topic format
   - 404: unknown clientId (no active SSE connection)

5. **Error type**: `SetSubscriptionsError` enum with `ClientNotFound` and `InvalidTopic` variants.

### Tests

- **11 new unit tests** covering subscription set/get/replace, empty subscriptions, invalid topics, `is_subscribed` matching, disconnect cleanup
- **11 new integration tests** covering the full POST endpoint: success, replacement, clearing, all error cases (missing clientId, empty clientId, unknown client, invalid topic, missing subscriptions, non-array subscriptions, non-string items)

## Files Modified

- `crates/zerobase-api/src/handlers/realtime.rs` — Added subscription state, validation, hub methods, POST handler, request/response types, error type, and unit tests
- `crates/zerobase-api/src/lib.rs` — Added POST route to `realtime_routes()`, exported `SetSubscriptionsError`
- `crates/zerobase-api/tests/realtime_endpoints.rs` — Added 11 integration tests for POST /api/realtime
