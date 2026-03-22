# Graceful Shutdown with In-Flight Request Draining

**Date:** 2026-03-21
**Task ID:** befxqub3lupruw5
**Phase:** 12

## Summary

Implemented a comprehensive graceful shutdown system for Zerobase with in-flight request draining, configurable timeout, database connection cleanup, and log flushing.

### What was done

1. **Created `ShutdownCoordinator`** (`crates/zerobase-server/src/shutdown.rs`):
   - Manages the full shutdown lifecycle via a `watch` channel
   - Configurable timeout for draining in-flight requests (default 30s)
   - `trigger()` for programmatic shutdown (useful for testing)
   - `is_shutting_down()` state query
   - `cleanup()` for post-shutdown database close and log flush

2. **Created `serve_with_shutdown()`** function:
   - Orchestrates the full lifecycle: signal → drain → timeout → cleanup
   - Spawns background task listening for OS signals (SIGINT/SIGTERM)
   - Wraps axum's graceful shutdown with `tokio::time::timeout`
   - Force-exits if drain exceeds configured timeout
   - Runs cleanup (DB close, log flush) after server stops

3. **Added `shutdown_timeout_secs` configuration** to `ServerSettings`:
   - Configurable via `zerobase.toml`, env vars (`ZEROBASE__SERVER__SHUTDOWN_TIMEOUT_SECS`)
   - Default: 30 seconds
   - Updated `sample.zerobase.toml` with documentation

4. **Updated `main.rs` and `lib.rs`**:
   - Replaced standalone `shutdown_signal()` with `ShutdownCoordinator`
   - Both binary and library mode use the new system
   - Timeout configured from settings

5. **Wrote 11 tests** covering:
   - Coordinator state management (initial state, trigger, idempotent triggers)
   - Shutdown signal resolution
   - Custom timeout configuration
   - Cleanup without panic
   - **In-flight requests complete during shutdown** (slow handler finishes)
   - **New connections rejected after shutdown**
   - **Full lifecycle via `serve_with_shutdown()`**
   - **Shutdown timeout forces exit** (hanging handler doesn't block forever)
   - Database connection cleanup verification

## Files Modified

- `crates/zerobase-server/src/shutdown.rs` — **NEW** — ShutdownCoordinator and serve_with_shutdown
- `crates/zerobase-server/src/main.rs` — Updated to use ShutdownCoordinator
- `crates/zerobase-server/src/lib.rs` — Updated serve() and exported shutdown module
- `crates/zerobase-core/src/configuration.rs` — Added `shutdown_timeout_secs` field
- `sample.zerobase.toml` — Documented new setting

## Test Results

- 11 new shutdown tests: all passing
- 50 total zerobase-server lib tests: all passing
- Full workspace tests: all passing (no regressions)
