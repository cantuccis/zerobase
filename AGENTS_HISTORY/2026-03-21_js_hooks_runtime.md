# Embedded JavaScript Hooks Runtime

**Date:** 2026-03-21
**Task ID:** yelptm3ko6sbvi5
**Phase:** 11

## Summary

Implemented an embedded JavaScript hooks runtime using Boa Engine (v0.20), enabling JS-based hook scripts in `pb_hooks/*.pb.js` files that execute on record events, mirroring PocketBase's JS hook API. Includes full `$app.dao()` CRUD operations, `$app.newMailMessage()` mail builder, `routerAdd()` custom route registration, and hot-reload file watching.

## Changes

### New Crate: `zerobase-hooks`

**`crates/zerobase-hooks/Cargo.toml`** — New crate with dependencies on `boa_engine 0.20`, `notify 7`, `parking_lot`, `tokio`, `serde_json`, `tracing`, `thiserror`.

**`crates/zerobase-hooks/src/lib.rs`** — Module root exporting `JsHook`, `JsHookEngine`, `JsHookError`, `HooksWatcher`, `DaoHandler`, `DaoRequest`, `DaoResponse`, `MailMessage`, `NoOpDaoHandler`.

**`crates/zerobase-hooks/src/error.rs`** — `JsHookError` enum with variants: `FileRead`, `JsEval`, `JsCallback`, `InvalidRegistration`, `Watcher`.

**`crates/zerobase-hooks/src/engine.rs`** — Core engine (~1100 lines):
- `JsHookEngine`: Loads `*.pb.js` files, evaluates them in Boa contexts to collect registrations.
- `JsHookEngine::with_dao_handler()`: Constructor accepting a custom `DaoHandler` for real database access during hook execution.
- `JsHook`: Implements the `Hook` trait from `zerobase-core`. During execution, re-evaluates the original file source in a fresh JS context with modified registration functions that invoke the target callback.
- Supports 10 event types: `onRecord{Before,After}{Create,Update,Delete,View,List}Request`.
- Record modifications in before-hooks propagate back to the Rust `HookContext`.
- Multiple hooks from different files chain correctly (modifications carry forward).
- DAO operations and mail messages from JS hooks are collected during execution and accessible via `drain_mail_queue()`.
- 20 tests covering loading, execution, collection filtering, record modification, auth access, operation matching, reload, DAO integration, mail collection, custom routes.

**`crates/zerobase-hooks/src/bindings.rs`** — JS runtime bindings (~910 lines):
- `$app` global with `logger()`, `dao()`, and `newMailMessage()` methods.
- `$app.dao()` returns proxy object with 5 DAO methods:
  - `findRecordById(collection, id)` — returns record or null
  - `findFirstRecordByFilter(collection, filter)` — returns record or null
  - `findRecordsByFilter(collection, filter, sort, limit, offset)` — returns array
  - `saveRecord(collection, data)` — creates/updates, returns saved record
  - `deleteRecord(collection, id)` — returns boolean
- `DaoHandler` trait for pluggable database access. `NoOpDaoHandler` for loading phase.
- `DaoRequest` / `DaoResponse` enums for the JS-Rust bridge.
- `$app.newMailMessage()` returns builder with `setTo/setSubject/setBody/send` (chainable).
- `MailMessage` struct queued via `__mail_send` global and collected after execution.
- `routerAdd(method, path, handler)` for custom route registration.
- `console.log/warn/error/debug` bridged to Rust `tracing`.
- `register_app_bindings_with_mail_queue()` variant for external mail queue access.
- 12 tests covering DAO methods, custom handlers, save/delete, mail builder, chaining, routes.

**`crates/zerobase-hooks/src/watcher.rs`** — File watcher for hot-reload (~170 lines):
- `HooksWatcher` using `notify::RecommendedWatcher` with 300ms debounce.
- Watches for `*.pb.js` file create/modify/delete events.
- Spawns a tokio task; supports graceful shutdown via channel.
- 5 tests.

### Modified Files

**`Cargo.toml` (workspace root)** — Added `zerobase-hooks` to workspace members and dependencies.

**`crates/zerobase-server/Cargo.toml`** — Added `zerobase-hooks` dependency.

**`crates/zerobase-server/src/lib.rs`** — Added:
- `js_hook_engine: Option<JsHookEngine>` and `hooks_watcher: Option<HooksWatcher>` fields.
- `with_js_hooks(hooks_dir)` builder method to load hooks and register `JsHook` in the `HookRegistry`.
- `with_js_hooks_watcher()` builder method to enable file watching for development hot-reload.

## Architecture Decisions

1. **Boa Engine over deno_core**: Pure Rust, no external V8 dependency, simpler integration.
2. **File re-evaluation strategy**: Boa's JS objects are `!Send`, so we store original file sources and re-evaluate them in fresh contexts per invocation. During execution, registration functions are replaced with versions that invoke the target callback at the correct index.
3. **`unsafe NativeFunction::from_closure`**: Required because Boa's `from_copy_closure` demands `Copy` on captured values. Safety justified: captured `Arc` and `String` types don't participate in Boa's GC.
4. **Chained modifications**: Multiple hooks on the same event correctly chain — each subsequent callback receives the record data as modified by the previous one.
5. **Synchronous DaoHandler trait**: Since Boa contexts are `!Send`, DAO operations must be synchronous. The `DaoHandler` trait provides a sync interface called during JS execution. Implementors bridge to the actual async DB layer.
6. **JSON bridge for complex data**: DAO responses are serialized to JSON strings and parsed back in JS via `JSON.parse()`, ensuring reliable data transfer between Rust and Boa.
7. **External mail queue pattern**: `register_app_bindings_with_mail_queue()` accepts an external `Arc<RwLock<Vec<MailMessage>>>` so the engine can collect mail messages queued during JS execution without coupling to the bindings' internal state.

## Test Results

41 tests passing across all modules (engine: 20, bindings: 12, watcher: 5, doc-tests: 4).
