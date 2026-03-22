# Rule Expression Parser Implementation

**Task ID:** `djqhvzn3b3kb4ck`
**Date:** 2026-03-21
**Status:** Complete

## Summary

Implemented a full rule expression parser for PocketBase-compatible access rule expressions in `zerobase-core`.

## What was done

### New file: `crates/zerobase-core/src/schema/rule_parser.rs`

A complete recursive-descent parser for access rule expressions with:

- **Tokenizer** — Handles identifiers, string/number literals, booleans, null, all 16 comparison operators (including multi-value `?=`, `?!=`, etc.), logical operators (`&&`, `||`, `!`), parentheses, and all `@`-prefixed references.

- **AST types:**
  - `RuleExpr` — `Condition`, `And`, `Or`, `Not`, `Group`
  - `Operand` — `Field`, `RequestAuth`, `RequestData`, `RequestQuery`, `RequestHeaders`, `RequestMethod`, `RequestContext`, `CollectionRef`, `String`, `Number`, `Bool`, `Null`, `Now`, `Today`, `Month`, `Year`
  - `ComparisonOp` — 16 operators (standard + multi-value variants)

- **Context variables supported:**
  - `@request.auth.*` — authenticated user fields
  - `@request.data.*` / `@request.body.*` — incoming request data
  - `@request.query.*` — query parameters
  - `@request.headers.*` — request headers
  - `@request.method` — HTTP method
  - `@request.context` — request context (e.g., "realtime")
  - `@collection.<name>.<path>` — cross-collection lookups
  - `@now`, `@today`, `@month`, `@year` — date macros

- **Error handling** via `RuleParseError` enum with descriptive messages using `thiserror`.

- **Public API:** `parse_rule(expr) -> Result<RuleExpr>` and `validate_rule(expr) -> Result<()>`

- **81 tests** across tokenizer, parser, integration, and validation modules.

### Modified files

- `crates/zerobase-core/src/schema/mod.rs` — Added `pub mod rule_parser` and re-exports
- No changes to `lib.rs` or `Cargo.toml` required (no new dependencies)

## Verification

- All 81 rule_parser tests pass
- Full workspace compiles clean (`cargo check --workspace`)
- No new dependencies added
