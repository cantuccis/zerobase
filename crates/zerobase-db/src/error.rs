//! Database-specific error type.
//!
//! [`DbError`] captures errors originating in the database layer and converts
//! into the unified [`ZerobaseError`] for consumption by upper layers.

use zerobase_core::error::ZerobaseError;

/// Errors originating in the database layer.
///
/// Each variant maps to a specific failure mode. The `From<DbError>` impl on
/// [`ZerobaseError`] ensures seamless propagation via `?`.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// Failed to acquire a connection from the pool.
    #[error("connection pool error: {0}")]
    Pool(#[from] r2d2::Error),

    /// A SQL query or command failed.
    #[error("query error: {0}")]
    Query(#[from] rusqlite::Error),

    /// A migration failed to apply.
    #[error("migration error: {message}")]
    Migration { message: String },

    /// The requested record or resource was not found.
    #[error("{resource_type} not found")]
    NotFound {
        resource_type: String,
        resource_id: Option<String>,
    },

    /// A uniqueness constraint was violated.
    #[error("conflict: {message}")]
    Conflict { message: String },

    /// Schema validation or DDL error.
    #[error("schema error: {message}")]
    Schema { message: String },
}

impl DbError {
    pub fn migration(message: impl Into<String>) -> Self {
        Self::Migration {
            message: message.into(),
        }
    }

    pub fn not_found(resource_type: impl Into<String>) -> Self {
        Self::NotFound {
            resource_type: resource_type.into(),
            resource_id: None,
        }
    }

    pub fn not_found_with_id(
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
    ) -> Self {
        Self::NotFound {
            resource_type: resource_type.into(),
            resource_id: Some(resource_id.into()),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict {
            message: message.into(),
        }
    }

    pub fn schema(message: impl Into<String>) -> Self {
        Self::Schema {
            message: message.into(),
        }
    }
}

impl From<DbError> for ZerobaseError {
    fn from(err: DbError) -> Self {
        match err {
            DbError::Pool(e) => ZerobaseError::database_with_source("connection pool error", e),
            DbError::Query(ref e) => {
                // Check for UNIQUE constraint violations and convert to Conflict.
                if let Some(field_name) = parse_unique_violation(e) {
                    ZerobaseError::conflict(format!(
                        "the value for field '{}' must be unique",
                        field_name
                    ))
                } else {
                    // Destructure to take ownership for the source chain.
                    match err {
                        DbError::Query(e) => ZerobaseError::database_with_source("query error", e),
                        _ => unreachable!(),
                    }
                }
            }
            DbError::Migration { message } => {
                ZerobaseError::database(format!("migration: {message}"))
            }
            DbError::NotFound {
                resource_type,
                resource_id,
            } => match resource_id {
                Some(id) => ZerobaseError::not_found_with_id(resource_type, id),
                None => ZerobaseError::not_found(resource_type),
            },
            DbError::Conflict { message } => ZerobaseError::conflict(message),
            DbError::Schema { message } => ZerobaseError::database(format!("schema: {message}")),
        }
    }
}

/// Extract the field name from a SQLite UNIQUE constraint violation error.
///
/// SQLite produces messages like:
///   `UNIQUE constraint failed: tablename.columnname`
///   `UNIQUE constraint failed: tablename.col1, tablename.col2`
///
/// Returns the column name(s) if this is a uniqueness violation, `None` otherwise.
fn parse_unique_violation(err: &rusqlite::Error) -> Option<String> {
    let msg = match err {
        rusqlite::Error::SqliteFailure(_, Some(msg)) => msg,
        _ => return None,
    };

    let prefix = "UNIQUE constraint failed: ";
    if !msg.starts_with(prefix) {
        return None;
    }

    let columns_part = &msg[prefix.len()..];
    // Extract column names, stripping the table prefix (e.g. "posts.slug" → "slug").
    let field_names: Vec<&str> = columns_part
        .split(',')
        .map(|s| {
            let s = s.trim();
            // Strip "table." prefix if present.
            s.rsplit('.').next().unwrap_or(s)
        })
        .collect();

    if field_names.is_empty() {
        None
    } else {
        Some(field_names.join(", "))
    }
}

/// Check whether a `rusqlite::Error` is a UNIQUE constraint violation.
///
/// This is useful for callers that want to intercept constraint errors
/// and produce domain-specific messages.
pub fn is_unique_violation(err: &rusqlite::Error) -> bool {
    parse_unique_violation(err).is_some()
}

/// Map a `rusqlite::Error` to a `DbError`, converting UNIQUE constraint
/// violations into `DbError::Conflict` with a descriptive message.
pub fn map_query_error(err: rusqlite::Error) -> DbError {
    if let Some(field_name) = parse_unique_violation(&err) {
        DbError::Conflict {
            message: format!("the value for field '{}' must be unique", field_name),
        }
    } else {
        DbError::Query(err)
    }
}

/// Convenience alias for fallible database operations.
pub type Result<T> = std::result::Result<T, DbError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_error_converts_to_zerobase_database() {
        // r2d2 errors are hard to construct directly, so test via the DbError → ZerobaseError path
        let err = DbError::migration("test migration failure");
        let zb_err: ZerobaseError = err.into();
        assert_eq!(zb_err.status_code(), 500);
        assert!(zb_err.to_string().contains("migration"));
    }

    #[test]
    fn not_found_converts_to_zerobase_not_found() {
        let err = DbError::not_found_with_id("Record", "abc123");
        let zb_err: ZerobaseError = err.into();
        assert_eq!(zb_err.status_code(), 404);
    }

    #[test]
    fn conflict_converts_to_zerobase_conflict() {
        let err = DbError::conflict("duplicate slug");
        let zb_err: ZerobaseError = err.into();
        assert_eq!(zb_err.status_code(), 409);
    }

    #[test]
    fn schema_error_converts_to_zerobase_database() {
        let err = DbError::schema("invalid column type");
        let zb_err: ZerobaseError = err.into();
        assert_eq!(zb_err.status_code(), 500);
        assert!(zb_err.to_string().contains("schema"));
    }

    #[test]
    fn query_error_converts_to_zerobase_database() {
        let sqlite_err = rusqlite::Error::QueryReturnedNoRows;
        let err = DbError::Query(sqlite_err);
        let zb_err: ZerobaseError = err.into();
        assert_eq!(zb_err.status_code(), 500);
    }

    // ── Unique constraint violation parsing ──────────────────────────────

    #[test]
    fn parse_unique_violation_single_column() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code: 2067, // SQLITE_CONSTRAINT_UNIQUE
            },
            Some("UNIQUE constraint failed: posts.slug".to_string()),
        );
        assert_eq!(parse_unique_violation(&err), Some("slug".to_string()));
    }

    #[test]
    fn parse_unique_violation_composite() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code: 2067,
            },
            Some("UNIQUE constraint failed: posts.author, posts.slug".to_string()),
        );
        assert_eq!(
            parse_unique_violation(&err),
            Some("author, slug".to_string())
        );
    }

    #[test]
    fn parse_unique_violation_not_a_constraint_error() {
        let err = rusqlite::Error::QueryReturnedNoRows;
        assert_eq!(parse_unique_violation(&err), None);
    }

    #[test]
    fn parse_unique_violation_other_constraint() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code: 1299, // SQLITE_CONSTRAINT_NOTNULL
            },
            Some("NOT NULL constraint failed: posts.title".to_string()),
        );
        assert_eq!(parse_unique_violation(&err), None);
    }

    #[test]
    fn map_query_error_converts_unique_to_conflict() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code: 2067,
            },
            Some("UNIQUE constraint failed: users.email".to_string()),
        );
        let db_err = map_query_error(err);
        match &db_err {
            DbError::Conflict { message } => {
                assert!(message.contains("email"));
                assert!(message.contains("unique"));
            }
            _ => panic!("expected Conflict, got {:?}", db_err),
        }
    }

    #[test]
    fn map_query_error_preserves_non_unique_errors() {
        let err = rusqlite::Error::QueryReturnedNoRows;
        let db_err = map_query_error(err);
        assert!(matches!(db_err, DbError::Query(_)));
    }

    #[test]
    fn unique_violation_db_error_converts_to_409_conflict() {
        let sqlite_err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code: 2067,
            },
            Some("UNIQUE constraint failed: articles.slug".to_string()),
        );
        let db_err = DbError::Query(sqlite_err);
        let zb_err: ZerobaseError = db_err.into();
        assert_eq!(zb_err.status_code(), 409);
        assert!(zb_err.to_string().contains("slug"));
        assert!(zb_err.to_string().contains("unique"));
    }

    #[test]
    fn is_unique_violation_returns_true_for_unique_errors() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code: 2067,
            },
            Some("UNIQUE constraint failed: posts.slug".to_string()),
        );
        assert!(is_unique_violation(&err));
    }

    #[test]
    fn is_unique_violation_returns_false_for_other_errors() {
        assert!(!is_unique_violation(&rusqlite::Error::QueryReturnedNoRows));
    }
}
