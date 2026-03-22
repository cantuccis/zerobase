//! External auth repository — SQLite implementation.
//!
//! Implements [`ExternalAuthRepository`] on [`Database`] for CRUD operations
//! on the `_externalAuths` system table.

use rusqlite::params;

use zerobase_core::error::ZerobaseError;
use zerobase_core::services::external_auth::{ExternalAuth, ExternalAuthRepository};

use crate::error::map_query_error;
use crate::pool::Database;

fn row_to_external_auth(row: &rusqlite::Row) -> rusqlite::Result<ExternalAuth> {
    Ok(ExternalAuth {
        id: row.get("id")?,
        collection_id: row.get("collection_id")?,
        record_id: row.get("record_id")?,
        provider: row.get("provider")?,
        provider_id: row.get("provider_id")?,
        created: row.get("created")?,
        updated: row.get("updated")?,
    })
}

impl ExternalAuthRepository for Database {
    fn find_by_provider(
        &self,
        provider: &str,
        provider_id: &str,
    ) -> Result<Option<ExternalAuth>, ZerobaseError> {
        let conn = self.read_conn().map_err(ZerobaseError::from)?;

        let mut stmt = conn
            .prepare(
                "SELECT id, collection_id, record_id, provider, provider_id, created, updated
                 FROM _externalAuths
                 WHERE provider = ?1 AND provider_id = ?2",
            )
            .map_err(|e| ZerobaseError::from(map_query_error(e)))?;

        match stmt.query_row(params![provider, provider_id], row_to_external_auth) {
            Ok(auth) => Ok(Some(auth)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ZerobaseError::from(map_query_error(e))),
        }
    }

    fn find_by_record(
        &self,
        collection_id: &str,
        record_id: &str,
    ) -> Result<Vec<ExternalAuth>, ZerobaseError> {
        let conn = self.read_conn().map_err(ZerobaseError::from)?;

        let mut stmt = conn
            .prepare(
                "SELECT id, collection_id, record_id, provider, provider_id, created, updated
                 FROM _externalAuths
                 WHERE collection_id = ?1 AND record_id = ?2
                 ORDER BY created ASC",
            )
            .map_err(|e| ZerobaseError::from(map_query_error(e)))?;

        let rows = stmt
            .query_map(params![collection_id, record_id], row_to_external_auth)
            .map_err(|e| ZerobaseError::from(map_query_error(e)))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| ZerobaseError::from(map_query_error(e)))?);
        }
        Ok(results)
    }

    fn create(&self, auth: &ExternalAuth) -> Result<(), ZerobaseError> {
        self.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO _externalAuths (id, collection_id, record_id, provider, provider_id, created, updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    auth.id,
                    auth.collection_id,
                    auth.record_id,
                    auth.provider,
                    auth.provider_id,
                    auth.created,
                    auth.updated,
                ],
            )
            .map_err(map_query_error)?;
            Ok(())
        })
        .map_err(ZerobaseError::from)
    }

    fn delete(&self, id: &str) -> Result<(), ZerobaseError> {
        self.with_write_conn(|conn| {
            conn.execute("DELETE FROM _externalAuths WHERE id = ?1", params![id])
                .map_err(map_query_error)?;
            Ok(())
        })
        .map_err(ZerobaseError::from)
    }

    fn delete_by_record(&self, collection_id: &str, record_id: &str) -> Result<(), ZerobaseError> {
        self.with_write_conn(|conn| {
            conn.execute(
                "DELETE FROM _externalAuths WHERE collection_id = ?1 AND record_id = ?2",
                params![collection_id, record_id],
            )
            .map_err(map_query_error)?;
            Ok(())
        })
        .map_err(ZerobaseError::from)
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

    fn sample_auth(id: &str, provider: &str, provider_id: &str, record_id: &str) -> ExternalAuth {
        ExternalAuth {
            id: id.to_string(),
            collection_id: "col1".to_string(),
            record_id: record_id.to_string(),
            provider: provider.to_string(),
            provider_id: provider_id.to_string(),
            created: "2025-01-01 00:00:00".to_string(),
            updated: "2025-01-01 00:00:00".to_string(),
        }
    }

    #[test]
    fn create_and_find_by_provider() {
        let db = test_db();
        let auth = sample_auth("ea1", "google", "gid1", "rec1");
        db.create(&auth).unwrap();

        let found = db.find_by_provider("google", "gid1").unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.record_id, "rec1");
        assert_eq!(found.provider, "google");
    }

    #[test]
    fn find_by_provider_returns_none_for_unknown() {
        let db = test_db();
        assert!(db
            .find_by_provider("google", "nonexistent")
            .unwrap()
            .is_none());
    }

    #[test]
    fn find_by_record() {
        let db = test_db();
        db.create(&sample_auth("ea1", "google", "gid1", "rec1"))
            .unwrap();
        db.create(&sample_auth("ea2", "github", "ghid1", "rec1"))
            .unwrap();
        db.create(&sample_auth("ea3", "google", "gid2", "rec2"))
            .unwrap();

        let auths = db.find_by_record("col1", "rec1").unwrap();
        assert_eq!(auths.len(), 2);
    }

    #[test]
    fn delete_by_id() {
        let db = test_db();
        db.create(&sample_auth("ea1", "google", "gid1", "rec1"))
            .unwrap();
        db.delete("ea1").unwrap();
        assert!(db.find_by_provider("google", "gid1").unwrap().is_none());
    }

    #[test]
    fn delete_by_record() {
        let db = test_db();
        db.create(&sample_auth("ea1", "google", "gid1", "rec1"))
            .unwrap();
        db.create(&sample_auth("ea2", "github", "ghid1", "rec1"))
            .unwrap();

        db.delete_by_record("col1", "rec1").unwrap();
        assert!(db.find_by_record("col1", "rec1").unwrap().is_empty());
    }

    #[test]
    fn duplicate_provider_id_rejected() {
        let db = test_db();
        db.create(&sample_auth("ea1", "google", "gid1", "rec1"))
            .unwrap();
        // Same provider + provider_id but different record.
        let result = db.create(&sample_auth("ea2", "google", "gid1", "rec2"));
        assert!(result.is_err());
    }

    #[test]
    fn duplicate_record_provider_rejected() {
        let db = test_db();
        db.create(&sample_auth("ea1", "google", "gid1", "rec1"))
            .unwrap();
        // Same record + provider but different provider_id.
        let result = db.create(&sample_auth("ea2", "google", "gid2", "rec1"));
        assert!(result.is_err());
    }
}
