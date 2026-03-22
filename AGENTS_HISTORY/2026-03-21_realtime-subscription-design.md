# Realtime Subscription System Design - Agent Log

**Date:** 2026-03-21
**Task ID:** 5w5moo4ylawunnq
**Task:** Design SSE-based realtime subscription system

## Summary

Designed a comprehensive SSE-based realtime subscription system for Zerobase, mirroring PocketBase's realtime model. The design covers:

- **SSE protocol**: Connection endpoint (`GET /api/realtime`), `PB_CONNECT` handshake event, keepalive mechanism, and event format for record changes (create/update/delete).
- **Subscription management**: `POST /api/realtime` endpoint for set-based subscription replacement, topic formats (`collection` and `collection/recordId`), validation rules.
- **Server architecture**: `ConnectionManager` (tracks clients and subscriptions via `RwLock<HashMap>`), `EventBroadcaster` (tokio broadcast channel), and per-client mpsc channels for SSE streaming.
- **Event broadcasting**: RecordService integration — events emitted after successful mutations, fan-out to subscribed clients with access rule filtering.
- **Access rule enforcement**: Uses `view_rule` at event delivery time (not subscription time), with `@request.context = "realtime"` in the rule evaluation context. Field visibility stripping applied to event payloads.
- **Auth integration**: Token captured at connection time, token via query param supported for EventSource API compatibility.
- **Configuration**: `max_connections`, `keepalive_interval_secs`, `max_subscriptions_per_client`, `broadcast_channel_capacity`.
- **Testing strategy**: Unit tests for ConnectionManager, EventBroadcaster, subscription validation; integration tests for full SSE flow, access rules, concurrent clients.
- **Crate placement**: Event types in `zerobase-core`, handlers and connection management in `zerobase-api`.
- **PocketBase compatibility**: Matches PocketBase's event names, endpoint paths, subscription model, and rule enforcement semantics.

## Files Modified

- **Created:** `docs/plans/2026-03-21-realtime-subscription-system.md` — Full design document
