//! Record repository — SQLite implementation.
//!
//! Implements [`RecordRepository`] on [`Database`] for CRUD operations on
//! records within dynamic collection tables.

use std::collections::HashMap;

use rusqlite::types::Value as SqlValue;
use serde_json::Value as JsonValue;

use zerobase_core::services::record_service::{
    RecordList, RecordQuery, RecordRepoError, RecordRepository, SortDirection,
};

use crate::error::map_query_error;
use crate::filter;
use crate::fts;
use crate::pool::Database;

impl RecordRepository for Database {
    fn find_one(
        &self,
        collection: &str,
        id: &str,
    ) -> std::result::Result<HashMap<String, JsonValue>, RecordRepoError> {
        let conn = self.read_conn().map_err(db_err_to_repo)?;

        let sql = format!(
            "SELECT * FROM \"{}\" WHERE id = ?1",
            sanitize_table_name(collection)?
        );

        let mut stmt = conn.prepare(&sql).map_err(sqlite_err_to_repo)?;

        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

        stmt.query_row(rusqlite::params![id], |row| {
            let mut record = HashMap::new();
            for (i, col_name) in column_names.iter().enumerate() {
                let value = sqlite_value_to_json(row, i);
                record.insert(col_name.clone(), value);
            }
            Ok(record)
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => RecordRepoError::NotFound {
                resource_type: "Record".to_string(),
                resource_id: Some(id.to_string()),
            },
            other => sqlite_err_to_repo(other),
        })
    }

    fn find_many(
        &self,
        collection: &str,
        query: &RecordQuery,
    ) -> std::result::Result<RecordList, RecordRepoError> {
        let conn = self.read_conn().map_err(db_err_to_repo)?;

        let table = sanitize_table_name(collection)?;
        let page = query.page.max(1);
        let per_page = query.per_page.max(1).min(500);

        // Parse filter if present.
        let filter_result = match &query.filter {
            Some(f) => {
                filter::parse_and_generate_sql(f).map_err(|e| RecordRepoError::Database {
                    message: format!("invalid filter: {e}"),
                })?
            }
            None => None,
        };

        // Parse and sanitize search query.
        let sanitized_search = query.search.as_deref().and_then(fts::sanitize_search_query);

        // Determine if FTS join is needed (search + FTS table exists).
        let fts_table = fts::fts_table_name(collection);
        let use_fts =
            sanitized_search.is_some() && fts::fts_table_exists(&conn, collection).unwrap_or(false);

        // Build FROM clause: with or without FTS JOIN.
        let from_clause = if use_fts {
            format!(
                "\"{table}\" INNER JOIN \"{fts_table}\" ON \"{fts_table}\".rowid = \"{table}\".rowid"
            )
        } else {
            format!("\"{table}\"")
        };

        // Build WHERE conditions.
        let mut where_parts: Vec<String> = Vec::new();
        let mut all_params: Vec<rusqlite::types::Value> = Vec::new();

        if let Some(ref built) = filter_result {
            where_parts.push(built.sql.clone());
            all_params.extend(built.params.iter().cloned());
        }

        if use_fts {
            let search_str = sanitized_search.as_deref().unwrap();
            let param_idx = all_params.len() + 1;
            where_parts.push(format!("\"{fts_table}\" MATCH ?{param_idx}"));
            all_params.push(rusqlite::types::Value::Text(search_str.to_string()));
        }

        let where_clause = if where_parts.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", where_parts.join(" AND "))
        };

        // Count total records matching the filter + search.
        let count_sql = format!("SELECT COUNT(*) FROM {from_clause}{where_clause}");
        let total_items: u64 = conn
            .query_row(
                &count_sql,
                rusqlite::params_from_iter(all_params.iter()),
                |row| row.get(0),
            )
            .map_err(sqlite_err_to_repo)?;

        let total_pages = if total_items == 0 {
            1
        } else {
            ((total_items as f64) / (per_page as f64)).ceil() as u32
        };

        // Build the data query.
        let offset = ((page - 1) * per_page) as u64;

        let order_clause = if !query.sort.is_empty() {
            let parts: Vec<String> = query
                .sort
                .iter()
                .map(|(col, dir)| {
                    let d = match dir {
                        SortDirection::Asc => "ASC",
                        SortDirection::Desc => "DESC",
                    };
                    format!("\"{table}\".\"{col}\" {d}")
                })
                .collect();
            format!(" ORDER BY {}", parts.join(", "))
        } else if use_fts {
            // When searching without explicit sort, rank by relevance.
            " ORDER BY rank".to_string()
        } else {
            format!(" ORDER BY \"{table}\".\"created\" DESC")
        };

        let data_sql = format!(
            "SELECT \"{table}\".* FROM {from_clause}{where_clause}{order_clause} LIMIT {per_page} OFFSET {offset}"
        );

        let mut stmt = conn.prepare(&data_sql).map_err(sqlite_err_to_repo)?;

        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

        let items: Vec<HashMap<String, JsonValue>> = stmt
            .query_map(rusqlite::params_from_iter(all_params.iter()), |row| {
                let mut record = HashMap::new();
                for (i, col_name) in column_names.iter().enumerate() {
                    let value = sqlite_value_to_json(row, i);
                    record.insert(col_name.clone(), value);
                }
                Ok(record)
            })
            .map_err(sqlite_err_to_repo)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_err_to_repo)?;

        Ok(RecordList {
            items,
            total_items,
            page,
            per_page,
            total_pages,
        })
    }

    fn insert(
        &self,
        collection: &str,
        data: &HashMap<String, JsonValue>,
    ) -> std::result::Result<(), RecordRepoError> {
        let table = sanitize_table_name(collection)?;

        self.with_write_conn(|conn| {
            let mut columns: Vec<String> = Vec::new();
            let mut placeholders: Vec<String> = Vec::new();
            let mut params: Vec<SqlValue> = Vec::new();

            for (i, (col, val)) in data.iter().enumerate() {
                columns.push(format!("\"{}\"", col));
                placeholders.push(format!("?{}", i + 1));
                params.push(json_to_sqlite_value(val));
            }

            let sql = format!(
                "INSERT INTO \"{}\" ({}) VALUES ({})",
                table,
                columns.join(", "),
                placeholders.join(", ")
            );

            conn.execute(&sql, rusqlite::params_from_iter(params.iter()))
                .map_err(map_query_error)?;

            Ok(())
        })
        .map_err(db_err_to_repo)
    }

    fn update(
        &self,
        collection: &str,
        id: &str,
        data: &HashMap<String, JsonValue>,
    ) -> std::result::Result<bool, RecordRepoError> {
        let table = sanitize_table_name(collection)?;

        self.with_write_conn(|conn| {
            let mut set_parts: Vec<String> = Vec::new();
            let mut params: Vec<SqlValue> = Vec::new();
            let mut idx = 1usize;

            for (col, val) in data {
                if col == "id" {
                    continue;
                }
                set_parts.push(format!("\"{}\" = ?{}", col, idx));
                params.push(json_to_sqlite_value(val));
                idx += 1;
            }

            if set_parts.is_empty() {
                return Ok(false);
            }

            params.push(SqlValue::Text(id.to_string()));
            let sql = format!(
                "UPDATE \"{}\" SET {} WHERE id = ?{}",
                table,
                set_parts.join(", "),
                idx
            );

            let rows_affected = conn
                .execute(&sql, rusqlite::params_from_iter(params.iter()))
                .map_err(map_query_error)?;

            Ok(rows_affected > 0)
        })
        .map_err(db_err_to_repo)
    }

    fn delete(&self, collection: &str, id: &str) -> std::result::Result<bool, RecordRepoError> {
        let table = sanitize_table_name(collection)?;

        self.with_write_conn(|conn| {
            let sql = format!("DELETE FROM \"{}\" WHERE id = ?1", table);
            let rows_affected = conn
                .execute(&sql, rusqlite::params![id])
                .map_err(crate::error::DbError::Query)?;
            Ok(rows_affected > 0)
        })
        .map_err(db_err_to_repo)
    }

    fn count(
        &self,
        collection: &str,
        filter_str: Option<&str>,
    ) -> std::result::Result<u64, RecordRepoError> {
        let conn = self.read_conn().map_err(db_err_to_repo)?;

        let table = sanitize_table_name(collection)?;

        let filter_result = match filter_str {
            Some(f) => {
                filter::parse_and_generate_sql(f).map_err(|e| RecordRepoError::Database {
                    message: format!("invalid filter: {e}"),
                })?
            }
            None => None,
        };

        let (where_clause, filter_params) = match &filter_result {
            Some(built) => (format!(" WHERE {}", built.sql), &built.params),
            None => (String::new(), &Vec::new() as &Vec<rusqlite::types::Value>),
        };

        let sql = format!("SELECT COUNT(*) FROM \"{}\"{}", table, where_clause);

        conn.query_row(
            &sql,
            rusqlite::params_from_iter(filter_params.iter()),
            |row| row.get(0),
        )
        .map_err(sqlite_err_to_repo)
    }

    fn find_referencing_records(
        &self,
        collection: &str,
        field_name: &str,
        referenced_id: &str,
    ) -> std::result::Result<Vec<HashMap<String, JsonValue>>, RecordRepoError> {
        let conn = self.read_conn().map_err(db_err_to_repo)?;
        let table = sanitize_table_name(collection)?;
        let col = sanitize_table_name(field_name)?;

        // For relation fields, the value is stored as either:
        // - A plain string ID (single relation)
        // - A JSON array of string IDs (multi-relation)
        // We match both cases: exact equality for single relations,
        // and JSON array containment for multi-relations.
        let sql = format!(
            "SELECT * FROM \"{}\" WHERE \"{}\" = ?1 \
             OR (json_valid(\"{}\") AND EXISTS( \
                 SELECT 1 FROM json_each(\"{}\") WHERE json_each.value = ?1 \
             ))",
            table, col, col, col
        );

        let mut stmt = conn.prepare(&sql).map_err(sqlite_err_to_repo)?;
        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

        let rows = stmt
            .query_map(rusqlite::params![referenced_id], |row| {
                let mut record = HashMap::new();
                for (i, col_name) in column_names.iter().enumerate() {
                    let value = sqlite_value_to_json(row, i);
                    record.insert(col_name.clone(), value);
                }
                Ok(record)
            })
            .map_err(sqlite_err_to_repo)?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(sqlite_err_to_repo)?);
        }
        Ok(results)
    }

    fn find_referencing_records_limited(
        &self,
        collection: &str,
        field_name: &str,
        referenced_id: &str,
        limit: usize,
    ) -> std::result::Result<Vec<HashMap<String, JsonValue>>, RecordRepoError> {
        let conn = self.read_conn().map_err(db_err_to_repo)?;
        let table = sanitize_table_name(collection)?;
        let col = sanitize_table_name(field_name)?;

        // Same logic as find_referencing_records but with SQL LIMIT for efficiency.
        let sql = format!(
            "SELECT * FROM \"{}\" WHERE \"{}\" = ?1 \
             OR (json_valid(\"{}\") AND EXISTS( \
                 SELECT 1 FROM json_each(\"{}\") WHERE json_each.value = ?1 \
             )) LIMIT ?2",
            table, col, col, col
        );

        let mut stmt = conn.prepare(&sql).map_err(sqlite_err_to_repo)?;
        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

        let rows = stmt
            .query_map(rusqlite::params![referenced_id, limit as i64], |row| {
                let mut record = HashMap::new();
                for (i, col_name) in column_names.iter().enumerate() {
                    let value = sqlite_value_to_json(row, i);
                    record.insert(col_name.clone(), value);
                }
                Ok(record)
            })
            .map_err(sqlite_err_to_repo)?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(sqlite_err_to_repo)?);
        }
        Ok(results)
    }
}

// ── Error conversion helpers ──────────────────────────────────────────────────

/// Convert a `DbError` into a `RecordRepoError`.
fn db_err_to_repo(err: impl std::fmt::Display) -> RecordRepoError {
    let msg = err.to_string();
    if msg.contains("unique") || msg.contains("conflict") {
        RecordRepoError::Conflict { message: msg }
    } else {
        RecordRepoError::Database { message: msg }
    }
}

/// Convert a `rusqlite::Error` into a `RecordRepoError`.
fn sqlite_err_to_repo(err: rusqlite::Error) -> RecordRepoError {
    RecordRepoError::Database {
        message: err.to_string(),
    }
}

// ── Value conversion helpers ──────────────────────────────────────────────────

/// Sanitize a table name for safe inclusion in double-quoted SQL identifiers.
///
/// Collection names are validated at creation time (only `[a-zA-Z0-9_]`),
/// but as defense-in-depth we reject any name that contains characters which
/// could break out of a double-quoted identifier — specifically double quotes,
/// semicolons, null bytes, and backslashes.
///
/// Returns an error instead of panicking so that a single malformed name
/// cannot crash the entire server.
fn sanitize_table_name(name: &str) -> std::result::Result<&str, RecordRepoError> {
    if name.is_empty() {
        return Err(RecordRepoError::Database {
            message: "table name must not be empty".to_string(),
        });
    }
    if name.contains('"')
        || name.contains(';')
        || name.contains('\0')
        || name.contains('\\')
        || name.contains('\n')
        || name.contains('\r')
    {
        return Err(RecordRepoError::Database {
            message: format!("table name contains forbidden character: {name:?}"),
        });
    }
    Ok(name)
}

/// Convert a SQLite row value at the given index to a JSON value.
fn sqlite_value_to_json(row: &rusqlite::Row, idx: usize) -> JsonValue {
    // Try string first (most common type in our schema).
    if let Ok(v) = row.get::<_, Option<String>>(idx) {
        match v {
            Some(s) => {
                // Try parsing as JSON for stored JSON/array fields.
                if (s.starts_with('{') && s.ends_with('}'))
                    || (s.starts_with('[') && s.ends_with(']'))
                {
                    serde_json::from_str(&s).unwrap_or(JsonValue::String(s))
                } else {
                    JsonValue::String(s)
                }
            }
            None => JsonValue::Null,
        }
    } else if let Ok(v) = row.get::<_, Option<i64>>(idx) {
        match v {
            Some(n) => JsonValue::Number(n.into()),
            None => JsonValue::Null,
        }
    } else if let Ok(v) = row.get::<_, Option<f64>>(idx) {
        match v {
            Some(n) => serde_json::Number::from_f64(n)
                .map(JsonValue::Number)
                .unwrap_or(JsonValue::Null),
            None => JsonValue::Null,
        }
    } else {
        JsonValue::Null
    }
}

/// Convert a JSON value to a SQLite-compatible value for parameterized queries.
fn json_to_sqlite_value(json: &JsonValue) -> SqlValue {
    match json {
        JsonValue::Null => SqlValue::Null,
        JsonValue::Bool(b) => SqlValue::Integer(if *b { 1 } else { 0 }),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                SqlValue::Real(f)
            } else {
                SqlValue::Text(n.to_string())
            }
        }
        JsonValue::String(s) => SqlValue::Text(s.clone()),
        JsonValue::Array(_) | JsonValue::Object(_) => SqlValue::Text(json.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Unit tests for value conversion ──────────────────────────────────

    #[test]
    fn json_null_converts_to_sql_null() {
        assert!(matches!(
            json_to_sqlite_value(&JsonValue::Null),
            SqlValue::Null
        ));
    }

    #[test]
    fn json_bool_converts_to_sql_integer() {
        assert!(matches!(
            json_to_sqlite_value(&JsonValue::Bool(true)),
            SqlValue::Integer(1)
        ));
        assert!(matches!(
            json_to_sqlite_value(&JsonValue::Bool(false)),
            SqlValue::Integer(0)
        ));
    }

    #[test]
    fn json_integer_converts_to_sql_integer() {
        let val = serde_json::json!(42);
        assert!(matches!(json_to_sqlite_value(&val), SqlValue::Integer(42)));
    }

    #[test]
    fn json_float_converts_to_sql_real() {
        let val = serde_json::json!(3.14);
        if let SqlValue::Real(f) = json_to_sqlite_value(&val) {
            assert!((f - 3.14).abs() < f64::EPSILON);
        } else {
            panic!("expected Real");
        }
    }

    #[test]
    fn json_string_converts_to_sql_text() {
        let val = serde_json::json!("hello");
        if let SqlValue::Text(s) = json_to_sqlite_value(&val) {
            assert_eq!(s, "hello");
        } else {
            panic!("expected Text");
        }
    }

    #[test]
    fn json_object_converts_to_sql_text_json() {
        let val = serde_json::json!({"key": "value"});
        if let SqlValue::Text(s) = json_to_sqlite_value(&val) {
            let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
            assert_eq!(parsed["key"], "value");
        } else {
            panic!("expected Text");
        }
    }

    // ── Integration tests against real SQLite ────────────────────────────

    use crate::pool::PoolConfig;
    use crate::{CollectionSchema, ColumnDef, SchemaRepository};
    use zerobase_core::services::record_service::RecordRepository;

    /// Create an in-memory database with system tables, then create a
    /// "posts" collection with title (TEXT NOT NULL) and views (REAL).
    fn setup_db_with_posts() -> Database {
        let db = Database::open_in_memory(&PoolConfig::default()).unwrap();
        db.run_system_migrations().unwrap();

        let schema = CollectionSchema {
            name: "posts".to_string(),
            collection_type: "base".to_string(),
            columns: vec![
                ColumnDef {
                    name: "title".to_string(),
                    sql_type: "TEXT".to_string(),
                    not_null: true,
                    default: None,
                    unique: false,
                },
                ColumnDef {
                    name: "views".to_string(),
                    sql_type: "REAL".to_string(),
                    not_null: false,
                    default: Some("0".to_string()),
                    unique: false,
                },
            ],
            indexes: vec![],
            searchable_fields: vec![],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();
        db
    }

    /// Helper to build a record HashMap for insertion.
    fn record_data(pairs: &[(&str, JsonValue)]) -> HashMap<String, JsonValue> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn now_iso() -> String {
        "2025-01-01 00:00:00.000Z".to_string()
    }

    // ── insert + find_one ────────────────────────────────────────────────

    #[test]
    fn insert_and_find_one_roundtrips() {
        let db = setup_db_with_posts();
        let ts = now_iso();
        let data = record_data(&[
            ("id", JsonValue::String("rec00000000001".into())),
            ("title", JsonValue::String("Hello World".into())),
            ("views", serde_json::json!(42)),
            ("created", JsonValue::String(ts.clone())),
            ("updated", JsonValue::String(ts.clone())),
        ]);

        db.insert("posts", &data).unwrap();

        let found = db.find_one("posts", "rec00000000001").unwrap();
        assert_eq!(found["id"], "rec00000000001");
        assert_eq!(found["title"], "Hello World");
    }

    #[test]
    fn find_one_returns_not_found_for_missing_record() {
        let db = setup_db_with_posts();
        let err = db.find_one("posts", "nonexistent00001").unwrap_err();
        assert!(matches!(err, RecordRepoError::NotFound { .. }));
    }

    // ── find_many with pagination ────────────────────────────────────────

    #[test]
    fn find_many_paginates_correctly() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        for i in 0..5 {
            let data = record_data(&[
                ("id", JsonValue::String(format!("rec{i:011}a"))),
                ("title", JsonValue::String(format!("Post {i}"))),
                ("views", serde_json::json!(i)),
                ("created", JsonValue::String(ts.clone())),
                ("updated", JsonValue::String(ts.clone())),
            ]);
            db.insert("posts", &data).unwrap();
        }

        let query = RecordQuery {
            page: 1,
            per_page: 2,
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();

        assert_eq!(result.items.len(), 2);
        assert_eq!(result.total_items, 5);
        assert_eq!(result.total_pages, 3);
        assert_eq!(result.page, 1);
        assert_eq!(result.per_page, 2);
    }

    #[test]
    fn find_many_returns_empty_for_empty_table() {
        let db = setup_db_with_posts();
        let query = RecordQuery {
            page: 1,
            per_page: 20,
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();
        assert!(result.items.is_empty());
        assert_eq!(result.total_items, 0);
        assert_eq!(result.total_pages, 1);
    }

    #[test]
    fn find_many_sorts_by_column() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        for (i, title) in ["Charlie", "Alice", "Bob"].iter().enumerate() {
            let data = record_data(&[
                ("id", JsonValue::String(format!("rec{i:011}b"))),
                ("title", JsonValue::String(title.to_string())),
                ("created", JsonValue::String(ts.clone())),
                ("updated", JsonValue::String(ts.clone())),
            ]);
            db.insert("posts", &data).unwrap();
        }

        let query = RecordQuery {
            page: 1,
            per_page: 10,
            sort: vec![("title".to_string(), SortDirection::Asc)],
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();
        let titles: Vec<&str> = result
            .items
            .iter()
            .map(|r| r["title"].as_str().unwrap())
            .collect();
        assert_eq!(titles, vec!["Alice", "Bob", "Charlie"]);
    }

    #[test]
    fn find_many_sorts_descending() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        for (i, title) in ["Alice", "Bob", "Charlie"].iter().enumerate() {
            let data = record_data(&[
                ("id", JsonValue::String(format!("dsc{i:011}b"))),
                ("title", JsonValue::String(title.to_string())),
                ("created", JsonValue::String(ts.clone())),
                ("updated", JsonValue::String(ts.clone())),
            ]);
            db.insert("posts", &data).unwrap();
        }

        let query = RecordQuery {
            page: 1,
            per_page: 10,
            sort: vec![("title".to_string(), SortDirection::Desc)],
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();
        let titles: Vec<&str> = result
            .items
            .iter()
            .map(|r| r["title"].as_str().unwrap())
            .collect();
        assert_eq!(titles, vec!["Charlie", "Bob", "Alice"]);
    }

    #[test]
    fn find_many_multi_field_sort() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        // Insert records with same views but different titles.
        let entries = [
            ("mfs00000000001", "Banana", 10),
            ("mfs00000000002", "Apple", 10),
            ("mfs00000000003", "Cherry", 5),
            ("mfs00000000004", "Date", 5),
        ];
        for (id, title, views) in entries {
            let data = record_data(&[
                ("id", JsonValue::String(id.to_string())),
                ("title", JsonValue::String(title.to_string())),
                ("views", serde_json::json!(views)),
                ("created", JsonValue::String(ts.clone())),
                ("updated", JsonValue::String(ts.clone())),
            ]);
            db.insert("posts", &data).unwrap();
        }

        // Sort by views ASC first, then title ASC.
        let query = RecordQuery {
            page: 1,
            per_page: 10,
            sort: vec![
                ("views".to_string(), SortDirection::Asc),
                ("title".to_string(), SortDirection::Asc),
            ],
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();
        let titles: Vec<&str> = result
            .items
            .iter()
            .map(|r| r["title"].as_str().unwrap())
            .collect();
        // views=5 first (Cherry, Date), then views=10 (Apple, Banana)
        assert_eq!(titles, vec!["Cherry", "Date", "Apple", "Banana"]);
    }

    #[test]
    fn find_many_default_sort_is_created_desc() {
        let db = setup_db_with_posts();

        // Insert with different created timestamps.
        let entries = [
            ("dft00000000001", "First", "2025-01-01 00:00:00.000Z"),
            ("dft00000000002", "Second", "2025-01-02 00:00:00.000Z"),
            ("dft00000000003", "Third", "2025-01-03 00:00:00.000Z"),
        ];
        for (id, title, created) in entries {
            let data = record_data(&[
                ("id", JsonValue::String(id.to_string())),
                ("title", JsonValue::String(title.to_string())),
                ("created", JsonValue::String(created.to_string())),
                ("updated", JsonValue::String(created.to_string())),
            ]);
            db.insert("posts", &data).unwrap();
        }

        // No sort specified — should default to created DESC.
        let query = RecordQuery {
            page: 1,
            per_page: 10,
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();
        let titles: Vec<&str> = result
            .items
            .iter()
            .map(|r| r["title"].as_str().unwrap())
            .collect();
        assert_eq!(titles, vec!["Third", "Second", "First"]);
    }

    // ── update ───────────────────────────────────────────────────────────

    #[test]
    fn update_modifies_existing_record() {
        let db = setup_db_with_posts();
        let ts = now_iso();
        let data = record_data(&[
            ("id", JsonValue::String("upd00000000001".into())),
            ("title", JsonValue::String("Original".into())),
            ("views", serde_json::json!(0)),
            ("created", JsonValue::String(ts.clone())),
            ("updated", JsonValue::String(ts.clone())),
        ]);
        db.insert("posts", &data).unwrap();

        let update = record_data(&[
            ("title", JsonValue::String("Updated".into())),
            ("views", serde_json::json!(99)),
        ]);
        let updated = db.update("posts", "upd00000000001", &update).unwrap();
        assert!(updated);

        let found = db.find_one("posts", "upd00000000001").unwrap();
        assert_eq!(found["title"], "Updated");
    }

    #[test]
    fn update_returns_false_for_missing_record() {
        let db = setup_db_with_posts();
        let update = record_data(&[("title", JsonValue::String("Nope".into()))]);
        let updated = db.update("posts", "missing000000001", &update).unwrap();
        assert!(!updated);
    }

    // ── delete ───────────────────────────────────────────────────────────

    #[test]
    fn delete_removes_record() {
        let db = setup_db_with_posts();
        let ts = now_iso();
        let data = record_data(&[
            ("id", JsonValue::String("del00000000001".into())),
            ("title", JsonValue::String("To Delete".into())),
            ("created", JsonValue::String(ts.clone())),
            ("updated", JsonValue::String(ts.clone())),
        ]);
        db.insert("posts", &data).unwrap();

        let deleted = db.delete("posts", "del00000000001").unwrap();
        assert!(deleted);

        let err = db.find_one("posts", "del00000000001").unwrap_err();
        assert!(matches!(err, RecordRepoError::NotFound { .. }));
    }

    #[test]
    fn delete_returns_false_for_missing_record() {
        let db = setup_db_with_posts();
        let deleted = db.delete("posts", "missing000000001").unwrap();
        assert!(!deleted);
    }

    // ── count ────────────────────────────────────────────────────────────

    #[test]
    fn count_returns_correct_total() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        assert_eq!(db.count("posts", None).unwrap(), 0);

        for i in 0..3 {
            let data = record_data(&[
                ("id", JsonValue::String(format!("cnt{i:011}c"))),
                ("title", JsonValue::String(format!("P{i}"))),
                ("created", JsonValue::String(ts.clone())),
                ("updated", JsonValue::String(ts.clone())),
            ]);
            db.insert("posts", &data).unwrap();
        }

        assert_eq!(db.count("posts", None).unwrap(), 3);
    }

    // ── full CRUD lifecycle ──────────────────────────────────────────────

    #[test]
    fn full_crud_lifecycle() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        // Create
        let data = record_data(&[
            ("id", JsonValue::String("lifecycle000001".into())),
            ("title", JsonValue::String("First".into())),
            ("views", serde_json::json!(0)),
            ("created", JsonValue::String(ts.clone())),
            ("updated", JsonValue::String(ts.clone())),
        ]);
        db.insert("posts", &data).unwrap();

        // Read
        let record = db.find_one("posts", "lifecycle000001").unwrap();
        assert_eq!(record["title"], "First");

        // Update
        let update = record_data(&[
            ("title", JsonValue::String("Second".into())),
            ("views", serde_json::json!(10)),
        ]);
        assert!(db.update("posts", "lifecycle000001", &update).unwrap());
        let record = db.find_one("posts", "lifecycle000001").unwrap();
        assert_eq!(record["title"], "Second");

        // List
        let list = db
            .find_many(
                "posts",
                &RecordQuery {
                    page: 1,
                    per_page: 10,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(list.total_items, 1);
        assert_eq!(list.items[0]["title"], "Second");

        // Delete
        assert!(db.delete("posts", "lifecycle000001").unwrap());
        assert_eq!(db.count("posts", None).unwrap(), 0);
    }

    // ── JSON object values roundtrip ─────────────────────────────────────

    #[test]
    fn json_object_field_roundtrips_through_sqlite() {
        let db = Database::open_in_memory(&PoolConfig::default()).unwrap();
        db.run_system_migrations().unwrap();

        let schema = CollectionSchema {
            name: "meta".to_string(),
            collection_type: "base".to_string(),
            columns: vec![ColumnDef {
                name: "payload".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: false,
                default: None,
                unique: false,
            }],
            indexes: vec![],
            searchable_fields: vec![],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();

        let ts = now_iso();
        let payload = serde_json::json!({"nested": {"key": "value"}, "arr": [1, 2, 3]});
        let data = record_data(&[
            ("id", JsonValue::String("json0000000001".into())),
            ("payload", payload.clone()),
            ("created", JsonValue::String(ts.clone())),
            ("updated", JsonValue::String(ts.clone())),
        ]);
        db.insert("meta", &data).unwrap();

        let found = db.find_one("meta", "json0000000001").unwrap();
        assert_eq!(found["payload"], payload);
    }

    // ── timestamps auto-managed via RecordService ────────────────────────

    // ── Pagination edge cases ─────────────────────────────────────────

    #[test]
    fn find_many_page_beyond_last_returns_empty_items() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        for i in 0..3 {
            let data = record_data(&[
                ("id", JsonValue::String(format!("pge{i:011}a"))),
                ("title", JsonValue::String(format!("Post {i}"))),
                ("created", JsonValue::String(ts.clone())),
                ("updated", JsonValue::String(ts.clone())),
            ]);
            db.insert("posts", &data).unwrap();
        }

        // Page 5 of 3 items with per_page=2 → beyond last page
        let query = RecordQuery {
            page: 5,
            per_page: 2,
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();

        assert!(result.items.is_empty());
        assert_eq!(result.total_items, 3);
        assert_eq!(result.total_pages, 2);
        assert_eq!(result.page, 5);
    }

    #[test]
    fn find_many_last_page_partial_results() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        for i in 0..5 {
            let data = record_data(&[
                ("id", JsonValue::String(format!("lp{i:012}a"))),
                ("title", JsonValue::String(format!("Post {i}"))),
                ("created", JsonValue::String(ts.clone())),
                ("updated", JsonValue::String(ts.clone())),
            ]);
            db.insert("posts", &data).unwrap();
        }

        // 5 items, per_page=3 → page 2 should have 2 items
        let query = RecordQuery {
            page: 2,
            per_page: 3,
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();

        assert_eq!(result.items.len(), 2);
        assert_eq!(result.total_items, 5);
        assert_eq!(result.total_pages, 2);
        assert_eq!(result.page, 2);
        assert_eq!(result.per_page, 3);
    }

    #[test]
    fn find_many_page_zero_clamps_to_one() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        let data = record_data(&[
            ("id", JsonValue::String("pz0000000000a".into())),
            ("title", JsonValue::String("Only Post".into())),
            ("created", JsonValue::String(ts.clone())),
            ("updated", JsonValue::String(ts.clone())),
        ]);
        db.insert("posts", &data).unwrap();

        let query = RecordQuery {
            page: 0,
            per_page: 10,
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();

        assert_eq!(result.page, 1);
        assert_eq!(result.items.len(), 1);
    }

    #[test]
    fn find_many_per_page_zero_clamps_to_one() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        let data = record_data(&[
            ("id", JsonValue::String("ppz000000000a".into())),
            ("title", JsonValue::String("Only Post".into())),
            ("created", JsonValue::String(ts.clone())),
            ("updated", JsonValue::String(ts.clone())),
        ]);
        db.insert("posts", &data).unwrap();

        let query = RecordQuery {
            page: 1,
            per_page: 0,
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();

        assert_eq!(result.per_page, 1);
        assert_eq!(result.items.len(), 1);
    }

    #[test]
    fn find_many_per_page_clamped_to_500() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        let data = record_data(&[
            ("id", JsonValue::String("clm000000000a".into())),
            ("title", JsonValue::String("Only Post".into())),
            ("created", JsonValue::String(ts.clone())),
            ("updated", JsonValue::String(ts.clone())),
        ]);
        db.insert("posts", &data).unwrap();

        let query = RecordQuery {
            page: 1,
            per_page: 9999,
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();

        assert_eq!(result.per_page, 500);
    }

    #[test]
    fn find_many_single_item_single_page() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        let data = record_data(&[
            ("id", JsonValue::String("sgl000000000a".into())),
            ("title", JsonValue::String("Solo".into())),
            ("created", JsonValue::String(ts.clone())),
            ("updated", JsonValue::String(ts.clone())),
        ]);
        db.insert("posts", &data).unwrap();

        let query = RecordQuery {
            page: 1,
            per_page: 10,
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.total_items, 1);
        assert_eq!(result.total_pages, 1);
    }

    #[test]
    fn find_many_exact_page_boundary() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        // Insert exactly 6 items, per_page=3 → exactly 2 pages
        for i in 0..6 {
            let data = record_data(&[
                ("id", JsonValue::String(format!("epb{i:011}a"))),
                ("title", JsonValue::String(format!("Post {i}"))),
                ("created", JsonValue::String(ts.clone())),
                ("updated", JsonValue::String(ts.clone())),
            ]);
            db.insert("posts", &data).unwrap();
        }

        let query = RecordQuery {
            page: 1,
            per_page: 3,
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();

        assert_eq!(result.items.len(), 3);
        assert_eq!(result.total_items, 6);
        assert_eq!(result.total_pages, 2);

        // Page 2 should also have exactly 3 items
        let query_p2 = RecordQuery {
            page: 2,
            per_page: 3,
            ..Default::default()
        };
        let result_p2 = db.find_many("posts", &query_p2).unwrap();
        assert_eq!(result_p2.items.len(), 3);
    }

    #[test]
    fn find_many_total_pages_is_1_for_empty_table() {
        let db = setup_db_with_posts();

        let query = RecordQuery {
            page: 1,
            per_page: 10,
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();

        // PocketBase returns totalPages=1 even for empty collections
        assert_eq!(result.total_pages, 1);
        assert_eq!(result.total_items, 0);
        assert!(result.items.is_empty());
    }

    #[test]
    fn find_many_with_filter_pagination_metadata_reflects_filtered_count() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        // Insert 5 items with different views
        for i in 0..5 {
            let data = record_data(&[
                ("id", JsonValue::String(format!("flt{i:011}a"))),
                ("title", JsonValue::String(format!("Post {i}"))),
                ("views", serde_json::json!(i * 10)),
                ("created", JsonValue::String(ts.clone())),
                ("updated", JsonValue::String(ts.clone())),
            ]);
            db.insert("posts", &data).unwrap();
        }

        // Filter for views >= 20 → should match 3 records (20, 30, 40)
        let query = RecordQuery {
            page: 1,
            per_page: 2,
            filter: Some("views >= 20".to_string()),
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();

        assert_eq!(result.total_items, 3);
        assert_eq!(result.total_pages, 2);
        assert_eq!(result.items.len(), 2);
    }

    #[test]
    fn find_many_large_page_number_returns_empty() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        let data = record_data(&[
            ("id", JsonValue::String("lgp000000000a".into())),
            ("title", JsonValue::String("Solo".into())),
            ("created", JsonValue::String(ts.clone())),
            ("updated", JsonValue::String(ts.clone())),
        ]);
        db.insert("posts", &data).unwrap();

        let query = RecordQuery {
            page: 999999,
            per_page: 10,
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();

        assert!(result.items.is_empty());
        assert_eq!(result.total_items, 1);
        assert_eq!(result.total_pages, 1);
    }

    #[test]
    fn find_many_per_page_equals_total_items() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        for i in 0..5 {
            let data = record_data(&[
                ("id", JsonValue::String(format!("ppt{i:011}a"))),
                ("title", JsonValue::String(format!("Post {i}"))),
                ("created", JsonValue::String(ts.clone())),
                ("updated", JsonValue::String(ts.clone())),
            ]);
            db.insert("posts", &data).unwrap();
        }

        // per_page exactly equals total items → 1 page
        let query = RecordQuery {
            page: 1,
            per_page: 5,
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();

        assert_eq!(result.items.len(), 5);
        assert_eq!(result.total_items, 5);
        assert_eq!(result.total_pages, 1);
    }

    #[test]
    fn find_many_per_page_1_creates_many_pages() {
        let db = setup_db_with_posts();
        let ts = now_iso();

        for i in 0..4 {
            let data = record_data(&[
                ("id", JsonValue::String(format!("pp1{i:011}a"))),
                ("title", JsonValue::String(format!("Post {i}"))),
                ("created", JsonValue::String(ts.clone())),
                ("updated", JsonValue::String(ts.clone())),
            ]);
            db.insert("posts", &data).unwrap();
        }

        let query = RecordQuery {
            page: 3,
            per_page: 1,
            ..Default::default()
        };
        let result = db.find_many("posts", &query).unwrap();

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.total_items, 4);
        assert_eq!(result.total_pages, 4);
        assert_eq!(result.page, 3);
    }

    // ── timestamps auto-managed via RecordService ────────────────────────

    #[test]
    fn record_service_auto_sets_timestamps() {
        use zerobase_core::schema::{Collection, Field, FieldType, TextOptions};
        use zerobase_core::services::record_service::SchemaLookup;

        let db = setup_db_with_posts();

        // SchemaLookup impl for the integration test.
        struct TestSchema;
        impl SchemaLookup for TestSchema {
            fn get_collection(&self, _name: &str) -> zerobase_core::error::Result<Collection> {
                Ok(Collection::base(
                    "posts",
                    vec![
                        Field::new("title", FieldType::Text(TextOptions::default())).required(true),
                        Field::new(
                            "views",
                            FieldType::Number(zerobase_core::schema::NumberOptions {
                                min: None,
                                max: None,
                                only_int: false,
                            }),
                        ),
                    ],
                ))
            }
        }

        let service = zerobase_core::RecordService::new(db, TestSchema);

        // Create a record — timestamps should be auto-injected.
        let data = serde_json::json!({"title": "Auto Timestamp", "views": 5});
        let record = service.create_record("posts", data).unwrap();

        assert!(record.contains_key("created"));
        assert!(record.contains_key("updated"));
        let created = record["created"].as_str().unwrap();
        let updated = record["updated"].as_str().unwrap();
        assert!(!created.is_empty());
        assert!(!updated.is_empty());

        // ID should be 15 alphanumeric chars.
        let id = record["id"].as_str().unwrap();
        assert_eq!(id.len(), 15);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    // ── FTS search integration tests ─────────────────────────────────────

    /// Create a "posts" collection with FTS enabled on the title field.
    fn setup_db_with_fts_posts() -> Database {
        let db = Database::open_in_memory(&PoolConfig::default()).unwrap();
        db.run_system_migrations().unwrap();

        let schema = CollectionSchema {
            name: "posts".to_string(),
            collection_type: "base".to_string(),
            columns: vec![
                ColumnDef {
                    name: "title".to_string(),
                    sql_type: "TEXT".to_string(),
                    not_null: true,
                    default: None,
                    unique: false,
                },
                ColumnDef {
                    name: "body".to_string(),
                    sql_type: "TEXT".to_string(),
                    not_null: false,
                    default: Some("''".to_string()),
                    unique: false,
                },
                ColumnDef {
                    name: "views".to_string(),
                    sql_type: "REAL".to_string(),
                    not_null: false,
                    default: Some("0".to_string()),
                    unique: false,
                },
            ],
            indexes: vec![],
            searchable_fields: vec!["title".to_string(), "body".to_string()],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();
        db
    }

    fn insert_fts_posts(db: &Database) {
        let ts = now_iso();
        let posts = vec![
            (
                "rec00000000001",
                "Rust Programming Guide",
                "Learn systems programming with Rust",
            ),
            (
                "rec00000000002",
                "Python for Data Science",
                "Machine learning with Python",
            ),
            (
                "rec00000000003",
                "Advanced Rust Patterns",
                "Ownership, borrowing, and lifetimes in Rust",
            ),
            (
                "rec00000000004",
                "JavaScript Frameworks",
                "React, Vue, and Angular comparison",
            ),
            (
                "rec00000000005",
                "Rust Web Development",
                "Building web servers with Actix and Axum",
            ),
        ];

        for (id, title, body) in posts {
            let data = record_data(&[
                ("id", JsonValue::String(id.into())),
                ("title", JsonValue::String(title.into())),
                ("body", JsonValue::String(body.into())),
                ("views", serde_json::json!(0)),
                ("created", JsonValue::String(ts.clone())),
                ("updated", JsonValue::String(ts.clone())),
            ]);
            db.insert("posts", &data).unwrap();
        }
    }

    #[test]
    fn fts_search_returns_matching_records() {
        let db = setup_db_with_fts_posts();
        insert_fts_posts(&db);

        let query = RecordQuery {
            page: 1,
            per_page: 30,
            search: Some("rust".to_string()),
            ..Default::default()
        };

        let result = db.find_many("posts", &query).unwrap();
        assert_eq!(result.total_items, 3);
        let titles: Vec<&str> = result
            .items
            .iter()
            .map(|r| r["title"].as_str().unwrap())
            .collect();
        assert!(titles.iter().all(|t| t.to_lowercase().contains("rust")
            || result.items.iter().any(|r| r["body"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("rust"))));
    }

    #[test]
    fn fts_search_returns_empty_for_no_match() {
        let db = setup_db_with_fts_posts();
        insert_fts_posts(&db);

        let query = RecordQuery {
            page: 1,
            per_page: 30,
            search: Some("golang".to_string()),
            ..Default::default()
        };

        let result = db.find_many("posts", &query).unwrap();
        assert_eq!(result.total_items, 0);
        assert!(result.items.is_empty());
    }

    #[test]
    fn fts_search_combined_with_filter() {
        let db = setup_db_with_fts_posts();
        insert_fts_posts(&db);

        // Search for "rust" but filter to a specific record.
        let query = RecordQuery {
            page: 1,
            per_page: 30,
            search: Some("rust".to_string()),
            filter: Some("id = 'rec00000000001'".to_string()),
            ..Default::default()
        };

        let result = db.find_many("posts", &query).unwrap();
        assert_eq!(result.total_items, 1);
        assert_eq!(result.items[0]["id"], "rec00000000001");
    }

    #[test]
    fn fts_search_with_explicit_sort_overrides_relevance() {
        let db = setup_db_with_fts_posts();
        insert_fts_posts(&db);

        let query = RecordQuery {
            page: 1,
            per_page: 30,
            search: Some("rust".to_string()),
            sort: vec![("title".to_string(), SortDirection::Asc)],
            ..Default::default()
        };

        let result = db.find_many("posts", &query).unwrap();
        assert_eq!(result.total_items, 3);

        // Should be sorted alphabetically by title.
        let titles: Vec<&str> = result
            .items
            .iter()
            .map(|r| r["title"].as_str().unwrap())
            .collect();
        assert_eq!(titles[0], "Advanced Rust Patterns");
        assert_eq!(titles[1], "Rust Programming Guide");
        assert_eq!(titles[2], "Rust Web Development");
    }

    #[test]
    fn fts_search_pagination_works() {
        let db = setup_db_with_fts_posts();
        insert_fts_posts(&db);

        let query = RecordQuery {
            page: 1,
            per_page: 2,
            search: Some("rust".to_string()),
            sort: vec![("title".to_string(), SortDirection::Asc)],
            ..Default::default()
        };

        let result = db.find_many("posts", &query).unwrap();
        assert_eq!(result.total_items, 3);
        assert_eq!(result.total_pages, 2);
        assert_eq!(result.items.len(), 2);

        // Page 2 should have 1 item.
        let query_p2 = RecordQuery {
            page: 2,
            per_page: 2,
            search: Some("rust".to_string()),
            sort: vec![("title".to_string(), SortDirection::Asc)],
            ..Default::default()
        };
        let result_p2 = db.find_many("posts", &query_p2).unwrap();
        assert_eq!(result_p2.items.len(), 1);
    }

    #[test]
    fn fts_search_without_fts_table_falls_back_gracefully() {
        // Use a collection without searchable fields (no FTS table).
        let db = setup_db_with_posts();
        let ts = now_iso();
        let data = record_data(&[
            ("id", JsonValue::String("rec00000000001".into())),
            ("title", JsonValue::String("Hello".into())),
            ("views", serde_json::json!(0)),
            ("created", JsonValue::String(ts.clone())),
            ("updated", JsonValue::String(ts)),
        ]);
        db.insert("posts", &data).unwrap();

        // Search on a collection without FTS — should return all records (search ignored).
        let query = RecordQuery {
            page: 1,
            per_page: 30,
            search: Some("Hello".to_string()),
            ..Default::default()
        };

        let result = db.find_many("posts", &query).unwrap();
        // FTS table doesn't exist, so search is silently ignored.
        assert_eq!(result.total_items, 1);
    }

    #[test]
    fn fts_search_phrase_query() {
        let db = setup_db_with_fts_posts();
        insert_fts_posts(&db);

        // Phrase search: "web development" should match only the exact phrase.
        let query = RecordQuery {
            page: 1,
            per_page: 30,
            search: Some("\"web development\"".to_string()),
            ..Default::default()
        };

        let result = db.find_many("posts", &query).unwrap();
        // Only "Rust Web Development" has "web" in title AND "web servers" in body.
        // The phrase "web development" only appears together in the title of rec00000000005.
        assert!(result.total_items >= 1);
        let ids: Vec<&str> = result
            .items
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"rec00000000005"));
    }

    #[test]
    fn fts_index_created_with_collection_and_dropped_on_delete() {
        let db = Database::open_in_memory(&PoolConfig::default()).unwrap();
        db.run_system_migrations().unwrap();

        let schema = CollectionSchema {
            name: "articles".to_string(),
            collection_type: "base".to_string(),
            columns: vec![ColumnDef {
                name: "title".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: None,
                unique: false,
            }],
            indexes: vec![],
            searchable_fields: vec!["title".to_string()],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();

        // FTS table should exist after creation.
        {
            let conn = db.read_conn().unwrap();
            assert!(crate::fts::fts_table_exists(&conn, "articles").unwrap());
        }

        // Delete collection — FTS table should be dropped too.
        db.delete_collection("articles").unwrap();
        {
            let conn = db.read_conn().unwrap();
            assert!(!crate::fts::fts_table_exists(&conn, "articles").unwrap());
        }
    }

    // ── sanitize_table_name security tests ──────────────────────────────

    #[test]
    fn sanitize_table_name_accepts_valid_names() {
        assert_eq!(sanitize_table_name("posts").unwrap(), "posts");
        assert_eq!(sanitize_table_name("user_profiles").unwrap(), "user_profiles");
        assert_eq!(sanitize_table_name("_superusers").unwrap(), "_superusers");
    }

    #[test]
    fn sanitize_table_name_rejects_empty() {
        let err = sanitize_table_name("").unwrap_err();
        match err {
            RecordRepoError::Database { message } => {
                assert!(message.contains("empty"), "expected 'empty' in: {message}");
            }
            other => panic!("expected Database error, got: {other:?}"),
        }
    }

    #[test]
    fn sanitize_table_name_rejects_double_quote() {
        let err = sanitize_table_name("posts\"--").unwrap_err();
        assert!(matches!(err, RecordRepoError::Database { .. }));
    }

    #[test]
    fn sanitize_table_name_rejects_semicolon() {
        let err = sanitize_table_name("posts; DROP TABLE users").unwrap_err();
        assert!(matches!(err, RecordRepoError::Database { .. }));
    }

    #[test]
    fn sanitize_table_name_rejects_null_byte() {
        let err = sanitize_table_name("posts\0").unwrap_err();
        assert!(matches!(err, RecordRepoError::Database { .. }));
    }

    #[test]
    fn sanitize_table_name_rejects_backslash() {
        let err = sanitize_table_name("posts\\evil").unwrap_err();
        assert!(matches!(err, RecordRepoError::Database { .. }));
    }

    #[test]
    fn sanitize_table_name_rejects_newline() {
        let err = sanitize_table_name("posts\nnewline").unwrap_err();
        assert!(matches!(err, RecordRepoError::Database { .. }));
    }

    #[test]
    fn sanitize_table_name_rejects_carriage_return() {
        let err = sanitize_table_name("posts\revil").unwrap_err();
        assert!(matches!(err, RecordRepoError::Database { .. }));
    }
}
