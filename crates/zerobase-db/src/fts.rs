//! Full-Text Search (FTS5) management for collection tables.
//!
//! Each collection with searchable fields gets a companion FTS5 virtual table
//! named `{collection}_fts`. This module handles:
//!
//! - Creating/dropping FTS5 tables when collections are created/modified.
//! - Syncing FTS content via SQLite triggers (INSERT/UPDATE/DELETE).
//! - Building search queries that JOIN the FTS table with the main table.
//!
//! # FTS5 Content-Sync Strategy
//!
//! We use an **external content** FTS5 table (`content='{table}'`) so that
//! the source of truth remains the main collection table. SQLite triggers
//! keep the FTS index in sync automatically.
//!
//! # Search Query Format
//!
//! User search strings are sanitized and converted to FTS5 query syntax.
//! Simple words are joined with implicit AND. Quoted phrases are preserved.
//! Special FTS5 operators in user input are escaped.

use rusqlite::Connection;
use tracing::debug;

use crate::error::{DbError, Result};

/// Name of the FTS5 virtual table for a collection.
pub fn fts_table_name(collection: &str) -> String {
    format!("{}_fts", collection)
}

/// Create the FTS5 virtual table and sync triggers for a collection.
///
/// The FTS table uses `content` and `content_rowid` options to reference the
/// main table, meaning the FTS index doesn't store its own copy of the data.
///
/// # Arguments
///
/// * `conn` - An open SQLite connection (typically within a transaction).
/// * `collection` - The collection/table name.
/// * `searchable_fields` - The field names to index for full-text search.
pub fn create_fts_index(
    conn: &Connection,
    collection: &str,
    searchable_fields: &[&str],
) -> Result<()> {
    if searchable_fields.is_empty() {
        return Ok(());
    }

    let fts_table = fts_table_name(collection);
    let field_list = searchable_fields.join(", ");

    // Create the FTS5 virtual table as an external-content table.
    // We use `content=''` (contentless) with manual sync via triggers,
    // which avoids the need for rowid mapping while keeping the index lean.
    // However, since we need to JOIN with the original table anyway,
    // we use content-sync mode with the original table as content source.
    let create_sql = format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS \"{fts_table}\" USING fts5(\
            {field_list}, \
            content=\"{collection}\", \
            content_rowid=\"rowid\"\
        )"
    );

    debug!(table = %fts_table, fields = %field_list, "Creating FTS5 index");
    conn.execute_batch(&create_sql).map_err(DbError::Query)?;

    // Create triggers to keep FTS in sync with the main table.
    create_sync_triggers(conn, collection, searchable_fields)?;

    // Populate the FTS index from existing data.
    rebuild_fts_index(conn, collection)?;

    Ok(())
}

/// Drop the FTS5 virtual table and its sync triggers.
pub fn drop_fts_index(conn: &Connection, collection: &str) -> Result<()> {
    let fts_table = fts_table_name(collection);

    // Drop triggers first.
    drop_sync_triggers(conn, collection)?;

    let sql = format!("DROP TABLE IF EXISTS \"{fts_table}\"");
    debug!(table = %fts_table, "Dropping FTS5 index");
    conn.execute_batch(&sql).map_err(DbError::Query)?;

    Ok(())
}

/// Rebuild the FTS index from the main table's current data.
///
/// This is useful after bulk imports or schema changes.
pub fn rebuild_fts_index(conn: &Connection, collection: &str) -> Result<()> {
    let fts_table = fts_table_name(collection);
    let sql = format!("INSERT INTO \"{fts_table}\"(\"{fts_table}\") VALUES('rebuild')");
    debug!(table = %fts_table, "Rebuilding FTS5 index");
    conn.execute_batch(&sql).map_err(DbError::Query)?;
    Ok(())
}

/// Create INSERT/UPDATE/DELETE triggers to keep the FTS index in sync.
fn create_sync_triggers(
    conn: &Connection,
    collection: &str,
    searchable_fields: &[&str],
) -> Result<()> {
    let fts_table = fts_table_name(collection);
    let new_fields: Vec<String> = searchable_fields
        .iter()
        .map(|f| format!("new.\"{}\"", f))
        .collect();
    let old_fields: Vec<String> = searchable_fields
        .iter()
        .map(|f| format!("old.\"{}\"", f))
        .collect();
    let field_list = searchable_fields
        .iter()
        .map(|f| format!("\"{}\"", f))
        .collect::<Vec<_>>()
        .join(", ");

    // INSERT trigger.
    let insert_trigger = format!(
        "CREATE TRIGGER IF NOT EXISTS \"{collection}_fts_ai\" AFTER INSERT ON \"{collection}\" BEGIN \
            INSERT INTO \"{fts_table}\"(rowid, {field_list}) VALUES (new.rowid, {new_vals}); \
        END",
        new_vals = new_fields.join(", "),
    );

    // DELETE trigger — FTS5 requires inserting a special delete command.
    let delete_trigger = format!(
        "CREATE TRIGGER IF NOT EXISTS \"{collection}_fts_ad\" AFTER DELETE ON \"{collection}\" BEGIN \
            INSERT INTO \"{fts_table}\"(\"{fts_table}\", rowid, {field_list}) VALUES('delete', old.rowid, {old_vals}); \
        END",
        old_vals = old_fields.join(", "),
    );

    // UPDATE trigger: delete old, insert new.
    let update_trigger = format!(
        "CREATE TRIGGER IF NOT EXISTS \"{collection}_fts_au\" AFTER UPDATE ON \"{collection}\" BEGIN \
            INSERT INTO \"{fts_table}\"(\"{fts_table}\", rowid, {field_list}) VALUES('delete', old.rowid, {old_vals}); \
            INSERT INTO \"{fts_table}\"(rowid, {field_list}) VALUES (new.rowid, {new_vals}); \
        END",
        old_vals = old_fields.join(", "),
        new_vals = new_fields.join(", "),
    );

    conn.execute_batch(&insert_trigger)
        .map_err(DbError::Query)?;
    conn.execute_batch(&delete_trigger)
        .map_err(DbError::Query)?;
    conn.execute_batch(&update_trigger)
        .map_err(DbError::Query)?;

    debug!(collection = %collection, "Created FTS5 sync triggers");
    Ok(())
}

/// Drop the FTS sync triggers for a collection.
fn drop_sync_triggers(conn: &Connection, collection: &str) -> Result<()> {
    let sql = format!(
        "DROP TRIGGER IF EXISTS \"{collection}_fts_ai\"; \
         DROP TRIGGER IF EXISTS \"{collection}_fts_ad\"; \
         DROP TRIGGER IF EXISTS \"{collection}_fts_au\";"
    );
    conn.execute_batch(&sql).map_err(DbError::Query)?;
    Ok(())
}

/// Sanitize a user-provided search query for safe use with FTS5 MATCH.
///
/// FTS5 query syntax supports operators like AND, OR, NOT, NEAR, column
/// filters, etc. To prevent injection of complex query operators, we escape
/// or remove special characters while preserving quoted phrases.
///
/// # Strategy
///
/// 1. Quoted phrases (`"..."`) are preserved as-is (FTS5 phrase queries).
/// 2. Unquoted tokens are stripped of FTS5 special characters and joined
///    with implicit AND (FTS5 default).
/// 3. Empty queries after sanitization return `None`.
pub fn sanitize_search_query(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut result = String::new();
    let mut chars = trimmed.chars().peekable();

    while let Some(&ch) = chars.peek() {
        if ch == '"' {
            // Consume a quoted phrase.
            chars.next(); // skip opening "
            let mut phrase = String::new();
            while let Some(&c) = chars.peek() {
                if c == '"' {
                    chars.next(); // skip closing "
                    break;
                }
                phrase.push(c);
                chars.next();
            }
            if !phrase.trim().is_empty() {
                if !result.is_empty() {
                    result.push(' ');
                }
                result.push('"');
                result.push_str(phrase.trim());
                result.push('"');
            }
        } else if ch.is_alphanumeric() || ch == '_' {
            // Consume a word token.
            let mut word = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_alphanumeric() || c == '_' || c == '\'' {
                    word.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            // Skip FTS5 keywords used as operators.
            let upper = word.to_uppercase();
            if upper == "AND" || upper == "OR" || upper == "NOT" || upper == "NEAR" {
                // Treat as a literal word by quoting it.
                if !result.is_empty() {
                    result.push(' ');
                }
                result.push('"');
                result.push_str(&word);
                result.push('"');
            } else if !word.is_empty() {
                if !result.is_empty() {
                    result.push(' ');
                }
                result.push_str(&word);
            }
        } else {
            // Skip special characters (*, +, -, ^, :, etc.)
            chars.next();
        }
    }

    if result.trim().is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Check if an FTS5 table exists for a collection.
pub fn fts_table_exists(conn: &Connection, collection: &str) -> Result<bool> {
    let fts_table = fts_table_name(collection);
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            rusqlite::params![fts_table],
            |row| row.get(0),
        )
        .map_err(DbError::Query)?;
    Ok(exists)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── sanitize_search_query ──────────────────────────────────────────────

    #[test]
    fn sanitize_empty_returns_none() {
        assert_eq!(sanitize_search_query(""), None);
        assert_eq!(sanitize_search_query("   "), None);
    }

    #[test]
    fn sanitize_simple_words() {
        assert_eq!(
            sanitize_search_query("hello world"),
            Some("hello world".to_string())
        );
    }

    #[test]
    fn sanitize_preserves_quoted_phrases() {
        assert_eq!(
            sanitize_search_query("\"hello world\""),
            Some("\"hello world\"".to_string())
        );
    }

    #[test]
    fn sanitize_mixed_words_and_phrases() {
        assert_eq!(
            sanitize_search_query("rust \"hello world\" programming"),
            Some("rust \"hello world\" programming".to_string())
        );
    }

    #[test]
    fn sanitize_strips_special_characters() {
        assert_eq!(
            sanitize_search_query("hello* +world -test"),
            Some("hello world test".to_string())
        );
    }

    #[test]
    fn sanitize_escapes_fts5_keywords() {
        assert_eq!(
            sanitize_search_query("cats AND dogs"),
            Some("cats \"AND\" dogs".to_string())
        );
        assert_eq!(
            sanitize_search_query("NOT bad"),
            Some("\"NOT\" bad".to_string())
        );
    }

    #[test]
    fn sanitize_only_special_chars_returns_none() {
        assert_eq!(sanitize_search_query("* + - ^"), None);
    }

    #[test]
    fn sanitize_unclosed_quote_captures_phrase() {
        // Unclosed quotes should capture everything after the opening quote.
        let result = sanitize_search_query("\"hello world");
        assert_eq!(result, Some("\"hello world\"".to_string()));
    }

    // ── FTS table operations (requires SQLite) ─────────────────────────────

    #[test]
    fn create_and_query_fts_index() {
        let conn = Connection::open_in_memory().unwrap();

        // Create a test table.
        conn.execute_batch(
            "CREATE TABLE posts (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL DEFAULT '',
                content TEXT NOT NULL DEFAULT '',
                created TEXT NOT NULL DEFAULT (datetime('now')),
                updated TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .unwrap();

        // Create FTS index on title and content.
        create_fts_index(&conn, "posts", &["title", "content"]).unwrap();

        // Verify FTS table exists.
        assert!(fts_table_exists(&conn, "posts").unwrap());

        // Insert data (triggers should sync to FTS).
        conn.execute(
            "INSERT INTO posts (id, title, content) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                "p1",
                "Rust Programming",
                "Learn Rust for systems programming"
            ],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO posts (id, title, content) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                "p2",
                "Python Basics",
                "Getting started with Python scripting"
            ],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO posts (id, title, content) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                "p3",
                "Advanced Rust",
                "Rust ownership and borrowing explained"
            ],
        )
        .unwrap();

        // Search for "rust".
        let mut stmt = conn
            .prepare(
                "SELECT p.id, p.title FROM posts p \
                 INNER JOIN posts_fts ON posts_fts.rowid = p.rowid \
                 WHERE posts_fts MATCH 'rust' \
                 ORDER BY rank",
            )
            .unwrap();

        let results: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(results.len(), 2);
        // Should find both "Rust Programming" and "Advanced Rust".
        let ids: Vec<&str> = results.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&"p1"));
        assert!(ids.contains(&"p3"));
    }

    #[test]
    fn fts_triggers_sync_on_update_and_delete() {
        let conn = Connection::open_in_memory().unwrap();

        conn.execute_batch(
            "CREATE TABLE articles (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL DEFAULT '',
                body TEXT NOT NULL DEFAULT ''
            )",
        )
        .unwrap();

        create_fts_index(&conn, "articles", &["title", "body"]).unwrap();

        // Insert.
        conn.execute(
            "INSERT INTO articles (id, title, body) VALUES ('a1', 'Original Title', 'Original body')",
            [],
        )
        .unwrap();

        // Verify original is searchable.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM articles_fts WHERE articles_fts MATCH 'Original'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Update title.
        conn.execute(
            "UPDATE articles SET title = 'Changed Title' WHERE id = 'a1'",
            [],
        )
        .unwrap();

        // "Original" should still match (body still has it).
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM articles_fts WHERE articles_fts MATCH 'Changed'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Delete.
        conn.execute("DELETE FROM articles WHERE id = 'a1'", [])
            .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM articles_fts WHERE articles_fts MATCH 'Changed'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn drop_fts_index_removes_everything() {
        let conn = Connection::open_in_memory().unwrap();

        conn.execute_batch(
            "CREATE TABLE items (id TEXT PRIMARY KEY, name TEXT NOT NULL DEFAULT '')",
        )
        .unwrap();

        create_fts_index(&conn, "items", &["name"]).unwrap();
        assert!(fts_table_exists(&conn, "items").unwrap());

        drop_fts_index(&conn, "items").unwrap();
        assert!(!fts_table_exists(&conn, "items").unwrap());

        // Triggers should also be gone — inserting should not fail.
        conn.execute("INSERT INTO items (id, name) VALUES ('i1', 'test')", [])
            .unwrap();
    }

    #[test]
    fn create_fts_index_with_empty_fields_is_noop() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE empty_tbl (id TEXT PRIMARY KEY)")
            .unwrap();

        // Should not error with empty field list.
        create_fts_index(&conn, "empty_tbl", &[]).unwrap();
        assert!(!fts_table_exists(&conn, "empty_tbl").unwrap());
    }

    #[test]
    fn fts_relevance_ranking() {
        let conn = Connection::open_in_memory().unwrap();

        conn.execute_batch(
            "CREATE TABLE docs (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL DEFAULT '',
                body TEXT NOT NULL DEFAULT ''
            )",
        )
        .unwrap();

        create_fts_index(&conn, "docs", &["title", "body"]).unwrap();

        // Insert documents with varying relevance to "database".
        conn.execute(
            "INSERT INTO docs (id, title, body) VALUES ('d1', 'Database Design', 'How to design a database schema')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO docs (id, title, body) VALUES ('d2', 'Web Development', 'Build web apps with a database backend')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO docs (id, title, body) VALUES ('d3', 'Cooking Recipes', 'No mention of tech here')",
            [],
        )
        .unwrap();

        // Search for "database" with ranking.
        let mut stmt = conn
            .prepare(
                "SELECT d.id, rank FROM docs d \
                 INNER JOIN docs_fts ON docs_fts.rowid = d.rowid \
                 WHERE docs_fts MATCH 'database' \
                 ORDER BY rank",
            )
            .unwrap();

        let results: Vec<(String, f64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        // Should find d1 and d2, not d3.
        assert_eq!(results.len(), 2);
        let ids: Vec<&str> = results.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&"d1"));
        assert!(ids.contains(&"d2"));

        // d1 should rank higher (title + body match) than d2 (body only).
        // FTS5 rank is negative; more negative = better match.
        let d1_rank = results.iter().find(|(id, _)| id == "d1").unwrap().1;
        let d2_rank = results.iter().find(|(id, _)| id == "d2").unwrap().1;
        assert!(
            d1_rank < d2_rank,
            "d1 ({d1_rank}) should rank better (more negative) than d2 ({d2_rank})"
        );
    }
}
