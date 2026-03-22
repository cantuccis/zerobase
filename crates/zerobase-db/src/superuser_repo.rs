//! Superuser repository — SQLite implementation.
//!
//! Implements [`SuperuserRepository`] on [`Database`] for CRUD operations on
//! the `_superusers` system table.

use std::collections::HashMap;

use rusqlite::params;
use serde_json::Value as JsonValue;

use zerobase_core::error::ZerobaseError;
use zerobase_core::services::superuser_service::SuperuserRepository;

use crate::error::map_query_error;
use crate::pool::Database;

/// Convert a SQLite row into a HashMap by reading all columns.
fn row_to_map(
    row: &rusqlite::Row,
    column_names: &[String],
) -> rusqlite::Result<HashMap<String, JsonValue>> {
    let mut record = HashMap::new();
    for (i, col_name) in column_names.iter().enumerate() {
        let value: rusqlite::types::Value = row.get(i)?;
        let json_value = match value {
            rusqlite::types::Value::Null => JsonValue::Null,
            rusqlite::types::Value::Integer(n) => JsonValue::Number(n.into()),
            rusqlite::types::Value::Real(f) => serde_json::Number::from_f64(f)
                .map(JsonValue::Number)
                .unwrap_or(JsonValue::Null),
            rusqlite::types::Value::Text(s) => JsonValue::String(s),
            rusqlite::types::Value::Blob(_) => JsonValue::Null,
        };
        record.insert(col_name.clone(), json_value);
    }
    Ok(record)
}

impl SuperuserRepository for Database {
    fn find_by_id(&self, id: &str) -> Result<Option<HashMap<String, JsonValue>>, ZerobaseError> {
        let conn = self.read_conn().map_err(ZerobaseError::from)?;

        let mut stmt = conn
            .prepare("SELECT * FROM _superusers WHERE id = ?1")
            .map_err(|e| ZerobaseError::from(map_query_error(e)))?;

        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

        match stmt.query_row(params![id], |row| row_to_map(row, &column_names)) {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ZerobaseError::from(map_query_error(e))),
        }
    }

    fn find_by_email(
        &self,
        email: &str,
    ) -> Result<Option<HashMap<String, JsonValue>>, ZerobaseError> {
        let conn = self.read_conn().map_err(ZerobaseError::from)?;

        let mut stmt = conn
            .prepare("SELECT * FROM _superusers WHERE email = ?1")
            .map_err(|e| ZerobaseError::from(map_query_error(e)))?;

        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

        match stmt.query_row(params![email], |row| row_to_map(row, &column_names)) {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ZerobaseError::from(map_query_error(e))),
        }
    }

    fn insert(&self, data: &HashMap<String, JsonValue>) -> Result<(), ZerobaseError> {
        let id = data.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let email = data.get("email").and_then(|v| v.as_str()).unwrap_or("");
        let password = data.get("password").and_then(|v| v.as_str()).unwrap_or("");
        let token_key = data.get("tokenKey").and_then(|v| v.as_str()).unwrap_or("");
        let created = data.get("created").and_then(|v| v.as_str()).unwrap_or("");
        let updated = data.get("updated").and_then(|v| v.as_str()).unwrap_or("");

        self.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO _superusers (id, email, password, tokenKey, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, email, password, token_key, created, updated],
            )
            .map_err(map_query_error)?;
            Ok(())
        })
        .map_err(ZerobaseError::from)
    }

    fn update(&self, id: &str, data: &HashMap<String, JsonValue>) -> Result<(), ZerobaseError> {
        self.with_write_conn(|conn| {
            // Build SET clause from provided fields (skip id).
            let mut set_parts = Vec::new();
            let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            for (key, val) in data {
                if key == "id" {
                    continue;
                }
                set_parts.push(format!("\"{}\" = ?", key));
                match val {
                    JsonValue::String(s) => values.push(Box::new(s.clone())),
                    JsonValue::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            values.push(Box::new(i));
                        } else if let Some(f) = n.as_f64() {
                            values.push(Box::new(f));
                        }
                    }
                    JsonValue::Bool(b) => values.push(Box::new(*b)),
                    JsonValue::Null => values.push(Box::new(rusqlite::types::Null)),
                    _ => values.push(Box::new(val.to_string())),
                }
            }

            if set_parts.is_empty() {
                return Ok(());
            }

            // Add updated timestamp.
            let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
            set_parts.push("\"updated\" = ?".to_string());
            values.push(Box::new(now));

            values.push(Box::new(id.to_string()));

            let sql = format!(
                "UPDATE _superusers SET {} WHERE id = ?",
                set_parts.join(", ")
            );

            let params: Vec<&dyn rusqlite::types::ToSql> =
                values.iter().map(|v| v.as_ref()).collect();
            conn.execute(&sql, params.as_slice())
                .map_err(map_query_error)?;
            Ok(())
        })
        .map_err(ZerobaseError::from)
    }

    fn delete(&self, id: &str) -> Result<bool, ZerobaseError> {
        self.with_write_conn(|conn| {
            let affected = conn
                .execute("DELETE FROM _superusers WHERE id = ?1", params![id])
                .map_err(map_query_error)?;
            Ok(affected > 0)
        })
        .map_err(ZerobaseError::from)
    }

    fn list_all(&self) -> Result<Vec<HashMap<String, JsonValue>>, ZerobaseError> {
        let conn = self.read_conn().map_err(ZerobaseError::from)?;

        let mut stmt = conn
            .prepare("SELECT * FROM _superusers ORDER BY created ASC")
            .map_err(|e| ZerobaseError::from(map_query_error(e)))?;

        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

        let rows = stmt
            .query_map([], |row| row_to_map(row, &column_names))
            .map_err(|e| ZerobaseError::from(map_query_error(e)))?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(|e| ZerobaseError::from(map_query_error(e)))?);
        }
        Ok(records)
    }

    fn count(&self) -> Result<u64, ZerobaseError> {
        let conn = self.read_conn().map_err(ZerobaseError::from)?;

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM _superusers", [], |row| row.get(0))
            .map_err(|e| ZerobaseError::from(map_query_error(e)))?;

        Ok(count as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::PoolConfig;

    fn test_db() -> Database {
        let db = Database::open_in_memory(&PoolConfig::default()).unwrap();
        db.run_system_migrations().unwrap();
        db
    }

    #[test]
    fn insert_and_find_by_id() {
        let db = test_db();
        let mut data = HashMap::new();
        data.insert("id".to_string(), JsonValue::String("su1".into()));
        data.insert(
            "email".to_string(),
            JsonValue::String("admin@test.com".into()),
        );
        data.insert("password".to_string(), JsonValue::String("hashed".into()));
        data.insert("tokenKey".to_string(), JsonValue::String("tk1".into()));
        data.insert(
            "created".to_string(),
            JsonValue::String("2025-01-01 00:00:00".into()),
        );
        data.insert(
            "updated".to_string(),
            JsonValue::String("2025-01-01 00:00:00".into()),
        );

        db.insert(&data).unwrap();

        let found = db.find_by_id("su1").unwrap();
        assert!(found.is_some());
        let record = found.unwrap();
        assert_eq!(
            record.get("email").unwrap().as_str().unwrap(),
            "admin@test.com"
        );
    }

    #[test]
    fn find_by_email() {
        let db = test_db();
        let mut data = HashMap::new();
        data.insert("id".to_string(), JsonValue::String("su1".into()));
        data.insert(
            "email".to_string(),
            JsonValue::String("admin@test.com".into()),
        );
        data.insert("password".to_string(), JsonValue::String("hashed".into()));
        data.insert("tokenKey".to_string(), JsonValue::String("tk1".into()));
        data.insert(
            "created".to_string(),
            JsonValue::String("2025-01-01 00:00:00".into()),
        );
        data.insert(
            "updated".to_string(),
            JsonValue::String("2025-01-01 00:00:00".into()),
        );

        db.insert(&data).unwrap();

        let found = db.find_by_email("admin@test.com").unwrap();
        assert!(found.is_some());
        assert!(db.find_by_email("nobody@test.com").unwrap().is_none());
    }

    #[test]
    fn delete_superuser() {
        let db = test_db();
        let mut data = HashMap::new();
        data.insert("id".to_string(), JsonValue::String("su1".into()));
        data.insert(
            "email".to_string(),
            JsonValue::String("admin@test.com".into()),
        );
        data.insert("password".to_string(), JsonValue::String("hashed".into()));
        data.insert("tokenKey".to_string(), JsonValue::String("tk1".into()));
        data.insert(
            "created".to_string(),
            JsonValue::String("2025-01-01 00:00:00".into()),
        );
        data.insert(
            "updated".to_string(),
            JsonValue::String("2025-01-01 00:00:00".into()),
        );

        db.insert(&data).unwrap();
        assert!(db.delete("su1").unwrap());
        assert!(!db.delete("su1").unwrap());
        assert!(db.find_by_id("su1").unwrap().is_none());
    }

    #[test]
    fn list_and_count() {
        let db = test_db();
        assert_eq!(db.count().unwrap(), 0);

        for i in 0..3 {
            let mut data = HashMap::new();
            data.insert("id".to_string(), JsonValue::String(format!("su{i}")));
            data.insert(
                "email".to_string(),
                JsonValue::String(format!("admin{i}@test.com")),
            );
            data.insert("password".to_string(), JsonValue::String("hashed".into()));
            data.insert("tokenKey".to_string(), JsonValue::String(format!("tk{i}")));
            data.insert(
                "created".to_string(),
                JsonValue::String("2025-01-01 00:00:00".into()),
            );
            data.insert(
                "updated".to_string(),
                JsonValue::String("2025-01-01 00:00:00".into()),
            );
            db.insert(&data).unwrap();
        }

        assert_eq!(db.count().unwrap(), 3);
        assert_eq!(db.list_all().unwrap().len(), 3);
    }

    #[test]
    fn duplicate_email_rejected() {
        let db = test_db();
        let mut data = HashMap::new();
        data.insert("id".to_string(), JsonValue::String("su1".into()));
        data.insert(
            "email".to_string(),
            JsonValue::String("admin@test.com".into()),
        );
        data.insert("password".to_string(), JsonValue::String("hashed".into()));
        data.insert("tokenKey".to_string(), JsonValue::String("tk1".into()));
        data.insert(
            "created".to_string(),
            JsonValue::String("2025-01-01 00:00:00".into()),
        );
        data.insert(
            "updated".to_string(),
            JsonValue::String("2025-01-01 00:00:00".into()),
        );
        db.insert(&data).unwrap();

        let mut data2 = HashMap::new();
        data2.insert("id".to_string(), JsonValue::String("su2".into()));
        data2.insert(
            "email".to_string(),
            JsonValue::String("admin@test.com".into()),
        );
        data2.insert("password".to_string(), JsonValue::String("hashed2".into()));
        data2.insert("tokenKey".to_string(), JsonValue::String("tk2".into()));
        data2.insert(
            "created".to_string(),
            JsonValue::String("2025-01-01 00:00:00".into()),
        );
        data2.insert(
            "updated".to_string(),
            JsonValue::String("2025-01-01 00:00:00".into()),
        );

        let err = db.insert(&data2).unwrap_err();
        assert!(matches!(err, ZerobaseError::Conflict { .. }));
    }
}
