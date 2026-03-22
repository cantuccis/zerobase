# Rule Evaluation Engine

**Date**: 2026-03-21
**Task ID**: 4urfjah5scu2oea
**Phase**: 5 — Access Rules & Auth

## Summary

Implemented the rule evaluation engine that evaluates parsed rule ASTs against request contexts. The engine supports two evaluation modes: in-memory boolean evaluation for single-record operations and SQL WHERE clause generation for list operations.

## Changes Made

### New Files
- `crates/zerobase-core/src/schema/rule_engine.rs` — Complete rule evaluation engine (~650 lines)

### Modified Files
- `crates/zerobase-core/src/schema/mod.rs` — Registered `rule_engine` module and added public exports

## Architecture

### RequestContext
Carries all `@request.*` variables available during rule evaluation:
- `auth` — Authenticated user's record fields (`@request.auth.*`)
- `data` — Request body fields (`@request.data.*`)
- `query` — URL query parameters (`@request.query.*`)
- `headers` — Request headers (`@request.headers.*`)
- `method` — HTTP method (`@request.method`)
- `context` — Request context identifier (`@request.context`)

Constructors: `anonymous()`, `authenticated(auth)`, and `Default`.

### Rule Decision Triage
`check_rule(rule: &Option<String>) -> RuleDecision`:
- `None` → `Deny` (locked, superusers only)
- `Some("")` → `Allow` (open to everyone)
- `Some(expr)` → `Evaluate(expr)` (needs evaluation)

### In-Memory Evaluation
`evaluate_rule(expr: &RuleExpr, ctx: &RequestContext, record: &HashMap<String, JsonValue>) -> bool`

Recursively evaluates the rule AST against the request context and target record:
- Resolves all operand types (fields, @request.*, literals, date functions)
- Handles type coercion (string↔number, bool↔string)
- SQL-style null semantics (NULL = NULL is true, NULL op anything else follows IS NULL/IS NOT NULL)
- LIKE/NOT LIKE with case-insensitive contains matching
- Any* operators for multi-value/array field comparisons
- AND/OR/NOT/Group logical combinators

### SQL Generation
`rule_to_sql(expr: &RuleExpr, ctx: &RequestContext) -> RuleSqlClause`

Converts rule AST to parameterized SQL WHERE clause:
- Context variables (@request.*) resolved at generation time as SQL parameters
- Record fields become column references
- `?N` placeholder style for parameter binding
- json_each() subqueries for Any* multi-value operators
- Proper NULL handling (IS NULL / IS NOT NULL)

### PocketBase Compatibility
- Anonymous users: `@request.auth.*` resolves to empty string (matching PocketBase behavior where `@request.auth.id != ""` is the standard authentication check)
- Cross-collection references (`@collection.*`) return NULL for in-memory eval (requires DB access)

## Tests

49 tests covering:
- Rule decision triage (None/empty/expression)
- Field comparisons (string, number, bool, null)
- Null semantics (IS NULL / IS NOT NULL)
- Logical operators (AND, OR, NOT, grouping, precedence)
- LIKE / NOT LIKE pattern matching
- Auth context (authenticated vs anonymous, role checks, ownership)
- Request data/method/context resolution
- Any* multi-value operators on arrays
- Combined PocketBase patterns (public-or-owner, team membership)
- SQL generation for all operator types
- SQL compound expressions and parameterization
- Error handling for invalid expressions

## Key Design Decisions

1. **Dual evaluation modes**: In-memory for single-record ops (view/create/update/delete), SQL for list ops (acts as additional WHERE filter)
2. **SQL-style null semantics**: Ensures consistency between in-memory and SQL evaluation
3. **Anonymous auth → empty string**: Matches PocketBase convention where `@request.auth.id != ""` checks authentication
4. **Parameterized SQL**: All context values injected as parameters to prevent SQL injection
