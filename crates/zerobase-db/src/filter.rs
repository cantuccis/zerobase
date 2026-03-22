//! PocketBase-compatible filter parser and SQL generator.
//!
//! Parses filter strings like `status = "published" && views > 100` into an AST,
//! then generates parameterized SQL WHERE clauses. All user-supplied values are
//! bound as parameters — never interpolated — to prevent SQL injection.
//!
//! # Supported Syntax
//!
//! ## Comparison operators
//! - `=`  — equals
//! - `!=` — not equals
//! - `>`  — greater than
//! - `>=` — greater than or equal
//! - `<`  — less than
//! - `<=` — less than or equal
//! - `~`  — contains (LIKE %value%)
//! - `!~` — does not contain (NOT LIKE %value%)
//!
//! ## Multi-value operators (for JSON array / multi-select / relation fields)
//! - `?=`  — any element equals
//! - `?!=` — any element not equals
//! - `?>`  — any element greater than
//! - `?>=` — any element greater or equal
//! - `?<`  — any element less than
//! - `?<=` — any element less or equal
//! - `?~`  — any element contains
//! - `?!~` — any element does not contain
//!
//! ## Logical operators
//! - `&&` — AND
//! - `||` — OR
//!
//! ## Grouping
//! - `(` ... `)` — parentheses
//!
//! ## Values
//! - Strings: `"hello"` or `'hello'`
//! - Numbers: `42`, `3.14`, `-1`
//! - Booleans: `true`, `false`
//! - Null: `null`
//! - Date macros: `@now`, `@today`, `@month`, `@year`
//!
//! ## Identifiers
//! - Field names: `title`, `author.name` (dot-notation for relations)

use rusqlite::types::Value as SqlValue;

use crate::query_builder::BuiltQuery;

// ── Error Type ──────────────────────────────────────────────────────────────

/// Errors produced by the filter parser.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FilterError {
    #[error("unexpected character '{ch}' at position {pos}")]
    UnexpectedChar { ch: char, pos: usize },

    #[error("unterminated string starting at position {pos}")]
    UnterminatedString { pos: usize },

    #[error("unexpected token: expected {expected}, got {got}")]
    UnexpectedToken { expected: String, got: String },

    #[error("unexpected end of filter expression")]
    UnexpectedEnd,

    #[error("empty filter expression")]
    Empty,

    #[error("invalid number: {value}")]
    InvalidNumber { value: String },
}

// ── Tokens ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Token {
    // Identifiers & literals
    Ident(String),
    StringLit(String),
    NumberLit(f64),
    True,
    False,
    Null,

    // Date macros
    AtNow,
    AtToday,
    AtMonth,
    AtYear,

    // Comparison operators
    Eq,      // =
    Neq,     // !=
    Gt,      // >
    Gte,     // >=
    Lt,      // <
    Lte,     // <=
    Like,    // ~
    NotLike, // !~

    // Multi-value operators
    AnyEq,      // ?=
    AnyNeq,     // ?!=
    AnyGt,      // ?>
    AnyGte,     // ?>=
    AnyLt,      // ?<
    AnyLte,     // ?<=
    AnyLike,    // ?~
    AnyNotLike, // ?!~

    // Logical
    And, // &&
    Or,  // ||

    // Grouping
    LParen,
    RParen,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Ident(s) => write!(f, "identifier '{s}'"),
            Token::StringLit(s) => write!(f, "string \"{s}\""),
            Token::NumberLit(n) => write!(f, "number {n}"),
            Token::True => write!(f, "true"),
            Token::False => write!(f, "false"),
            Token::Null => write!(f, "null"),
            Token::AtNow => write!(f, "@now"),
            Token::AtToday => write!(f, "@today"),
            Token::AtMonth => write!(f, "@month"),
            Token::AtYear => write!(f, "@year"),
            Token::Eq => write!(f, "'='"),
            Token::Neq => write!(f, "'!='"),
            Token::Gt => write!(f, "'>'"),
            Token::Gte => write!(f, "'>='"),
            Token::Lt => write!(f, "'<'"),
            Token::Lte => write!(f, "'<='"),
            Token::Like => write!(f, "'~'"),
            Token::NotLike => write!(f, "'!~'"),
            Token::AnyEq => write!(f, "'?='"),
            Token::AnyNeq => write!(f, "'?!='"),
            Token::AnyGt => write!(f, "'?>'"),
            Token::AnyGte => write!(f, "'?>='"),
            Token::AnyLt => write!(f, "'?<'"),
            Token::AnyLte => write!(f, "'?<='"),
            Token::AnyLike => write!(f, "'?~'"),
            Token::AnyNotLike => write!(f, "'?!~'"),
            Token::And => write!(f, "'&&'"),
            Token::Or => write!(f, "'||'"),
            Token::LParen => write!(f, "'('"),
            Token::RParen => write!(f, "')'"),
        }
    }
}

// ── Tokenizer ───────────────────────────────────────────────────────────────

/// Tokenize a PocketBase filter expression.
pub(crate) fn tokenize(input: &str) -> Result<Vec<Token>, FilterError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];

        // Skip whitespace
        if ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // String literals (single or double quoted)
        if ch == '"' || ch == '\'' {
            let quote = ch;
            let start = i;
            i += 1;
            let mut value = String::new();
            while i < len && chars[i] != quote {
                if chars[i] == '\\' && i + 1 < len {
                    // Escape sequence
                    i += 1;
                    match chars[i] {
                        'n' => value.push('\n'),
                        't' => value.push('\t'),
                        '\\' => value.push('\\'),
                        c if c == quote => value.push(c),
                        c => {
                            value.push('\\');
                            value.push(c);
                        }
                    }
                } else {
                    value.push(chars[i]);
                }
                i += 1;
            }
            if i >= len {
                return Err(FilterError::UnterminatedString { pos: start });
            }
            i += 1; // skip closing quote
            tokens.push(Token::StringLit(value));
            continue;
        }

        // Numbers (including negative)
        if ch.is_ascii_digit() || (ch == '-' && i + 1 < len && chars[i + 1].is_ascii_digit()) {
            let start = i;
            if ch == '-' {
                i += 1;
            }
            while i < len && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < len && chars[i] == '.' && i + 1 < len && chars[i + 1].is_ascii_digit() {
                i += 1;
                while i < len && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let num_str: String = chars[start..i].iter().collect();
            let num = num_str
                .parse::<f64>()
                .map_err(|_| FilterError::InvalidNumber {
                    value: num_str.clone(),
                })?;
            tokens.push(Token::NumberLit(num));
            continue;
        }

        // @ macros
        if ch == '@' {
            i += 1;
            let start = i;
            while i < len && chars[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let macro_name: String = chars[start..i].iter().collect();
            match macro_name.as_str() {
                "now" => tokens.push(Token::AtNow),
                "today" => tokens.push(Token::AtToday),
                "month" => tokens.push(Token::AtMonth),
                "year" => tokens.push(Token::AtYear),
                _ => {
                    return Err(FilterError::UnexpectedChar {
                        ch: '@',
                        pos: start - 1,
                    })
                }
            }
            continue;
        }

        // Multi-character operators starting with ?
        if ch == '?' {
            i += 1;
            if i < len {
                match chars[i] {
                    '!' => {
                        i += 1;
                        if i < len && chars[i] == '=' {
                            i += 1;
                            tokens.push(Token::AnyNeq);
                        } else if i < len && chars[i] == '~' {
                            i += 1;
                            tokens.push(Token::AnyNotLike);
                        } else {
                            return Err(FilterError::UnexpectedChar {
                                ch: if i < len { chars[i] } else { '?' },
                                pos: i,
                            });
                        }
                    }
                    '=' => {
                        i += 1;
                        tokens.push(Token::AnyEq);
                    }
                    '>' => {
                        i += 1;
                        if i < len && chars[i] == '=' {
                            i += 1;
                            tokens.push(Token::AnyGte);
                        } else {
                            tokens.push(Token::AnyGt);
                        }
                    }
                    '<' => {
                        i += 1;
                        if i < len && chars[i] == '=' {
                            i += 1;
                            tokens.push(Token::AnyLte);
                        } else {
                            tokens.push(Token::AnyLt);
                        }
                    }
                    '~' => {
                        i += 1;
                        tokens.push(Token::AnyLike);
                    }
                    _ => {
                        return Err(FilterError::UnexpectedChar {
                            ch: chars[i],
                            pos: i,
                        });
                    }
                }
            } else {
                return Err(FilterError::UnexpectedEnd);
            }
            continue;
        }

        // Operators starting with !
        if ch == '!' {
            i += 1;
            if i < len && chars[i] == '=' {
                i += 1;
                tokens.push(Token::Neq);
            } else if i < len && chars[i] == '~' {
                i += 1;
                tokens.push(Token::NotLike);
            } else {
                return Err(FilterError::UnexpectedChar {
                    ch: if i < len { chars[i] } else { '!' },
                    pos: i.saturating_sub(1),
                });
            }
            continue;
        }

        // Operators starting with >
        if ch == '>' {
            i += 1;
            if i < len && chars[i] == '=' {
                i += 1;
                tokens.push(Token::Gte);
            } else {
                tokens.push(Token::Gt);
            }
            continue;
        }

        // Operators starting with <
        if ch == '<' {
            i += 1;
            if i < len && chars[i] == '=' {
                i += 1;
                tokens.push(Token::Lte);
            } else {
                tokens.push(Token::Lt);
            }
            continue;
        }

        // Single-char operators
        if ch == '=' {
            i += 1;
            tokens.push(Token::Eq);
            continue;
        }

        if ch == '~' {
            i += 1;
            tokens.push(Token::Like);
            continue;
        }

        // Logical operators
        if ch == '&' {
            i += 1;
            if i < len && chars[i] == '&' {
                i += 1;
                tokens.push(Token::And);
            } else {
                return Err(FilterError::UnexpectedChar {
                    ch: '&',
                    pos: i.saturating_sub(1),
                });
            }
            continue;
        }

        if ch == '|' {
            i += 1;
            if i < len && chars[i] == '|' {
                i += 1;
                tokens.push(Token::Or);
            } else {
                return Err(FilterError::UnexpectedChar {
                    ch: '|',
                    pos: i.saturating_sub(1),
                });
            }
            continue;
        }

        // Parentheses
        if ch == '(' {
            i += 1;
            tokens.push(Token::LParen);
            continue;
        }
        if ch == ')' {
            i += 1;
            tokens.push(Token::RParen);
            continue;
        }

        // Identifiers and keywords
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            while i < len
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match word.as_str() {
                "true" => tokens.push(Token::True),
                "false" => tokens.push(Token::False),
                "null" => tokens.push(Token::Null),
                _ => tokens.push(Token::Ident(word)),
            }
            continue;
        }

        return Err(FilterError::UnexpectedChar { ch, pos: i });
    }

    Ok(tokens)
}

// ── AST ─────────────────────────────────────────────────────────────────────

/// A node in the filter expression AST.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterExpr {
    /// A comparison between a field and a value.
    Condition {
        field: String,
        operator: ComparisonOp,
        value: FilterValue,
    },
    /// Logical AND of two expressions.
    And(Box<FilterExpr>, Box<FilterExpr>),
    /// Logical OR of two expressions.
    Or(Box<FilterExpr>, Box<FilterExpr>),
    /// Parenthesized (grouped) expression.
    Group(Box<FilterExpr>),
}

/// Comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonOp {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    Like,
    NotLike,
    AnyEq,
    AnyNeq,
    AnyGt,
    AnyGte,
    AnyLt,
    AnyLte,
    AnyLike,
    AnyNotLike,
}

/// A value in a filter expression.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterValue {
    String(String),
    Number(f64),
    Bool(bool),
    Null,
    /// @now — current UTC datetime
    Now,
    /// @today — start of today UTC
    Today,
    /// @month — start of current month UTC
    Month,
    /// @year — start of current year UTC
    Year,
}

// ── Parser ──────────────────────────────────────────────────────────────────

/// Recursive descent parser for filter expressions.
///
/// Grammar (precedence low to high):
///   expr     = or_expr
///   or_expr  = and_expr ("||" and_expr)*
///   and_expr = primary ("&&" primary)*
///   primary  = "(" expr ")" | condition
///   condition = IDENT operator value
///   operator  = "=" | "!=" | ">" | ">=" | "<" | "<=" | "~" | "!~"
///             | "?=" | "?!=" | "?>" | "?>=" | "?<" | "?<=" | "?~" | "?!~"
///   value    = STRING | NUMBER | "true" | "false" | "null" | "@now" | "@today" | "@month" | "@year"
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        if self.pos < self.tokens.len() {
            let token = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(token)
        } else {
            None
        }
    }

    fn parse(mut self) -> Result<FilterExpr, FilterError> {
        if self.tokens.is_empty() {
            return Err(FilterError::Empty);
        }
        let expr = self.parse_or()?;
        if self.pos < self.tokens.len() {
            return Err(FilterError::UnexpectedToken {
                expected: "end of expression".to_string(),
                got: format!("{}", self.tokens[self.pos]),
            });
        }
        Ok(expr)
    }

    fn parse_or(&mut self) -> Result<FilterExpr, FilterError> {
        let mut left = self.parse_and()?;
        while self.peek() == Some(&Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = FilterExpr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<FilterExpr, FilterError> {
        let mut left = self.parse_primary()?;
        while self.peek() == Some(&Token::And) {
            self.advance();
            let right = self.parse_primary()?;
            left = FilterExpr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<FilterExpr, FilterError> {
        match self.peek() {
            Some(Token::LParen) => {
                self.advance(); // consume '('
                let expr = self.parse_or()?;
                match self.advance() {
                    Some(Token::RParen) => Ok(FilterExpr::Group(Box::new(expr))),
                    Some(tok) => Err(FilterError::UnexpectedToken {
                        expected: "')'".to_string(),
                        got: format!("{tok}"),
                    }),
                    None => Err(FilterError::UnexpectedEnd),
                }
            }
            Some(Token::Ident(_)) => self.parse_condition(),
            Some(tok) => Err(FilterError::UnexpectedToken {
                expected: "field name or '('".to_string(),
                got: format!("{tok}"),
            }),
            None => Err(FilterError::UnexpectedEnd),
        }
    }

    fn parse_condition(&mut self) -> Result<FilterExpr, FilterError> {
        // Field name
        let field = match self.advance() {
            Some(Token::Ident(name)) => name,
            Some(tok) => {
                return Err(FilterError::UnexpectedToken {
                    expected: "field name".to_string(),
                    got: format!("{tok}"),
                })
            }
            None => return Err(FilterError::UnexpectedEnd),
        };

        // Operator
        let operator = match self.advance() {
            Some(Token::Eq) => ComparisonOp::Eq,
            Some(Token::Neq) => ComparisonOp::Neq,
            Some(Token::Gt) => ComparisonOp::Gt,
            Some(Token::Gte) => ComparisonOp::Gte,
            Some(Token::Lt) => ComparisonOp::Lt,
            Some(Token::Lte) => ComparisonOp::Lte,
            Some(Token::Like) => ComparisonOp::Like,
            Some(Token::NotLike) => ComparisonOp::NotLike,
            Some(Token::AnyEq) => ComparisonOp::AnyEq,
            Some(Token::AnyNeq) => ComparisonOp::AnyNeq,
            Some(Token::AnyGt) => ComparisonOp::AnyGt,
            Some(Token::AnyGte) => ComparisonOp::AnyGte,
            Some(Token::AnyLt) => ComparisonOp::AnyLt,
            Some(Token::AnyLte) => ComparisonOp::AnyLte,
            Some(Token::AnyLike) => ComparisonOp::AnyLike,
            Some(Token::AnyNotLike) => ComparisonOp::AnyNotLike,
            Some(tok) => {
                return Err(FilterError::UnexpectedToken {
                    expected: "comparison operator".to_string(),
                    got: format!("{tok}"),
                })
            }
            None => return Err(FilterError::UnexpectedEnd),
        };

        // Value
        let value = match self.advance() {
            Some(Token::StringLit(s)) => FilterValue::String(s),
            Some(Token::NumberLit(n)) => FilterValue::Number(n),
            Some(Token::True) => FilterValue::Bool(true),
            Some(Token::False) => FilterValue::Bool(false),
            Some(Token::Null) => FilterValue::Null,
            Some(Token::AtNow) => FilterValue::Now,
            Some(Token::AtToday) => FilterValue::Today,
            Some(Token::AtMonth) => FilterValue::Month,
            Some(Token::AtYear) => FilterValue::Year,
            Some(tok) => {
                return Err(FilterError::UnexpectedToken {
                    expected: "value (string, number, boolean, null, or date macro)".to_string(),
                    got: format!("{tok}"),
                })
            }
            None => return Err(FilterError::UnexpectedEnd),
        };

        Ok(FilterExpr::Condition {
            field,
            operator,
            value,
        })
    }
}

// ── Public parse function ───────────────────────────────────────────────────

/// Parse a PocketBase-style filter expression into an AST.
///
/// Returns `FilterError` if the expression is malformed.
pub fn parse_filter(input: &str) -> Result<FilterExpr, FilterError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(FilterError::Empty);
    }
    let tokens = tokenize(trimmed)?;
    Parser::new(tokens).parse()
}

// ── SQL Generation ──────────────────────────────────────────────────────────

/// Generate a parameterized SQL WHERE clause from a filter AST.
///
/// Returns a `BuiltQuery` containing just the WHERE condition (without the
/// `WHERE` keyword) and the bound parameters.
///
/// # Field name validation
///
/// Field names are validated to contain only alphanumeric characters,
/// underscores, and dots (for relation traversal). They are double-quoted
/// in the generated SQL as defense in depth.
pub fn generate_sql(expr: &FilterExpr) -> Result<BuiltQuery, FilterError> {
    let mut params = Vec::new();
    let mut param_idx = 1usize;
    let sql = emit_sql(expr, &mut params, &mut param_idx)?;
    Ok(BuiltQuery { sql, params })
}

/// Recursively emit SQL for a filter expression.
fn emit_sql(
    expr: &FilterExpr,
    params: &mut Vec<SqlValue>,
    idx: &mut usize,
) -> Result<String, FilterError> {
    match expr {
        FilterExpr::Condition {
            field,
            operator,
            value,
        } => emit_condition(field, *operator, value, params, idx),

        FilterExpr::And(left, right) => {
            let left_sql = emit_sql(left, params, idx)?;
            let right_sql = emit_sql(right, params, idx)?;
            Ok(format!("({left_sql} AND {right_sql})"))
        }

        FilterExpr::Or(left, right) => {
            let left_sql = emit_sql(left, params, idx)?;
            let right_sql = emit_sql(right, params, idx)?;
            Ok(format!("({left_sql} OR {right_sql})"))
        }

        FilterExpr::Group(inner) => emit_sql(inner, params, idx),
    }
}

/// Emit SQL for a single comparison condition.
fn emit_condition(
    field: &str,
    op: ComparisonOp,
    value: &FilterValue,
    params: &mut Vec<SqlValue>,
    idx: &mut usize,
) -> Result<String, FilterError> {
    let quoted_field = quote_field_name(field);

    // Handle null comparisons specially (IS NULL / IS NOT NULL).
    if matches!(value, FilterValue::Null) {
        return match op {
            ComparisonOp::Eq => Ok(format!("{quoted_field} IS NULL")),
            ComparisonOp::Neq => Ok(format!("{quoted_field} IS NOT NULL")),
            _ => {
                // For other operators with null, still use parameterized comparison
                let param_marker = format!("?{}", *idx);
                *idx += 1;
                params.push(SqlValue::Null);
                let sql_op = standard_sql_op(op);
                Ok(format!("{quoted_field} {sql_op} {param_marker}"))
            }
        };
    }

    // Handle multi-value (any) operators — these query JSON arrays stored as TEXT.
    if is_any_operator(op) {
        return emit_any_condition(&quoted_field, field, op, value, params, idx);
    }

    // Standard comparison operators.
    let param_marker = format!("?{}", *idx);
    *idx += 1;

    match op {
        ComparisonOp::Like => {
            // LIKE with % wrapping for contains semantics.
            let sql_value = filter_value_to_sql(value);
            let like_value = match &sql_value {
                SqlValue::Text(s) => SqlValue::Text(format!("%{s}%")),
                other => other.clone(),
            };
            params.push(like_value);
            Ok(format!("{quoted_field} LIKE {param_marker}"))
        }
        ComparisonOp::NotLike => {
            let sql_value = filter_value_to_sql(value);
            let like_value = match &sql_value {
                SqlValue::Text(s) => SqlValue::Text(format!("%{s}%")),
                other => other.clone(),
            };
            params.push(like_value);
            Ok(format!("{quoted_field} NOT LIKE {param_marker}"))
        }
        _ => {
            let sql_op = standard_sql_op(op);
            params.push(filter_value_to_sql(value));
            Ok(format!("{quoted_field} {sql_op} {param_marker}"))
        }
    }
}

/// Emit SQL for a multi-value (any) operator.
///
/// Multi-value fields are stored as JSON arrays in TEXT columns.
/// We use `json_each()` to unpack them and check if ANY element matches.
fn emit_any_condition(
    quoted_field: &str,
    _raw_field: &str,
    op: ComparisonOp,
    value: &FilterValue,
    params: &mut Vec<SqlValue>,
    idx: &mut usize,
) -> Result<String, FilterError> {
    let param_marker = format!("?{}", *idx);
    *idx += 1;

    let inner_op = match op {
        ComparisonOp::AnyEq => "=",
        ComparisonOp::AnyNeq => "!=",
        ComparisonOp::AnyGt => ">",
        ComparisonOp::AnyGte => ">=",
        ComparisonOp::AnyLt => "<",
        ComparisonOp::AnyLte => "<=",
        ComparisonOp::AnyLike => "LIKE",
        ComparisonOp::AnyNotLike => "NOT LIKE",
        _ => unreachable!(),
    };

    match op {
        ComparisonOp::AnyLike | ComparisonOp::AnyNotLike => {
            let sql_value = filter_value_to_sql(value);
            let like_value = match &sql_value {
                SqlValue::Text(s) => SqlValue::Text(format!("%{s}%")),
                other => other.clone(),
            };
            params.push(like_value);
        }
        _ => {
            params.push(filter_value_to_sql(value));
        }
    }

    Ok(format!(
        "EXISTS (SELECT 1 FROM json_each({quoted_field}) WHERE json_each.value {inner_op} {param_marker})"
    ))
}

/// Map a standard (non-any, non-like) comparison operator to its SQL string.
fn standard_sql_op(op: ComparisonOp) -> &'static str {
    match op {
        ComparisonOp::Eq | ComparisonOp::AnyEq => "=",
        ComparisonOp::Neq | ComparisonOp::AnyNeq => "!=",
        ComparisonOp::Gt | ComparisonOp::AnyGt => ">",
        ComparisonOp::Gte | ComparisonOp::AnyGte => ">=",
        ComparisonOp::Lt | ComparisonOp::AnyLt => "<",
        ComparisonOp::Lte | ComparisonOp::AnyLte => "<=",
        ComparisonOp::Like | ComparisonOp::AnyLike => "LIKE",
        ComparisonOp::NotLike | ComparisonOp::AnyNotLike => "NOT LIKE",
    }
}

/// Check if an operator is a multi-value (any) operator.
fn is_any_operator(op: ComparisonOp) -> bool {
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

/// Convert a `FilterValue` to a `SqlValue` for binding.
fn filter_value_to_sql(value: &FilterValue) -> SqlValue {
    match value {
        FilterValue::String(s) => SqlValue::Text(s.clone()),
        FilterValue::Number(n) => {
            // Use integer if the number has no fractional part.
            if n.fract() == 0.0 && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                SqlValue::Integer(*n as i64)
            } else {
                SqlValue::Real(*n)
            }
        }
        FilterValue::Bool(b) => SqlValue::Integer(if *b { 1 } else { 0 }),
        FilterValue::Null => SqlValue::Null,
        FilterValue::Now => {
            // Generate current UTC datetime in ISO 8601 format.
            SqlValue::Text(
                chrono::Utc::now()
                    .format("%Y-%m-%d %H:%M:%S%.3fZ")
                    .to_string(),
            )
        }
        FilterValue::Today => {
            let today = chrono::Utc::now().date_naive();
            SqlValue::Text(format!("{today} 00:00:00.000Z"))
        }
        FilterValue::Month => {
            let now = chrono::Utc::now().date_naive();
            let first_of_month = now.with_day(1).unwrap_or(now);
            SqlValue::Text(format!("{first_of_month} 00:00:00.000Z"))
        }
        FilterValue::Year => {
            let now = chrono::Utc::now().date_naive();
            let first_of_year = chrono::NaiveDate::from_ymd_opt(now.year(), 1, 1).unwrap_or(now);
            SqlValue::Text(format!("{first_of_year} 00:00:00.000Z"))
        }
    }
}

/// Quote a field name for safe inclusion in SQL.
///
/// Supports dot-notation (e.g. `author.name`). Each segment is validated
/// and double-quoted.
fn quote_field_name(field: &str) -> String {
    field
        .split('.')
        .map(|segment| format!("\"{}\"", segment.replace('"', "")))
        .collect::<Vec<_>>()
        .join(".")
}

// ── Convenience: parse + generate in one step ───────────────────────────────

/// Parse a PocketBase-style filter string and generate a parameterized
/// SQL WHERE clause.
///
/// Returns `None` for empty/blank input (caller should skip the WHERE clause).
/// Returns `Err` for malformed input.
pub fn parse_and_generate_sql(filter: &str) -> Result<Option<BuiltQuery>, FilterError> {
    let trimmed = filter.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let ast = parse_filter(trimmed)?;
    let query = generate_sql(&ast)?;
    Ok(Some(query))
}

// We need chrono for date macros
use chrono::Datelike;

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Tokenizer tests ─────────────────────────────────────────────────

    mod tokenizer {
        use super::*;

        #[test]
        fn simple_equality() {
            let tokens = tokenize("name = \"Alice\"").unwrap();
            assert_eq!(
                tokens,
                vec![
                    Token::Ident("name".into()),
                    Token::Eq,
                    Token::StringLit("Alice".into()),
                ]
            );
        }

        #[test]
        fn single_quoted_string() {
            let tokens = tokenize("name = 'Bob'").unwrap();
            assert_eq!(
                tokens,
                vec![
                    Token::Ident("name".into()),
                    Token::Eq,
                    Token::StringLit("Bob".into()),
                ]
            );
        }

        #[test]
        fn number_literal() {
            let tokens = tokenize("views > 100").unwrap();
            assert_eq!(
                tokens,
                vec![
                    Token::Ident("views".into()),
                    Token::Gt,
                    Token::NumberLit(100.0),
                ]
            );
        }

        #[test]
        fn negative_number() {
            let tokens = tokenize("balance >= -50").unwrap();
            assert_eq!(
                tokens,
                vec![
                    Token::Ident("balance".into()),
                    Token::Gte,
                    Token::NumberLit(-50.0),
                ]
            );
        }

        #[test]
        fn float_number() {
            let tokens = tokenize("price < 9.99").unwrap();
            assert_eq!(
                tokens,
                vec![
                    Token::Ident("price".into()),
                    Token::Lt,
                    Token::NumberLit(9.99),
                ]
            );
        }

        #[test]
        fn boolean_and_null() {
            let tokens = tokenize("active = true && deleted = null").unwrap();
            assert_eq!(
                tokens,
                vec![
                    Token::Ident("active".into()),
                    Token::Eq,
                    Token::True,
                    Token::And,
                    Token::Ident("deleted".into()),
                    Token::Eq,
                    Token::Null,
                ]
            );
        }

        #[test]
        fn all_comparison_operators() {
            let ops = vec![
                ("a = 1", Token::Eq),
                ("a != 1", Token::Neq),
                ("a > 1", Token::Gt),
                ("a >= 1", Token::Gte),
                ("a < 1", Token::Lt),
                ("a <= 1", Token::Lte),
                ("a ~ 'x'", Token::Like),
                ("a !~ 'x'", Token::NotLike),
            ];
            for (input, expected_op) in ops {
                let tokens = tokenize(input).unwrap();
                assert_eq!(tokens[1], expected_op, "failed for input: {input}");
            }
        }

        #[test]
        fn multi_value_operators() {
            let ops = vec![
                ("tags ?= 'rust'", Token::AnyEq),
                ("tags ?!= 'java'", Token::AnyNeq),
                ("scores ?> 90", Token::AnyGt),
                ("scores ?>= 90", Token::AnyGte),
                ("scores ?< 50", Token::AnyLt),
                ("scores ?<= 50", Token::AnyLte),
                ("tags ?~ 'ru'", Token::AnyLike),
                ("tags ?!~ 'ja'", Token::AnyNotLike),
            ];
            for (input, expected_op) in ops {
                let tokens = tokenize(input).unwrap();
                assert_eq!(tokens[1], expected_op, "failed for input: {input}");
            }
        }

        #[test]
        fn logical_operators_and_parens() {
            let tokens = tokenize("(a = 1 || b = 2) && c = 3").unwrap();
            assert_eq!(
                tokens,
                vec![
                    Token::LParen,
                    Token::Ident("a".into()),
                    Token::Eq,
                    Token::NumberLit(1.0),
                    Token::Or,
                    Token::Ident("b".into()),
                    Token::Eq,
                    Token::NumberLit(2.0),
                    Token::RParen,
                    Token::And,
                    Token::Ident("c".into()),
                    Token::Eq,
                    Token::NumberLit(3.0),
                ]
            );
        }

        #[test]
        fn date_macros() {
            let tokens = tokenize("created > @now && updated < @today").unwrap();
            assert!(tokens.contains(&Token::AtNow));
            assert!(tokens.contains(&Token::AtToday));
        }

        #[test]
        fn at_month_and_year() {
            let tokens = tokenize("created >= @month && created < @year").unwrap();
            assert!(tokens.contains(&Token::AtMonth));
            assert!(tokens.contains(&Token::AtYear));
        }

        #[test]
        fn dot_notation_field() {
            let tokens = tokenize("author.name = 'Alice'").unwrap();
            assert_eq!(tokens[0], Token::Ident("author.name".into()));
        }

        #[test]
        fn escaped_string() {
            let tokens = tokenize(r#"name = "say \"hello\"""#).unwrap();
            assert_eq!(tokens[2], Token::StringLit("say \"hello\"".into()));
        }

        #[test]
        fn unterminated_string_error() {
            let err = tokenize("name = \"unclosed").unwrap_err();
            assert!(matches!(err, FilterError::UnterminatedString { .. }));
        }

        #[test]
        fn unexpected_char_error() {
            let err = tokenize("name # 1").unwrap_err();
            assert!(matches!(err, FilterError::UnexpectedChar { .. }));
        }

        #[test]
        fn false_literal() {
            let tokens = tokenize("active = false").unwrap();
            assert_eq!(tokens[2], Token::False);
        }
    }

    // ── Parser tests ────────────────────────────────────────────────────

    mod parser {
        use super::*;

        #[test]
        fn simple_condition() {
            let ast = parse_filter("name = \"Alice\"").unwrap();
            assert!(matches!(
                ast,
                FilterExpr::Condition {
                    ref field,
                    operator: ComparisonOp::Eq,
                    value: FilterValue::String(ref s),
                } if field == "name" && s == "Alice"
            ));
        }

        #[test]
        fn and_expression() {
            let ast = parse_filter("a = 1 && b = 2").unwrap();
            assert!(matches!(ast, FilterExpr::And(_, _)));
        }

        #[test]
        fn or_expression() {
            let ast = parse_filter("a = 1 || b = 2").unwrap();
            assert!(matches!(ast, FilterExpr::Or(_, _)));
        }

        #[test]
        fn grouped_expression() {
            let ast = parse_filter("(a = 1 || b = 2) && c = 3").unwrap();
            match ast {
                FilterExpr::And(left, right) => {
                    assert!(matches!(*left, FilterExpr::Group(_)));
                    assert!(matches!(*right, FilterExpr::Condition { .. }));
                }
                _ => panic!("expected And"),
            }
        }

        #[test]
        fn nested_groups() {
            let ast = parse_filter("((a = 1))").unwrap();
            match ast {
                FilterExpr::Group(inner) => {
                    assert!(matches!(*inner, FilterExpr::Group(_)));
                }
                _ => panic!("expected Group"),
            }
        }

        #[test]
        fn precedence_and_binds_tighter_than_or() {
            // a = 1 || b = 2 && c = 3 should parse as a = 1 || (b = 2 && c = 3)
            let ast = parse_filter("a = 1 || b = 2 && c = 3").unwrap();
            match ast {
                FilterExpr::Or(left, right) => {
                    assert!(matches!(*left, FilterExpr::Condition { .. }));
                    assert!(matches!(*right, FilterExpr::And(_, _)));
                }
                _ => panic!("expected Or at top level"),
            }
        }

        #[test]
        fn null_comparison() {
            let ast = parse_filter("deleted = null").unwrap();
            assert!(matches!(
                ast,
                FilterExpr::Condition {
                    value: FilterValue::Null,
                    ..
                }
            ));
        }

        #[test]
        fn boolean_value() {
            let ast = parse_filter("active = true").unwrap();
            assert!(matches!(
                ast,
                FilterExpr::Condition {
                    value: FilterValue::Bool(true),
                    ..
                }
            ));
        }

        #[test]
        fn date_macro_value() {
            let ast = parse_filter("created > @now").unwrap();
            assert!(matches!(
                ast,
                FilterExpr::Condition {
                    value: FilterValue::Now,
                    ..
                }
            ));
        }

        #[test]
        fn any_operator() {
            let ast = parse_filter("tags ?= 'rust'").unwrap();
            assert!(matches!(
                ast,
                FilterExpr::Condition {
                    operator: ComparisonOp::AnyEq,
                    ..
                }
            ));
        }

        #[test]
        fn complex_expression() {
            // Multi-level nesting with different operators.
            let ast = parse_filter(
                "(status = 'published' || status = 'draft') && views >= 100 && author.name ~ 'John'",
            )
            .unwrap();
            // Should be: And(And(Group(Or(...)), Condition), Condition)
            match ast {
                FilterExpr::And(_, _) => {} // valid
                _ => panic!("expected And at top level"),
            }
        }

        #[test]
        fn empty_filter_error() {
            assert!(matches!(parse_filter(""), Err(FilterError::Empty)));
            assert!(matches!(parse_filter("   "), Err(FilterError::Empty)));
        }

        #[test]
        fn missing_value_error() {
            let err = parse_filter("name =").unwrap_err();
            assert!(matches!(err, FilterError::UnexpectedEnd));
        }

        #[test]
        fn missing_operator_error() {
            let err = parse_filter("name 'Alice'").unwrap_err();
            assert!(matches!(err, FilterError::UnexpectedToken { .. }));
        }

        #[test]
        fn unclosed_paren_error() {
            let err = parse_filter("(a = 1").unwrap_err();
            assert!(matches!(err, FilterError::UnexpectedEnd));
        }

        #[test]
        fn extra_tokens_error() {
            let err = parse_filter("a = 1 b = 2").unwrap_err();
            assert!(matches!(err, FilterError::UnexpectedToken { .. }));
        }
    }

    // ── SQL Generation tests ────────────────────────────────────────────

    mod sql_gen {
        use super::*;

        /// Helper: parse a filter and generate SQL, returning (sql, params).
        pub(super) fn gen(input: &str) -> (String, Vec<SqlValue>) {
            let ast = parse_filter(input).unwrap();
            let query = generate_sql(&ast).unwrap();
            (query.sql, query.params)
        }

        #[test]
        fn simple_equality() {
            let (sql, params) = gen("name = 'Alice'");
            assert_eq!(sql, "\"name\" = ?1");
            assert_eq!(params.len(), 1);
            assert!(matches!(&params[0], SqlValue::Text(s) if s == "Alice"));
        }

        #[test]
        fn not_equal() {
            let (sql, _) = gen("status != 'deleted'");
            assert_eq!(sql, "\"status\" != ?1");
        }

        #[test]
        fn greater_than() {
            let (sql, params) = gen("views > 100");
            assert_eq!(sql, "\"views\" > ?1");
            assert!(matches!(&params[0], SqlValue::Integer(100)));
        }

        #[test]
        fn greater_equal() {
            let (sql, _) = gen("views >= 50");
            assert_eq!(sql, "\"views\" >= ?1");
        }

        #[test]
        fn less_than() {
            let (sql, _) = gen("price < 10");
            assert_eq!(sql, "\"price\" < ?1");
        }

        #[test]
        fn less_equal() {
            let (sql, _) = gen("price <= 9.99");
            assert_eq!(sql, "\"price\" <= ?1");
        }

        #[test]
        fn like_wraps_with_percent() {
            let (sql, params) = gen("title ~ 'hello'");
            assert_eq!(sql, "\"title\" LIKE ?1");
            assert!(matches!(&params[0], SqlValue::Text(s) if s == "%hello%"));
        }

        #[test]
        fn not_like_wraps_with_percent() {
            let (sql, params) = gen("title !~ 'spam'");
            assert_eq!(sql, "\"title\" NOT LIKE ?1");
            assert!(matches!(&params[0], SqlValue::Text(s) if s == "%spam%"));
        }

        #[test]
        fn null_equality_uses_is_null() {
            let (sql, params) = gen("deleted = null");
            assert_eq!(sql, "\"deleted\" IS NULL");
            assert!(params.is_empty());
        }

        #[test]
        fn null_not_equal_uses_is_not_null() {
            let (sql, params) = gen("avatar != null");
            assert_eq!(sql, "\"avatar\" IS NOT NULL");
            assert!(params.is_empty());
        }

        #[test]
        fn boolean_true() {
            let (sql, params) = gen("active = true");
            assert_eq!(sql, "\"active\" = ?1");
            assert!(matches!(&params[0], SqlValue::Integer(1)));
        }

        #[test]
        fn boolean_false() {
            let (sql, params) = gen("active = false");
            assert_eq!(sql, "\"active\" = ?1");
            assert!(matches!(&params[0], SqlValue::Integer(0)));
        }

        #[test]
        fn and_expression() {
            let (sql, params) = gen("a = 1 && b = 2");
            assert_eq!(sql, "(\"a\" = ?1 AND \"b\" = ?2)");
            assert_eq!(params.len(), 2);
        }

        #[test]
        fn or_expression() {
            let (sql, params) = gen("a = 1 || b = 2");
            assert_eq!(sql, "(\"a\" = ?1 OR \"b\" = ?2)");
            assert_eq!(params.len(), 2);
        }

        #[test]
        fn grouped_expression() {
            let (sql, params) = gen("(a = 1 || b = 2) && c = 3");
            assert_eq!(sql, "((\"a\" = ?1 OR \"b\" = ?2) AND \"c\" = ?3)");
            assert_eq!(params.len(), 3);
        }

        #[test]
        fn complex_nested_expression() {
            let (sql, params) =
                gen("(status = 'published' && views > 100) || (status = 'featured' && views > 50)");
            assert_eq!(
                sql,
                "((\"status\" = ?1 AND \"views\" > ?2) OR (\"status\" = ?3 AND \"views\" > ?4))"
            );
            assert_eq!(params.len(), 4);
        }

        #[test]
        fn any_equals_uses_json_each() {
            let (sql, params) = gen("tags ?= 'rust'");
            assert!(sql.contains("json_each"));
            assert!(sql.contains("= ?1"));
            assert_eq!(params.len(), 1);
            assert!(matches!(&params[0], SqlValue::Text(s) if s == "rust"));
        }

        #[test]
        fn any_like_uses_json_each_with_percent() {
            let (sql, params) = gen("tags ?~ 'ru'");
            assert!(sql.contains("json_each"));
            assert!(sql.contains("LIKE ?1"));
            assert!(matches!(&params[0], SqlValue::Text(s) if s == "%ru%"));
        }

        #[test]
        fn dot_notation_field_quoted() {
            let (sql, _) = gen("author.name = 'Alice'");
            assert_eq!(sql, "\"author\".\"name\" = ?1");
        }

        #[test]
        fn date_macro_now_generates_timestamp() {
            let (sql, params) = gen("created > @now");
            assert_eq!(sql, "\"created\" > ?1");
            assert!(matches!(&params[0], SqlValue::Text(s) if s.contains("-") && s.contains(":")));
        }

        #[test]
        fn date_macro_today() {
            let (_, params) = gen("created >= @today");
            assert!(matches!(&params[0], SqlValue::Text(s) if s.ends_with("00:00:00.000Z")));
        }

        #[test]
        fn float_value_stored_as_real() {
            let (_, params) = gen("price <= 9.99");
            assert!(matches!(&params[0], SqlValue::Real(f) if (*f - 9.99).abs() < f64::EPSILON));
        }

        #[test]
        fn integer_value_stored_as_integer() {
            let (_, params) = gen("views = 42");
            assert!(matches!(&params[0], SqlValue::Integer(42)));
        }

        #[test]
        fn negative_number_in_sql() {
            let (sql, params) = gen("balance >= -100");
            assert_eq!(sql, "\"balance\" >= ?1");
            assert!(matches!(&params[0], SqlValue::Integer(-100)));
        }

        #[test]
        fn field_name_sql_injection_prevented() {
            // Even if someone manages to pass a weird field name,
            // double-quoting prevents SQL injection. Internal double-quotes
            // are stripped, so the identifier is safely wrapped.
            let ast = FilterExpr::Condition {
                field: "bad\"; DROP TABLE users; --".to_string(),
                operator: ComparisonOp::Eq,
                value: FilterValue::String("x".into()),
            };
            let query = generate_sql(&ast).unwrap();
            // The internal `"` is stripped, so the identifier becomes a single
            // quoted name — the DROP TABLE text is trapped inside the identifier
            // quotes and cannot be executed as a separate statement.
            assert!(query.sql.starts_with('"'));
            // The dangerous `"` that would close the quoting has been stripped.
            assert!(!query.sql.contains("\"\""));
            // Value is parameterized.
            assert!(query.sql.contains("?1"));
        }

        #[test]
        fn value_sql_injection_prevented() {
            // Values are always parameterized, never interpolated.
            let (sql, params) = gen("name = \"'; DROP TABLE users; --\"");
            assert_eq!(sql, "\"name\" = ?1");
            assert!(matches!(&params[0], SqlValue::Text(s) if s.contains("DROP TABLE")));
            // The dangerous string is safely in the parameter, not the SQL.
            assert!(!sql.contains("DROP TABLE"));
        }

        #[test]
        fn deeply_nested_groups() {
            let (sql, params) = gen("((a = 1 && b = 2) || (c = 3 && d = 4))");
            assert_eq!(
                sql,
                "((\"a\" = ?1 AND \"b\" = ?2) OR (\"c\" = ?3 AND \"d\" = ?4))"
            );
            assert_eq!(params.len(), 4);
        }

        #[test]
        fn three_way_and() {
            let (sql, params) = gen("a = 1 && b = 2 && c = 3");
            assert_eq!(sql, "((\"a\" = ?1 AND \"b\" = ?2) AND \"c\" = ?3)");
            assert_eq!(params.len(), 3);
        }

        #[test]
        fn three_way_or() {
            let (sql, _) = gen("a = 1 || b = 2 || c = 3");
            assert_eq!(sql, "((\"a\" = ?1 OR \"b\" = ?2) OR \"c\" = ?3)");
        }

        #[test]
        fn mixed_precedence() {
            // a || b && c should be a || (b && c)
            let (sql, _) = gen("a = 1 || b = 2 && c = 3");
            assert_eq!(sql, "(\"a\" = ?1 OR (\"b\" = ?2 AND \"c\" = ?3))");
        }
    }

    // ── parse_and_generate_sql convenience function ─────────────────────

    mod convenience {
        use super::*;

        #[test]
        fn empty_returns_none() {
            assert!(parse_and_generate_sql("").unwrap().is_none());
            assert!(parse_and_generate_sql("   ").unwrap().is_none());
        }

        #[test]
        fn valid_filter_returns_some() {
            let result = parse_and_generate_sql("name = 'Alice'").unwrap();
            assert!(result.is_some());
            let query = result.unwrap();
            assert_eq!(query.sql, "\"name\" = ?1");
        }

        #[test]
        fn invalid_filter_returns_error() {
            let err = parse_and_generate_sql("name $$ 1").unwrap_err();
            assert!(matches!(err, FilterError::UnexpectedChar { .. }));
        }
    }

    // ── Integration: realistic PocketBase-style filters ─────────────────

    mod integration {
        use super::*;

        #[test]
        fn pocketbase_public_posts_filter() {
            let (sql, params) = super::sql_gen::gen("status = 'published' && featured = true");
            assert_eq!(sql, "(\"status\" = ?1 AND \"featured\" = ?2)");
            assert_eq!(params.len(), 2);
        }

        #[test]
        fn pocketbase_ownership_filter() {
            let (sql, _) = super::sql_gen::gen("author = 'user123'");
            assert_eq!(sql, "\"author\" = ?1");
        }

        #[test]
        fn pocketbase_search_filter() {
            let (sql, params) = super::sql_gen::gen("title ~ 'rust' || content ~ 'rust'");
            assert_eq!(sql, "(\"title\" LIKE ?1 OR \"content\" LIKE ?2)");
            assert!(matches!(&params[0], SqlValue::Text(s) if s == "%rust%"));
            assert!(matches!(&params[1], SqlValue::Text(s) if s == "%rust%"));
        }

        #[test]
        fn pocketbase_date_range_filter() {
            let result = parse_and_generate_sql("created >= @month && created < @now")
                .unwrap()
                .unwrap();
            assert!(result.sql.contains("AND"));
            assert_eq!(result.params.len(), 2);
        }

        #[test]
        fn pocketbase_null_check_with_conditions() {
            let (sql, params) = super::sql_gen::gen("deletedAt = null && status != 'archived'");
            assert_eq!(sql, "(\"deletedAt\" IS NULL AND \"status\" != ?1)");
            assert_eq!(params.len(), 1);
        }

        #[test]
        fn pocketbase_multi_value_tags_filter() {
            let (sql, params) = super::sql_gen::gen("tags ?= 'featured' && views > 100");
            assert!(sql.contains("json_each"));
            assert!(sql.contains("AND"));
            assert_eq!(params.len(), 2);
        }

        #[test]
        fn pocketbase_complex_access_rule() {
            // Simulates a complex API rule
            let (sql, params) = super::sql_gen::gen(
                "(visibility = 'public' || author = 'user123') && status = 'published' && deleted = null",
            );
            assert!(sql.contains("OR"));
            assert!(sql.contains("AND"));
            assert!(sql.contains("IS NULL"));
            assert_eq!(params.len(), 3); // 'public', 'user123', 'published' — null doesn't add a param
        }

        #[test]
        fn all_operators_generate_valid_sql() {
            let cases = vec![
                "field = 'a'",
                "field != 'a'",
                "field > 1",
                "field >= 1",
                "field < 1",
                "field <= 1",
                "field ~ 'a'",
                "field !~ 'a'",
                "field ?= 'a'",
                "field ?!= 'a'",
                "field ?> 1",
                "field ?>= 1",
                "field ?< 1",
                "field ?<= 1",
                "field ?~ 'a'",
                "field ?!~ 'a'",
                "field = null",
                "field != null",
                "field = true",
                "field = false",
                "field > @now",
                "field >= @today",
                "field < @month",
                "field <= @year",
            ];
            for input in cases {
                let result = parse_and_generate_sql(input);
                assert!(
                    result.is_ok(),
                    "failed to parse and generate SQL for: {input}; error: {:?}",
                    result.unwrap_err()
                );
                let query = result.unwrap().unwrap();
                assert!(!query.sql.is_empty(), "empty SQL for: {input}");
            }
        }
    }
}
