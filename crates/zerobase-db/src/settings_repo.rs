//! Settings repository — SQLite implementation.
//!
//! Implements [`SettingsRepository`] on [`Database`] for CRUD operations on
//! the `_settings` system table.

use rusqlite::params;

use zerobase_core::services::settings_service::{SettingsRepoError, SettingsRepository};

use crate::pool::Database;

impl SettingsRepository for Database {
    fn get_setting(&self, key: &str) -> std::result::Result<Option<String>, SettingsRepoError> {
        let conn = self.read_conn().map_err(|e| SettingsRepoError::Database {
            message: e.to_string(),
        })?;

        let mut stmt = conn
            .prepare("SELECT value FROM _settings WHERE key = ?1")
            .map_err(|e| SettingsRepoError::Database {
                message: e.to_string(),
            })?;

        match stmt.query_row(params![key], |row| row.get::<_, String>(0)) {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(SettingsRepoError::Database {
                message: e.to_string(),
            }),
        }
    }

    fn get_all_settings(
        &self,
    ) -> std::result::Result<Vec<(String, String)>, SettingsRepoError> {
        let conn = self.read_conn().map_err(|e| SettingsRepoError::Database {
            message: e.to_string(),
        })?;

        let mut stmt = conn
            .prepare("SELECT key, value FROM _settings")
            .map_err(|e| SettingsRepoError::Database {
                message: e.to_string(),
            })?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| SettingsRepoError::Database {
                message: e.to_string(),
            })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| SettingsRepoError::Database {
                message: e.to_string(),
            })?);
        }

        Ok(results)
    }

    fn set_setting(&self, key: &str, value: &str) -> std::result::Result<(), SettingsRepoError> {
        self.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO _settings (key, value, updated) VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated = datetime('now')",
                params![key, value],
            )?;
            Ok(())
        })
        .map_err(|e| SettingsRepoError::Database {
            message: e.to_string(),
        })
    }

    fn delete_setting(&self, key: &str) -> std::result::Result<(), SettingsRepoError> {
        self.with_write_conn(|conn| {
            conn.execute("DELETE FROM _settings WHERE key = ?1", params![key])?;
            Ok(())
        })
        .map_err(|e| SettingsRepoError::Database {
            message: e.to_string(),
        })
    }
}
