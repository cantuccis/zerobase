//! Application-level unique field constraint enforcement.
//!
//! Provides pre-insert/pre-update uniqueness checks by querying the database
//! before the actual write. This complements SQLite's UNIQUE column constraint
//! with clear, field-level error messages.

use std::collections::HashMap;

use rusqlite::{params_from_iter, Connection};
use serde_json::Value as JsonValue;

use crate::error::{DbError, Result};
use crate::Database;

/// Describes a field that has a unique constraint.
#[derive(Debug, Clone)]
pub struct UniqueFieldSpec {
    /// The column/field name in the collection table.
    pub name: String,
}

/// Check that all unique fields in a record have values that don't conflict
/// with existing records in the collection.
///
/// # Arguments
///
/// * `conn` - A database connection (read or write).
/// * `collection` - The collection (table) name.
/// * `unique_fields` - The list of fields that must be unique.
/// * `data` - The record data being inserted or updated.
/// * `exclude_id` - If updating, the ID of the record being updated (to exclude it from the check).
///
/// # Returns
///
/// `Ok(())` if all unique fields are unique, or `Err(DbError::Conflict)` with
/// a message naming the first field that has a duplicate value.
pub fn check_unique_fields(
    conn: &Connection,
    collection: &str,
    unique_fields: &[UniqueFieldSpec],
    data: &HashMap<String, JsonValue>,
    exclude_id: Option<&str>,
) -> Result<()> {
    let mut violations: HashMap<String, String> = HashMap::new();

    for field in unique_fields {
        let value = match data.get(&field.name) {
            Some(v) if !v.is_null() => v,
            // Null values don't violate UNIQUE constraints (SQL standard).
            _ => continue,
        };

        let has_duplicate =
            check_field_uniqueness(conn, collection, &field.name, value, exclude_id)?;
        if has_duplicate {
            violations.insert(
                field.name.clone(),
                format!("the value for field '{}' must be unique", field.name),
            );
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        // Return the first violation as a Conflict error.
        // We pick the first alphabetically for deterministic behavior.
        let mut sorted: Vec<_> = violations.into_iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let (field_name, _) = sorted.into_iter().next().unwrap();
        Err(DbError::conflict(format!(
            "the value for field '{}' must be unique",
            field_name
        )))
    }
}

/// Check whether a specific field value already exists in the collection.
fn check_field_uniqueness(
    conn: &Connection,
    collection: &str,
    field_name: &str,
    value: &JsonValue,
    exclude_id: Option<&str>,
) -> Result<bool> {
    let (sql, params) = build_uniqueness_query(collection, field_name, value, exclude_id);

    let exists: bool = conn
        .query_row(&sql, params_from_iter(params.iter()), |row| row.get(0))
        .map_err(DbError::Query)?;

    Ok(exists)
}

/// Build a SQL query to check if a value already exists for a given field.
fn build_uniqueness_query(
    collection: &str,
    field_name: &str,
    value: &JsonValue,
    exclude_id: Option<&str>,
) -> (String, Vec<String>) {
    let mut params: Vec<String> = Vec::new();
    let value_str = json_value_to_sql_param(value);
    params.push(value_str);

    let sql = if let Some(id) = exclude_id {
        params.push(id.to_string());
        format!(
            "SELECT EXISTS(SELECT 1 FROM \"{}\" WHERE \"{}\" = ?1 AND id != ?2)",
            collection, field_name
        )
    } else {
        format!(
            "SELECT EXISTS(SELECT 1 FROM \"{}\" WHERE \"{}\" = ?1)",
            collection, field_name
        )
    };

    (sql, params)
}

/// Convert a JSON value to a SQL parameter string.
fn json_value_to_sql_param(value: &JsonValue) -> String {
    match value {
        JsonValue::String(s) => s.clone(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        _ => value.to_string(),
    }
}

// ── Database convenience method ─────────────────────────────────────────────

impl Database {
    /// Check uniqueness of fields before creating a record.
    ///
    /// This performs application-level checks using a read connection,
    /// providing clear error messages that reference field names.
    pub fn check_unique_for_create(
        &self,
        collection: &str,
        unique_fields: &[UniqueFieldSpec],
        data: &HashMap<String, JsonValue>,
    ) -> Result<()> {
        let conn = self.read_conn()?;
        check_unique_fields(&conn, collection, unique_fields, data, None)
    }

    /// Check uniqueness of fields before updating a record.
    ///
    /// Excludes the record being updated from the check.
    pub fn check_unique_for_update(
        &self,
        collection: &str,
        unique_fields: &[UniqueFieldSpec],
        data: &HashMap<String, JsonValue>,
        record_id: &str,
    ) -> Result<()> {
        let conn = self.read_conn()?;
        check_unique_fields(&conn, collection, unique_fields, data, Some(record_id))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::PoolConfig;

    fn setup_db() -> Database {
        let db = Database::open_in_memory(&PoolConfig::default()).unwrap();
        db.run_system_migrations().unwrap();

        // Create a test table with unique fields.
        db.with_write_conn(|conn| {
            conn.execute_batch(
                "CREATE TABLE test_items (
                    id TEXT PRIMARY KEY NOT NULL,
                    slug TEXT UNIQUE,
                    email TEXT UNIQUE,
                    title TEXT,
                    created TEXT NOT NULL DEFAULT (datetime('now')),
                    updated TEXT NOT NULL DEFAULT (datetime('now'))
                )",
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        db
    }

    fn insert_record(db: &Database, id: &str, slug: &str, email: &str, title: &str) {
        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO test_items (id, slug, email, title) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, slug, email, title],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();
    }

    fn unique_specs(names: &[&str]) -> Vec<UniqueFieldSpec> {
        names
            .iter()
            .map(|n| UniqueFieldSpec {
                name: n.to_string(),
            })
            .collect()
    }

    fn data_map(pairs: &[(&str, &str)]) -> HashMap<String, JsonValue> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), JsonValue::String(v.to_string())))
            .collect()
    }

    // ── check_unique_for_create ──────────────────────────────────────────

    #[test]
    fn create_passes_when_no_duplicates() {
        let db = setup_db();
        insert_record(&db, "rec1", "hello-world", "a@b.com", "Hello");

        let specs = unique_specs(&["slug", "email"]);
        let data = data_map(&[("slug", "different-slug"), ("email", "other@b.com")]);

        let result = db.check_unique_for_create("test_items", &specs, &data);
        assert!(result.is_ok());
    }

    #[test]
    fn create_fails_when_slug_duplicated() {
        let db = setup_db();
        insert_record(&db, "rec1", "hello-world", "a@b.com", "Hello");

        let specs = unique_specs(&["slug", "email"]);
        let data = data_map(&[("slug", "hello-world"), ("email", "new@b.com")]);

        let result = db.check_unique_for_create("test_items", &specs, &data);
        assert!(result.is_err());
        let err = result.unwrap_err();
        match &err {
            DbError::Conflict { message } => {
                assert!(
                    message.contains("slug"),
                    "error should mention 'slug': {}",
                    message
                );
                assert!(
                    message.contains("unique"),
                    "error should mention 'unique': {}",
                    message
                );
            }
            _ => panic!("expected Conflict, got {:?}", err),
        }
    }

    #[test]
    fn create_fails_when_email_duplicated() {
        let db = setup_db();
        insert_record(&db, "rec1", "hello-world", "a@b.com", "Hello");

        let specs = unique_specs(&["slug", "email"]);
        let data = data_map(&[("slug", "new-slug"), ("email", "a@b.com")]);

        let result = db.check_unique_for_create("test_items", &specs, &data);
        assert!(result.is_err());
        let err = result.unwrap_err();
        match &err {
            DbError::Conflict { message } => {
                assert!(message.contains("email"));
            }
            _ => panic!("expected Conflict, got {:?}", err),
        }
    }

    #[test]
    fn create_allows_null_values_for_unique_fields() {
        let db = setup_db();
        insert_record(&db, "rec1", "hello-world", "a@b.com", "Hello");

        let specs = unique_specs(&["slug"]);
        let mut data = HashMap::new();
        data.insert("slug".to_string(), JsonValue::Null);

        let result = db.check_unique_for_create("test_items", &specs, &data);
        assert!(result.is_ok());
    }

    #[test]
    fn create_allows_missing_unique_fields() {
        let db = setup_db();
        insert_record(&db, "rec1", "hello-world", "a@b.com", "Hello");

        let specs = unique_specs(&["slug"]);
        let data = HashMap::new(); // slug not present

        let result = db.check_unique_for_create("test_items", &specs, &data);
        assert!(result.is_ok());
    }

    // ── check_unique_for_update ──────────────────────────────────────────

    #[test]
    fn update_allows_same_value_on_own_record() {
        let db = setup_db();
        insert_record(&db, "rec1", "hello-world", "a@b.com", "Hello");

        let specs = unique_specs(&["slug"]);
        let data = data_map(&[("slug", "hello-world")]);

        // Updating rec1 with its own slug should be fine.
        let result = db.check_unique_for_update("test_items", &specs, &data, "rec1");
        assert!(result.is_ok());
    }

    #[test]
    fn update_rejects_value_taken_by_another_record() {
        let db = setup_db();
        insert_record(&db, "rec1", "hello-world", "a@b.com", "Hello");
        insert_record(&db, "rec2", "other-slug", "b@b.com", "Other");

        let specs = unique_specs(&["slug"]);
        let data = data_map(&[("slug", "hello-world")]);

        // Updating rec2 to use rec1's slug should fail.
        let result = db.check_unique_for_update("test_items", &specs, &data, "rec2");
        assert!(result.is_err());
        let err = result.unwrap_err();
        match &err {
            DbError::Conflict { message } => {
                assert!(message.contains("slug"));
            }
            _ => panic!("expected Conflict, got {:?}", err),
        }
    }

    #[test]
    fn update_allows_different_unique_value() {
        let db = setup_db();
        insert_record(&db, "rec1", "hello-world", "a@b.com", "Hello");
        insert_record(&db, "rec2", "other-slug", "b@b.com", "Other");

        let specs = unique_specs(&["slug"]);
        let data = data_map(&[("slug", "brand-new-slug")]);

        let result = db.check_unique_for_update("test_items", &specs, &data, "rec2");
        assert!(result.is_ok());
    }

    // ── SQLite UNIQUE constraint also enforced at DB level ───────────────

    #[test]
    fn sqlite_unique_constraint_produces_conflict_error() {
        let db = setup_db();
        insert_record(&db, "rec1", "hello-world", "a@b.com", "Hello");

        // Try to insert a duplicate slug directly (bypassing app-level check).
        let result = db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO test_items (id, slug, email, title) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params!["rec2", "hello-world", "new@b.com", "Dupe"],
            )
            .map_err(crate::error::map_query_error)?;
            Ok(())
        });

        assert!(result.is_err());
        let err = result.unwrap_err();
        match &err {
            DbError::Conflict { message } => {
                assert!(message.contains("slug"));
                assert!(message.contains("unique"));
            }
            _ => panic!("expected Conflict, got {:?}", err),
        }
    }

    #[test]
    fn sqlite_unique_constraint_on_email_produces_conflict() {
        let db = setup_db();
        insert_record(&db, "rec1", "slug1", "a@b.com", "Hello");

        let result = db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO test_items (id, slug, email, title) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params!["rec2", "slug2", "a@b.com", "Dupe"],
            )
            .map_err(crate::error::map_query_error)?;
            Ok(())
        });

        assert!(result.is_err());
        let err = result.unwrap_err();
        match &err {
            DbError::Conflict { message } => {
                assert!(message.contains("email"));
            }
            _ => panic!("expected Conflict, got {:?}", err),
        }
    }

    // ── Numeric unique values ───────────────────────────────────────────

    #[test]
    fn unique_check_works_with_numeric_values() {
        let db = setup_db();

        // Create a table with a numeric unique field.
        db.with_write_conn(|conn| {
            conn.execute_batch(
                "CREATE TABLE numeric_items (
                    id TEXT PRIMARY KEY NOT NULL,
                    code INTEGER UNIQUE,
                    created TEXT NOT NULL DEFAULT (datetime('now')),
                    updated TEXT NOT NULL DEFAULT (datetime('now'))
                )",
            )
            .map_err(DbError::Query)?;
            conn.execute("INSERT INTO numeric_items (id, code) VALUES ('r1', 42)", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let specs = unique_specs(&["code"]);
        let mut data = HashMap::new();
        data.insert("code".to_string(), JsonValue::Number(42.into()));

        let result = db.check_unique_for_create("numeric_items", &specs, &data);
        assert!(result.is_err());

        // Different number should pass.
        let mut data2 = HashMap::new();
        data2.insert("code".to_string(), JsonValue::Number(99.into()));
        let result2 = db.check_unique_for_create("numeric_items", &specs, &data2);
        assert!(result2.is_ok());
    }
}
