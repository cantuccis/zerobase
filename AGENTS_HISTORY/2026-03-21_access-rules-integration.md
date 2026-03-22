# Access Rules Integration into Record API Endpoints

**Date:** 2026-03-21
**Task ID:** oa9u95o21z7fbhn
**Phase:** 5

## Summary

Integrated collection access rules (list_rule, view_rule, create_rule, update_rule, delete_rule) into all record API endpoints, enforcing PocketBase-compatible authorization semantics.

## Changes Made

### New Files
- `crates/zerobase-api/src/middleware/mod.rs` — middleware module with auth_context and request_id submodules
- `crates/zerobase-api/src/middleware/auth_context.rs` — `AuthInfo` axum extractor that determines caller authentication from Authorization header (superuser, authenticated, anonymous)

### Modified Files
- `crates/zerobase-api/src/handlers/records.rs` — Added rule enforcement to all 6 handlers (list, view, create, update, delete, count):
  - `enforce_rule_on_record()` helper: checks rule, superusers bypass, evaluates expressions
  - `enforce_rule_no_record()` helper: for operations without target record (create)
  - `check_list_rule()` helper: list-specific logic returning `ListRuleResult::Proceed` or `EmptyResult`
  - Unit tests for rule enforcement logic

- `crates/zerobase-api/tests/records_endpoints.rs` — Updated and expanded:
  - Fixed `make_posts_collection()` to use `ApiRules::open()` so existing tests continue to pass
  - Added `make_posts_collection_with_rules()` and `spawn_posts_app_with_rules()` helpers
  - Added 18 new integration tests covering:
    - Null rules (locked) blocking anonymous on all operations (list→200 empty, view/update/delete→403, count→200 zero)
    - Superuser bypassing locked rules (list, view, create, delete)
    - Empty rules (open) allowing anonymous access (list, view, create, delete)
    - Expression rules allowing authenticated users and denying anonymous
    - Public-read rules (open read, auth-required write)
    - Superuser bypassing expression rules

## Rule Semantics (PocketBase-compatible)

| Rule Value | Meaning | Non-superuser Behavior |
|---|---|---|
| `None` (null) | Locked | List/Count: 200 empty; View/Update/Delete: 403; Create: 403 |
| `Some("")` (empty) | Open | All operations allowed |
| `Some(expr)` | Conditional | Expression evaluated against request context |
| Any | Superuser | Always allowed (bypass) |

**PocketBase behavior notes:**
- List always returns 200 (empty items on deny, not 403)
- View/Update/Delete return 404 on expression denial (hides record existence)
- Create returns 403 on denial

## Test Results

All 59 integration tests pass, plus all unit tests across the workspace.
