# Hooks System Design - 2026-03-21 15:30

## Summary

Designed the extensibility/hooks system for Zerobase. Created a comprehensive design document covering all hook points, both extension modes (Rust library and JS hooks), architecture, and example use cases.

## Work Done

1. **Explored the full codebase** — understood the layered architecture (core/db/auth/api/files/server), trait patterns, service structure, middleware chain, and router setup.
2. **Authored design document** at `docs/design/extensibility-hooks-system.md` covering:
   - 30+ hook points across 7 categories (record lifecycle, auth, collection schema, request lifecycle, realtime, file, mail)
   - Core types: `HookPoint` enum, `HookEvent` trait, `HookHandler` async trait, `HookRegistry` type-erased container
   - Rust Library Mode: `Zerobase` app struct with ergonomic convenience methods, `IntoHookHandler` trait for closures, `HookBuilder` for collection filtering
   - JS Hooks Mode: Rquickjs-based embedded runtime, `$app` global bindings, `JsHookAdapter` bridge, file loading from `pb_hooks/`, hot reload, security/resource limits
   - Custom route support for both modes
   - Crate placement strategy with new `zerobase-hooks` crate and feature flags
   - 7 detailed example use cases
   - Testing strategy (unit, integration, JS-specific)
   - 5-phase migration path

## Files Modified

- **Created:** `docs/design/extensibility-hooks-system.md`
- **Created:** `AGENTS_HISTORY/2026-03-21-1530-hooks-system-design.md`
