# Filtering Engine for Record Queries

**Task ID:** `4mvoofsop7386tk`
**Date:** 2026-03-21
**Status:** Complete

## Summary

Implemented a PocketBase-compatible filter parser and parameterized SQL generator for record queries.

## What Was Done

### 1. Filter Module (`crates/zerobase-db/src/filter.rs`)

Created a complete filter engine with three stages:

- **Tokenizer (Lexer):** Converts filter strings into tokens. Supports:
  - String literals (single/double quoted with escape sequences)
  - Number literals (integer, float, negative)
  - Boolean literals (`true`, `false`) and `null`
  - Date macros (`@now`, `@today`, `@month`, `@year`)
  - All comparison operators (`=`, `!=`, `>`, `>=`, `<`, `<=`, `~`, `!~`)
  - Multi-value operators (`?=`, `?!=`, `?>`, `?>=`, `?<`, `?<=`, `?~`, `?!~`)
  - Logical operators (`&&`, `||`)
  - Parentheses for grouping
  - Field identifiers (with dot-notation for relations)

- **Recursive Descent Parser:** Builds an AST (`FilterExpr`) from tokens with correct operator precedence (AND binds tighter than OR). Supports arbitrary nesting via parentheses.

- **Parameterized SQL Generator:** Converts the AST into a `BuiltQuery` with:
  - Parameterized values (never interpolated) to prevent SQL injection
  - Field names double-quoted with internal quotes stripped
  - `IS NULL` / `IS NOT NULL` for null comparisons
  - `LIKE %value%` wrapping for `~` / `!~` operators
  - `json_each()` subqueries for multi-value (`?=`, `?~`, etc.) operators
  - Date macro resolution via `chrono` (`@now` → current datetime, `@today` → today start, `@month` → month start, `@year` → year start)

### 2. Repository Integration (`crates/zerobase-db/src/record_repo.rs`)

- Integrated filter engine into `find_many()`: filter WHERE clause applied to both the count query and data query
- Integrated filter engine into `count()`: filter WHERE clause applied when filter string is provided
- Invalid filter strings produce descriptive error messages

### 3. Dependencies & Module Registration

- Added `chrono = { workspace = true }` to `zerobase-db/Cargo.toml`
- Registered `pub mod filter;` in `crates/zerobase-db/src/lib.rs`

## Test Coverage

- **293 tests pass** in `zerobase-db` (including ~80 new filter tests)
- Full workspace tests pass with no regressions
- Test categories:
  - Tokenizer: all token types, edge cases, error handling
  - Parser: operator precedence, nesting, all operator types
  - SQL Generation: all operators, null handling, LIKE wrapping, multi-value, date macros, SQL injection prevention
  - Convenience: `parse_and_generate_sql` function
  - Integration: PocketBase-style real-world filter patterns

## Files Modified

- `crates/zerobase-db/src/filter.rs` (new)
- `crates/zerobase-db/src/record_repo.rs` (modified)
- `crates/zerobase-db/src/lib.rs` (modified)
- `crates/zerobase-db/Cargo.toml` (modified)
