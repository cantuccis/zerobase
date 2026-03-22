# Implement Event Broadcasting on Record Changes

**Date:** 2026-03-21
**Task ID:** 7hn95thwqu62v4g

## Summary

Implemented event broadcasting on record changes (create, update, delete) to subscribed SSE clients with per-client access rule filtering. When records are modified via the REST API, events are broadcast through the RealtimeHub to all connected SSE clients that have matching subscriptions and pass the collection's view_rule access checks.

## Changes Made

### 1. `crates/zerobase-api/src/handlers/realtime.rs`

**New fields on `RealtimeEvent`:**
- `topic: String` — subscription matching key (e.g. `"posts/rec_123"`), empty for system events
- `rules: Option<ApiRules>` — collection access rules for per-client filtering, `None` for system events

**New field on `ClientInfo`:**
- `auth: AuthInfo` — client's authentication context at connection time

**Updated `connect()` signature:**
- Now accepts `auth: AuthInfo` parameter to store per-client auth context

**New methods on `RealtimeHub`:**
- `broadcast_record_event(collection_name, record_id, action, record, rules)` — constructs a `RealtimeEvent` with topic and rules and broadcasts it
- `should_client_receive(client_id, event)` — checks subscription match + access rules for a specific client
- `topic_matches_subscriptions(topic, subscriptions)` — static helper for subscription matching logic
- `client_passes_view_rule(auth, rules, event_data)` — evaluates view_rule and manage_rule against client's auth context; superusers bypass all rules

**Updated `sse_connect` handler:**
- Accepts `AuthInfo` extractor
- Uses `.then()` + `.filter_map()` pattern (instead of async `filter_map`) to apply per-client filtering on the SSE stream

**Updated unit tests (32 total):**
- All existing tests updated to use new `connect(AuthInfo)` signature
- All `RealtimeEvent` constructions updated with `topic` and `rules` fields
- 15 new tests added for: `topic_matches_subscriptions`, `client_passes_view_rule`, `broadcast_record_event`, `should_client_receive`

### 2. `crates/zerobase-api/src/handlers/records.rs`

**Added `RealtimeHub` to `RecordState`:**
- New optional field `realtime_hub: Option<RealtimeHub>`

**Added `broadcast_change()` helper:**
- Calls `hub.broadcast_record_event()` if a hub is configured

**Broadcast hooks in handlers:**
- `create_record` — broadcasts `"create"` event after successful record creation (both file-upload and JSON-only paths)
- `update_record` — broadcasts `"update"` event after successful record update (both file-upload and JSON-only paths)
- `delete_record` — broadcasts `"delete"` event with pre-deletion record data after successful deletion

### 3. `crates/zerobase-api/src/lib.rs`

**New `record_routes_full()` function:**
- Accepts optional `RealtimeHub` in addition to optional `FileService`
- Existing `record_routes()` and `record_routes_with_files()` delegate to it with `None` hub

### 4. `crates/zerobase-api/tests/realtime_endpoints.rs`

**Updated existing test:**
- Fixed `RealtimeEvent` construction to include `topic` and `rules` fields

**5 new integration tests:**
- `broadcast_record_event_delivers_to_subscribed_sse_client` — verifies SSE delivery with correct event format
- `broadcast_record_event_not_delivered_to_unsubscribed_client` — verifies subscription filtering
- `broadcast_record_event_locked_rules_blocks_anon_allows_after_open` — verifies view_rule enforcement
- `broadcast_record_event_delete_action_format` — verifies delete event format
- `broadcast_record_event_collection_subscription_matches_any_record` — verifies collection-level subscriptions

## Event Format

```json
{
  "action": "create" | "update" | "delete",
  "record": { "id": "...", ...record fields... }
}
```

## Access Rule Semantics

- **Superuser clients**: receive all events regardless of rules
- **`view_rule: None` (locked)**: only superusers receive events
- **`view_rule: Some("")` (open)**: all clients receive events
- **`view_rule: Some(expr)` (expression)**: evaluated against client's auth context
- **`manage_rule`**: checked before `view_rule` — if client passes manage_rule, they receive the event

## Test Results

- 32 unit tests in `realtime.rs` — all pass
- 23 integration tests in `realtime_endpoints.rs` — all pass
- Full workspace: 2040+ tests, 0 failures
