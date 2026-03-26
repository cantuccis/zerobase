# Add view_rule access checks to relation expansion service

**Date:** 2026-03-22
**Task ID:** z0x3nto1u9fouhj

## Summary

Added comprehensive test coverage for the view_rule access checks in the relation expansion service (`expand.rs`). The core security implementation (`can_view_expanded_record`) was already in place, checking `view_rule`, `manage_rule`, and superuser bypass for both forward and back-relation expansions. The task focused on verifying and strengthening this security behavior with additional edge-case tests.

## What was verified

The existing implementation at `crates/zerobase-core/src/services/expand.rs` already:

1. Calls `can_view_expanded_record()` for every expanded record (both forward relations at line 354 and back-relations at line 437)
2. Checks superuser bypass, manage_rule bypass, and view_rule evaluation
3. Silently omits records that fail the view_rule check (PocketBase-compatible behavior)

## Tests added (5 new unit tests)

| Test | Description |
|------|-------------|
| `expand_view_rule_locked_hides_from_authenticated_non_superuser` | Authenticated non-superuser denied when view_rule is locked |
| `expand_multi_relation_partial_view_rule_filters_individually` | Multi-relation where only some referenced records pass the expression-based view_rule |
| `expand_nested_locked_intermediate_hides_deeper_levels` | Nested `secret.nested_ref` expansion stops at locked intermediate collection |
| `expand_manage_rule_bypasses_view_rule` | Authenticated user with manage_rule="" bypasses locked view_rule; anonymous cannot |
| `expand_back_relation_expression_view_rule_filters_per_record` | Back-relation with expression-based view_rule filters comments per-record |

## Test results

- **Unit tests:** 46 passed (41 existing + 5 new)
- **Integration tests:** 16 passed (all existing expand-related HTTP tests)

## Files modified

- `crates/zerobase-core/src/services/expand.rs` — Added 5 new unit tests in the `tests` module
