# Implement SSE Connection Endpoint

**Date:** 2026-03-21
**Task ID:** 1nkzxf356uwev68

## Summary

Implemented the `GET /api/realtime` SSE (Server-Sent Events) endpoint for the Zerobase project. This endpoint establishes persistent SSE connections with clients, assigns unique client IDs, sends periodic keep-alive comments, and supports event broadcasting via tokio broadcast channels.

### What was built

1. **`RealtimeHub`** — Central connection manager that tracks connected clients and distributes events via `tokio::sync::broadcast`. Clone-friendly (Arc-wrapped internals). Configurable channel capacity and keep-alive interval.

2. **`sse_connect` handler** — Axum handler for `GET /api/realtime` that:
   - Assigns a unique client ID (nanoid, 15 chars)
   - Sends `PB_CONNECT` event with `{"clientId": "..."}` on connection
   - Streams broadcast events as SSE frames
   - Sends `: ping` keep-alive comments at configurable intervals (default 30s)
   - Cleans up client registration on disconnect via a drop-guard stream wrapper

3. **`realtime_routes()` builder** — Follows the project's route builder pattern, returns an axum `Router` with the SSE endpoint.

4. **PocketBase compatibility** — Uses `PB_CONNECT` event name and SSE comment-based keep-alive, matching PocketBase's realtime protocol.

### Tests

- **7 unit tests** for `RealtimeHub`: unique IDs, connect/disconnect tracking, broadcast delivery, zero-receiver handling, custom config, default keep-alive
- **8 integration tests**: content-type verification, PB_CONNECT event parsing, unique client IDs across connections, hub client tracking, disconnect cleanup, broadcast event delivery, keep-alive ping detection, health endpoint coexistence

All 15 tests pass.

## Files Modified

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Added `tokio-stream` workspace dependency with `sync` feature |
| `crates/zerobase-api/Cargo.toml` | Added `nanoid` and `tokio-stream` dependencies |
| `crates/zerobase-api/src/handlers/mod.rs` | Registered `realtime` module |
| `crates/zerobase-api/src/handlers/realtime.rs` | **NEW** — RealtimeHub, SSE handler, drop-guard stream, unit tests |
| `crates/zerobase-api/src/lib.rs` | Added `realtime_routes()` builder, re-exported `RealtimeHub`, `RealtimeEvent`, `RealtimeHubConfig`, `RealtimeState` |
| `crates/zerobase-api/tests/realtime_endpoints.rs` | **NEW** — 8 integration tests for the SSE endpoint |
