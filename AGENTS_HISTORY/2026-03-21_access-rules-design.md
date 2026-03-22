# Access Rules System Design - 2026-03-21

## Task
Design the access rules system for Zerobase — the rule evaluation engine that gates per-collection, per-operation API access using filter-like expressions.

## Summary of Work Done

Explored the existing Zerobase codebase to understand the current architecture, then authored a comprehensive design document covering:

1. **Rule syntax** — reuses the existing PocketBase-compatible filter parser with extensions for `@request.*` and `@collection.*` macros
2. **Evaluation context** — `RuleContext` and `AuthInfo` structs populated from JWT/auth middleware
3. **Dual evaluation modes** — boolean mode (create/update/delete) and filter mode (list/view where rules become SQL WHERE clauses)
4. **Filter integration** — rule filters AND-ed with user-supplied filters, wrapped in parentheses for safety
5. **Cross-collection rules** — `@collection` macro compiles to correlated SQL subqueries
6. **`@request.data` rules** — constrain what data can be written (e.g., prevent impersonation)
7. **Security considerations** — default deny, SQL injection prevention, information leakage protection
8. **Example rules** — 8 common scenarios (public read, owner-only, role-based, membership-gated, etc.)
9. **Implementation plan** — 6 phases from auth middleware through cross-collection support
10. **Testing strategy** — unit, integration, and property-based test plan

## Files Modified
- **Created**: `docs/plans/2026-03-21-access-rules-system.md` — Full design document
