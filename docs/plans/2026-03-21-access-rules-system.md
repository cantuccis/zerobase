# Access Rules System Design

> Design document for Zerobase's per-collection, per-operation access control system.
> Mirrors PocketBase's rule model: filter-like expressions that gate API access and act as implicit query filters.

---

## 1. Overview

Every collection has five **API rules** (defined in `ApiRules`):

| Rule | Applies to | Governs |
|------|-----------|---------|
| `list_rule` | `GET /api/collections/{c}/records` | Which records a user can see in list queries |
| `view_rule` | `GET /api/collections/{c}/records/{id}` | Whether a user can view a specific record |
| `create_rule` | `POST /api/collections/{c}/records` | Whether a user can create a record |
| `update_rule` | `PATCH /api/collections/{c}/records/{id}` | Whether a user can update a specific record |
| `delete_rule` | `DELETE /api/collections/{c}/records/{id}` | Whether a user can delete a specific record |

### Rule Values

| Value | Meaning | Example |
|-------|---------|---------|
| `None` (null) | **Locked** — only superusers can perform this operation | Default for all new collections |
| `Some("")` (empty string) | **Open** — anyone can perform this operation (no auth required) | Public blog listing |
| `Some("expr")` | **Conditional** — the expression is evaluated per request; access granted only if it evaluates to `true` | `@request.auth.id != ""` (authenticated users only) |

**Security default**: All rules default to `None` (locked). Collections are closed by default and must be explicitly opened.

---

## 2. Rule Expression Syntax

Rule expressions reuse the **existing filter parser** (`zerobase-db/src/filter.rs`) with extensions for request-context macros. This keeps one grammar, one parser, one SQL generator.

### 2.1 Operators

Inherited from the filter parser (already implemented):

| Category | Operators |
|----------|-----------|
| Comparison | `=`, `!=`, `>`, `>=`, `<`, `<=`, `~` (contains), `!~` (not contains) |
| Multi-value | `?=`, `?!=`, `?>`, `?>=`, `?<`, `?<=`, `?~`, `?!~` |
| Logical | `&&` (AND), `\|\|` (OR) |
| Grouping | `(`, `)` |

### 2.2 Values

| Type | Examples |
|------|---------|
| String | `"hello"`, `'world'` |
| Number | `42`, `3.14`, `-1` |
| Boolean | `true`, `false` |
| Null | `null` |
| Date macros | `@now`, `@today`, `@month`, `@year` |

### 2.3 Request-Context Macros

These are the **new** identifiers that the rule engine resolves from the current HTTP request before passing to the filter parser.

| Macro | Type | Resolves To | Description |
|-------|------|-------------|-------------|
| `@request.auth.id` | `String` | Authenticated user's record ID, or `""` if unauthenticated | Primary identity check |
| `@request.auth.collectionId` | `String` | ID of the auth collection the user belongs to | Distinguish user types (e.g., `users` vs `admins`) |
| `@request.auth.collectionName` | `String` | Name of the auth collection | More readable alternative to `collectionId` |
| `@request.auth.verified` | `Bool` | Whether the user's email is verified | Gate features behind email verification |
| `@request.auth.email` | `String` | Authenticated user's email | Email-based rules (rare, prefer ID-based) |
| `@request.auth.<field>` | `any` | Any field on the authenticated user's record | Access custom fields on auth collections |
| `@request.data.<field>` | `any` | Value of field in the request body (create/update) | Validate submitted data at rule level |
| `@request.method` | `String` | HTTP method (`GET`, `POST`, `PATCH`, `DELETE`) | Rarely needed since rules are already per-operation |
| `@request.headers.<name>` | `String` | Value of a specific request header | Advanced use cases (API keys, custom headers) |
| `@collection.<collectionName>.<field>` | `subquery` | Cross-collection subquery | Check data in other collections |

### 2.4 Record-Context Identifiers

Within a rule expression, bare identifiers (not prefixed with `@`) refer to **fields on the record being accessed**:

| Identifier | Resolves To |
|------------|-------------|
| `id` | The record's ID |
| `created` | The record's creation timestamp |
| `updated` | The record's last update timestamp |
| `<fieldName>` | The value of a user-defined field on the record |
| `<relation>.<field>` | Dot-notation traversal through a relation field |

---

## 3. Evaluation Context

The **RuleContext** is the runtime structure populated from the HTTP request before rule evaluation.

```rust
/// Runtime context for evaluating access rules.
/// Populated from the HTTP request by the auth middleware.
pub struct RuleContext {
    /// The authenticated user, if any.
    /// Contains the full record from the auth collection.
    pub auth: Option<AuthInfo>,

    /// The request body data (for create/update operations).
    /// Empty map for read/delete operations.
    pub request_data: serde_json::Map<String, serde_json::Value>,

    /// HTTP method.
    pub method: String,

    /// Request headers (lowercased keys).
    pub headers: HashMap<String, String>,
}

/// Information about the authenticated user.
pub struct AuthInfo {
    /// The user's record ID.
    pub id: String,

    /// The auth collection's ID.
    pub collection_id: String,

    /// The auth collection's name.
    pub collection_name: String,

    /// Whether the user's email is verified.
    pub verified: bool,

    /// The user's email.
    pub email: String,

    /// All fields from the user's auth record (for @request.auth.<field> access).
    pub record: serde_json::Map<String, serde_json::Value>,
}
```

### Where RuleContext Lives

- **Crate**: `zerobase-core` (it's a domain type, no I/O)
- **Populated by**: Auth middleware in `zerobase-api` (extracts JWT, loads user record)
- **Consumed by**: Rule evaluation engine in `zerobase-core` or `zerobase-db`
- **Injected via**: Axum request extensions (`request.extensions().get::<RuleContext>()`)

---

## 4. Rule Evaluation Engine

### 4.1 Architecture

The rule evaluation engine extends the existing filter parser. The key insight is: **rules ARE filters with macro substitution**.

```
Rule Expression String
        │
        ▼
┌─────────────────────────┐
│  1. Macro Resolution    │  Replace @request.auth.id → "user_abc123"
│     (string-level or    │  Replace @request.data.status → "published"
│      AST-level)         │  Replace @collection.* → SQL subquery
└─────────┬───────────────┘
          │
          ▼
┌─────────────────────────┐
│  2. Filter Parser       │  Existing tokenizer + parser
│     (zerobase-db)       │  Produces AST → parameterized SQL
└─────────┬───────────────┘
          │
          ▼
┌─────────────────────────┐
│  3. SQL Generation      │  WHERE clause with bound params
└─────────┬───────────────┘
          │
          ▼
   Parameterized SQL
```

### 4.2 Macro Resolution Strategy

**Approach: AST-level resolution** (preferred over string replacement for safety and correctness).

1. **Tokenizer extension**: The tokenizer already handles `@`-prefixed tokens (it recognizes `@now`, `@today`, etc.). Extend it to recognize `@request.*` and `@collection.*` as structured macro tokens.

2. **New token variants**:
```rust
enum Token {
    // ... existing tokens ...

    // Request-context macros (new)
    RequestAuthField(String),    // @request.auth.id, @request.auth.verified, etc.
    RequestDataField(String),    // @request.data.title, @request.data.status, etc.
    RequestMethod,               // @request.method
    RequestHeader(String),       // @request.headers.x_api_key

    // Cross-collection macro (new)
    CollectionRef {              // @collection.posts.author
        collection: String,
        field: String,
    },
}
```

3. **Resolution during SQL generation**: When the SQL generator encounters a macro token, it resolves it from the `RuleContext`:
   - `RequestAuthField("id")` → bound parameter with value from `context.auth.as_ref().map(|a| a.id.clone()).unwrap_or_default()`
   - `RequestDataField("status")` → bound parameter with value from `context.request_data.get("status")`
   - `CollectionRef { collection, field }` → SQL subquery: `SELECT "{field}" FROM "{collection}" WHERE ...`

### 4.3 Core Trait

```rust
/// Evaluates an access rule expression against a request context.
pub trait RuleEvaluator: Send + Sync {
    /// Evaluate a rule as a boolean (for create/update/delete).
    ///
    /// Returns `true` if the rule allows the operation.
    /// For rules that reference record fields, `record` contains the
    /// current record data (existing record for update/delete, or the
    /// proposed data for create).
    fn evaluate(
        &self,
        rule: &str,
        context: &RuleContext,
        record: Option<&serde_json::Map<String, serde_json::Value>>,
    ) -> Result<bool, ZerobaseError>;

    /// Convert a rule into a SQL WHERE clause (for list/view).
    ///
    /// The returned clause is AND-ed with any user-supplied filter.
    /// All macro values are emitted as bound parameters.
    fn to_filter(
        &self,
        rule: &str,
        context: &RuleContext,
    ) -> Result<(String, Vec<rusqlite::types::Value>), ZerobaseError>;
}
```

### 4.4 Dual Evaluation Modes

Rules serve two purposes depending on the operation:

#### Boolean Mode (create, update, delete)

The rule is evaluated as a **predicate** against a single record. The engine:

1. Resolves all `@request.*` macros from the `RuleContext`.
2. For record field references, substitutes values from the record data.
3. Evaluates the expression to `true` or `false`.
4. If `false`, returns `403 Forbidden`.

For **update/delete**, the record data is the **existing** record (before modification).
For **create**, the record data is the **submitted** data from the request body.

#### Filter Mode (list, view)

The rule is compiled into a **SQL WHERE clause** that is AND-ed with the user's filter. The engine:

1. Resolves all `@request.*` macros to bound SQL parameters.
2. Record field references remain as column references in SQL.
3. Returns `(where_clause, params)` to be injected into the query.

This means rules on list/view operations are **automatically scoping** — a user only sees records that match their rule.

---

## 5. Integration Points

### 5.1 Middleware Layer (zerobase-api)

A new **`rule_guard` middleware** (or per-handler logic) sits between the auth middleware and the record handlers:

```
Request
  │
  ▼
┌──────────────────────┐
│ Auth Middleware       │  Extract JWT → load user → populate RuleContext
│ (populate context)   │  Store in request extensions
└──────────┬───────────┘
           │
           ▼
┌──────────────────────┐
│ Rule Guard           │  Load collection's ApiRules
│ (check permissions)  │  Check the relevant rule for this operation
│                      │  For list/view: inject filter into query
│                      │  For CUD: evaluate boolean, return 403 if denied
└──────────┬───────────┘
           │
           ▼
┌──────────────────────┐
│ Record Handler       │  Normal CRUD logic
└──────────────────────┘
```

### 5.2 Handler Changes

**List records handler** (`GET /api/collections/{c}/records`):

```rust
async fn list_records(
    State(service): State<Arc<RecordService<R, S>>>,
    Path(collection): Path<String>,
    Query(params): Query<ListRecordsParams>,
    rule_context: Extension<RuleContext>,  // Injected by auth middleware
) -> Result<impl IntoResponse, ZerobaseError> {
    let schema = service.get_collection_schema(&collection)?;

    // 1. Check list_rule
    match &schema.rules.list_rule {
        None => return Err(ZerobaseError::Forbidden("...".into())),
        Some(rule) if rule.is_empty() => { /* open, no filter */ },
        Some(rule) => {
            // 2. Convert rule to SQL filter and AND it with user's filter
            let (rule_filter, rule_params) = evaluator.to_filter(rule, &rule_context)?;
            query.add_rule_filter(rule_filter, rule_params);
        }
    }

    // 3. Execute query (rule filter is now part of the WHERE clause)
    let records = service.list(&collection, &query)?;
    Ok(Json(records))
}
```

**View record handler** (`GET /api/collections/{c}/records/{id}`):

The view rule is applied the same way as list — as a SQL filter. If the record doesn't match the filter, it's as if it doesn't exist (return `404`, not `403`, to avoid information leakage).

**Create/Update/Delete handlers**:

```rust
// For create:
match &schema.rules.create_rule {
    None => return Err(ZerobaseError::Forbidden("...".into())),
    Some(rule) if rule.is_empty() => { /* open */ },
    Some(rule) => {
        let allowed = evaluator.evaluate(rule, &rule_context, Some(&request_data))?;
        if !allowed {
            return Err(ZerobaseError::Forbidden("...".into()));
        }
    }
}
```

### 5.3 Superuser Bypass

**Superusers always bypass all rules.** The rule guard checks `RuleContext.is_superuser` first:

```rust
if rule_context.is_superuser {
    // Skip all rule evaluation — superusers have full access
    return Ok(next.run(request).await);
}
```

This is determined during JWT validation: superuser tokens are issued from the `_superusers` system collection.

### 5.4 Filter Merging

When both a **rule filter** and a **user-supplied filter** exist, they are combined with `AND`:

```sql
-- User filter: status = "published"
-- List rule:   author = @request.auth.id

SELECT * FROM posts
WHERE (status = ?)           -- user filter
  AND (author = ?)           -- rule filter (injected)
ORDER BY created DESC
LIMIT 30 OFFSET 0
```

The rule filter is **always wrapped in parentheses** to prevent operator-precedence issues when combined with user filters.

---

## 6. Cross-Collection Rules (`@collection`)

The `@collection` macro enables rules that reference data in other collections. This is the most complex feature.

### Syntax

```
@collection.<collectionName>.<field>
```

### SQL Generation

Cross-collection references compile to **correlated subqueries**:

```
// Rule: @collection.members.user ?= @request.auth.id
// Meaning: "the auth user's ID appears in the 'user' field of some record in 'members'"

// Generated SQL:
EXISTS (
    SELECT 1 FROM "members"
    WHERE "members"."user" = ?  -- @request.auth.id bound as parameter
)
```

For rules that correlate with the current record:

```
// Rule: @collection.memberships.user = @request.auth.id && @collection.memberships.group = id
// Meaning: "there exists a membership linking the current user to this group"

// Generated SQL:
EXISTS (
    SELECT 1 FROM "memberships"
    WHERE "memberships"."user" = ?         -- @request.auth.id
      AND "memberships"."group" = "groups"."id"  -- correlate with outer query
)
```

### Performance Considerations

- Cross-collection rules generate subqueries — they can be slow on large tables.
- The `referenced_fields()` method on `ApiRules` already extracts field names for automatic indexing. Extend this to also extract collection names from `@collection` references.
- Consider adding an index recommendation system that warns when `@collection` rules target unindexed fields.

---

## 7. `@request.data` Rules (Write Operations)

The `@request.data` macro accesses fields from the **request body**, enabling rules that constrain what data can be written.

### Use Cases

| Rule | Purpose |
|------|---------|
| `@request.data.role != "admin"` | Prevent non-superusers from creating admin accounts |
| `@request.data.author = @request.auth.id` | Ensure users can only create posts attributed to themselves |
| `@request.data.status != "published" \|\| @request.auth.verified = true` | Only verified users can publish |

### Evaluation

For **create** operations, `@request.data` refers to the submitted record data.

For **update** operations, `@request.data` refers to the **submitted patch** (only the fields being changed), not the full record. The existing record fields are accessible via bare field names.

This distinction matters:

```
// Update rule: @request.data.status != "archived" || status = "draft"
// Meaning: You can only archive a record if its current status is "draft"
//
// @request.data.status = the NEW status being set
// status = the CURRENT status of the record
```

---

## 8. Error Responses

### 403 Forbidden (rule evaluation returns false)

```json
{
    "code": 403,
    "message": "You are not allowed to perform this request.",
    "data": {}
}
```

### 404 Not Found (view/list rule filters out the record)

For view operations where the rule filter excludes the record, return `404` (not `403`) to avoid leaking the record's existence:

```json
{
    "code": 404,
    "message": "The requested resource wasn't found.",
    "data": {}
}
```

### 400 Bad Request (malformed rule expression)

If a rule expression stored in the collection schema is malformed (should be caught at schema save time):

```json
{
    "code": 400,
    "message": "Invalid rule expression in collection schema.",
    "data": {
        "rule": {
            "code": "validation_invalid_rule",
            "message": "unexpected character '!' at position 5"
        }
    }
}
```

---

## 9. Rule Validation at Schema Time

Rules should be **validated when the collection schema is saved**, not at request time. This catches syntax errors early and prevents broken rules from blocking API access.

### Validation Steps

1. **Parse the expression** using the filter parser. If parsing fails, reject the schema update.
2. **Validate field references** — check that referenced fields exist in the collection (or in related collections for dot-notation).
3. **Validate `@request.data` references** — ensure referenced fields exist in the collection schema.
4. **Validate `@collection` references** — ensure the referenced collection and field exist.
5. **Type checking (optional, future)** — warn if comparing incompatible types (e.g., string field `> 42`).

```rust
/// Validate a rule expression against a collection schema.
pub fn validate_rule(
    rule: &str,
    collection: &Collection,
    all_collections: &[Collection], // For @collection validation
) -> Result<(), Vec<ValidationError>>;
```

---

## 10. Example Rules for Common Scenarios

### Public Read, Authenticated Write

```json
{
    "listRule": "",
    "viewRule": "",
    "createRule": "@request.auth.id != \"\"",
    "updateRule": "@request.auth.id != \"\"",
    "deleteRule": "@request.auth.id != \"\""
}
```

Anyone can list and view. Only authenticated users can create, update, or delete.

### Owner-Only Access

```json
{
    "listRule": "author = @request.auth.id",
    "viewRule": "author = @request.auth.id",
    "createRule": "@request.auth.id != \"\"",
    "updateRule": "author = @request.auth.id",
    "deleteRule": "author = @request.auth.id"
}
```

Users can only see and modify their own records. The list endpoint automatically filters to show only the authenticated user's records.

### Published Content with Owner Edit

```json
{
    "listRule": "status = \"published\" || author = @request.auth.id",
    "viewRule": "status = \"published\" || author = @request.auth.id",
    "createRule": "@request.auth.id != \"\"",
    "updateRule": "author = @request.auth.id",
    "deleteRule": "author = @request.auth.id"
}
```

Anyone can see published content. Authors can also see their own drafts. Only authors can edit/delete their own records.

### Role-Based Access (via Auth Collection Field)

```json
{
    "listRule": "",
    "viewRule": "",
    "createRule": "@request.auth.role = \"editor\" || @request.auth.role = \"admin\"",
    "updateRule": "@request.auth.role = \"editor\" || @request.auth.role = \"admin\"",
    "deleteRule": "@request.auth.role = \"admin\""
}
```

Assumes the auth collection has a `role` field. Everyone can read. Editors and admins can create/update. Only admins can delete.

### Multi-Collection Membership Check

```json
{
    "listRule": "@collection.team_members.user_id ?= @request.auth.id && @collection.team_members.team_id = team_id",
    "viewRule": "@collection.team_members.user_id ?= @request.auth.id && @collection.team_members.team_id = team_id",
    "createRule": "@collection.team_members.user_id ?= @request.auth.id && @collection.team_members.team_id = team_id && @collection.team_members.role = \"admin\"",
    "updateRule": "@collection.team_members.user_id ?= @request.auth.id && @collection.team_members.team_id = team_id",
    "deleteRule": "@collection.team_members.user_id ?= @request.auth.id && @collection.team_members.team_id = team_id && @collection.team_members.role = \"admin\""
}
```

Access is gated by team membership. Only team admins can create or delete.

### Verified Users Only

```json
{
    "listRule": "",
    "viewRule": "",
    "createRule": "@request.auth.verified = true",
    "updateRule": "@request.auth.verified = true",
    "deleteRule": "@request.auth.verified = true"
}
```

Read is public. Write operations require email-verified users.

### Constrain Written Data

```json
{
    "createRule": "@request.auth.id != \"\" && @request.data.author = @request.auth.id",
    "updateRule": "author = @request.auth.id && @request.data.role != \"admin\""
}
```

On create: users must set `author` to their own ID (prevents impersonation).
On update: only the author can update, and they cannot set `role` to `"admin"`.

### Locked Collection (Superusers Only)

```json
{
    "listRule": null,
    "viewRule": null,
    "createRule": null,
    "updateRule": null,
    "deleteRule": null
}
```

This is the default. All operations require superuser authentication.

---

## 11. Implementation Plan

### Phase 1: RuleContext and Auth Middleware

1. Define `RuleContext` and `AuthInfo` structs in `zerobase-core`.
2. Implement auth middleware in `zerobase-api` that extracts JWT, loads user record, and populates `RuleContext` into request extensions.
3. Add `is_superuser` flag to `RuleContext`.

### Phase 2: Filter Parser Extensions

1. Extend the tokenizer in `zerobase-db/src/filter.rs` to recognize `@request.*` tokens.
2. Add new AST node types for request-context macros.
3. Implement macro resolution: accept a `RuleContext` and substitute macro values as bound parameters.
4. Update `to_sql()` to handle the new node types.

### Phase 3: Rule Evaluator

1. Implement `RuleEvaluator` trait with two methods: `evaluate()` (boolean) and `to_filter()` (SQL).
2. Boolean evaluation: generate SQL `SELECT 1 FROM <table> WHERE <rule_filter> AND id = ?`, execute, check if row exists.
3. Filter mode: return the parameterized WHERE clause for injection into list/view queries.

### Phase 4: Handler Integration

1. Add rule checking to all record handlers (list, view, create, update, delete).
2. Superuser bypass check.
3. Filter merging for list/view operations.
4. Return appropriate error codes (403 for denied, 404 for filtered-out view).

### Phase 5: Rule Validation

1. Validate rule expressions when saving collection schemas.
2. Field reference validation against collection schema.
3. `@collection` reference validation.

### Phase 6: Cross-Collection Rules

1. Implement `@collection` macro → SQL subquery generation.
2. Correlated subquery support for rules that reference the outer record.
3. Index recommendations for cross-collection rule targets.

---

## 12. Testing Strategy

### Unit Tests (zerobase-core)

- `RuleContext` construction and field access
- `AuthInfo` from various auth states (unauthenticated, regular user, superuser, verified, unverified)

### Unit Tests (zerobase-db)

- Tokenizer: `@request.auth.id`, `@request.data.field`, `@collection.name.field` parsed correctly
- Parser: expressions with macros produce correct AST
- SQL generation: macros resolve to bound parameters
- Filter merging: rule filter AND user filter combined correctly
- Cross-collection: `@collection` generates correct subqueries

### Integration Tests

- **Locked rule**: requests without superuser token return 403
- **Open rule**: unauthenticated requests succeed
- **Auth-required rule**: authenticated requests succeed, unauthenticated fail
- **Owner-only rule**: user A can see their records, not user B's
- **List filtering**: list endpoint returns only records matching the rule
- **View filtering**: view endpoint returns 404 for records not matching the rule
- **Create constraint**: `@request.data` rules reject invalid submissions
- **Update constraint**: existing record fields + request data evaluated correctly
- **Superuser bypass**: superusers access records regardless of rules
- **Cross-collection**: membership-gated access works correctly
- **Malformed rule**: schema save with invalid expression returns validation error

### Property Tests (optional)

- Any rule that parses successfully produces valid SQL
- Rule filters never allow SQL injection (fuzz with adversarial strings)
- Boolean evaluation and filter evaluation agree (for single-record checks)

---

## 13. Crate Placement

| Component | Crate | Rationale |
|-----------|-------|-----------|
| `RuleContext`, `AuthInfo` | `zerobase-core` | Domain types, no I/O |
| `RuleEvaluator` trait | `zerobase-core` | Interface definition, no I/O |
| Filter parser extensions | `zerobase-db` | SQL generation lives here |
| `SqliteRuleEvaluator` impl | `zerobase-db` | Needs DB access for boolean evaluation and subqueries |
| Rule guard middleware | `zerobase-api` | HTTP layer concern |
| Rule validation | `zerobase-core` | Validates at schema save time |

---

## 14. Security Considerations

1. **SQL injection prevention**: All macro values are emitted as bound parameters, never interpolated into SQL strings. The existing filter parser already enforces this.

2. **Information leakage**: View rules that filter out records return `404`, not `403`, to prevent attackers from enumerating record IDs.

3. **Default deny**: Collections default to locked (`None` rules). This is a critical security property — new collections are inaccessible until explicitly configured.

4. **Superuser isolation**: Superuser tokens are issued from a separate `_superusers` system collection, not from regular auth collections. This prevents privilege escalation through rule manipulation.

5. **Rule validation**: Malformed or referencing-nonexistent-fields rules are rejected at schema save time, preventing runtime errors that could fail open.

6. **Cross-collection safety**: `@collection` references are resolved to subqueries with proper table quoting and parameter binding. Collection names are validated against existing collections.
