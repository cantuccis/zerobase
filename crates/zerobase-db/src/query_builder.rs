//! Lightweight query builder for dynamic SQL generation.
//!
//! Produces parameterized `(sql, params)` tuples — never interpolates values
//! directly into SQL strings. This prevents SQL injection while supporting
//! the dynamic schemas that Pocketbase-style collections require.

use rusqlite::types::Value;

/// A built query ready for execution: SQL string + bound parameters.
#[derive(Debug, Clone)]
pub struct BuiltQuery {
    pub sql: String,
    pub params: Vec<Value>,
}

// ── SELECT ──────────────────────────────────────────────────────────────────

/// Builder for SELECT queries.
///
/// # Example
///
/// ```
/// use zerobase_db::query_builder::{SelectBuilder, SortDirection};
///
/// let query = SelectBuilder::new("users")
///     .columns(&["id", "name", "email"])
///     .where_clause("email = ?", vec![rusqlite::types::Value::Text("a@b.com".into())])
///     .order_by("name", SortDirection::Asc)
///     .limit(10)
///     .offset(20)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct SelectBuilder {
    table: String,
    columns: Vec<String>,
    where_clauses: Vec<String>,
    params: Vec<Value>,
    order_clauses: Vec<String>,
    limit: Option<u64>,
    offset: Option<u64>,
    joins: Vec<String>,
}

/// Sort direction for ORDER BY clauses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl std::fmt::Display for SortDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SortDirection::Asc => write!(f, "ASC"),
            SortDirection::Desc => write!(f, "DESC"),
        }
    }
}

impl SelectBuilder {
    pub fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            columns: Vec::new(),
            where_clauses: Vec::new(),
            params: Vec::new(),
            order_clauses: Vec::new(),
            limit: None,
            offset: None,
            joins: Vec::new(),
        }
    }

    /// Set the columns to select. If empty, defaults to `*`.
    pub fn columns(mut self, cols: &[&str]) -> Self {
        self.columns = cols.iter().map(|c| c.to_string()).collect();
        self
    }

    /// Add a WHERE condition with bound parameters.
    ///
    /// Multiple calls are combined with AND.
    pub fn where_clause(mut self, clause: &str, params: Vec<Value>) -> Self {
        self.where_clauses.push(clause.to_string());
        self.params.extend(params);
        self
    }

    /// Add an ORDER BY clause.
    pub fn order_by(mut self, column: &str, direction: SortDirection) -> Self {
        self.order_clauses.push(format!("{column} {direction}"));
        self
    }

    /// Set the LIMIT.
    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set the OFFSET.
    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Add a JOIN clause.
    pub fn join(mut self, join_clause: &str) -> Self {
        self.joins.push(join_clause.to_string());
        self
    }

    /// Build the final SQL query and parameters.
    pub fn build(self) -> BuiltQuery {
        let cols = if self.columns.is_empty() {
            "*".to_string()
        } else {
            self.columns.join(", ")
        };

        let mut sql = format!("SELECT {cols} FROM {}", self.table);

        for join in &self.joins {
            sql.push(' ');
            sql.push_str(join);
        }

        if !self.where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.where_clauses.join(" AND "));
        }

        if !self.order_clauses.is_empty() {
            sql.push_str(" ORDER BY ");
            sql.push_str(&self.order_clauses.join(", "));
        }

        if let Some(limit) = self.limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        }

        if let Some(offset) = self.offset {
            sql.push_str(&format!(" OFFSET {offset}"));
        }

        BuiltQuery {
            sql,
            params: self.params,
        }
    }
}

// ── INSERT ──────────────────────────────────────────────────────────────────

/// Builder for INSERT queries.
#[derive(Debug, Clone)]
pub struct InsertBuilder {
    table: String,
    columns: Vec<String>,
    params: Vec<Value>,
}

impl InsertBuilder {
    pub fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            columns: Vec::new(),
            params: Vec::new(),
        }
    }

    /// Add a column/value pair.
    pub fn set(mut self, column: &str, value: Value) -> Self {
        self.columns.push(column.to_string());
        self.params.push(value);
        self
    }

    /// Build the INSERT query.
    pub fn build(self) -> BuiltQuery {
        let cols = self.columns.join(", ");
        let placeholders: Vec<String> = (1..=self.columns.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            self.table,
            cols,
            placeholders.join(", ")
        );
        BuiltQuery {
            sql,
            params: self.params,
        }
    }
}

// ── UPDATE ──────────────────────────────────────────────────────────────────

/// Builder for UPDATE queries.
#[derive(Debug, Clone)]
pub struct UpdateBuilder {
    table: String,
    set_clauses: Vec<String>,
    where_clauses: Vec<String>,
    params: Vec<Value>,
    /// Track how many params belong to SET vs WHERE.
    set_param_count: usize,
}

impl UpdateBuilder {
    pub fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            set_clauses: Vec::new(),
            where_clauses: Vec::new(),
            params: Vec::new(),
            set_param_count: 0,
        }
    }

    /// Add a SET column = value pair.
    pub fn set(mut self, column: &str, value: Value) -> Self {
        let idx = self.set_param_count + 1;
        self.set_clauses.push(format!("{column} = ?{idx}"));
        // Insert SET params at the correct position.
        self.params.insert(self.set_param_count, value);
        self.set_param_count += 1;
        self
    }

    /// Add a WHERE condition.
    pub fn where_clause(mut self, clause: &str, params: Vec<Value>) -> Self {
        self.where_clauses.push(clause.to_string());
        self.params.extend(params);
        self
    }

    /// Build the UPDATE query.
    pub fn build(self) -> BuiltQuery {
        let mut sql = format!("UPDATE {} SET {}", self.table, self.set_clauses.join(", "));

        if !self.where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.where_clauses.join(" AND "));
        }

        BuiltQuery {
            sql,
            params: self.params,
        }
    }
}

// ── DELETE ──────────────────────────────────────────────────────────────────

/// Builder for DELETE queries.
#[derive(Debug, Clone)]
pub struct DeleteBuilder {
    table: String,
    where_clauses: Vec<String>,
    params: Vec<Value>,
}

impl DeleteBuilder {
    pub fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            where_clauses: Vec::new(),
            params: Vec::new(),
        }
    }

    /// Add a WHERE condition.
    pub fn where_clause(mut self, clause: &str, params: Vec<Value>) -> Self {
        self.where_clauses.push(clause.to_string());
        self.params.extend(params);
        self
    }

    /// Build the DELETE query.
    pub fn build(self) -> BuiltQuery {
        let mut sql = format!("DELETE FROM {}", self.table);

        if !self.where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.where_clauses.join(" AND "));
        }

        BuiltQuery {
            sql,
            params: self.params,
        }
    }
}

// ── COUNT helper ────────────────────────────────────────────────────────────

/// Build a COUNT(*) query for a table with optional WHERE clauses.
pub fn count_query(table: &str, where_clauses: &[&str], params: Vec<Value>) -> BuiltQuery {
    let mut sql = format!("SELECT COUNT(*) FROM {table}");
    if !where_clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_clauses.join(" AND "));
    }
    BuiltQuery { sql, params }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_all_columns() {
        let q = SelectBuilder::new("users").build();
        assert_eq!(q.sql, "SELECT * FROM users");
        assert!(q.params.is_empty());
    }

    #[test]
    fn select_specific_columns() {
        let q = SelectBuilder::new("users").columns(&["id", "name"]).build();
        assert_eq!(q.sql, "SELECT id, name FROM users");
    }

    #[test]
    fn select_with_where() {
        let q = SelectBuilder::new("users")
            .where_clause("email = ?1", vec![Value::Text("a@b.com".into())])
            .build();
        assert_eq!(q.sql, "SELECT * FROM users WHERE email = ?1");
        assert_eq!(q.params.len(), 1);
    }

    #[test]
    fn select_with_multiple_where() {
        let q = SelectBuilder::new("users")
            .where_clause("age > ?1", vec![Value::Integer(18)])
            .where_clause("active = ?2", vec![Value::Integer(1)])
            .build();
        assert_eq!(q.sql, "SELECT * FROM users WHERE age > ?1 AND active = ?2");
        assert_eq!(q.params.len(), 2);
    }

    #[test]
    fn select_with_order() {
        let q = SelectBuilder::new("users")
            .order_by("name", SortDirection::Asc)
            .order_by("created", SortDirection::Desc)
            .build();
        assert_eq!(q.sql, "SELECT * FROM users ORDER BY name ASC, created DESC");
    }

    #[test]
    fn select_with_limit_offset() {
        let q = SelectBuilder::new("users").limit(10).offset(20).build();
        assert_eq!(q.sql, "SELECT * FROM users LIMIT 10 OFFSET 20");
    }

    #[test]
    fn select_with_join() {
        let q = SelectBuilder::new("posts")
            .columns(&["posts.id", "users.name"])
            .join("LEFT JOIN users ON posts.user_id = users.id")
            .build();
        assert_eq!(
            q.sql,
            "SELECT posts.id, users.name FROM posts LEFT JOIN users ON posts.user_id = users.id"
        );
    }

    #[test]
    fn select_full_query() {
        let q = SelectBuilder::new("records")
            .columns(&["id", "title"])
            .where_clause("collection = ?1", vec![Value::Text("posts".into())])
            .order_by("created", SortDirection::Desc)
            .limit(20)
            .offset(0)
            .build();
        assert_eq!(
            q.sql,
            "SELECT id, title FROM records WHERE collection = ?1 ORDER BY created DESC LIMIT 20 OFFSET 0"
        );
    }

    #[test]
    fn insert_builds_correctly() {
        let q = InsertBuilder::new("users")
            .set("name", Value::Text("Alice".into()))
            .set("email", Value::Text("alice@example.com".into()))
            .build();
        assert_eq!(q.sql, "INSERT INTO users (name, email) VALUES (?1, ?2)");
        assert_eq!(q.params.len(), 2);
    }

    #[test]
    fn update_builds_correctly() {
        let q = UpdateBuilder::new("users")
            .set("name", Value::Text("Bob".into()))
            .where_clause(&format!("id = ?{}", 2), vec![Value::Text("abc123".into())])
            .build();
        assert_eq!(q.sql, "UPDATE users SET name = ?1 WHERE id = ?2");
        assert_eq!(q.params.len(), 2);
    }

    #[test]
    fn update_multiple_sets() {
        let q = UpdateBuilder::new("users")
            .set("name", Value::Text("Bob".into()))
            .set("email", Value::Text("bob@b.com".into()))
            .build();
        assert_eq!(q.sql, "UPDATE users SET name = ?1, email = ?2");
        assert_eq!(q.params.len(), 2);
    }

    #[test]
    fn delete_builds_correctly() {
        let q = DeleteBuilder::new("users")
            .where_clause("id = ?1", vec![Value::Text("abc".into())])
            .build();
        assert_eq!(q.sql, "DELETE FROM users WHERE id = ?1");
        assert_eq!(q.params.len(), 1);
    }

    #[test]
    fn delete_without_where() {
        let q = DeleteBuilder::new("temp_data").build();
        assert_eq!(q.sql, "DELETE FROM temp_data");
    }

    #[test]
    fn count_query_builds() {
        let q = count_query("users", &["active = ?1"], vec![Value::Integer(1)]);
        assert_eq!(q.sql, "SELECT COUNT(*) FROM users WHERE active = ?1");
        assert_eq!(q.params.len(), 1);
    }

    #[test]
    fn count_query_no_where() {
        let q = count_query("users", &[], vec![]);
        assert_eq!(q.sql, "SELECT COUNT(*) FROM users");
    }
}
