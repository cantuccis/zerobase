# Verify Video Slide Code Snippets Against Source Code

**Date:** 2026-03-22

## Summary

Reviewed 3 use cases (8, 7, and 7 slides respectively) containing code snippets and diffs referencing actual source files in the repository. Verified each `code_snippet`, `additions`, and `deletions` field against the real source code character-by-character.

## Changes Made

### UC-1: View Rule Access Checks on Relation Expansion
- **Slide 2 (ExpandAuth struct):** Fixed doc comment — added missing line "This avoids coupling the core expand service to the API layer's `AuthInfo` type." and corrected the RequestContext doc comment. Adjusted highlight line numbers from [6,8,10] to [7,9,11].
- **Slide 3 (can_view_expanded_record):** Fixed comment text from "Check manage_rule bypass first." to "Also check manage_rule: if the user matches it, they bypass view_rule." Added inline comment on line 12. Fixed `evaluate_rule_str` formatting to single-line call.
- **Slide 6 (test):** Added missing `let expand = body.get("expand");` line. Fixed assert message from short form to full "locked view_rule on target collection must hide expanded relation for anonymous user". Added comment lines about expand being absent. Adjusted highlights from [3,10,25,26] to [3,10,27,28].

### UC-2: Cascade Delete/Update Error Propagation Fix
- **Slide 3 (cascade delete diff):** Updated `additions` to match actual source which uses `ref_collection_name` (not `&source_collection.name`) and includes the comment line.
- **Slide 5 (set-null diff):** Updated `additions` to single line matching actual source with `ref_collection_name, ref_id, &update_data`.

### UC-3: Replace panic! with Result in sanitize_table_name
- **Slide 3 (sanitize_table_name diff):** Replaced `InvalidArgument` error variant with actual `Database` variant. Updated to include all 6 forbidden characters (", ;, \0, \\, \n, \r) instead of just the double-quote check.
- **Slide 4 (find_one):** Replaced entire snippet to match actual source — different variable naming pattern, uses `read_conn()` and `db_err_to_repo`, formats SQL differently.

## Files Referenced (read-only)
- `crates/zerobase-core/src/services/expand.rs`
- `crates/zerobase-core/src/services/record_service.rs`
- `crates/zerobase-api/src/handlers/records.rs`
- `crates/zerobase-api/tests/records_endpoints.rs`
- `crates/zerobase-db/src/record_repo.rs`
