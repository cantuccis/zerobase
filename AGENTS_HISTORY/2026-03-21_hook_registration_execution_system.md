# Hook Registration and Execution System

**Date**: 2026-03-21
**Task ID**: ubbxtiqi455hyn3
**Phase**: 11

## Summary

Implemented the core hooks infrastructure for Zerobase. The system allows external code to intercept and modify record lifecycle operations (create, update, delete, view, list) at well-defined before/after points. Before-hooks can modify record data or abort operations; after-hooks perform side effects.

## Architecture

### Core Types

- **`RecordOperation`** - Enum: Create, Update, Delete, View, List
- **`HookPhase`** - Enum: Before, After
- **`HookContext`** - Mutable context carrying operation state (record data, auth info, metadata for inter-hook communication)
- **`HookAuthInfo`** - Authentication context (anonymous, authenticated, superuser)
- **`Hook` trait** - Interface for hook implementors with `name()`, `matches()`, `before_operation()`, `after_operation()`
- **`HookRegistry`** - Stores and executes hooks in priority order
- **`HookResult<T>`** - Result alias for hook operations

### Key Design Decisions

1. **Priority-based ordering**: Hooks execute in priority order (lower number = higher priority). Default priority is 100.
2. **Before-hooks short-circuit**: If any before-hook returns an error, the chain stops and the operation is aborted.
3. **After-hooks are resilient**: All after-hooks execute regardless of individual failures; errors are logged but don't affect the committed result.
4. **Filtering**: Hooks can filter by operation type and collection name via `matches()`.
5. **Metadata**: Hooks communicate with each other via a shared metadata map on the context.
6. **Thread-safe**: Hooks are `Send + Sync`, and the registry stores `Arc<dyn Hook>`.
7. **Optional integration**: `RecordService` holds an `Option<HookRegistry>`, so hooks are opt-in with zero overhead when unused.

### Integration Points

Hooks are called in `RecordService` for:
- **create_record**: Before-hooks run after validation but before persistence; after-hooks run after persistence
- **update_record**: Same pattern as create
- **delete_record**: Before-hooks run after existence check but before cascade/delete; after-hooks run after deletion

## Test Coverage

32 dedicated hook tests covering:
- RecordOperation and HookPhase display
- HookContext creation, auth attachment, metadata
- HookAuthInfo variants (anonymous, authenticated, superuser)
- HookRegistry: empty, register, unregister, len, names, debug format
- Before-hook execution, data modification, abort/short-circuit
- After-hook execution, error collection without stopping
- Priority ordering verification
- Operation filtering and collection filtering
- Sequential data modification chain
- Cross-hook data visibility
- Inter-hook metadata communication
- Auth context access from hooks
- Conditional abort based on record data
- Full before+after lifecycle

All 1153 existing tests continue to pass.

## Files Modified

- `crates/zerobase-core/src/hooks.rs` — **NEW** — Hook trait, HookContext, HookRegistry, tests (~650 lines)
- `crates/zerobase-core/src/lib.rs` — Added `hooks` module and re-exports
- `crates/zerobase-core/src/services/record_service.rs` — Added `hooks` field to `RecordService`, `with_hooks()`/`set_hooks()`/`hooks()` methods, hook invocations in create/update/delete
