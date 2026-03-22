# Records Rules Integration Tests

**Task ID:** 82za0v1hg8ykejr (Phase 12)
**Date:** 2026-03-21
**Status:** Completed

## Summary

Created comprehensive integration test suite for record CRUD operations with access rules, covering all rule types, context variable resolution, and PocketBase-compatible behavior.

## Files Modified

- **`crates/zerobase-api/tests/records_rules_integration.rs`** (new, ~1050 lines, 56 tests)

## Test Coverage (56 tests across 10 modules)

| Module | Tests | Description |
|--------|-------|-------------|
| `owner_rules` | 9 | `owner = @request.auth.id` matching for view/update/delete/create/list, superuser bypass, anonymous denial |
| `record_field_rules` | 3 | `status = "published"` field matching, superuser bypass |
| `auth_context_variables` | 7 | `@request.auth.role = "admin"` with rich auth middleware encoding extra fields |
| `compound_rules` | 5 | `||` (OR) and `&&` (AND) compound expressions |
| `asymmetric_rules` | 7 | Different rule types per operation (open list/view, auth-required create, owner update, locked delete) |
| `manage_rule` | 7 | manage_rule bypass of locked individual rules, empty manage_rule for any-authenticated, anonymous blocking |
| `request_data_context` | 2 | `owner = @request.auth.id` evaluated against request body on create |
| `list_rule_as_filter` | 4 | Locked list → 200 empty (not 403), expression filtering, superuser bypass, open list |
| `mixed_lifecycle` | 1 | Full blog scenario combining public read, auth create, owner update/delete |
| `edge_cases` | 3 | `!=` operator, per-operation rule isolation, superuser always bypasses |

## Key Design Decisions

- **Rich auth middleware**: Enhanced Bearer token format (`Bearer <id>;role=admin;org=x`) to test `@request.auth.*` context variables beyond just `id`
- **Record IDs**: All test record IDs are exactly 15 characters to pass validation
- **PocketBase behavior**: Verified 404 (not 403) for denied view/update/delete, 200 with empty items for denied list
- **Mock infrastructure**: Duplicated from `records_endpoints.rs` with added `owner` field support

## Issues Encountered

- Record IDs initially 16 characters, causing 400 validation errors (max 15). Fixed by shortening all IDs.
