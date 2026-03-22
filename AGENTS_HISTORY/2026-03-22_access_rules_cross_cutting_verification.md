# Access Rules Engine Cross-Cutting Verification

**Date:** 2026-03-22
**Task ID:** mr1h64c21378hus
**Phase:** 13

## Summary

Implemented comprehensive cross-cutting verification tests for the access rules engine across all API surfaces (REST, realtime SSE, file access, relation expansion). Also fixed a security gap: `Collection::validate()` now validates rule syntax at save time, preventing invalid rules from being saved and causing runtime errors.

## Test Scenarios Covered (42 tests total)

### 1. Null Rule Blocks Non-Superusers (6 tests)
- Authenticated users blocked on view, create, update, delete
- List returns 200 with empty items (not 403) for locked list_rule
- Superuser bypasses all null rules

### 2. @request.auth.* Context Resolution (5 tests)
- Multi-field auth context (role + org) with && conditions
- Correct empty string resolution for missing auth fields
- Owner-based update rules with auth.id matching
- Non-owner denied despite matching other auth fields

### 3. List Rule Pagination Metadata (6 tests)
- Anonymous gets zero totalItems with auth-required list_rule
- Authenticated user sees all items when rule allows
- Pagination metadata (totalPages, page) correct when rule allows
- Locked list_rule shows zero total despite existing records
- Superuser ignores list rules

### 4. Realtime SSE Event Filtering (4 tests)
- Client without view access does not receive events
- Superuser receives all events regardless of rules
- Null view_rule blocks regular users in realtime
- manage_rule grants realtime access

### 5. Protected File Access (2 tests)
- File field protection flag correctly set in schema
- Collection with protected file + locked view_rule integration

### 6. Relation Expansion Access Control (1 test)
- Documents that expand currently fetches data from locked collections (known security gap)

### 7. manage_rule Override (6 tests)
- Overrides locked view_rule, locked delete_rule, owner restrictions
- Lets manage users list all records
- Non-matching manage falls through to individual rules
- Non-matching manage blocked by locked individual rules

### 8. Rule Syntax Validation (12 tests)
- Valid rules pass validation
- Invalid operator, unterminated string, incomplete expression, unknown macro, invalid request path, incomplete collection ref, unmatched parenthesis — all return actionable errors
- All error variants include position or context information
- Collection::validate() rejects invalid rule syntax
- Collection::validate() accepts valid rules
- Each rule field validated independently with error identifying the offending rule

## Code Changes

### New Files
- `crates/zerobase-api/tests/access_rules_cross_cutting.rs` — 42 comprehensive integration tests

### Modified Files
- `crates/zerobase-core/src/schema/collection.rs` — Added `validate_rules()` method and integrated it into `validate()` and `validate_fields_and_type()`. Added `use super::rule_parser::validate_rule` import.
- `crates/zerobase-api/src/handlers/realtime.rs` — Made `client_passes_view_rule()` public for test access.

## Security Gaps Identified

1. **Rule syntax validation at save time** — FIXED. `Collection::validate()` now validates all rule expressions when a collection is created or updated, returning actionable error messages.

2. **Relation expansion data leakage** — DOCUMENTED. The `expand_record()` function does not check `view_rule` on the target collection. This means expanding a relation to a locked collection will leak that collection's data. This should be addressed in a future task.
