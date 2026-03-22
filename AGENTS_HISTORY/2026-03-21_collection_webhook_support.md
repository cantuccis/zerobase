# Collection Webhook Support

**Date**: 2026-03-21
**Task ID**: kgwsksp72j0rf3m
**Status**: Complete

## Summary

Implemented a collection webhook system that fires HTTP notifications when records are created, updated, or deleted. Webhooks are configured per-collection with URL, events, optional HMAC secret for payload signing, and enable/disable toggle.

## Files Created

### `crates/zerobase-core/src/webhooks.rs`
Module root with submodule declarations and re-exports.

### `crates/zerobase-core/src/webhooks/model.rs`
Domain types:
- `WebhookEvent` enum (Create, Update, Delete)
- `Webhook` struct (id, collection, url, events, secret, enabled, timestamps)
- `WebhookDeliveryStatus` enum (Success, Failed, Pending)
- `WebhookDeliveryLog` struct for delivery tracking
- `WebhookPayload` struct for HTTP request body
- Unit tests for serialization and display

### `crates/zerobase-core/src/webhooks/service.rs`
Business logic layer:
- `WebhookRepository` trait (persistence abstraction)
- `WebhookRepoError` with conversion to `ZerobaseError`
- `WebhookService<R>` with full CRUD: list, get, create, update, delete
- `get_active_for_event()` for finding matching webhooks
- Input validation: URL scheme, non-empty events, no duplicates
- `InMemoryWebhookRepo` for testing
- `CreateWebhookInput` / `UpdateWebhookInput` structs
- Comprehensive unit tests (16 tests)

### `crates/zerobase-core/src/webhooks/dispatcher.rs`
HTTP delivery engine:
- `HttpSender` trait (async_trait) for testable HTTP
- `ReqwestSender` default implementation (30s timeout)
- `WebhookDispatcher<S, R>` with `dispatch()` method
- HMAC-SHA256 signing (`sha256={hex}` format, `X-Webhook-Signature` header)
- Retry logic: 3 attempts, exponential backoff (1s, 2s, 4s)
- Headers: `X-Webhook-Event`, `X-Webhook-Id`, `X-Webhook-Signature`
- `compute_hmac_signature()` / `verify_hmac_signature()` helpers
- Every attempt logged to repository
- Unit tests with MockSender (12 tests)

### `crates/zerobase-core/src/webhooks/hook.rs`
Hook system integration:
- `WebhookHook<S, R>` implements the `Hook` trait
- Matches only Create/Update/Delete operations
- Fires in `after_operation` phase (after persistence)
- Looks up active webhooks for collection+event
- Spawns background tokio tasks for non-blocking delivery
- Unit tests (5 tests)

## Files Modified

### `Cargo.toml` (workspace)
Added workspace dependencies: `hmac = "0.12"`, `sha2 = "0.10"`, `hex = "0.4"`

### `crates/zerobase-core/Cargo.toml`
Added dependencies: `hmac`, `sha2`, `hex`, `reqwest`, `tokio` (moved from dev-dependencies)

### `crates/zerobase-core/src/lib.rs`
Added `pub mod webhooks;`

## Architecture

```
RecordService (create/update/delete)
    |
    v
HookRegistry.run_after()
    |
    v
WebhookHook.after_operation()
    |
    v
WebhookRepository.get_active_webhooks(collection, event)
    |
    v
tokio::spawn for each matching webhook
    |
    v
WebhookDispatcher.dispatch() (retry loop with backoff)
    |
    v
HttpSender.post() -> WebhookRepository.insert_delivery_log()
```

## Test Results

39 tests, all passing:
- 7 model tests (serialization, display, roundtrip)
- 16 service tests (CRUD, validation, filtering)
- 11 dispatcher tests (HMAC, delivery, retry, headers)
- 5 hook tests (matching, event mapping, integration)
