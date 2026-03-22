//! Parser for PocketBase-compatible access rule expressions.
//!
//! Rule expressions are used in collection API rules (`listRule`, `viewRule`, etc.)
//! to control access based on the current request context. They differ from
//! client-side filter expressions by supporting additional context variables:
//!
//! # Context Variables
//!
//! - `@request.auth.*` — fields from the authenticated user's record
//! - `@request.data.*` — fields from the incoming request body
//! - `@request.query.*` — URL query parameters
//! - `@request.headers.*` — request headers
//! - `@request.method` — HTTP method
//! - `@request.context` — request context (default, realtime)
//! - `@collection.<name>.*` — cross-collection lookups
//!
//! # Date Macros
//!
//! - `@now` — current UTC datetime
//! - `@today` — start of today UTC
//! - `@month` — start of current month UTC
//! - `@year` — start of current year UTC
//!
//! # Operators
//!
//! ## Comparison
//! `=`, `!=`, `>`, `>=`, `<`, `<=`, `~` (contains), `!~` (not contains)
//!
//! ## Multi-value (any element matches)
//! `?=`, `?!=`, `?>`, `?>=`, `?<`, `?<=`, `?~`, `?!~`
//!
//! ## Logical
//! `&&` (AND), `||` (OR), `!` (NOT, unary prefix)
//!
//! ## Grouping
//! `(` ... `)`
//!
//! # Examples
//!
//! ```
//! use zerobase_core::schema::rule_parser::{parse_rule, RuleExpr, Operand};
//!
//! // Simple ownership check
//! let ast = parse_rule("owner = @request.auth.id").unwrap();
//!
//! // Cross-collection lookup
//! let ast = parse_rule(
//!     "@collection.team_members.user ?= @request.auth.id && @collection.team_members.team ?= team"
//! ).unwrap();
//!
//! // Negation
//! let ast = parse_rule("!(status = \"deleted\")").unwrap();
//! ```

use std::fmt;

// ── Error Type ──────────────────────────────────────────────────────────────

/// Errors produced by the rule expression parser.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleParseError {
    #[error("unexpected character '{ch}' at position {pos}")]
    UnexpectedChar { ch: char, pos: usize },

    #[error("unterminated string starting at position {pos}")]
    UnterminatedString { pos: usize },

    #[error("unexpected token: expected {expected}, got {got}")]
    UnexpectedToken { expected: String, got: String },

    #[error("unexpected end of rule expression")]
    UnexpectedEnd,

    #[error("empty rule expression")]
    Empty,

    #[error("invalid number: {value}")]
    InvalidNumber { value: String },

    #[error("unknown @-macro '{name}' at position {pos}")]
    UnknownMacro { name: String, pos: usize },

    #[error("invalid @request path '{path}' at position {pos}: must start with auth, data, query, headers, method, or context")]
    InvalidRequestPath { path: String, pos: usize },

    #[error(
        "incomplete @collection reference at position {pos}: expected @collection.<name>.<field>"
    )]
    IncompleteCollectionRef { pos: usize },
}

// ── Tokens ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
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

    // Context references — stored as full dotted path after @
    /// `@request.auth.id` → RequestRef("auth.id")
    RequestRef(String),
    /// `@collection.team_members.user` → CollectionRef { collection: "team_members", path: "user" }
    CollectionRef {
        collection: String,
        path: String,
    },

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
    Not, // !

    // Grouping
    LParen,
    RParen,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
            Token::RequestRef(path) => write!(f, "@request.{path}"),
            Token::CollectionRef { collection, path } => {
                write!(f, "@collection.{collection}.{path}")
            }
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
            Token::Not => write!(f, "'!'"),
            Token::LParen => write!(f, "'('"),
            Token::RParen => write!(f, "')'"),
        }
    }
}

// ── AST ─────────────────────────────────────────────────────────────────────

/// A node in the rule expression AST.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleExpr {
    /// A comparison: `<operand> <op> <operand>`.
    Condition {
        left: Operand,
        operator: ComparisonOp,
        right: Operand,
    },
    /// Logical AND of two expressions.
    And(Box<RuleExpr>, Box<RuleExpr>),
    /// Logical OR of two expressions.
    Or(Box<RuleExpr>, Box<RuleExpr>),
    /// Logical NOT (negation) of an expression.
    Not(Box<RuleExpr>),
    /// Parenthesized (grouped) expression.
    Group(Box<RuleExpr>),
}

/// An operand in a rule condition — can be a field, a context variable, or a literal.
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// A record field reference, possibly with dot-notation (e.g. `author.name`).
    Field(String),
    /// `@request.auth.*` — authenticated user's record field.
    RequestAuth(String),
    /// `@request.data.*` — incoming request body field.
    RequestData(String),
    /// `@request.query.*` — URL query parameter.
    RequestQuery(String),
    /// `@request.headers.*` — request header.
    RequestHeaders(String),
    /// `@request.method` — HTTP method.
    RequestMethod,
    /// `@request.context` — request context.
    RequestContext,
    /// `@collection.<name>.<path>` — cross-collection field lookup.
    CollectionRef { collection: String, path: String },
    /// A string literal value.
    String(String),
    /// A numeric literal value.
    Number(f64),
    /// A boolean literal value.
    Bool(bool),
    /// Null literal.
    Null,
    /// `@now` — current UTC datetime.
    Now,
    /// `@today` — start of today UTC.
    Today,
    /// `@month` — start of current month UTC.
    Month,
    /// `@year` — start of current year UTC.
    Year,
}

/// Comparison operators supported in rule expressions.
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

impl fmt::Display for ComparisonOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComparisonOp::Eq => write!(f, "="),
            ComparisonOp::Neq => write!(f, "!="),
            ComparisonOp::Gt => write!(f, ">"),
            ComparisonOp::Gte => write!(f, ">="),
            ComparisonOp::Lt => write!(f, "<"),
            ComparisonOp::Lte => write!(f, "<="),
            ComparisonOp::Like => write!(f, "~"),
            ComparisonOp::NotLike => write!(f, "!~"),
            ComparisonOp::AnyEq => write!(f, "?="),
            ComparisonOp::AnyNeq => write!(f, "?!="),
            ComparisonOp::AnyGt => write!(f, "?>"),
            ComparisonOp::AnyGte => write!(f, "?>="),
            ComparisonOp::AnyLt => write!(f, "?<"),
            ComparisonOp::AnyLte => write!(f, "?<="),
            ComparisonOp::AnyLike => write!(f, "?~"),
            ComparisonOp::AnyNotLike => write!(f, "?!~"),
        }
    }
}

// ── Tokenizer ───────────────────────────────────────────────────────────────

/// Tokenize a rule expression string.
fn tokenize(input: &str) -> Result<Vec<Token>, RuleParseError> {
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

        // String literals
        if ch == '"' || ch == '\'' {
            let quote = ch;
            let start = i;
            i += 1;
            let mut value = String::new();
            while i < len && chars[i] != quote {
                if chars[i] == '\\' && i + 1 < len {
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
                return Err(RuleParseError::UnterminatedString { pos: start });
            }
            i += 1; // skip closing quote
            tokens.push(Token::StringLit(value));
            continue;
        }

        // Numbers (including negative — only when preceded by an operator or start)
        if ch.is_ascii_digit()
            || (ch == '-'
                && i + 1 < len
                && chars[i + 1].is_ascii_digit()
                && (tokens.is_empty() || is_operator_token(tokens.last())))
        {
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
                .map_err(|_| RuleParseError::InvalidNumber {
                    value: num_str.clone(),
                })?;
            tokens.push(Token::NumberLit(num));
            continue;
        }

        // @ macros and context references
        if ch == '@' {
            let macro_start = i;
            i += 1;
            let start = i;
            // Read the full dotted path: @request.auth.id, @collection.name.field, @now, etc.
            while i < len
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            let full_path: String = chars[start..i].iter().collect();

            if full_path.is_empty() {
                return Err(RuleParseError::UnexpectedChar {
                    ch: '@',
                    pos: macro_start,
                });
            }

            // Parse the @-prefixed reference
            let token = parse_at_reference(&full_path, macro_start)?;
            tokens.push(token);
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
                            return Err(RuleParseError::UnexpectedChar {
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
                        return Err(RuleParseError::UnexpectedChar {
                            ch: chars[i],
                            pos: i,
                        });
                    }
                }
            } else {
                return Err(RuleParseError::UnexpectedEnd);
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
                // Standalone ! is the NOT operator
                tokens.push(Token::Not);
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
                return Err(RuleParseError::UnexpectedChar {
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
                return Err(RuleParseError::UnexpectedChar {
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

        return Err(RuleParseError::UnexpectedChar { ch, pos: i });
    }

    Ok(tokens)
}

/// Parse a `@`-prefixed reference into the appropriate token.
fn parse_at_reference(path: &str, pos: usize) -> Result<Token, RuleParseError> {
    // Date macros
    match path {
        "now" => return Ok(Token::AtNow),
        "today" => return Ok(Token::AtToday),
        "month" => return Ok(Token::AtMonth),
        "year" => return Ok(Token::AtYear),
        _ => {}
    }

    // @request.*
    if let Some(rest) = path.strip_prefix("request.") {
        if rest.is_empty() {
            return Err(RuleParseError::InvalidRequestPath {
                path: path.to_string(),
                pos,
            });
        }
        return Ok(Token::RequestRef(rest.to_string()));
    }

    if path == "request" {
        return Err(RuleParseError::InvalidRequestPath {
            path: path.to_string(),
            pos,
        });
    }

    // @collection.<name>.<path>
    if let Some(rest) = path.strip_prefix("collection.") {
        // Must have at least collection_name.field
        if let Some(dot_pos) = rest.find('.') {
            let collection = &rest[..dot_pos];
            let field_path = &rest[dot_pos + 1..];
            if collection.is_empty() || field_path.is_empty() {
                return Err(RuleParseError::IncompleteCollectionRef { pos });
            }
            return Ok(Token::CollectionRef {
                collection: collection.to_string(),
                path: field_path.to_string(),
            });
        }
        return Err(RuleParseError::IncompleteCollectionRef { pos });
    }

    if path == "collection" {
        return Err(RuleParseError::IncompleteCollectionRef { pos });
    }

    Err(RuleParseError::UnknownMacro {
        name: path.to_string(),
        pos,
    })
}

/// Check if the last token is an operator (used for negative number disambiguation).
fn is_operator_token(token: Option<&Token>) -> bool {
    match token {
        None => true, // start of expression
        Some(t) => matches!(
            t,
            Token::Eq
                | Token::Neq
                | Token::Gt
                | Token::Gte
                | Token::Lt
                | Token::Lte
                | Token::Like
                | Token::NotLike
                | Token::AnyEq
                | Token::AnyNeq
                | Token::AnyGt
                | Token::AnyGte
                | Token::AnyLt
                | Token::AnyLte
                | Token::AnyLike
                | Token::AnyNotLike
                | Token::And
                | Token::Or
                | Token::Not
                | Token::LParen
        ),
    }
}

// ── Parser ──────────────────────────────────────────────────────────────────

/// Recursive descent parser for rule expressions.
///
/// Grammar (precedence low to high):
///   expr      = or_expr
///   or_expr   = and_expr ("||" and_expr)*
///   and_expr  = unary ("&&" unary)*
///   unary     = "!" unary | primary
///   primary   = "(" expr ")" | condition
///   condition = operand operator operand
///   operand   = IDENT | STRING | NUMBER | "true" | "false" | "null"
///             | "@now" | "@today" | "@month" | "@year"
///             | "@request.auth.*" | "@request.data.*" | "@request.query.*"
///             | "@request.headers.*" | "@request.method" | "@request.context"
///             | "@collection.<name>.<path>"
///   operator  = "=" | "!=" | ">" | ">=" | "<" | "<=" | "~" | "!~"
///             | "?=" | "?!=" | "?>" | "?>=" | "?<" | "?<=" | "?~" | "?!~"
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

    fn parse(mut self) -> Result<RuleExpr, RuleParseError> {
        if self.tokens.is_empty() {
            return Err(RuleParseError::Empty);
        }
        let expr = self.parse_or()?;
        if self.pos < self.tokens.len() {
            return Err(RuleParseError::UnexpectedToken {
                expected: "end of expression".to_string(),
                got: format!("{}", self.tokens[self.pos]),
            });
        }
        Ok(expr)
    }

    fn parse_or(&mut self) -> Result<RuleExpr, RuleParseError> {
        let mut left = self.parse_and()?;
        while self.peek() == Some(&Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = RuleExpr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<RuleExpr, RuleParseError> {
        let mut left = self.parse_unary()?;
        while self.peek() == Some(&Token::And) {
            self.advance();
            let right = self.parse_unary()?;
            left = RuleExpr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<RuleExpr, RuleParseError> {
        if self.peek() == Some(&Token::Not) {
            self.advance(); // consume !
            let expr = self.parse_unary()?;
            return Ok(RuleExpr::Not(Box::new(expr)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<RuleExpr, RuleParseError> {
        match self.peek() {
            Some(Token::LParen) => {
                self.advance(); // consume '('
                let expr = self.parse_or()?;
                match self.advance() {
                    Some(Token::RParen) => Ok(RuleExpr::Group(Box::new(expr))),
                    Some(tok) => Err(RuleParseError::UnexpectedToken {
                        expected: "')'".to_string(),
                        got: format!("{tok}"),
                    }),
                    None => Err(RuleParseError::UnexpectedEnd),
                }
            }
            Some(_) => self.parse_condition(),
            None => Err(RuleParseError::UnexpectedEnd),
        }
    }

    fn parse_condition(&mut self) -> Result<RuleExpr, RuleParseError> {
        let left = self.parse_operand()?;
        let operator = self.parse_operator()?;
        let right = self.parse_operand()?;

        Ok(RuleExpr::Condition {
            left,
            operator,
            right,
        })
    }

    fn parse_operand(&mut self) -> Result<Operand, RuleParseError> {
        match self.advance() {
            Some(Token::Ident(name)) => Ok(Operand::Field(name)),
            Some(Token::StringLit(s)) => Ok(Operand::String(s)),
            Some(Token::NumberLit(n)) => Ok(Operand::Number(n)),
            Some(Token::True) => Ok(Operand::Bool(true)),
            Some(Token::False) => Ok(Operand::Bool(false)),
            Some(Token::Null) => Ok(Operand::Null),
            Some(Token::AtNow) => Ok(Operand::Now),
            Some(Token::AtToday) => Ok(Operand::Today),
            Some(Token::AtMonth) => Ok(Operand::Month),
            Some(Token::AtYear) => Ok(Operand::Year),
            Some(Token::RequestRef(path)) => parse_request_operand(&path),
            Some(Token::CollectionRef { collection, path }) => {
                Ok(Operand::CollectionRef { collection, path })
            }
            Some(tok) => Err(RuleParseError::UnexpectedToken {
                expected: "operand (field, value, @request.*, or @collection.*)".to_string(),
                got: format!("{tok}"),
            }),
            None => Err(RuleParseError::UnexpectedEnd),
        }
    }

    fn parse_operator(&mut self) -> Result<ComparisonOp, RuleParseError> {
        match self.advance() {
            Some(Token::Eq) => Ok(ComparisonOp::Eq),
            Some(Token::Neq) => Ok(ComparisonOp::Neq),
            Some(Token::Gt) => Ok(ComparisonOp::Gt),
            Some(Token::Gte) => Ok(ComparisonOp::Gte),
            Some(Token::Lt) => Ok(ComparisonOp::Lt),
            Some(Token::Lte) => Ok(ComparisonOp::Lte),
            Some(Token::Like) => Ok(ComparisonOp::Like),
            Some(Token::NotLike) => Ok(ComparisonOp::NotLike),
            Some(Token::AnyEq) => Ok(ComparisonOp::AnyEq),
            Some(Token::AnyNeq) => Ok(ComparisonOp::AnyNeq),
            Some(Token::AnyGt) => Ok(ComparisonOp::AnyGt),
            Some(Token::AnyGte) => Ok(ComparisonOp::AnyGte),
            Some(Token::AnyLt) => Ok(ComparisonOp::AnyLt),
            Some(Token::AnyLte) => Ok(ComparisonOp::AnyLte),
            Some(Token::AnyLike) => Ok(ComparisonOp::AnyLike),
            Some(Token::AnyNotLike) => Ok(ComparisonOp::AnyNotLike),
            Some(tok) => Err(RuleParseError::UnexpectedToken {
                expected: "comparison operator".to_string(),
                got: format!("{tok}"),
            }),
            None => Err(RuleParseError::UnexpectedEnd),
        }
    }
}

/// Convert a `@request.*` path into an `Operand`.
fn parse_request_operand(path: &str) -> Result<Operand, RuleParseError> {
    if let Some(rest) = path.strip_prefix("auth.") {
        if rest.is_empty() {
            return Err(RuleParseError::InvalidRequestPath {
                path: format!("request.{path}"),
                pos: 0,
            });
        }
        return Ok(Operand::RequestAuth(rest.to_string()));
    }
    if path == "auth" {
        // @request.auth by itself — refers to the entire auth record
        return Ok(Operand::RequestAuth(String::new()));
    }

    if let Some(rest) = path.strip_prefix("data.") {
        if rest.is_empty() {
            return Err(RuleParseError::InvalidRequestPath {
                path: format!("request.{path}"),
                pos: 0,
            });
        }
        return Ok(Operand::RequestData(rest.to_string()));
    }

    // Also support @request.body.* as alias for @request.data.*
    if let Some(rest) = path.strip_prefix("body.") {
        if rest.is_empty() {
            return Err(RuleParseError::InvalidRequestPath {
                path: format!("request.{path}"),
                pos: 0,
            });
        }
        return Ok(Operand::RequestData(rest.to_string()));
    }

    if let Some(rest) = path.strip_prefix("query.") {
        if rest.is_empty() {
            return Err(RuleParseError::InvalidRequestPath {
                path: format!("request.{path}"),
                pos: 0,
            });
        }
        return Ok(Operand::RequestQuery(rest.to_string()));
    }

    if let Some(rest) = path.strip_prefix("headers.") {
        if rest.is_empty() {
            return Err(RuleParseError::InvalidRequestPath {
                path: format!("request.{path}"),
                pos: 0,
            });
        }
        return Ok(Operand::RequestHeaders(rest.to_string()));
    }

    if path == "method" {
        return Ok(Operand::RequestMethod);
    }

    if path == "context" {
        return Ok(Operand::RequestContext);
    }

    Err(RuleParseError::InvalidRequestPath {
        path: format!("request.{path}"),
        pos: 0,
    })
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Parse a PocketBase-style rule expression into an AST.
///
/// Returns `RuleParseError` if the expression is malformed.
///
/// # Examples
///
/// ```
/// use zerobase_core::schema::rule_parser::parse_rule;
///
/// // Ownership check
/// let ast = parse_rule("owner = @request.auth.id").unwrap();
///
/// // Authenticated user check
/// let ast = parse_rule("@request.auth.id != \"\"").unwrap();
///
/// // Complex rule
/// let ast = parse_rule(
///     "(visibility = \"public\" || author = @request.auth.id) && status = \"published\""
/// ).unwrap();
/// ```
pub fn parse_rule(input: &str) -> Result<RuleExpr, RuleParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(RuleParseError::Empty);
    }
    let tokens = tokenize(trimmed)?;
    Parser::new(tokens).parse()
}

/// Validate a rule expression string without producing an AST.
///
/// Returns `Ok(())` if the rule is syntactically valid, or `Err` with a description
/// of the problem. This is useful for validating rules at collection creation time.
pub fn validate_rule(input: &str) -> Result<(), RuleParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        // Empty string is a valid rule (means "open to everyone")
        return Ok(());
    }
    parse_rule(trimmed)?;
    Ok(())
}

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
            assert_eq!(tokens[2], Token::StringLit("Bob".into()));
        }

        #[test]
        fn request_auth_reference() {
            let tokens = tokenize("@request.auth.id != \"\"").unwrap();
            assert_eq!(
                tokens,
                vec![
                    Token::RequestRef("auth.id".into()),
                    Token::Neq,
                    Token::StringLit(String::new()),
                ]
            );
        }

        #[test]
        fn request_data_reference() {
            let tokens = tokenize("@request.data.title ~ \"hello\"").unwrap();
            assert_eq!(tokens[0], Token::RequestRef("data.title".into()));
        }

        #[test]
        fn request_body_reference() {
            let tokens = tokenize("@request.body.owner = @request.auth.id").unwrap();
            assert_eq!(tokens[0], Token::RequestRef("body.owner".into()));
            assert_eq!(tokens[2], Token::RequestRef("auth.id".into()));
        }

        #[test]
        fn request_query_reference() {
            let tokens = tokenize("@request.query.publicOnly = \"true\"").unwrap();
            assert_eq!(tokens[0], Token::RequestRef("query.publicOnly".into()));
        }

        #[test]
        fn request_headers_reference() {
            let tokens = tokenize("@request.headers.x_api_key != \"\"").unwrap();
            assert_eq!(tokens[0], Token::RequestRef("headers.x_api_key".into()));
        }

        #[test]
        fn request_method_reference() {
            let tokens = tokenize("@request.method = \"GET\"").unwrap();
            assert_eq!(tokens[0], Token::RequestRef("method".into()));
        }

        #[test]
        fn request_context_reference() {
            let tokens = tokenize("@request.context = \"default\"").unwrap();
            assert_eq!(tokens[0], Token::RequestRef("context".into()));
        }

        #[test]
        fn collection_reference() {
            let tokens = tokenize("@collection.team_members.user ?= @request.auth.id").unwrap();
            assert_eq!(
                tokens[0],
                Token::CollectionRef {
                    collection: "team_members".into(),
                    path: "user".into(),
                }
            );
            assert_eq!(tokens[1], Token::AnyEq);
            assert_eq!(tokens[2], Token::RequestRef("auth.id".into()));
        }

        #[test]
        fn collection_reference_with_nested_path() {
            let tokens = tokenize("@collection.org_members.org = org").unwrap();
            assert_eq!(
                tokens[0],
                Token::CollectionRef {
                    collection: "org_members".into(),
                    path: "org".into(),
                }
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
        fn not_operator() {
            let tokens = tokenize("!(active = false)").unwrap();
            assert_eq!(
                tokens,
                vec![
                    Token::Not,
                    Token::LParen,
                    Token::Ident("active".into()),
                    Token::Eq,
                    Token::False,
                    Token::RParen,
                ]
            );
        }

        #[test]
        fn not_vs_neq_disambiguation() {
            // `!` followed by `=` is `!=`
            let tokens = tokenize("a != 1").unwrap();
            assert_eq!(tokens[1], Token::Neq);

            // `!` followed by `~` is `!~`
            let tokens = tokenize("a !~ 'x'").unwrap();
            assert_eq!(tokens[1], Token::NotLike);

            // standalone `!` is NOT
            let tokens = tokenize("!a = 1").unwrap();
            assert_eq!(tokens[0], Token::Not);
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
            assert!(tokens.contains(&Token::LParen));
            assert!(tokens.contains(&Token::RParen));
            assert!(tokens.contains(&Token::Or));
            assert!(tokens.contains(&Token::And));
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
        fn number_literals() {
            let tokens = tokenize("views > 100").unwrap();
            assert_eq!(tokens[2], Token::NumberLit(100.0));

            let tokens = tokenize("price < 9.99").unwrap();
            assert_eq!(tokens[2], Token::NumberLit(9.99));
        }

        #[test]
        fn negative_number() {
            let tokens = tokenize("balance >= -50").unwrap();
            assert_eq!(tokens[2], Token::NumberLit(-50.0));
        }

        #[test]
        fn boolean_and_null() {
            let tokens = tokenize("active = true && deleted = null").unwrap();
            assert!(tokens.contains(&Token::True));
            assert!(tokens.contains(&Token::Null));
        }

        // ── Error cases ─────────────────────────────────────────────────

        #[test]
        fn unterminated_string_error() {
            let err = tokenize("name = \"unclosed").unwrap_err();
            assert!(matches!(err, RuleParseError::UnterminatedString { .. }));
        }

        #[test]
        fn unexpected_char_error() {
            let err = tokenize("name # 1").unwrap_err();
            assert!(matches!(err, RuleParseError::UnexpectedChar { .. }));
        }

        #[test]
        fn unknown_at_macro_error() {
            let err = tokenize("@unknown = 1").unwrap_err();
            assert!(matches!(err, RuleParseError::UnknownMacro { .. }));
        }

        #[test]
        fn incomplete_collection_ref_error() {
            let err = tokenize("@collection.users = 1").unwrap_err();
            assert!(matches!(
                err,
                RuleParseError::IncompleteCollectionRef { .. }
            ));
        }

        #[test]
        fn bare_at_request_error() {
            let err = tokenize("@request = 1").unwrap_err();
            assert!(matches!(err, RuleParseError::InvalidRequestPath { .. }));
        }

        #[test]
        fn bare_collection_error() {
            let err = tokenize("@collection = 1").unwrap_err();
            assert!(matches!(
                err,
                RuleParseError::IncompleteCollectionRef { .. }
            ));
        }
    }

    // ── Parser tests ────────────────────────────────────────────────────

    mod parser {
        use super::*;

        #[test]
        fn simple_field_equality() {
            let ast = parse_rule("name = \"Alice\"").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::Field("name".into()),
                    operator: ComparisonOp::Eq,
                    right: Operand::String("Alice".into()),
                }
            );
        }

        #[test]
        fn ownership_check() {
            let ast = parse_rule("owner = @request.auth.id").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::Field("owner".into()),
                    operator: ComparisonOp::Eq,
                    right: Operand::RequestAuth("id".into()),
                }
            );
        }

        #[test]
        fn auth_check_not_empty() {
            let ast = parse_rule("@request.auth.id != \"\"").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::RequestAuth("id".into()),
                    operator: ComparisonOp::Neq,
                    right: Operand::String(String::new()),
                }
            );
        }

        #[test]
        fn request_data_field() {
            let ast = parse_rule("@request.data.owner = @request.auth.id").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::RequestData("owner".into()),
                    operator: ComparisonOp::Eq,
                    right: Operand::RequestAuth("id".into()),
                }
            );
        }

        #[test]
        fn request_body_alias() {
            let ast = parse_rule("@request.body.owner = @request.auth.id").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::RequestData("owner".into()),
                    operator: ComparisonOp::Eq,
                    right: Operand::RequestAuth("id".into()),
                }
            );
        }

        #[test]
        fn collection_reference_in_rule() {
            let ast = parse_rule("@collection.team_members.user ?= @request.auth.id").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::CollectionRef {
                        collection: "team_members".into(),
                        path: "user".into(),
                    },
                    operator: ComparisonOp::AnyEq,
                    right: Operand::RequestAuth("id".into()),
                }
            );
        }

        #[test]
        fn date_macro_comparison() {
            let ast = parse_rule("created > @now").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::Field("created".into()),
                    operator: ComparisonOp::Gt,
                    right: Operand::Now,
                }
            );
        }

        #[test]
        fn field_to_field_comparison() {
            let ast = parse_rule("start_date < end_date").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::Field("start_date".into()),
                    operator: ComparisonOp::Lt,
                    right: Operand::Field("end_date".into()),
                }
            );
        }

        #[test]
        fn dot_notation_field() {
            let ast = parse_rule("author.name ~ 'John'").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::Field("author.name".into()),
                    operator: ComparisonOp::Like,
                    right: Operand::String("John".into()),
                }
            );
        }

        #[test]
        fn relation_via_auth() {
            let ast = parse_rule("team.members ?= @request.auth.id").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::Field("team.members".into()),
                    operator: ComparisonOp::AnyEq,
                    right: Operand::RequestAuth("id".into()),
                }
            );
        }

        #[test]
        fn and_expression() {
            let ast = parse_rule("a = 1 && b = 2").unwrap();
            assert!(matches!(ast, RuleExpr::And(_, _)));
        }

        #[test]
        fn or_expression() {
            let ast = parse_rule("a = 1 || b = 2").unwrap();
            assert!(matches!(ast, RuleExpr::Or(_, _)));
        }

        #[test]
        fn not_expression() {
            let ast = parse_rule("!(status = \"deleted\")").unwrap();
            match ast {
                RuleExpr::Not(inner) => {
                    assert!(matches!(*inner, RuleExpr::Group(_)));
                }
                _ => panic!("expected Not"),
            }
        }

        #[test]
        fn double_not() {
            let ast = parse_rule("!!active = true").unwrap();
            match ast {
                RuleExpr::Not(inner) => {
                    assert!(matches!(*inner, RuleExpr::Not(_)));
                }
                _ => panic!("expected Not(Not(...))"),
            }
        }

        #[test]
        fn grouped_expression() {
            let ast = parse_rule("(a = 1 || b = 2) && c = 3").unwrap();
            match ast {
                RuleExpr::And(left, right) => {
                    assert!(matches!(*left, RuleExpr::Group(_)));
                    assert!(matches!(*right, RuleExpr::Condition { .. }));
                }
                _ => panic!("expected And"),
            }
        }

        #[test]
        fn nested_groups() {
            let ast = parse_rule("((a = 1))").unwrap();
            match ast {
                RuleExpr::Group(inner) => {
                    assert!(matches!(*inner, RuleExpr::Group(_)));
                }
                _ => panic!("expected Group"),
            }
        }

        #[test]
        fn precedence_and_binds_tighter_than_or() {
            let ast = parse_rule("a = 1 || b = 2 && c = 3").unwrap();
            match ast {
                RuleExpr::Or(left, right) => {
                    assert!(matches!(*left, RuleExpr::Condition { .. }));
                    assert!(matches!(*right, RuleExpr::And(_, _)));
                }
                _ => panic!("expected Or at top level"),
            }
        }

        #[test]
        fn null_comparison() {
            let ast = parse_rule("deleted = null").unwrap();
            assert!(matches!(
                ast,
                RuleExpr::Condition {
                    right: Operand::Null,
                    ..
                }
            ));
        }

        #[test]
        fn boolean_values() {
            let ast = parse_rule("active = true").unwrap();
            assert!(matches!(
                ast,
                RuleExpr::Condition {
                    right: Operand::Bool(true),
                    ..
                }
            ));

            let ast = parse_rule("active = false").unwrap();
            assert!(matches!(
                ast,
                RuleExpr::Condition {
                    right: Operand::Bool(false),
                    ..
                }
            ));
        }

        #[test]
        fn number_comparison() {
            let ast = parse_rule("views > 100").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::Field("views".into()),
                    operator: ComparisonOp::Gt,
                    right: Operand::Number(100.0),
                }
            );
        }

        #[test]
        fn request_auth_verified() {
            let ast = parse_rule("@request.auth.verified = true").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::RequestAuth("verified".into()),
                    operator: ComparisonOp::Eq,
                    right: Operand::Bool(true),
                }
            );
        }

        #[test]
        fn request_query_param() {
            let ast = parse_rule("@request.query.publicOnly = \"true\"").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::RequestQuery("publicOnly".into()),
                    operator: ComparisonOp::Eq,
                    right: Operand::String("true".into()),
                }
            );
        }

        #[test]
        fn request_headers() {
            let ast = parse_rule("@request.headers.x_api_key != \"\"").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::RequestHeaders("x_api_key".into()),
                    operator: ComparisonOp::Neq,
                    right: Operand::String(String::new()),
                }
            );
        }

        #[test]
        fn request_method() {
            let ast = parse_rule("@request.method = \"GET\"").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::RequestMethod,
                    operator: ComparisonOp::Eq,
                    right: Operand::String("GET".into()),
                }
            );
        }

        #[test]
        fn request_context() {
            let ast = parse_rule("@request.context = \"realtime\"").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::RequestContext,
                    operator: ComparisonOp::Eq,
                    right: Operand::String("realtime".into()),
                }
            );
        }

        #[test]
        fn all_date_macros() {
            for (input, expected) in [
                ("field > @now", Operand::Now),
                ("field >= @today", Operand::Today),
                ("field < @month", Operand::Month),
                ("field <= @year", Operand::Year),
            ] {
                let ast = parse_rule(input).unwrap();
                match ast {
                    RuleExpr::Condition { right, .. } => assert_eq!(right, expected),
                    _ => panic!("expected Condition for: {input}"),
                }
            }
        }

        #[test]
        fn all_comparison_operators() {
            let ops = vec![
                ("a = 1", ComparisonOp::Eq),
                ("a != 1", ComparisonOp::Neq),
                ("a > 1", ComparisonOp::Gt),
                ("a >= 1", ComparisonOp::Gte),
                ("a < 1", ComparisonOp::Lt),
                ("a <= 1", ComparisonOp::Lte),
                ("a ~ 'x'", ComparisonOp::Like),
                ("a !~ 'x'", ComparisonOp::NotLike),
                ("a ?= 'x'", ComparisonOp::AnyEq),
                ("a ?!= 'x'", ComparisonOp::AnyNeq),
                ("a ?> 1", ComparisonOp::AnyGt),
                ("a ?>= 1", ComparisonOp::AnyGte),
                ("a ?< 1", ComparisonOp::AnyLt),
                ("a ?<= 1", ComparisonOp::AnyLte),
                ("a ?~ 'x'", ComparisonOp::AnyLike),
                ("a ?!~ 'x'", ComparisonOp::AnyNotLike),
            ];
            for (input, expected_op) in ops {
                let ast = parse_rule(input).unwrap();
                match ast {
                    RuleExpr::Condition { operator, .. } => {
                        assert_eq!(operator, expected_op, "failed for: {input}");
                    }
                    _ => panic!("expected Condition for: {input}"),
                }
            }
        }

        // ── Error cases ─────────────────────────────────────────────────

        #[test]
        fn empty_rule_error() {
            assert!(matches!(parse_rule(""), Err(RuleParseError::Empty)));
            assert!(matches!(parse_rule("   "), Err(RuleParseError::Empty)));
        }

        #[test]
        fn missing_value_error() {
            let err = parse_rule("name =").unwrap_err();
            assert!(matches!(err, RuleParseError::UnexpectedEnd));
        }

        #[test]
        fn missing_operator_error() {
            let err = parse_rule("name 'Alice'").unwrap_err();
            assert!(matches!(err, RuleParseError::UnexpectedToken { .. }));
        }

        #[test]
        fn unclosed_paren_error() {
            let err = parse_rule("(a = 1").unwrap_err();
            assert!(matches!(err, RuleParseError::UnexpectedEnd));
        }

        #[test]
        fn extra_tokens_error() {
            let err = parse_rule("a = 1 b = 2").unwrap_err();
            assert!(matches!(err, RuleParseError::UnexpectedToken { .. }));
        }

        #[test]
        fn bare_not_at_end_error() {
            let err = parse_rule("!").unwrap_err();
            assert!(matches!(err, RuleParseError::UnexpectedEnd));
        }
    }

    // ── Integration: realistic PocketBase-style rules ────────────────────

    mod integration {
        use super::*;

        #[test]
        fn public_read_auth_write_rules() {
            // listRule: "" (empty = open, handled at caller level)
            // createRule: @request.auth.id != ""
            let ast = parse_rule("@request.auth.id != \"\"").unwrap();
            match ast {
                RuleExpr::Condition {
                    left,
                    operator,
                    right,
                } => {
                    assert_eq!(left, Operand::RequestAuth("id".into()));
                    assert_eq!(operator, ComparisonOp::Neq);
                    assert_eq!(right, Operand::String(String::new()));
                }
                _ => panic!("expected Condition"),
            }
        }

        #[test]
        fn private_to_owner_rule() {
            let ast = parse_rule("owner = @request.auth.id").unwrap();
            match ast {
                RuleExpr::Condition {
                    left,
                    operator,
                    right,
                } => {
                    assert_eq!(left, Operand::Field("owner".into()));
                    assert_eq!(operator, ComparisonOp::Eq);
                    assert_eq!(right, Operand::RequestAuth("id".into()));
                }
                _ => panic!("expected Condition"),
            }
        }

        #[test]
        fn team_based_access_via_collection_lookup() {
            let ast = parse_rule(
                "@collection.team_members.user ?= @request.auth.id && @collection.team_members.team ?= team",
            ).unwrap();
            match ast {
                RuleExpr::And(left, right) => {
                    match *left {
                        RuleExpr::Condition {
                            left: l,
                            operator,
                            right: r,
                        } => {
                            assert_eq!(
                                l,
                                Operand::CollectionRef {
                                    collection: "team_members".into(),
                                    path: "user".into(),
                                }
                            );
                            assert_eq!(operator, ComparisonOp::AnyEq);
                            assert_eq!(r, Operand::RequestAuth("id".into()));
                        }
                        _ => panic!("expected Condition in left"),
                    }
                    match *right {
                        RuleExpr::Condition {
                            left: l,
                            operator,
                            right: r,
                        } => {
                            assert_eq!(
                                l,
                                Operand::CollectionRef {
                                    collection: "team_members".into(),
                                    path: "team".into(),
                                }
                            );
                            assert_eq!(operator, ComparisonOp::AnyEq);
                            assert_eq!(r, Operand::Field("team".into()));
                        }
                        _ => panic!("expected Condition in right"),
                    }
                }
                _ => panic!("expected And"),
            }
        }

        #[test]
        fn role_based_access() {
            let ast = parse_rule(
                "@collection.user_roles.user = @request.auth.id && @collection.user_roles.role = \"admin\"",
            ).unwrap();
            assert!(matches!(ast, RuleExpr::And(_, _)));
        }

        #[test]
        fn complex_visibility_rule() {
            let ast = parse_rule(
                "(visibility = \"public\" || author = @request.auth.id) && status = \"published\"",
            )
            .unwrap();
            match ast {
                RuleExpr::And(left, right) => {
                    assert!(matches!(*left, RuleExpr::Group(_)));
                    match *right {
                        RuleExpr::Condition {
                            left: l,
                            operator,
                            right: r,
                        } => {
                            assert_eq!(l, Operand::Field("status".into()));
                            assert_eq!(operator, ComparisonOp::Eq);
                            assert_eq!(r, Operand::String("published".into()));
                        }
                        _ => panic!("expected Condition"),
                    }
                }
                _ => panic!("expected And"),
            }
        }

        #[test]
        fn prevent_field_change_rule() {
            // PocketBase: @request.body.owner:isset = false
            // We represent the modifier as part of the field path
            let ast = parse_rule("@request.data.status != \"deleted\" && owner = @request.auth.id")
                .unwrap();
            assert!(matches!(ast, RuleExpr::And(_, _)));
        }

        #[test]
        fn negated_condition() {
            let ast = parse_rule("!(status = \"archived\")").unwrap();
            match ast {
                RuleExpr::Not(inner) => match *inner {
                    RuleExpr::Group(group_inner) => {
                        assert!(matches!(*group_inner, RuleExpr::Condition { .. }));
                    }
                    _ => panic!("expected Group inside Not"),
                },
                _ => panic!("expected Not"),
            }
        }

        #[test]
        fn negated_with_and() {
            let ast = parse_rule("!deleted = true && owner = @request.auth.id").unwrap();
            // `!` binds tighter than `&&`, so this is: (!condition) && condition
            match ast {
                RuleExpr::And(left, _right) => {
                    assert!(matches!(*left, RuleExpr::Not(_)));
                }
                _ => panic!("expected And"),
            }
        }

        #[test]
        fn deeply_nested_rule() {
            let ast =
                parse_rule("((a = 1 && b = 2) || (c = 3 && d = 4)) && @request.auth.id != \"\"")
                    .unwrap();
            assert!(matches!(ast, RuleExpr::And(_, _)));
        }

        #[test]
        fn string_matching_rule() {
            let ast = parse_rule("title ~ \"hello\" || content ~ \"hello\"").unwrap();
            assert!(matches!(ast, RuleExpr::Or(_, _)));
        }

        #[test]
        fn multi_value_relation_rule() {
            let ast = parse_rule("tags ?= \"featured\" && views > 100").unwrap();
            assert!(matches!(ast, RuleExpr::And(_, _)));
        }

        #[test]
        fn date_range_with_auth() {
            let ast = parse_rule("created >= @month && created < @now && owner = @request.auth.id")
                .unwrap();
            // Should be: And(And(cond, cond), cond)
            match ast {
                RuleExpr::And(_, _) => {} // valid
                _ => panic!("expected And"),
            }
        }

        #[test]
        fn auth_verified_check() {
            let ast = parse_rule("@request.auth.verified = true").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::RequestAuth("verified".into()),
                    operator: ComparisonOp::Eq,
                    right: Operand::Bool(true),
                }
            );
        }

        #[test]
        fn auth_role_check() {
            let ast = parse_rule("@request.auth.role = \"admin\"").unwrap();
            assert_eq!(
                ast,
                RuleExpr::Condition {
                    left: Operand::RequestAuth("role".into()),
                    operator: ComparisonOp::Eq,
                    right: Operand::String("admin".into()),
                }
            );
        }

        #[test]
        fn request_context_realtime() {
            let ast =
                parse_rule("@request.context = \"realtime\" || owner = @request.auth.id").unwrap();
            assert!(matches!(ast, RuleExpr::Or(_, _)));
        }
    }

    // ── Validate rule tests ─────────────────────────────────────────────

    mod validate {
        use super::*;

        #[test]
        fn empty_string_is_valid() {
            assert!(validate_rule("").is_ok());
            assert!(validate_rule("   ").is_ok());
        }

        #[test]
        fn valid_rule_passes() {
            assert!(validate_rule("owner = @request.auth.id").is_ok());
        }

        #[test]
        fn invalid_rule_fails() {
            assert!(validate_rule("name $$ 1").is_err());
        }

        #[test]
        fn incomplete_expression_fails() {
            assert!(validate_rule("name =").is_err());
        }
    }
}
