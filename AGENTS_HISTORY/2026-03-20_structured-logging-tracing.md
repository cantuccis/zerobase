# Set up structured logging with tracing

**Date:** 2026-03-20
**Task ID:** xdpf64emo420msg

## Summary

Integrated the `tracing` and `tracing-subscriber` crates across the workspace to provide structured logging with environment-aware formatting. Added request-ID propagation middleware for axum that generates/reuses UUIDs and injects them into tracing spans and response headers.

### What was done

1. **Telemetry module** (`zerobase-core/src/telemetry.rs`):
   - `LogFormat` enum (`Json` | `Pretty`) with serde deserialization.
   - `init_tracing(format)` — initializes the global subscriber with JSON or pretty formatting.
   - `init_json_tracing_to_writer()` — test helper that writes JSON logs to an in-memory buffer.
   - `default_env_filter()` — respects `RUST_LOG` with sensible defaults for all zerobase crates.

2. **Configuration** (`zerobase-core/src/configuration.rs`):
   - Added `log_format: LogFormat` field to `ServerSettings` (defaults to `json`).
   - Configurable via TOML (`server.log_format = "pretty"`) or env var (`ZEROBASE__SERVER__LOG_FORMAT=pretty`).

3. **Request-ID middleware** (`zerobase-api/src/middleware/request_id.rs`):
   - Reuses incoming `x-request-id` header or generates UUID v4.
   - Stores `RequestId` in request extensions for downstream handlers.
   - Records `request_id` in the current tracing span.
   - Echoes the ID back in the `x-request-id` response header.

4. **API router** (`zerobase-api/src/lib.rs`):
   - `api_router()` function wiring `TraceLayer` (with custom span including method, URI, request_id) and the request-ID middleware.
   - `/api/health` endpoint for smoke testing.

5. **Server entrypoint** (`zerobase-server/src/main.rs`):
   - Loads config, initializes tracing with the configured format, builds the router, and starts the HTTP server.

6. **Tests** (7 new tests, 46 total):
   - Unit tests: LogFormat deserialization, default filter, request-ID generation/reuse.
   - Integration tests: health check, response header presence, caller-supplied ID echo, JSON log structure validation, all log lines valid JSON.

## Files Modified

- `crates/zerobase-core/Cargo.toml` — added tracing, tracing-subscriber deps
- `crates/zerobase-core/src/lib.rs` — added telemetry module export
- `crates/zerobase-core/src/telemetry.rs` — **new file**
- `crates/zerobase-core/src/configuration.rs` — added `log_format` field to ServerSettings
- `crates/zerobase-api/Cargo.toml` — added uuid, tracing-subscriber, reqwest dev-deps
- `crates/zerobase-api/src/lib.rs` — added api_router with middleware stack
- `crates/zerobase-api/src/middleware/mod.rs` — **new file**
- `crates/zerobase-api/src/middleware/request_id.rs` — **new file**
- `crates/zerobase-api/tests/tracing_integration.rs` — **new file**
- `crates/zerobase-server/src/main.rs` — wired up tracing + axum server
