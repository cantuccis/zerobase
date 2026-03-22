//! Backup repository implementation for [`Database`].
//!
//! Uses SQLite's online backup API to create consistent database copies.
//! Backups are stored in a `pb_backups/` directory alongside the database file.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tracing::{info, warn};

use zerobase_core::services::backup_service::{BackupInfo, BackupRepoError, BackupRepository};

use crate::pool::Database;

impl Database {
    /// Return the backup directory for this database.
    ///
    /// Backups are stored in `<db_parent>/pb_backups/`.
    /// Returns `None` for in-memory databases (no path on disk).
    fn backup_dir(&self) -> Option<PathBuf> {
        self.db_path.as_ref().map(|p| {
            p.parent()
                .unwrap_or_else(|| Path::new("."))
                .join("pb_backups")
        })
    }

    /// Ensure the backup directory exists.
    fn ensure_backup_dir(&self) -> std::result::Result<PathBuf, BackupRepoError> {
        let dir = self.backup_dir().ok_or_else(|| BackupRepoError::OperationFailed {
            message: "backups are not supported for in-memory databases".to_string(),
        })?;
        if !dir.exists() {
            std::fs::create_dir_all(&dir).map_err(|e| BackupRepoError::OperationFailed {
                message: format!("failed to create backup directory: {e}"),
            })?;
        }
        Ok(dir)
    }
}

impl BackupRepository for Database {
    fn create_backup(&self, name: &str) -> std::result::Result<BackupInfo, BackupRepoError> {
        let dir = self.ensure_backup_dir()?;
        let backup_path = dir.join(name);

        if backup_path.exists() {
            return Err(BackupRepoError::AlreadyExists {
                name: name.to_string(),
            });
        }

        let db_path = self.db_path.as_ref().ok_or_else(|| {
            BackupRepoError::OperationFailed {
                message: "backups are not supported for in-memory databases".to_string(),
            }
        })?;

        // Open the source database read-only for backup.
        let src = Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| BackupRepoError::OperationFailed {
            message: format!("failed to open source database: {e}"),
        })?;

        // Create destination database.
        let mut dst = Connection::open(&backup_path).map_err(|e| {
            BackupRepoError::OperationFailed {
                message: format!("failed to create backup file: {e}"),
            }
        })?;

        // Use SQLite's backup API.
        let backup =
            rusqlite::backup::Backup::new(&src, &mut dst).map_err(|e| {
                // Clean up partial backup file on error.
                let _ = std::fs::remove_file(&backup_path);
                BackupRepoError::OperationFailed {
                    message: format!("failed to initialize backup: {e}"),
                }
            })?;

        // Copy all pages in one step (-1 = all pages).
        backup.step(-1).map_err(|e| {
            let _ = std::fs::remove_file(&backup_path);
            BackupRepoError::OperationFailed {
                message: format!("backup failed: {e}"),
            }
        })?;

        drop(backup);
        drop(dst);
        drop(src);

        info!(backup_name = name, "backup created successfully");

        // Read metadata from the created file.
        file_info(name, &backup_path)
    }

    fn list_backups(&self) -> std::result::Result<Vec<BackupInfo>, BackupRepoError> {
        let dir = match self.backup_dir() {
            Some(d) if d.exists() => d,
            Some(_) => return Ok(Vec::new()),
            None => return Ok(Vec::new()),
        };

        let mut backups = Vec::new();

        let entries = std::fs::read_dir(&dir).map_err(|e| BackupRepoError::OperationFailed {
            message: format!("failed to read backup directory: {e}"),
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| BackupRepoError::OperationFailed {
                message: format!("failed to read directory entry: {e}"),
            })?;
            let path = entry.path();
            if path.is_file() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".db") || name.ends_with(".zip") {
                    match file_info(&name, &path) {
                        Ok(info) => backups.push(info),
                        Err(e) => {
                            warn!(file = %name, error = %e, "skipping unreadable backup file");
                        }
                    }
                }
            }
        }

        // Sort newest first.
        backups.sort_by(|a, b| b.modified.cmp(&a.modified));

        Ok(backups)
    }

    fn get_backup(&self, name: &str) -> std::result::Result<BackupInfo, BackupRepoError> {
        let dir = self.backup_dir().ok_or_else(|| BackupRepoError::NotFound {
            name: name.to_string(),
        })?;
        let path = dir.join(name);
        if !path.is_file() {
            return Err(BackupRepoError::NotFound {
                name: name.to_string(),
            });
        }
        file_info(name, &path)
    }

    fn backup_path(&self, name: &str) -> std::result::Result<String, BackupRepoError> {
        let dir = self.backup_dir().ok_or_else(|| BackupRepoError::NotFound {
            name: name.to_string(),
        })?;
        let path = dir.join(name);
        if !path.is_file() {
            return Err(BackupRepoError::NotFound {
                name: name.to_string(),
            });
        }
        Ok(path.to_string_lossy().to_string())
    }

    fn delete_backup(&self, name: &str) -> std::result::Result<(), BackupRepoError> {
        let dir = self.backup_dir().ok_or_else(|| BackupRepoError::NotFound {
            name: name.to_string(),
        })?;
        let path = dir.join(name);
        if !path.is_file() {
            return Err(BackupRepoError::NotFound {
                name: name.to_string(),
            });
        }
        std::fs::remove_file(&path).map_err(|e| BackupRepoError::OperationFailed {
            message: format!("failed to delete backup: {e}"),
        })?;
        info!(backup_name = name, "backup deleted");
        Ok(())
    }

    fn restore_backup(&self, name: &str) -> std::result::Result<(), BackupRepoError> {
        let dir = self.backup_dir().ok_or_else(|| BackupRepoError::NotFound {
            name: name.to_string(),
        })?;
        let backup_path = dir.join(name);
        if !backup_path.is_file() {
            return Err(BackupRepoError::NotFound {
                name: name.to_string(),
            });
        }

        let db_path = self.db_path.as_ref().ok_or_else(|| {
            BackupRepoError::OperationFailed {
                message: "restore is not supported for in-memory databases".to_string(),
            }
        })?;

        // Open backup as source.
        let src = Connection::open_with_flags(
            &backup_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| BackupRepoError::OperationFailed {
            message: format!("failed to open backup file: {e}"),
        })?;

        // Use the write connection as destination via backup API.
        // We need to lock the write connection to prevent concurrent writes.
        let mut write_conn = self.write_conn.lock().map_err(|e| {
            BackupRepoError::OperationFailed {
                message: format!("failed to acquire write lock: {e}"),
            }
        })?;

        let backup = rusqlite::backup::Backup::new(&src, &mut write_conn).map_err(|e| {
            BackupRepoError::OperationFailed {
                message: format!("failed to initialize restore: {e}"),
            }
        })?;

        backup.step(-1).map_err(|e| BackupRepoError::OperationFailed {
            message: format!("restore failed: {e}"),
        })?;

        drop(backup);
        drop(src);
        drop(write_conn);

        info!(
            backup_name = name,
            db_path = %db_path.display(),
            "database restored from backup"
        );

        Ok(())
    }
}

/// Build a [`BackupInfo`] from filesystem metadata.
fn file_info(name: &str, path: &Path) -> std::result::Result<BackupInfo, BackupRepoError> {
    let meta = std::fs::metadata(path).map_err(|e| BackupRepoError::OperationFailed {
        message: format!("failed to read file metadata for {name}: {e}"),
    })?;

    let created = meta
        .created()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let modified = meta
        .modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

    let fmt = |t: std::time::SystemTime| -> String {
        let dt: chrono::DateTime<chrono::Utc> = t.into();
        dt.format("%Y-%m-%d %H:%M:%S%.3fZ").to_string()
    };

    Ok(BackupInfo {
        name: name.to_string(),
        size: meta.len(),
        created: fmt(created),
        modified: fmt(modified),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::PoolConfig;

    fn test_db_on_disk() -> (Database, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(&db_path, &PoolConfig::default()).unwrap();

        // Create some test data.
        db.with_write_conn(|conn| {
            conn.execute_batch(
                "CREATE TABLE test_data (id INTEGER PRIMARY KEY, val TEXT);
                 INSERT INTO test_data (val) VALUES ('hello');
                 INSERT INTO test_data (val) VALUES ('world');",
            )
            .map_err(crate::error::DbError::Query)?;
            Ok(())
        })
        .unwrap();

        (db, dir)
    }

    #[test]
    fn create_and_list_backups() {
        let (db, _dir) = test_db_on_disk();

        let info = db.create_backup("test_backup.db").unwrap();
        assert_eq!(info.name, "test_backup.db");
        assert!(info.size > 0);

        let list = db.list_backups().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test_backup.db");
    }

    #[test]
    fn create_backup_conflict() {
        let (db, _dir) = test_db_on_disk();

        db.create_backup("dup.db").unwrap();
        let err = db.create_backup("dup.db").unwrap_err();
        assert!(matches!(err, BackupRepoError::AlreadyExists { .. }));
    }

    #[test]
    fn get_backup_found() {
        let (db, _dir) = test_db_on_disk();
        db.create_backup("found.db").unwrap();

        let info = db.get_backup("found.db").unwrap();
        assert_eq!(info.name, "found.db");
    }

    #[test]
    fn get_backup_not_found() {
        let (db, _dir) = test_db_on_disk();
        let err = db.get_backup("missing.db").unwrap_err();
        assert!(matches!(err, BackupRepoError::NotFound { .. }));
    }

    #[test]
    fn backup_path_returns_full_path() {
        let (db, _dir) = test_db_on_disk();
        db.create_backup("path_test.db").unwrap();

        let path = db.backup_path("path_test.db").unwrap();
        assert!(path.contains("path_test.db"));
        assert!(path.contains("pb_backups"));
    }

    #[test]
    fn delete_backup_success() {
        let (db, _dir) = test_db_on_disk();
        db.create_backup("to_delete.db").unwrap();

        db.delete_backup("to_delete.db").unwrap();

        let list = db.list_backups().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn delete_backup_not_found() {
        let (db, _dir) = test_db_on_disk();
        let err = db.delete_backup("nope.db").unwrap_err();
        assert!(matches!(err, BackupRepoError::NotFound { .. }));
    }

    #[test]
    fn restore_backup_replaces_data() {
        let (db, _dir) = test_db_on_disk();

        // Create backup of current state (2 rows).
        db.create_backup("restore_test.db").unwrap();

        // Add more data.
        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO test_data (val) VALUES ('extra')", [])
                .map_err(crate::error::DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Verify 3 rows.
        let conn = db.read_conn().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM test_data", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3);
        drop(conn);

        // Restore from backup (should revert to 2 rows).
        db.restore_backup("restore_test.db").unwrap();

        let conn = db.read_conn().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM test_data", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn restore_not_found() {
        let (db, _dir) = test_db_on_disk();
        let err = db.restore_backup("ghost.db").unwrap_err();
        assert!(matches!(err, BackupRepoError::NotFound { .. }));
    }

    #[test]
    fn list_empty_when_no_backups() {
        let (db, _dir) = test_db_on_disk();
        let list = db.list_backups().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn in_memory_db_create_backup_fails() {
        let db = Database::open_in_memory(&PoolConfig::default()).unwrap();
        let err = db.create_backup("test.db").unwrap_err();
        assert!(matches!(err, BackupRepoError::OperationFailed { .. }));
    }

    #[test]
    fn in_memory_db_list_backups_returns_empty() {
        let db = Database::open_in_memory(&PoolConfig::default()).unwrap();
        let list = db.list_backups().unwrap();
        assert!(list.is_empty());
    }
}
