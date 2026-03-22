# System Collection Protections

**Date:** 2026-03-21 18:00
**Task ID:** d6qrmnyq64drhw3

## Summary

Implemented protections for system collections (those with names starting with `_`) to prevent unauthorized modification or deletion. System collections are internal tables managed by Zerobase's migration system and must remain immutable to external schema changes.

### What was implemented:

1. **System collection constants and helpers** (`collection.rs`):
   - `SYSTEM_COLLECTION_NAMES` — list of known system collection names
   - `BASE_SYSTEM_FIELDS` — immutable fields on all collections (`id`, `created`, `updated`)
   - `AUTH_SYSTEM_FIELDS` — immutable fields on auth collections (`email`, `emailVisibility`, `verified`, `password`, `tokenKey`)
   - `is_system_collection()` — helper to detect system collections by underscore prefix

2. **Delete protection** (`collection_service.rs`):
   - `delete_collection()` returns 403 Forbidden for any collection with underscore-prefixed name
   - Clear error message: "system collection '{name}' cannot be deleted"

3. **Rename protection** (`collection_service.rs`):
   - `update_collection()` returns 403 Forbidden when attempting to rename a system collection
   - Clear error message: "system collection '{name}' cannot be renamed"

4. **System field immutability** (`collection_service.rs`):
   - `update_collection()` checks that no user-defined fields collide with system field names
   - For auth-type system collections, also checks auth-specific system fields
   - Clear error message: "cannot modify system field '{field}' on system collection '{name}'"

5. **`validate_fields_and_type()`** (`collection.rs`):
   - New method that validates fields and type-specific constraints without checking the collection name
   - Used for system collection updates (since `validate_name` rejects underscore-prefixed names)

6. **25+ new tests** covering:
   - Deletion blocked for all known system collections (`_superusers`, `_collections`, `_fields`, `_settings`, `_migrations`)
   - Deletion blocked for any underscore-prefixed collection
   - Regular collection deletion still works
   - Renaming system collections is blocked (both to system and non-system names)
   - System fields (`id`, `created`, `updated`) cannot be defined on system collections
   - Auth system fields (`email`, `password`, `verified`, `tokenKey`, `emailVisibility`) cannot be defined on auth system collections
   - Non-system fields can still be added to system collections
   - System collection updates without rename succeed
   - `is_system_collection()` helper correctness
   - `validate_fields_and_type()` correctness

## Files Modified

- `crates/zerobase-core/src/schema/collection.rs` — Added system constants, `is_system_collection()`, `validate_fields_and_type()`, and tests
- `crates/zerobase-core/src/schema/mod.rs` — Updated re-exports for new public items
- `crates/zerobase-core/src/services/collection_service.rs` — Added protection logic in `update_collection()` and `delete_collection()`, added `validate_no_system_field_changes()` helper, and 25+ tests

## Test Results

All 651 tests in `zerobase-core` pass. Full workspace tests pass (one pre-existing flaky tracing test occasionally fails due to test parallelism, not related to this change).
