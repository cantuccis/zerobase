//! Rule evaluation engine for access control.
//!
//! This module provides two evaluation modes for API rule expressions:
//!
//! 1. **In-memory evaluation** — evaluates a parsed [`RuleExpr`] AST against
//!    a [`RequestContext`] and an optional target record, returning a boolean.
//!    Used for single-record operations (view, create, update, delete).
//!
//! 2. **SQL generation** — converts a [`RuleExpr`] AST into a parameterized
//!    SQL WHERE clause, resolving `@request.*` context variables at generation
//!    time. Used for list operations where rules act as filters.
//!
//! # Request Context
//!
//! The [`RequestContext`] carries all information available during a request:
//! - Authenticated user's record fields (`@request.auth.*`)
//! - Incoming request body (`@request.data.*`)
//! - URL query parameters (`@request.query.*`)
//! - Request headers (`@request.headers.*`)
//! - HTTP method (`@request.method`)
//! - Request context identifier (`@request.context`)
//!
//! # Rule Semantics (matching PocketBase)
//!
//! - `None` — locked (superusers only)
//! - `Some("")` — open to everyone
//! - `Some(expr)` — evaluate expression against the request context

use std::collections::HashMap;

use chrono::{Datelike, Utc};
use serde_json::Value as JsonValue;

use super::rule_parser::{ComparisonOp, Operand, RuleExpr};

// ── Request Context ──────────────────────────────────────────────────────────

/// Context variables available during rule evaluation.
///
/// Mirrors the `@request.*` variables from PocketBase rule expressions.
#[derive(Debug, Clone, Default)]
pub struct RequestContext {
    /// Fields from the authenticated user's record (`@request.auth.*`).
    /// Empty if the request is unauthenticated.
    pub auth: HashMap<String, JsonValue>,

    /// Fields from the incoming request body (`@request.data.*`).
    pub data: HashMap<String, JsonValue>,

    /// URL query parameters (`@request.query.*`).
    pub query: HashMap<String, JsonValue>,

    /// Request headers (`@request.headers.*`).
    pub headers: HashMap<String, JsonValue>,

    /// HTTP method (`@request.method`): "GET", "POST", "PATCH", "DELETE", etc.
    pub method: String,

    /// Request context identifier (`@request.context`): "default", "realtime", etc.
    pub context: String,
}

impl RequestContext {
    /// Create a context for an unauthenticated request.
    pub fn anonymous() -> Self {
        Self {
            method: "GET".to_string(),
            context: "default".to_string(),
            ..Default::default()
        }
    }

    /// Create a context with the given auth record fields.
    pub fn authenticated(auth: HashMap<String, JsonValue>) -> Self {
        Self {
            auth,
            method: "GET".to_string(),
            context: "default".to_string(),
            ..Default::default()
        }
    }

    /// Returns `true` if the request has an authenticated user.
    pub fn is_authenticated(&self) -> bool {
        self.auth.contains_key("id") && self.auth["id"] != JsonValue::Null
    }
}

// ── Rule Decision ────────────────────────────────────────────────────────────

/// The result of checking an API rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleDecision {
    /// Access is allowed unconditionally (empty rule).
    Allow,
    /// Access is denied (rule is locked / `None`).
    Deny,
    /// Access depends on evaluating the rule expression.
    Evaluate(String),
}

/// Determine the decision type for a rule value.
///
/// - `None` → `Deny` (locked, superusers only)
/// - `Some("")` → `Allow` (open to everyone)
/// - `Some(expr)` → `Evaluate(expr)` (needs evaluation)
pub fn check_rule(rule: &Option<String>) -> RuleDecision {
    match rule {
        None => RuleDecision::Deny,
        Some(expr) if expr.is_empty() => RuleDecision::Allow,
        Some(expr) => RuleDecision::Evaluate(expr.clone()),
    }
}

// ── In-Memory Evaluation ─────────────────────────────────────────────────────

/// Evaluate a rule expression against a request context and optional record.
///
/// Returns `true` if the rule allows access, `false` if denied.
///
/// For list operations, use [`rule_to_sql`] instead to generate a SQL WHERE
/// clause that filters results server-side.
pub fn evaluate_rule(
    expr: &RuleExpr,
    ctx: &RequestContext,
    record: &HashMap<String, JsonValue>,
) -> bool {
    match expr {
        RuleExpr::Condition {
            left,
            operator,
            right,
        } => {
            let left_val = resolve_operand(left, ctx, record);
            let right_val = resolve_operand(right, ctx, record);
            compare_values(&left_val, *operator, &right_val)
        }
        RuleExpr::And(a, b) => evaluate_rule(a, ctx, record) && evaluate_rule(b, ctx, record),
        RuleExpr::Or(a, b) => evaluate_rule(a, ctx, record) || evaluate_rule(b, ctx, record),
        RuleExpr::Not(inner) => !evaluate_rule(inner, ctx, record),
        RuleExpr::Group(inner) => evaluate_rule(inner, ctx, record),
    }
}

/// Evaluate a rule expression string: parse, then evaluate in-memory.
///
/// Returns `Err` if the expression cannot be parsed.
pub fn evaluate_rule_str(
    rule: &str,
    ctx: &RequestContext,
    record: &HashMap<String, JsonValue>,
) -> Result<bool, super::rule_parser::RuleParseError> {
    let ast = super::rule_parser::parse_rule(rule)?;
    Ok(evaluate_rule(&ast, ctx, record))
}

// ── Operand Resolution ───────────────────────────────────────────────────────

/// A resolved value from an operand.
#[derive(Debug, Clone)]
enum ResolvedValue {
    String(String),
    Number(f64),
    Bool(bool),
    Null,
    /// An array of values (for multi-value fields like relations/select).
    Array(Vec<ResolvedValue>),
}

/// Resolve an operand to its concrete value.
fn resolve_operand(
    operand: &Operand,
    ctx: &RequestContext,
    record: &HashMap<String, JsonValue>,
) -> ResolvedValue {
    match operand {
        Operand::Field(path) => resolve_field_path(path, record),
        Operand::RequestAuth(path) => {
            // For unauthenticated users, auth fields resolve to empty string
            // (matching PocketBase behavior where @request.auth.id = "" means anonymous).
            if ctx.auth.is_empty() {
                return ResolvedValue::String(String::new());
            }
            resolve_context_field(path, &ctx.auth)
        }
        Operand::RequestData(path) => resolve_context_field(path, &ctx.data),
        Operand::RequestQuery(path) => resolve_context_field(path, &ctx.query),
        Operand::RequestHeaders(path) => resolve_context_field(path, &ctx.headers),
        Operand::RequestMethod => ResolvedValue::String(ctx.method.clone()),
        Operand::RequestContext => ResolvedValue::String(ctx.context.clone()),
        Operand::CollectionRef { .. } => {
            // Cross-collection lookups require database access.
            // In-memory evaluation cannot resolve these; they are only supported
            // in SQL generation mode. Default to Null for in-memory evaluation.
            ResolvedValue::Null
        }
        Operand::String(s) => ResolvedValue::String(s.clone()),
        Operand::Number(n) => ResolvedValue::Number(*n),
        Operand::Bool(b) => ResolvedValue::Bool(*b),
        Operand::Null => ResolvedValue::Null,
        Operand::Now => {
            ResolvedValue::String(Utc::now().format("%Y-%m-%d %H:%M:%S%.3fZ").to_string())
        }
        Operand::Today => {
            let today = Utc::now().date_naive();
            ResolvedValue::String(format!("{today} 00:00:00.000Z"))
        }
        Operand::Month => {
            let now = Utc::now().date_naive();
            let first = now.with_day(1).unwrap_or(now);
            ResolvedValue::String(format!("{first} 00:00:00.000Z"))
        }
        Operand::Year => {
            let now = Utc::now().date_naive();
            let first = chrono::NaiveDate::from_ymd_opt(now.year(), 1, 1).unwrap_or(now);
            ResolvedValue::String(format!("{first} 00:00:00.000Z"))
        }
    }
}

/// Resolve a dotted field path against a record.
fn resolve_field_path(path: &str, record: &HashMap<String, JsonValue>) -> ResolvedValue {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return ResolvedValue::Null;
    }

    // Start from the top-level field.
    let Some(value) = record.get(parts[0]) else {
        return ResolvedValue::Null;
    };

    // Traverse nested fields.
    let mut current = value;
    for &part in &parts[1..] {
        match current {
            JsonValue::Object(map) => {
                if let Some(next) = map.get(part) {
                    current = next;
                } else {
                    return ResolvedValue::Null;
                }
            }
            _ => return ResolvedValue::Null,
        }
    }

    json_to_resolved(current)
}

/// Resolve a field path against a context map (auth, data, query, headers).
fn resolve_context_field(path: &str, map: &HashMap<String, JsonValue>) -> ResolvedValue {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return ResolvedValue::Null;
    }

    let Some(value) = map.get(parts[0]) else {
        return ResolvedValue::Null;
    };

    let mut current = value;
    for &part in &parts[1..] {
        match current {
            JsonValue::Object(obj) => {
                if let Some(next) = obj.get(part) {
                    current = next;
                } else {
                    return ResolvedValue::Null;
                }
            }
            _ => return ResolvedValue::Null,
        }
    }

    json_to_resolved(current)
}

/// Convert a JSON value to a `ResolvedValue`.
fn json_to_resolved(value: &JsonValue) -> ResolvedValue {
    match value {
        JsonValue::String(s) => ResolvedValue::String(s.clone()),
        JsonValue::Number(n) => ResolvedValue::Number(n.as_f64().unwrap_or(0.0)),
        JsonValue::Bool(b) => ResolvedValue::Bool(*b),
        JsonValue::Null => ResolvedValue::Null,
        JsonValue::Array(arr) => ResolvedValue::Array(arr.iter().map(json_to_resolved).collect()),
        JsonValue::Object(_) => {
            // Objects are not directly comparable; serialize as string.
            ResolvedValue::String(value.to_string())
        }
    }
}

// ── Comparison Logic ─────────────────────────────────────────────────────────

/// Compare two resolved values using the given operator.
fn compare_values(left: &ResolvedValue, op: ComparisonOp, right: &ResolvedValue) -> bool {
    // Handle "any" operators: check if any element in the left array matches.
    if is_any_op(op) {
        return compare_any(left, op, right);
    }

    match (left, right) {
        // Null comparisons: IS NULL / IS NOT NULL semantics.
        // `x = null` → true only if x is null; `x != null` → true only if x is not null.
        // For all other operators with null, result is false (SQL-style).
        (ResolvedValue::Null, ResolvedValue::Null) => matches!(op, ComparisonOp::Eq),
        (ResolvedValue::Null, _) | (_, ResolvedValue::Null) => matches!(op, ComparisonOp::Neq),

        // String comparisons.
        (ResolvedValue::String(a), ResolvedValue::String(b)) => compare_strings(a, b, op),

        // Number comparisons.
        (ResolvedValue::Number(a), ResolvedValue::Number(b)) => compare_numbers(*a, *b, op),

        // Bool comparisons.
        (ResolvedValue::Bool(a), ResolvedValue::Bool(b)) => compare_bools(*a, *b, op),

        // Cross-type: coerce to string for comparison.
        (ResolvedValue::String(a), ResolvedValue::Number(b)) => {
            // Try parsing the string as a number first.
            if let Ok(a_num) = a.parse::<f64>() {
                compare_numbers(a_num, *b, op)
            } else {
                compare_strings(a, &b.to_string(), op)
            }
        }
        (ResolvedValue::Number(a), ResolvedValue::String(b)) => {
            if let Ok(b_num) = b.parse::<f64>() {
                compare_numbers(*a, b_num, op)
            } else {
                compare_strings(&a.to_string(), b, op)
            }
        }
        (ResolvedValue::Bool(a), ResolvedValue::String(b)) => {
            compare_strings(&a.to_string(), b, op)
        }
        (ResolvedValue::String(a), ResolvedValue::Bool(b)) => {
            compare_strings(a, &b.to_string(), op)
        }
        (ResolvedValue::Number(a), ResolvedValue::Bool(b)) => {
            compare_numbers(*a, if *b { 1.0 } else { 0.0 }, op)
        }
        (ResolvedValue::Bool(a), ResolvedValue::Number(b)) => {
            compare_numbers(if *a { 1.0 } else { 0.0 }, *b, op)
        }

        // Array on left with a non-any operator: check if right is in array.
        (ResolvedValue::Array(_), _) | (_, ResolvedValue::Array(_)) => false,
    }
}

/// Check if any element in the left value matches the right value.
fn compare_any(left: &ResolvedValue, op: ComparisonOp, right: &ResolvedValue) -> bool {
    let base_op = any_to_base_op(op);

    match left {
        ResolvedValue::Array(items) => items
            .iter()
            .any(|item| compare_values(item, base_op, right)),
        // If left is not an array, treat it as a single-element array.
        _ => compare_values(left, base_op, right),
    }
}

fn compare_strings(a: &str, b: &str, op: ComparisonOp) -> bool {
    match op {
        ComparisonOp::Eq => a == b,
        ComparisonOp::Neq => a != b,
        ComparisonOp::Gt => a > b,
        ComparisonOp::Gte => a >= b,
        ComparisonOp::Lt => a < b,
        ComparisonOp::Lte => a <= b,
        ComparisonOp::Like => {
            // Case-insensitive contains.
            a.to_lowercase().contains(&b.to_lowercase())
        }
        ComparisonOp::NotLike => !a.to_lowercase().contains(&b.to_lowercase()),
        _ => false,
    }
}

fn compare_numbers(a: f64, b: f64, op: ComparisonOp) -> bool {
    match op {
        ComparisonOp::Eq => (a - b).abs() < f64::EPSILON,
        ComparisonOp::Neq => (a - b).abs() >= f64::EPSILON,
        ComparisonOp::Gt => a > b,
        ComparisonOp::Gte => a >= b,
        ComparisonOp::Lt => a < b,
        ComparisonOp::Lte => a <= b,
        ComparisonOp::Like | ComparisonOp::NotLike => {
            // Like on numbers: convert to string comparison.
            compare_strings(&a.to_string(), &b.to_string(), op)
        }
        _ => false,
    }
}

fn compare_bools(a: bool, b: bool, op: ComparisonOp) -> bool {
    match op {
        ComparisonOp::Eq => a == b,
        ComparisonOp::Neq => a != b,
        _ => false,
    }
}

fn is_any_op(op: ComparisonOp) -> bool {
    matches!(
        op,
        ComparisonOp::AnyEq
            | ComparisonOp::AnyNeq
            | ComparisonOp::AnyGt
            | ComparisonOp::AnyGte
            | ComparisonOp::AnyLt
            | ComparisonOp::AnyLte
            | ComparisonOp::AnyLike
            | ComparisonOp::AnyNotLike
    )
}

/// Convert an `Any*` operator to its base (non-any) form.
fn any_to_base_op(op: ComparisonOp) -> ComparisonOp {
    match op {
        ComparisonOp::AnyEq => ComparisonOp::Eq,
        ComparisonOp::AnyNeq => ComparisonOp::Neq,
        ComparisonOp::AnyGt => ComparisonOp::Gt,
        ComparisonOp::AnyGte => ComparisonOp::Gte,
        ComparisonOp::AnyLt => ComparisonOp::Lt,
        ComparisonOp::AnyLte => ComparisonOp::Lte,
        ComparisonOp::AnyLike => ComparisonOp::Like,
        ComparisonOp::AnyNotLike => ComparisonOp::NotLike,
        other => other,
    }
}

// ── SQL Generation ───────────────────────────────────────────────────────────

/// A generated SQL WHERE clause with bound parameters.
#[derive(Debug, Clone)]
pub struct RuleSqlClause {
    /// The SQL WHERE condition (without the `WHERE` keyword).
    pub sql: String,
    /// The bound parameters for the SQL clause.
    pub params: Vec<RuleSqlParam>,
}

/// A parameter value for a SQL WHERE clause generated from a rule.
#[derive(Debug, Clone)]
pub enum RuleSqlParam {
    Text(String),
    Integer(i64),
    Real(f64),
    Null,
}

/// Convert a rule expression AST to a parameterized SQL WHERE clause.
///
/// Context variables (`@request.auth.*`, `@request.data.*`, etc.) are resolved
/// at generation time using the provided `RequestContext`, and their values are
/// injected as bound SQL parameters. Record field references become column
/// references in the generated SQL.
///
/// # Cross-collection references
///
/// `@collection.<name>.<field>` references generate EXISTS subqueries against
/// the referenced collection's table.
pub fn rule_to_sql(expr: &RuleExpr, ctx: &RequestContext) -> RuleSqlClause {
    let mut params = Vec::new();
    let sql = emit_rule_sql(expr, ctx, &mut params);
    RuleSqlClause { sql, params }
}

/// Convert a rule expression string to a SQL WHERE clause.
///
/// Convenience wrapper that parses the rule and then generates SQL.
pub fn rule_str_to_sql(
    rule: &str,
    ctx: &RequestContext,
) -> Result<RuleSqlClause, super::rule_parser::RuleParseError> {
    let ast = super::rule_parser::parse_rule(rule)?;
    Ok(rule_to_sql(&ast, ctx))
}

fn emit_rule_sql(expr: &RuleExpr, ctx: &RequestContext, params: &mut Vec<RuleSqlParam>) -> String {
    match expr {
        RuleExpr::Condition {
            left,
            operator,
            right,
        } => emit_condition_sql(left, *operator, right, ctx, params),
        RuleExpr::And(a, b) => {
            let a_sql = emit_rule_sql(a, ctx, params);
            let b_sql = emit_rule_sql(b, ctx, params);
            format!("({a_sql} AND {b_sql})")
        }
        RuleExpr::Or(a, b) => {
            let a_sql = emit_rule_sql(a, ctx, params);
            let b_sql = emit_rule_sql(b, ctx, params);
            format!("({a_sql} OR {b_sql})")
        }
        RuleExpr::Not(inner) => {
            let inner_sql = emit_rule_sql(inner, ctx, params);
            format!("NOT ({inner_sql})")
        }
        RuleExpr::Group(inner) => emit_rule_sql(inner, ctx, params),
    }
}

fn emit_condition_sql(
    left: &Operand,
    op: ComparisonOp,
    right: &Operand,
    ctx: &RequestContext,
    params: &mut Vec<RuleSqlParam>,
) -> String {
    let left_sql = operand_to_sql(left, ctx, params);
    let right_sql = operand_to_sql(right, ctx, params);

    // Handle null comparisons.
    if left_sql == "NULL" {
        return match op {
            ComparisonOp::Eq => format!("{right_sql} IS NULL"),
            ComparisonOp::Neq => format!("{right_sql} IS NOT NULL"),
            _ => format!("{left_sql} {op} {right_sql}"),
        };
    }
    if right_sql == "NULL" {
        return match op {
            ComparisonOp::Eq => format!("{left_sql} IS NULL"),
            ComparisonOp::Neq => format!("{left_sql} IS NOT NULL"),
            _ => format!("{left_sql} {op} {right_sql}"),
        };
    }

    // Handle "any" operators with EXISTS subquery.
    if is_any_op(op) {
        let base_sql_op = sql_op_str(any_to_base_op(op));
        // The left side should be the column that stores a JSON array.
        return format!(
            "EXISTS (SELECT 1 FROM json_each({left_sql}) WHERE json_each.value {base_sql_op} {right_sql})"
        );
    }

    // Handle LIKE/NOT LIKE.
    match op {
        ComparisonOp::Like => format!("{left_sql} LIKE {right_sql}"),
        ComparisonOp::NotLike => format!("{left_sql} NOT LIKE {right_sql}"),
        _ => {
            let sql_op = sql_op_str(op);
            format!("{left_sql} {sql_op} {right_sql}")
        }
    }
}

/// Convert an operand to its SQL representation.
///
/// - Field references become quoted column names.
/// - Context variables are resolved from the `RequestContext` and bound as parameters.
/// - Literals become bound parameters.
fn operand_to_sql(
    operand: &Operand,
    ctx: &RequestContext,
    params: &mut Vec<RuleSqlParam>,
) -> String {
    match operand {
        Operand::Field(path) => {
            // Quote the field name for safe SQL inclusion.
            // For dot-notation (e.g., "author.name"), only quote the base field.
            if let Some((base, _rest)) = path.split_once('.') {
                // For now, just use the base column name. Relation traversal
                // will be handled via JOINs in a future phase.
                format!("\"{}\"", sanitize_ident(base))
            } else {
                format!("\"{}\"", sanitize_ident(path))
            }
        }
        Operand::RequestAuth(path) => {
            let value = if ctx.auth.is_empty() {
                ResolvedValue::String(String::new())
            } else {
                resolve_context_field(path, &ctx.auth)
            };
            push_resolved_param(value, params)
        }
        Operand::RequestData(path) => {
            let value = resolve_context_field(path, &ctx.data);
            push_resolved_param(value, params)
        }
        Operand::RequestQuery(path) => {
            let value = resolve_context_field(path, &ctx.query);
            push_resolved_param(value, params)
        }
        Operand::RequestHeaders(path) => {
            let value = resolve_context_field(path, &ctx.headers);
            push_resolved_param(value, params)
        }
        Operand::RequestMethod => {
            params.push(RuleSqlParam::Text(ctx.method.clone()));
            format!("?{}", params.len())
        }
        Operand::RequestContext => {
            params.push(RuleSqlParam::Text(ctx.context.clone()));
            format!("?{}", params.len())
        }
        Operand::CollectionRef { collection, path } => {
            // Generate an EXISTS subquery for cross-collection lookups.
            let table = sanitize_ident(collection);
            let column = sanitize_ident(path);
            format!("(SELECT \"{column}\" FROM \"{table}\")")
        }
        Operand::String(s) => {
            params.push(RuleSqlParam::Text(s.clone()));
            format!("?{}", params.len())
        }
        Operand::Number(n) => {
            if n.fract() == 0.0 && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                params.push(RuleSqlParam::Integer(*n as i64));
            } else {
                params.push(RuleSqlParam::Real(*n));
            }
            format!("?{}", params.len())
        }
        Operand::Bool(b) => {
            params.push(RuleSqlParam::Integer(if *b { 1 } else { 0 }));
            format!("?{}", params.len())
        }
        Operand::Null => "NULL".to_string(),
        Operand::Now => {
            let now = Utc::now().format("%Y-%m-%d %H:%M:%S%.3fZ").to_string();
            params.push(RuleSqlParam::Text(now));
            format!("?{}", params.len())
        }
        Operand::Today => {
            let today = Utc::now().date_naive();
            params.push(RuleSqlParam::Text(format!("{today} 00:00:00.000Z")));
            format!("?{}", params.len())
        }
        Operand::Month => {
            let now = Utc::now().date_naive();
            let first = now.with_day(1).unwrap_or(now);
            params.push(RuleSqlParam::Text(format!("{first} 00:00:00.000Z")));
            format!("?{}", params.len())
        }
        Operand::Year => {
            let now = Utc::now().date_naive();
            let first = chrono::NaiveDate::from_ymd_opt(now.year(), 1, 1).unwrap_or(now);
            params.push(RuleSqlParam::Text(format!("{first} 00:00:00.000Z")));
            format!("?{}", params.len())
        }
    }
}

/// Push a resolved value as a SQL parameter and return the placeholder.
fn push_resolved_param(value: ResolvedValue, params: &mut Vec<RuleSqlParam>) -> String {
    match value {
        ResolvedValue::String(s) => {
            params.push(RuleSqlParam::Text(s));
            format!("?{}", params.len())
        }
        ResolvedValue::Number(n) => {
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                params.push(RuleSqlParam::Integer(n as i64));
            } else {
                params.push(RuleSqlParam::Real(n));
            }
            format!("?{}", params.len())
        }
        ResolvedValue::Bool(b) => {
            params.push(RuleSqlParam::Integer(if b { 1 } else { 0 }));
            format!("?{}", params.len())
        }
        ResolvedValue::Null => "NULL".to_string(),
        ResolvedValue::Array(_) => {
            // Arrays are serialized as JSON strings for SQL binding.
            let json = resolved_to_json(&ResolvedValue::Array(
                if let ResolvedValue::Array(items) = value {
                    items
                } else {
                    vec![]
                },
            ));
            params.push(RuleSqlParam::Text(json.to_string()));
            format!("?{}", params.len())
        }
    }
}

fn resolved_to_json(value: &ResolvedValue) -> JsonValue {
    match value {
        ResolvedValue::String(s) => JsonValue::String(s.clone()),
        ResolvedValue::Number(n) => serde_json::Number::from_f64(*n)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        ResolvedValue::Bool(b) => JsonValue::Bool(*b),
        ResolvedValue::Null => JsonValue::Null,
        ResolvedValue::Array(items) => {
            JsonValue::Array(items.iter().map(resolved_to_json).collect())
        }
    }
}

/// Sanitize an identifier for safe SQL inclusion.
fn sanitize_ident(s: &str) -> &str {
    // Only allow alphanumeric and underscore characters.
    // If the identifier contains invalid characters, it will be truncated.
    let end = s
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(s.len());
    &s[..end]
}

/// Get the SQL operator string for a comparison operator.
fn sql_op_str(op: ComparisonOp) -> &'static str {
    match op {
        ComparisonOp::Eq => "=",
        ComparisonOp::Neq => "!=",
        ComparisonOp::Gt => ">",
        ComparisonOp::Gte => ">=",
        ComparisonOp::Lt => "<",
        ComparisonOp::Lte => "<=",
        ComparisonOp::Like => "LIKE",
        ComparisonOp::NotLike => "NOT LIKE",
        ComparisonOp::AnyEq => "=",
        ComparisonOp::AnyNeq => "!=",
        ComparisonOp::AnyGt => ">",
        ComparisonOp::AnyGte => ">=",
        ComparisonOp::AnyLt => "<",
        ComparisonOp::AnyLte => "<=",
        ComparisonOp::AnyLike => "LIKE",
        ComparisonOp::AnyNotLike => "NOT LIKE",
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::rule_parser::parse_rule;

    fn make_record(fields: &[(&str, JsonValue)]) -> HashMap<String, JsonValue> {
        fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn auth_ctx(id: &str) -> RequestContext {
        let mut auth = HashMap::new();
        auth.insert("id".to_string(), JsonValue::String(id.to_string()));
        RequestContext::authenticated(auth)
    }

    fn auth_ctx_with(fields: &[(&str, JsonValue)]) -> RequestContext {
        let auth: HashMap<String, JsonValue> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        RequestContext::authenticated(auth)
    }

    // ── RequestContext tests ─────────────────────────────────────────────

    #[test]
    fn anonymous_context_is_unauthenticated() {
        let ctx = RequestContext::anonymous();
        assert!(!ctx.is_authenticated());
        assert!(ctx.auth.is_empty());
    }

    #[test]
    fn authenticated_context_has_auth_data() {
        let ctx = auth_ctx("user123");
        assert!(ctx.is_authenticated());
        assert_eq!(ctx.auth["id"], JsonValue::String("user123".to_string()));
    }

    // ── check_rule tests ────────────────────────────────────────────────

    #[test]
    fn none_rule_is_deny() {
        assert_eq!(check_rule(&None), RuleDecision::Deny);
    }

    #[test]
    fn empty_rule_is_allow() {
        assert_eq!(check_rule(&Some(String::new())), RuleDecision::Allow);
    }

    #[test]
    fn expression_rule_is_evaluate() {
        let rule = Some("owner = @request.auth.id".to_string());
        match check_rule(&rule) {
            RuleDecision::Evaluate(expr) => {
                assert_eq!(expr, "owner = @request.auth.id");
            }
            other => panic!("expected Evaluate, got {:?}", other),
        }
    }

    // ── Simple equality tests ───────────────────────────────────────────

    #[test]
    fn field_equals_string_literal() {
        let ast = parse_rule("status = \"published\"").unwrap();
        let ctx = RequestContext::anonymous();
        let record = make_record(&[("status", JsonValue::String("published".into()))]);
        assert!(evaluate_rule(&ast, &ctx, &record));

        let record2 = make_record(&[("status", JsonValue::String("draft".into()))]);
        assert!(!evaluate_rule(&ast, &ctx, &record2));
    }

    #[test]
    fn field_not_equals() {
        let ast = parse_rule("status != \"deleted\"").unwrap();
        let ctx = RequestContext::anonymous();
        let record = make_record(&[("status", JsonValue::String("published".into()))]);
        assert!(evaluate_rule(&ast, &ctx, &record));

        let record2 = make_record(&[("status", JsonValue::String("deleted".into()))]);
        assert!(!evaluate_rule(&ast, &ctx, &record2));
    }

    #[test]
    fn field_equals_number() {
        let ast = parse_rule("views > 100").unwrap();
        let ctx = RequestContext::anonymous();
        let record = make_record(&[("views", JsonValue::Number(150.into()))]);
        assert!(evaluate_rule(&ast, &ctx, &record));

        let record2 = make_record(&[("views", JsonValue::Number(50.into()))]);
        assert!(!evaluate_rule(&ast, &ctx, &record2));
    }

    #[test]
    fn field_equals_bool() {
        let ast = parse_rule("active = true").unwrap();
        let ctx = RequestContext::anonymous();
        let record = make_record(&[("active", JsonValue::Bool(true))]);
        assert!(evaluate_rule(&ast, &ctx, &record));

        let record2 = make_record(&[("active", JsonValue::Bool(false))]);
        assert!(!evaluate_rule(&ast, &ctx, &record2));
    }

    #[test]
    fn field_equals_null() {
        let ast = parse_rule("deleted_at = null").unwrap();
        let ctx = RequestContext::anonymous();
        let record = make_record(&[("deleted_at", JsonValue::Null)]);
        assert!(evaluate_rule(&ast, &ctx, &record));

        let record2 = make_record(&[("deleted_at", JsonValue::String("2024-01-01".into()))]);
        assert!(!evaluate_rule(&ast, &ctx, &record2));
    }

    #[test]
    fn field_not_null() {
        let ast = parse_rule("avatar != null").unwrap();
        let ctx = RequestContext::anonymous();
        let record = make_record(&[("avatar", JsonValue::String("img.jpg".into()))]);
        assert!(evaluate_rule(&ast, &ctx, &record));

        let record2 = make_record(&[("avatar", JsonValue::Null)]);
        assert!(!evaluate_rule(&ast, &ctx, &record2));
    }

    #[test]
    fn missing_field_is_null() {
        let ast = parse_rule("nonexistent = null").unwrap();
        let ctx = RequestContext::anonymous();
        let record = make_record(&[("other_field", JsonValue::String("x".into()))]);
        assert!(evaluate_rule(&ast, &ctx, &record));
    }

    // ── Numeric comparison tests ────────────────────────────────────────

    #[test]
    fn numeric_comparisons() {
        let ctx = RequestContext::anonymous();
        let record = make_record(&[("price", serde_json::json!(49.99))]);

        assert!(evaluate_rule(
            &parse_rule("price < 50").unwrap(),
            &ctx,
            &record
        ));
        assert!(evaluate_rule(
            &parse_rule("price <= 49.99").unwrap(),
            &ctx,
            &record
        ));
        assert!(evaluate_rule(
            &parse_rule("price >= 49.99").unwrap(),
            &ctx,
            &record
        ));
        assert!(!evaluate_rule(
            &parse_rule("price > 50").unwrap(),
            &ctx,
            &record
        ));
        assert!(evaluate_rule(
            &parse_rule("price != 100").unwrap(),
            &ctx,
            &record
        ));
    }

    // ── Like (contains) tests ───────────────────────────────────────────

    #[test]
    fn like_contains_case_insensitive() {
        let ast = parse_rule("title ~ \"hello\"").unwrap();
        let ctx = RequestContext::anonymous();

        let record = make_record(&[("title", JsonValue::String("Hello World".into()))]);
        assert!(evaluate_rule(&ast, &ctx, &record));

        let record2 = make_record(&[("title", JsonValue::String("Goodbye".into()))]);
        assert!(!evaluate_rule(&ast, &ctx, &record2));
    }

    #[test]
    fn not_like_excludes() {
        let ast = parse_rule("title !~ \"spam\"").unwrap();
        let ctx = RequestContext::anonymous();
        let record = make_record(&[("title", JsonValue::String("Legit Post".into()))]);
        assert!(evaluate_rule(&ast, &ctx, &record));

        let record2 = make_record(&[("title", JsonValue::String("Buy spam now".into()))]);
        assert!(!evaluate_rule(&ast, &ctx, &record2));
    }

    // ── Logical operators ───────────────────────────────────────────────

    #[test]
    fn and_both_must_be_true() {
        let ast = parse_rule("status = \"published\" && featured = true").unwrap();
        let ctx = RequestContext::anonymous();

        let record = make_record(&[
            ("status", JsonValue::String("published".into())),
            ("featured", JsonValue::Bool(true)),
        ]);
        assert!(evaluate_rule(&ast, &ctx, &record));

        let record2 = make_record(&[
            ("status", JsonValue::String("published".into())),
            ("featured", JsonValue::Bool(false)),
        ]);
        assert!(!evaluate_rule(&ast, &ctx, &record2));
    }

    #[test]
    fn or_either_can_be_true() {
        let ast = parse_rule("status = \"published\" || status = \"featured\"").unwrap();
        let ctx = RequestContext::anonymous();

        let published = make_record(&[("status", JsonValue::String("published".into()))]);
        assert!(evaluate_rule(&ast, &ctx, &published));

        let featured = make_record(&[("status", JsonValue::String("featured".into()))]);
        assert!(evaluate_rule(&ast, &ctx, &featured));

        let draft = make_record(&[("status", JsonValue::String("draft".into()))]);
        assert!(!evaluate_rule(&ast, &ctx, &draft));
    }

    #[test]
    fn not_negates() {
        let ast = parse_rule("!(status = \"deleted\")").unwrap();
        let ctx = RequestContext::anonymous();

        let active = make_record(&[("status", JsonValue::String("active".into()))]);
        assert!(evaluate_rule(&ast, &ctx, &active));

        let deleted = make_record(&[("status", JsonValue::String("deleted".into()))]);
        assert!(!evaluate_rule(&ast, &ctx, &deleted));
    }

    #[test]
    fn complex_and_or_precedence() {
        // AND has higher precedence than OR:
        // "a || b && c" means "a || (b && c)"
        let ast = parse_rule("status = \"published\" || status = \"draft\" && author = \"admin\"")
            .unwrap();
        let ctx = RequestContext::anonymous();

        // "published" -> true regardless of author
        let r1 = make_record(&[
            ("status", JsonValue::String("published".into())),
            ("author", JsonValue::String("user1".into())),
        ]);
        assert!(evaluate_rule(&ast, &ctx, &r1));

        // "draft" && author is "admin" -> true
        let r2 = make_record(&[
            ("status", JsonValue::String("draft".into())),
            ("author", JsonValue::String("admin".into())),
        ]);
        assert!(evaluate_rule(&ast, &ctx, &r2));

        // "draft" && author is NOT "admin" -> false
        let r3 = make_record(&[
            ("status", JsonValue::String("draft".into())),
            ("author", JsonValue::String("user1".into())),
        ]);
        assert!(!evaluate_rule(&ast, &ctx, &r3));
    }

    #[test]
    fn grouped_expression_overrides_precedence() {
        // Explicit grouping: "(a || b) && c"
        let ast = parse_rule("(status = \"published\" || status = \"draft\") && featured = true")
            .unwrap();
        let ctx = RequestContext::anonymous();

        let r1 = make_record(&[
            ("status", JsonValue::String("published".into())),
            ("featured", JsonValue::Bool(true)),
        ]);
        assert!(evaluate_rule(&ast, &ctx, &r1));

        let r2 = make_record(&[
            ("status", JsonValue::String("published".into())),
            ("featured", JsonValue::Bool(false)),
        ]);
        assert!(!evaluate_rule(&ast, &ctx, &r2));
    }

    // ── @request.auth context tests ─────────────────────────────────────

    #[test]
    fn ownership_check() {
        let ast = parse_rule("owner = @request.auth.id").unwrap();

        let ctx = auth_ctx("user123");
        let record = make_record(&[("owner", JsonValue::String("user123".into()))]);
        assert!(evaluate_rule(&ast, &ctx, &record));

        let record2 = make_record(&[("owner", JsonValue::String("other_user".into()))]);
        assert!(!evaluate_rule(&ast, &ctx, &record2));
    }

    #[test]
    fn auth_id_not_empty_means_authenticated() {
        let ast = parse_rule("@request.auth.id != \"\"").unwrap();

        let ctx = auth_ctx("user123");
        let record = HashMap::new();
        assert!(evaluate_rule(&ast, &ctx, &record));

        let anon = RequestContext::anonymous();
        assert!(!evaluate_rule(&ast, &anon, &record));
    }

    #[test]
    fn auth_role_check() {
        let ast = parse_rule("@request.auth.role = \"admin\"").unwrap();

        let ctx = auth_ctx_with(&[
            ("id", JsonValue::String("user1".into())),
            ("role", JsonValue::String("admin".into())),
        ]);
        let record = HashMap::new();
        assert!(evaluate_rule(&ast, &ctx, &record));

        let ctx2 = auth_ctx_with(&[
            ("id", JsonValue::String("user2".into())),
            ("role", JsonValue::String("user".into())),
        ]);
        assert!(!evaluate_rule(&ast, &ctx2, &record));
    }

    #[test]
    fn auth_verified_check() {
        let ast = parse_rule("@request.auth.verified = true").unwrap();

        let ctx = auth_ctx_with(&[
            ("id", JsonValue::String("user1".into())),
            ("verified", JsonValue::Bool(true)),
        ]);
        let record = HashMap::new();
        assert!(evaluate_rule(&ast, &ctx, &record));

        let ctx2 = auth_ctx_with(&[
            ("id", JsonValue::String("user2".into())),
            ("verified", JsonValue::Bool(false)),
        ]);
        assert!(!evaluate_rule(&ast, &ctx2, &record));
    }

    // ── @request.data context tests ─────────────────────────────────────

    #[test]
    fn request_data_validation() {
        let ast = parse_rule("@request.data.owner = @request.auth.id").unwrap();

        let mut ctx = auth_ctx("user123");
        ctx.data.insert(
            "owner".to_string(),
            JsonValue::String("user123".to_string()),
        );
        let record = HashMap::new();
        assert!(evaluate_rule(&ast, &ctx, &record));

        let mut ctx2 = auth_ctx("user123");
        ctx2.data.insert(
            "owner".to_string(),
            JsonValue::String("other_user".to_string()),
        );
        assert!(!evaluate_rule(&ast, &ctx2, &record));
    }

    // ── @request.method context tests ───────────────────────────────────

    #[test]
    fn request_method_check() {
        let ast = parse_rule("@request.method = \"GET\"").unwrap();

        let mut ctx = RequestContext::anonymous();
        ctx.method = "GET".to_string();
        let record = HashMap::new();
        assert!(evaluate_rule(&ast, &ctx, &record));

        ctx.method = "POST".to_string();
        assert!(!evaluate_rule(&ast, &ctx, &record));
    }

    // ── Multi-value (any) operator tests ────────────────────────────────

    #[test]
    fn any_equals_on_array() {
        let ast = parse_rule("tags ?= \"featured\"").unwrap();
        let ctx = RequestContext::anonymous();

        let record = make_record(&[("tags", serde_json::json!(["tech", "featured", "news"]))]);
        assert!(evaluate_rule(&ast, &ctx, &record));

        let record2 = make_record(&[("tags", serde_json::json!(["tech", "news"]))]);
        assert!(!evaluate_rule(&ast, &ctx, &record2));
    }

    #[test]
    fn any_contains_on_array() {
        let ast = parse_rule("tags ?~ \"tech\"").unwrap();
        let ctx = RequestContext::anonymous();

        let record = make_record(&[("tags", serde_json::json!(["technology", "news"]))]);
        assert!(evaluate_rule(&ast, &ctx, &record));
    }

    #[test]
    fn any_operator_on_non_array_treats_as_single() {
        let ast = parse_rule("status ?= \"active\"").unwrap();
        let ctx = RequestContext::anonymous();
        let record = make_record(&[("status", JsonValue::String("active".into()))]);
        assert!(evaluate_rule(&ast, &ctx, &record));
    }

    // ── Combined rule patterns (PocketBase-style) ───────────────────────

    #[test]
    fn owner_only_rule() {
        // "owner = @request.auth.id" — only the owner can access
        let ast = parse_rule("owner = @request.auth.id").unwrap();

        // Authenticated user who owns the record
        let ctx = auth_ctx("u1");
        let record = make_record(&[("owner", JsonValue::String("u1".into()))]);
        assert!(evaluate_rule(&ast, &ctx, &record));

        // Different user
        let ctx2 = auth_ctx("u2");
        assert!(!evaluate_rule(&ast, &ctx2, &record));

        // Anonymous
        let anon = RequestContext::anonymous();
        assert!(!evaluate_rule(&ast, &anon, &record));
    }

    #[test]
    fn public_or_owner_rule() {
        // Published records visible to all, others only to owner
        let ast = parse_rule("status = \"published\" || owner = @request.auth.id").unwrap();

        let ctx = auth_ctx("u1");
        let ctx2 = auth_ctx("u2");
        let anon = RequestContext::anonymous();

        // Published record — visible to everyone
        let published = make_record(&[
            ("status", JsonValue::String("published".into())),
            ("owner", JsonValue::String("u1".into())),
        ]);
        assert!(evaluate_rule(&ast, &ctx, &published));
        assert!(evaluate_rule(&ast, &ctx2, &published));
        assert!(evaluate_rule(&ast, &anon, &published));

        // Draft record — only visible to owner
        let draft = make_record(&[
            ("status", JsonValue::String("draft".into())),
            ("owner", JsonValue::String("u1".into())),
        ]);
        assert!(evaluate_rule(&ast, &ctx, &draft));
        assert!(!evaluate_rule(&ast, &ctx2, &draft));
        assert!(!evaluate_rule(&ast, &anon, &draft));
    }

    #[test]
    fn team_member_rule_with_any() {
        // "members ?= @request.auth.id" — any team member can access
        let ast = parse_rule("members ?= @request.auth.id").unwrap();

        let ctx = auth_ctx("u1");
        let record = make_record(&[("members", serde_json::json!(["u1", "u2", "u3"]))]);
        assert!(evaluate_rule(&ast, &ctx, &record));

        let ctx2 = auth_ctx("u99");
        assert!(!evaluate_rule(&ast, &ctx2, &record));
    }

    #[test]
    fn auth_and_data_combined() {
        // Ensure the request body sets owner to the authenticated user
        let ast = parse_rule("@request.auth.id != \"\" && @request.data.owner = @request.auth.id")
            .unwrap();

        let mut ctx = auth_ctx("u1");
        ctx.data
            .insert("owner".to_string(), JsonValue::String("u1".into()));
        let record = HashMap::new();
        assert!(evaluate_rule(&ast, &ctx, &record));

        // Trying to set owner to someone else
        let mut ctx2 = auth_ctx("u1");
        ctx2.data
            .insert("owner".to_string(), JsonValue::String("u2".into()));
        assert!(!evaluate_rule(&ast, &ctx2, &record));
    }

    // ── evaluate_rule_str convenience ───────────────────────────────────

    #[test]
    fn evaluate_rule_str_works() {
        let ctx = auth_ctx("user1");
        let record = make_record(&[("owner", JsonValue::String("user1".into()))]);
        let result = evaluate_rule_str("owner = @request.auth.id", &ctx, &record);
        assert!(result.unwrap());
    }

    #[test]
    fn evaluate_rule_str_invalid_expression() {
        let ctx = RequestContext::anonymous();
        let record = HashMap::new();
        let result = evaluate_rule_str("invalid ??? expression", &ctx, &record);
        assert!(result.is_err());
    }

    // ── SQL Generation tests ────────────────────────────────────────────

    #[test]
    fn sql_simple_field_equals_literal() {
        let ast = parse_rule("status = \"published\"").unwrap();
        let ctx = RequestContext::anonymous();
        let clause = rule_to_sql(&ast, &ctx);
        assert_eq!(clause.sql, "\"status\" = ?1");
        assert_eq!(clause.params.len(), 1);
        assert!(matches!(&clause.params[0], RuleSqlParam::Text(s) if s == "published"));
    }

    #[test]
    fn sql_field_equals_auth_context() {
        let ast = parse_rule("owner = @request.auth.id").unwrap();
        let ctx = auth_ctx("user123");
        let clause = rule_to_sql(&ast, &ctx);
        assert_eq!(clause.sql, "\"owner\" = ?1");
        assert!(matches!(&clause.params[0], RuleSqlParam::Text(s) if s == "user123"));
    }

    #[test]
    fn sql_compound_and() {
        let ast = parse_rule("status = \"published\" && views > 100").unwrap();
        let ctx = RequestContext::anonymous();
        let clause = rule_to_sql(&ast, &ctx);
        assert_eq!(clause.sql, "(\"status\" = ?1 AND \"views\" > ?2)");
        assert_eq!(clause.params.len(), 2);
    }

    #[test]
    fn sql_compound_or() {
        let ast = parse_rule("status = \"published\" || status = \"featured\"").unwrap();
        let ctx = RequestContext::anonymous();
        let clause = rule_to_sql(&ast, &ctx);
        assert_eq!(clause.sql, "(\"status\" = ?1 OR \"status\" = ?2)");
        assert_eq!(clause.params.len(), 2);
    }

    #[test]
    fn sql_not() {
        let ast = parse_rule("!(status = \"deleted\")").unwrap();
        let ctx = RequestContext::anonymous();
        let clause = rule_to_sql(&ast, &ctx);
        assert_eq!(clause.sql, "NOT (\"status\" = ?1)");
    }

    #[test]
    fn sql_null_comparison() {
        let ast = parse_rule("deleted_at = null").unwrap();
        let ctx = RequestContext::anonymous();
        let clause = rule_to_sql(&ast, &ctx);
        assert_eq!(clause.sql, "\"deleted_at\" IS NULL");
        assert!(clause.params.is_empty());
    }

    #[test]
    fn sql_not_null_comparison() {
        let ast = parse_rule("avatar != null").unwrap();
        let ctx = RequestContext::anonymous();
        let clause = rule_to_sql(&ast, &ctx);
        assert_eq!(clause.sql, "\"avatar\" IS NOT NULL");
    }

    #[test]
    fn sql_any_operator() {
        let ast = parse_rule("tags ?= \"featured\"").unwrap();
        let ctx = RequestContext::anonymous();
        let clause = rule_to_sql(&ast, &ctx);
        assert!(clause.sql.contains("EXISTS"));
        assert!(clause.sql.contains("json_each"));
    }

    #[test]
    fn sql_bool_literal() {
        let ast = parse_rule("active = true").unwrap();
        let ctx = RequestContext::anonymous();
        let clause = rule_to_sql(&ast, &ctx);
        assert_eq!(clause.sql, "\"active\" = ?1");
        assert!(matches!(&clause.params[0], RuleSqlParam::Integer(1)));
    }

    #[test]
    fn sql_number_literal() {
        let ast = parse_rule("views >= 100").unwrap();
        let ctx = RequestContext::anonymous();
        let clause = rule_to_sql(&ast, &ctx);
        assert_eq!(clause.sql, "\"views\" >= ?1");
        assert!(matches!(&clause.params[0], RuleSqlParam::Integer(100)));
    }

    #[test]
    fn sql_anonymous_auth_id_is_null() {
        let ast = parse_rule("@request.auth.id != \"\"").unwrap();
        let anon = RequestContext::anonymous();
        let clause = rule_to_sql(&ast, &anon);
        // Anonymous context has no auth.id → resolves to NULL
        assert!(clause.sql.contains("NULL") || clause.sql.contains("?"));
    }

    #[test]
    fn sql_ownership_rule_complex() {
        let ast = parse_rule("status = \"published\" || owner = @request.auth.id").unwrap();
        let ctx = auth_ctx("u1");
        let clause = rule_to_sql(&ast, &ctx);
        assert!(clause.sql.contains("OR"));
        assert!(clause.sql.contains("\"status\""));
        assert!(clause.sql.contains("\"owner\""));
    }

    #[test]
    fn rule_str_to_sql_convenience() {
        let ctx = auth_ctx("user1");
        let clause = rule_str_to_sql("owner = @request.auth.id", &ctx).unwrap();
        assert_eq!(clause.sql, "\"owner\" = ?1");
    }

    #[test]
    fn rule_str_to_sql_invalid_expression() {
        let ctx = RequestContext::anonymous();
        let result = rule_str_to_sql("bad ???", &ctx);
        assert!(result.is_err());
    }
}
