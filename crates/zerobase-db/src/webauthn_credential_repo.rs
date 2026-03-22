//! WebAuthn credential repository — SQLite implementation.
//!
//! Implements [`WebauthnCredentialRepository`] on [`Database`] for CRUD operations
//! on the `_webauthn_credentials` system table.

use rusqlite::params;

use zerobase_core::error::ZerobaseError;
use zerobase_core::services::webauthn_credential::{
    WebauthnCredential, WebauthnCredentialRepository,
};

use crate::error::map_query_error;
use crate::pool::Database;

fn row_to_credential(row: &rusqlite::Row) -> rusqlite::Result<WebauthnCredential> {
    Ok(WebauthnCredential {
        id: row.get("id")?,
        collection_id: row.get("collection_id")?,
        record_id: row.get("record_id")?,
        name: row.get("name")?,
        credential_id: row.get("credential_id")?,
        credential_data: row.get("credential_data")?,
        created: row.get("created")?,
        updated: row.get("updated")?,
    })
}

impl WebauthnCredentialRepository for Database {
    fn find_by_credential_id(
        &self,
        credential_id: &str,
    ) -> Result<Option<WebauthnCredential>, ZerobaseError> {
        let conn = self.read_conn().map_err(ZerobaseError::from)?;

        let mut stmt = conn
            .prepare(
                "SELECT id, collection_id, record_id, name, credential_id, credential_data, created, updated
                 FROM _webauthn_credentials
                 WHERE credential_id = ?1",
            )
            .map_err(|e| ZerobaseError::from(map_query_error(e)))?;

        match stmt.query_row(params![credential_id], row_to_credential) {
            Ok(cred) => Ok(Some(cred)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ZerobaseError::from(map_query_error(e))),
        }
    }

    fn find_by_record(
        &self,
        collection_id: &str,
        record_id: &str,
    ) -> Result<Vec<WebauthnCredential>, ZerobaseError> {
        let conn = self.read_conn().map_err(ZerobaseError::from)?;

        let mut stmt = conn
            .prepare(
                "SELECT id, collection_id, record_id, name, credential_id, credential_data, created, updated
                 FROM _webauthn_credentials
                 WHERE collection_id = ?1 AND record_id = ?2
                 ORDER BY created ASC",
            )
            .map_err(|e| ZerobaseError::from(map_query_error(e)))?;

        let rows = stmt
            .query_map(params![collection_id, record_id], row_to_credential)
            .map_err(|e| ZerobaseError::from(map_query_error(e)))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| ZerobaseError::from(map_query_error(e)))?);
        }
        Ok(results)
    }

    fn find_by_collection(
        &self,
        collection_id: &str,
    ) -> Result<Vec<WebauthnCredential>, ZerobaseError> {
        let conn = self.read_conn().map_err(ZerobaseError::from)?;

        let mut stmt = conn
            .prepare(
                "SELECT id, collection_id, record_id, name, credential_id, credential_data, created, updated
                 FROM _webauthn_credentials
                 WHERE collection_id = ?1
                 ORDER BY created ASC",
            )
            .map_err(|e| ZerobaseError::from(map_query_error(e)))?;

        let rows = stmt
            .query_map(params![collection_id], row_to_credential)
            .map_err(|e| ZerobaseError::from(map_query_error(e)))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| ZerobaseError::from(map_query_error(e)))?);
        }
        Ok(results)
    }

    fn create(&self, credential: &WebauthnCredential) -> Result<(), ZerobaseError> {
        self.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO _webauthn_credentials (id, collection_id, record_id, name, credential_id, credential_data, created, updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    credential.id,
                    credential.collection_id,
                    credential.record_id,
                    credential.name,
                    credential.credential_id,
                    credential.credential_data,
                    credential.created,
                    credential.updated,
                ],
            )
            .map_err(map_query_error)?;
            Ok(())
        })
        .map_err(ZerobaseError::from)
    }

    fn delete(&self, id: &str) -> Result<(), ZerobaseError> {
        self.with_write_conn(|conn| {
            conn.execute(
                "DELETE FROM _webauthn_credentials WHERE id = ?1",
                params![id],
            )
            .map_err(map_query_error)?;
            Ok(())
        })
        .map_err(ZerobaseError::from)
    }

    fn delete_by_record(&self, collection_id: &str, record_id: &str) -> Result<(), ZerobaseError> {
        self.with_write_conn(|conn| {
            conn.execute(
                "DELETE FROM _webauthn_credentials WHERE collection_id = ?1 AND record_id = ?2",
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

    fn sample_credential(id: &str, record_id: &str, cred_id: &str) -> WebauthnCredential {
        WebauthnCredential {
            id: id.to_string(),
            collection_id: "col1".to_string(),
            record_id: record_id.to_string(),
            name: "My Passkey".to_string(),
            credential_id: cred_id.to_string(),
            credential_data: r#"{"type":"passkey","data":"test"}"#.to_string(),
            created: "2025-01-01 00:00:00".to_string(),
            updated: "2025-01-01 00:00:00".to_string(),
        }
    }

    #[test]
    fn create_and_find_by_credential_id() {
        let db = test_db();
        let cred = sample_credential("wc1", "rec1", "cred_abc123");
        db.create(&cred).unwrap();

        let found = db.find_by_credential_id("cred_abc123").unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.record_id, "rec1");
        assert_eq!(found.name, "My Passkey");
    }

    #[test]
    fn find_by_credential_id_returns_none_for_unknown() {
        let db = test_db();
        assert!(db.find_by_credential_id("nonexistent").unwrap().is_none());
    }

    #[test]
    fn find_by_record() {
        let db = test_db();
        db.create(&sample_credential("wc1", "rec1", "cred1"))
            .unwrap();
        db.create(&sample_credential("wc2", "rec1", "cred2"))
            .unwrap();
        db.create(&sample_credential("wc3", "rec2", "cred3"))
            .unwrap();

        let creds = db.find_by_record("col1", "rec1").unwrap();
        assert_eq!(creds.len(), 2);
    }

    #[test]
    fn find_by_collection() {
        let db = test_db();
        db.create(&sample_credential("wc1", "rec1", "cred1"))
            .unwrap();
        db.create(&sample_credential("wc2", "rec2", "cred2"))
            .unwrap();

        let creds = db.find_by_collection("col1").unwrap();
        assert_eq!(creds.len(), 2);
    }

    #[test]
    fn delete_by_id() {
        let db = test_db();
        db.create(&sample_credential("wc1", "rec1", "cred1"))
            .unwrap();
        db.delete("wc1").unwrap();
        assert!(db.find_by_credential_id("cred1").unwrap().is_none());
    }

    #[test]
    fn delete_by_record() {
        let db = test_db();
        db.create(&sample_credential("wc1", "rec1", "cred1"))
            .unwrap();
        db.create(&sample_credential("wc2", "rec1", "cred2"))
            .unwrap();

        db.delete_by_record("col1", "rec1").unwrap();
        assert!(db.find_by_record("col1", "rec1").unwrap().is_empty());
    }

    #[test]
    fn duplicate_credential_id_rejected() {
        let db = test_db();
        db.create(&sample_credential("wc1", "rec1", "cred1"))
            .unwrap();
        let result = db.create(&sample_credential("wc2", "rec2", "cred1"));
        assert!(result.is_err());
    }

    #[test]
    fn multiple_passkeys_per_user_supported() {
        let db = test_db();
        db.create(&sample_credential("wc1", "rec1", "cred_a"))
            .unwrap();
        db.create(&sample_credential("wc2", "rec1", "cred_b"))
            .unwrap();
        db.create(&sample_credential("wc3", "rec1", "cred_c"))
            .unwrap();

        let creds = db.find_by_record("col1", "rec1").unwrap();
        assert_eq!(creds.len(), 3);
    }
}
