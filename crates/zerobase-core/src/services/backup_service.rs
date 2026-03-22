//! Backup and restore service.
//!
//! [`BackupService`] manages creating, listing, downloading, deleting,
//! and restoring database backups. Backups are SQLite database copies
//! stored as files.
//!
//! # Design
//!
//! - The service is generic over `R: BackupRepository` for testability.
//! - Backup names follow the pattern `pb_backup_<timestamp>.db` when
//!   no custom name is given.
//! - Restore replaces the current database from a backup file.

use serde::{Deserialize, Serialize};

use crate::error::{Result, ZerobaseError};

// ── Repository trait ────────────────────────────────────────────────────────

/// Persistence and I/O contract for backup operations.
///
/// Defined in core so the service doesn't depend on `zerobase-db` directly.
/// The DB crate implements this trait on `Database`.
pub trait BackupRepository: Send + Sync {
    /// Create a backup of the current database.
    ///
    /// The `name` is the filename for the backup (e.g. `"my_backup.db"`).
    /// Returns metadata about the created backup.
    fn create_backup(&self, name: &str) -> std::result::Result<BackupInfo, BackupRepoError>;

    /// List all available backups, ordered by creation time (newest first).
    fn list_backups(&self) -> std::result::Result<Vec<BackupInfo>, BackupRepoError>;

    /// Get metadata for a single backup by name.
    fn get_backup(&self, name: &str) -> std::result::Result<BackupInfo, BackupRepoError>;

    /// Get the full filesystem path for a backup file (for downloads).
    fn backup_path(&self, name: &str) -> std::result::Result<String, BackupRepoError>;

    /// Delete a backup by name.
    fn delete_backup(&self, name: &str) -> std::result::Result<(), BackupRepoError>;

    /// Restore the database from a named backup.
    ///
    /// This replaces the current database contents with the backup.
    /// **Warning**: This is a destructive operation.
    fn restore_backup(&self, name: &str) -> std::result::Result<(), BackupRepoError>;
}

/// Errors that a backup repository can produce.
#[derive(Debug, thiserror::Error)]
pub enum BackupRepoError {
    #[error("backup not found: {name}")]
    NotFound { name: String },
    #[error("backup already exists: {name}")]
    AlreadyExists { name: String },
    #[error("backup operation failed: {message}")]
    OperationFailed { message: String },
    #[error("invalid backup name: {message}")]
    InvalidName { message: String },
}

// ── DTOs ────────────────────────────────────────────────────────────────────

/// Metadata about a single backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    /// Backup filename (e.g. `"pb_backup_20250101120000.db"`).
    pub name: String,
    /// File size in bytes.
    pub size: u64,
    /// Creation timestamp (ISO 8601).
    pub created: String,
    /// Last modified timestamp (ISO 8601).
    pub modified: String,
}

/// Request body for creating a backup.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateBackupRequest {
    /// Optional custom name for the backup. If omitted, an auto-generated
    /// name based on the current timestamp is used.
    pub name: Option<String>,
}

// ── Service ─────────────────────────────────────────────────────────────────

/// Application service for backup management.
///
/// Wraps a [`BackupRepository`] implementation and adds input validation,
/// name generation, and error mapping.
pub struct BackupService<R: BackupRepository> {
    repo: R,
}

impl<R: BackupRepository> BackupService<R> {
    /// Create a new backup service wrapping the given repository.
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    /// Create a new backup.
    ///
    /// If `name` is `None`, generates a timestamped name.
    /// The name must end with `.db` or `.zip`.
    pub fn create_backup(&self, name: Option<&str>) -> Result<BackupInfo> {
        let backup_name = match name {
            Some(n) => {
                let n = n.trim();
                if n.is_empty() {
                    return Err(ZerobaseError::validation("backup name cannot be empty"));
                }
                Self::validate_backup_name(n)?;
                n.to_string()
            }
            None => Self::generate_backup_name(),
        };

        self.repo.create_backup(&backup_name).map_err(map_repo_error)
    }

    /// List all available backups.
    pub fn list_backups(&self) -> Result<Vec<BackupInfo>> {
        self.repo.list_backups().map_err(map_repo_error)
    }

    /// Get metadata for a single backup.
    pub fn get_backup(&self, name: &str) -> Result<BackupInfo> {
        self.repo.get_backup(name).map_err(map_repo_error)
    }

    /// Get the filesystem path for downloading a backup.
    pub fn backup_path(&self, name: &str) -> Result<String> {
        self.repo.backup_path(name).map_err(map_repo_error)
    }

    /// Delete a backup by name.
    pub fn delete_backup(&self, name: &str) -> Result<()> {
        if name.trim().is_empty() {
            return Err(ZerobaseError::validation("backup name cannot be empty"));
        }
        self.repo.delete_backup(name).map_err(map_repo_error)
    }

    /// Restore the database from a backup.
    pub fn restore_backup(&self, name: &str) -> Result<()> {
        if name.trim().is_empty() {
            return Err(ZerobaseError::validation("backup name cannot be empty"));
        }
        self.repo.restore_backup(name).map_err(map_repo_error)
    }

    /// Generate a timestamped backup name.
    fn generate_backup_name() -> String {
        let now = chrono::Utc::now();
        format!("pb_backup_{}.db", now.format("%Y%m%d%H%M%S"))
    }

    /// Validate that a backup name is safe and well-formed.
    fn validate_backup_name(name: &str) -> Result<()> {
        // Must end with .db or .zip
        if !name.ends_with(".db") && !name.ends_with(".zip") {
            return Err(ZerobaseError::validation(
                "backup name must end with .db or .zip",
            ));
        }

        // Must not contain path separators or dangerous characters
        if name.contains('/')
            || name.contains('\\')
            || name.contains("..")
            || name.contains('\0')
        {
            return Err(ZerobaseError::validation(
                "backup name contains invalid characters",
            ));
        }

        // Must be a reasonable length
        if name.len() > 200 {
            return Err(ZerobaseError::validation("backup name is too long"));
        }

        Ok(())
    }
}

// ── Error mapping ───────────────────────────────────────────────────────────

fn map_repo_error(err: BackupRepoError) -> ZerobaseError {
    match err {
        BackupRepoError::NotFound { name } => {
            ZerobaseError::not_found_with_id("Backup", name)
        }
        BackupRepoError::AlreadyExists { name } => {
            ZerobaseError::conflict(format!("backup '{name}' already exists"))
        }
        BackupRepoError::OperationFailed { message } => {
            ZerobaseError::internal(message)
        }
        BackupRepoError::InvalidName { message } => {
            ZerobaseError::validation(message)
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Mock backup repository for testing.
    struct MockBackupRepo {
        backups: Mutex<Vec<BackupInfo>>,
        fail_create: bool,
        fail_restore: bool,
    }

    impl MockBackupRepo {
        fn new() -> Self {
            Self {
                backups: Mutex::new(Vec::new()),
                fail_create: false,
                fail_restore: false,
            }
        }

        fn with_backups(backups: Vec<BackupInfo>) -> Self {
            Self {
                backups: Mutex::new(backups),
                fail_create: false,
                fail_restore: false,
            }
        }
    }

    impl BackupRepository for MockBackupRepo {
        fn create_backup(&self, name: &str) -> std::result::Result<BackupInfo, BackupRepoError> {
            if self.fail_create {
                return Err(BackupRepoError::OperationFailed {
                    message: "disk full".to_string(),
                });
            }
            let mut backups = self.backups.lock().unwrap();
            if backups.iter().any(|b| b.name == name) {
                return Err(BackupRepoError::AlreadyExists {
                    name: name.to_string(),
                });
            }
            let info = BackupInfo {
                name: name.to_string(),
                size: 1024,
                created: "2025-01-01 00:00:00.000Z".to_string(),
                modified: "2025-01-01 00:00:00.000Z".to_string(),
            };
            backups.push(info.clone());
            Ok(info)
        }

        fn list_backups(&self) -> std::result::Result<Vec<BackupInfo>, BackupRepoError> {
            let backups = self.backups.lock().unwrap();
            let mut list = backups.clone();
            list.reverse();
            Ok(list)
        }

        fn get_backup(&self, name: &str) -> std::result::Result<BackupInfo, BackupRepoError> {
            let backups = self.backups.lock().unwrap();
            backups
                .iter()
                .find(|b| b.name == name)
                .cloned()
                .ok_or(BackupRepoError::NotFound {
                    name: name.to_string(),
                })
        }

        fn backup_path(&self, name: &str) -> std::result::Result<String, BackupRepoError> {
            let backups = self.backups.lock().unwrap();
            if backups.iter().any(|b| b.name == name) {
                Ok(format!("/backups/{name}"))
            } else {
                Err(BackupRepoError::NotFound {
                    name: name.to_string(),
                })
            }
        }

        fn delete_backup(&self, name: &str) -> std::result::Result<(), BackupRepoError> {
            let mut backups = self.backups.lock().unwrap();
            let len_before = backups.len();
            backups.retain(|b| b.name != name);
            if backups.len() == len_before {
                Err(BackupRepoError::NotFound {
                    name: name.to_string(),
                })
            } else {
                Ok(())
            }
        }

        fn restore_backup(&self, name: &str) -> std::result::Result<(), BackupRepoError> {
            if self.fail_restore {
                return Err(BackupRepoError::OperationFailed {
                    message: "restore failed".to_string(),
                });
            }
            let backups = self.backups.lock().unwrap();
            if backups.iter().any(|b| b.name == name) {
                Ok(())
            } else {
                Err(BackupRepoError::NotFound {
                    name: name.to_string(),
                })
            }
        }
    }

    fn sample_backup(name: &str) -> BackupInfo {
        BackupInfo {
            name: name.to_string(),
            size: 2048,
            created: "2025-01-01 12:00:00.000Z".to_string(),
            modified: "2025-01-01 12:00:00.000Z".to_string(),
        }
    }

    // ── Create ──────────────────────────────────────────────────────────

    #[test]
    fn create_backup_with_custom_name() {
        let repo = MockBackupRepo::new();
        let service = BackupService::new(repo);
        let info = service.create_backup(Some("my_backup.db")).unwrap();
        assert_eq!(info.name, "my_backup.db");
    }

    #[test]
    fn create_backup_with_auto_name() {
        let repo = MockBackupRepo::new();
        let service = BackupService::new(repo);
        let info = service.create_backup(None).unwrap();
        assert!(info.name.starts_with("pb_backup_"));
        assert!(info.name.ends_with(".db"));
    }

    #[test]
    fn create_backup_rejects_empty_name() {
        let repo = MockBackupRepo::new();
        let service = BackupService::new(repo);
        let err = service.create_backup(Some("")).unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn create_backup_rejects_invalid_extension() {
        let repo = MockBackupRepo::new();
        let service = BackupService::new(repo);
        let err = service.create_backup(Some("backup.txt")).unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn create_backup_rejects_path_traversal() {
        let repo = MockBackupRepo::new();
        let service = BackupService::new(repo);
        let err = service.create_backup(Some("../evil.db")).unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn create_backup_rejects_slash() {
        let repo = MockBackupRepo::new();
        let service = BackupService::new(repo);
        let err = service.create_backup(Some("/etc/passwd.db")).unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn create_backup_allows_zip_extension() {
        let repo = MockBackupRepo::new();
        let service = BackupService::new(repo);
        let info = service.create_backup(Some("backup.zip")).unwrap();
        assert_eq!(info.name, "backup.zip");
    }

    #[test]
    fn create_backup_conflict_when_exists() {
        let repo = MockBackupRepo::with_backups(vec![sample_backup("existing.db")]);
        let service = BackupService::new(repo);
        let err = service.create_backup(Some("existing.db")).unwrap_err();
        assert_eq!(err.status_code(), 409);
    }

    // ── List ────────────────────────────────────────────────────────────

    #[test]
    fn list_backups_returns_all() {
        let repo = MockBackupRepo::with_backups(vec![
            sample_backup("a.db"),
            sample_backup("b.db"),
        ]);
        let service = BackupService::new(repo);
        let list = service.list_backups().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn list_backups_empty() {
        let repo = MockBackupRepo::new();
        let service = BackupService::new(repo);
        let list = service.list_backups().unwrap();
        assert!(list.is_empty());
    }

    // ── Get ─────────────────────────────────────────────────────────────

    #[test]
    fn get_backup_found() {
        let repo = MockBackupRepo::with_backups(vec![sample_backup("test.db")]);
        let service = BackupService::new(repo);
        let info = service.get_backup("test.db").unwrap();
        assert_eq!(info.name, "test.db");
    }

    #[test]
    fn get_backup_not_found() {
        let repo = MockBackupRepo::new();
        let service = BackupService::new(repo);
        let err = service.get_backup("missing.db").unwrap_err();
        assert_eq!(err.status_code(), 404);
    }

    // ── Delete ──────────────────────────────────────────────────────────

    #[test]
    fn delete_backup_success() {
        let repo = MockBackupRepo::with_backups(vec![sample_backup("del.db")]);
        let service = BackupService::new(repo);
        service.delete_backup("del.db").unwrap();
        let list = service.list_backups().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn delete_backup_not_found() {
        let repo = MockBackupRepo::new();
        let service = BackupService::new(repo);
        let err = service.delete_backup("missing.db").unwrap_err();
        assert_eq!(err.status_code(), 404);
    }

    #[test]
    fn delete_backup_rejects_empty_name() {
        let repo = MockBackupRepo::new();
        let service = BackupService::new(repo);
        let err = service.delete_backup("").unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    // ── Restore ─────────────────────────────────────────────────────────

    #[test]
    fn restore_backup_success() {
        let repo = MockBackupRepo::with_backups(vec![sample_backup("restore.db")]);
        let service = BackupService::new(repo);
        service.restore_backup("restore.db").unwrap();
    }

    #[test]
    fn restore_backup_not_found() {
        let repo = MockBackupRepo::new();
        let service = BackupService::new(repo);
        let err = service.restore_backup("missing.db").unwrap_err();
        assert_eq!(err.status_code(), 404);
    }

    #[test]
    fn restore_backup_rejects_empty_name() {
        let repo = MockBackupRepo::new();
        let service = BackupService::new(repo);
        let err = service.restore_backup("").unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    // ── Backup path ─────────────────────────────────────────────────────

    #[test]
    fn backup_path_found() {
        let repo = MockBackupRepo::with_backups(vec![sample_backup("path.db")]);
        let service = BackupService::new(repo);
        let path = service.backup_path("path.db").unwrap();
        assert!(path.contains("path.db"));
    }

    #[test]
    fn backup_path_not_found() {
        let repo = MockBackupRepo::new();
        let service = BackupService::new(repo);
        let err = service.backup_path("missing.db").unwrap_err();
        assert_eq!(err.status_code(), 404);
    }

    // ── Name validation ─────────────────────────────────────────────────

    #[test]
    fn validate_rejects_too_long_name() {
        let long_name = format!("{}.db", "a".repeat(200));
        let err = BackupService::<MockBackupRepo>::validate_backup_name(&long_name).unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn validate_rejects_null_byte() {
        let err =
            BackupService::<MockBackupRepo>::validate_backup_name("bad\0name.db").unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn validate_accepts_valid_names() {
        assert!(BackupService::<MockBackupRepo>::validate_backup_name("backup_2025.db").is_ok());
        assert!(
            BackupService::<MockBackupRepo>::validate_backup_name("my-backup.zip").is_ok()
        );
    }
}
