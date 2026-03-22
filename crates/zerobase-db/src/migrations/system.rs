//! System migrations that create Zerobase's internal tables.
//!
//! These migrations set up the schema that Zerobase needs to operate:
//! - `_migrations` — tracked automatically by the migration runner
//! - `_collections` — collection (table) metadata
//! - `_fields` — field definitions per collection
//! - `_settings` — application key-value settings
//! - `_superusers` — admin accounts
//!
//! All system tables are prefixed with `_` to distinguish them from
//! user-created collections. The initial migration (v1) creates all of
//! them in a single function migration for type safety and atomicity.

use rusqlite::Connection;

use zerobase_core::services::superuser_service::SUPERUSERS_COLLECTION_ID;

use crate::error::{DbError, Result};
use crate::migrations::Migration;

/// Return the list of all system migrations.
///
/// These must be applied before any user operations can run. The migration
/// runner handles idempotency — calling this multiple times is safe.
pub fn system_migrations() -> Vec<Migration> {
    vec![
        Migration::function(1, "create_system_tables", create_system_tables),
        Migration::function(
            2,
            "create_external_auths_table",
            create_external_auths_table_migration,
        ),
        Migration::function(
            3,
            "create_webauthn_credentials_table",
            create_webauthn_credentials_table_migration,
        ),
        Migration::function(
            4,
            "add_view_query_to_collections",
            add_view_query_column_migration,
        ),
        Migration::function(5, "create_logs_table", create_logs_table_migration),
    ]
}

/// Initial migration: create all system tables.
///
/// This is a Rust function (not raw SQL) for type safety — every table
/// creation, index, and constraint is checked at compile time via
/// rusqlite's parameter binding, and complex validation logic can be
/// added without string manipulation.
fn create_system_tables(conn: &Connection) -> Result<()> {
    create_collections_table(conn)?;
    create_fields_table(conn)?;
    create_settings_table(conn)?;
    create_superusers_table(conn)?;
    register_system_collections(conn)?;
    Ok(())
}

/// Create `_collections` — stores metadata for every user-defined collection.
///
/// Each collection maps to a SQLite table. This table tracks the collection's
/// name, type (base/auth/view), and access rules (list, view, create, update,
/// delete). The `id` is a text primary key (15-char alphanumeric, like PocketBase).
fn create_collections_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE _collections (
            id          TEXT PRIMARY KEY NOT NULL,
            name        TEXT NOT NULL UNIQUE,
            type        TEXT NOT NULL DEFAULT 'base'
                        CHECK (type IN ('base', 'auth', 'view')),
            system      INTEGER NOT NULL DEFAULT 0,
            list_rule   TEXT,
            view_rule   TEXT,
            create_rule TEXT,
            update_rule TEXT,
            delete_rule TEXT,
            manage_rule TEXT,
            created     TEXT NOT NULL DEFAULT (datetime('now')),
            updated     TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX idx_collections_name ON _collections(name);
        CREATE INDEX idx_collections_type ON _collections(type);",
    )
    .map_err(|e| DbError::migration(format!("failed to create _collections table: {e}")))?;

    Ok(())
}

/// Create `_fields` — stores field definitions for each collection.
///
/// Each row represents a single field in a collection. Fields have a type
/// (text, number, bool, email, url, date, select, json, file, relation, editor),
/// validation options stored as JSON, and ordering within the collection.
fn create_fields_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE _fields (
            id            TEXT PRIMARY KEY NOT NULL,
            collection_id TEXT NOT NULL REFERENCES _collections(id) ON DELETE CASCADE,
            name          TEXT NOT NULL,
            type          TEXT NOT NULL,
            required      INTEGER NOT NULL DEFAULT 0,
            unique_field  INTEGER NOT NULL DEFAULT 0,
            options       TEXT NOT NULL DEFAULT '{}',
            sort_order    INTEGER NOT NULL DEFAULT 0,
            created       TEXT NOT NULL DEFAULT (datetime('now')),
            updated       TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE (collection_id, name)
        );

        CREATE INDEX idx_fields_collection ON _fields(collection_id);
        CREATE INDEX idx_fields_type ON _fields(type);",
    )
    .map_err(|e| DbError::migration(format!("failed to create _fields table: {e}")))?;

    Ok(())
}

/// Create `_settings` — application-wide key-value settings.
///
/// Stores configuration like SMTP settings, S3 credentials, OAuth providers,
/// app name, etc. Values are stored as JSON for flexibility.
fn create_settings_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE _settings (
            key     TEXT PRIMARY KEY NOT NULL,
            value   TEXT NOT NULL DEFAULT '{}',
            updated TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .map_err(|e| DbError::migration(format!("failed to create _settings table: {e}")))?;

    Ok(())
}

/// Create `_superusers` — admin accounts that can manage the system.
///
/// Superusers have access to the admin dashboard, can modify collections,
/// fields, settings, and perform any operation. Passwords are stored as
/// argon2 hashes.
fn create_superusers_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE _superusers (
            id        TEXT PRIMARY KEY NOT NULL,
            email     TEXT NOT NULL UNIQUE,
            password  TEXT NOT NULL,
            tokenKey  TEXT NOT NULL DEFAULT '',
            created   TEXT NOT NULL DEFAULT (datetime('now')),
            updated   TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE UNIQUE INDEX idx_superusers_email ON _superusers(email);",
    )
    .map_err(|e| DbError::migration(format!("failed to create _superusers table: {e}")))?;

    Ok(())
}

/// Migration v2: create the `_externalAuths` table for OAuth2 account linking.
///
/// Each row links a local user record to an external OAuth2 provider identity.
/// The `(collection_id, record_id, provider)` tuple is unique — a user can only
/// have one link per provider. The `(provider, provider_id)` tuple is also unique
/// — an external identity can only be linked to one local account.
fn create_external_auths_table_migration(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _externalAuths (
            id            TEXT PRIMARY KEY NOT NULL,
            collection_id TEXT NOT NULL,
            record_id     TEXT NOT NULL,
            provider      TEXT NOT NULL,
            provider_id   TEXT NOT NULL,
            created       TEXT NOT NULL DEFAULT (datetime('now')),
            updated       TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_externalAuths_record_provider
            ON _externalAuths(collection_id, record_id, provider);

        CREATE UNIQUE INDEX IF NOT EXISTS idx_externalAuths_provider_id
            ON _externalAuths(provider, provider_id);

        CREATE INDEX IF NOT EXISTS idx_externalAuths_collection
            ON _externalAuths(collection_id);

        CREATE INDEX IF NOT EXISTS idx_externalAuths_record
            ON _externalAuths(record_id);",
    )
    .map_err(|e| DbError::migration(format!("failed to create _externalAuths table: {e}")))?;

    Ok(())
}

/// Migration v3: create the `_webauthn_credentials` table for passkey storage.
///
/// Each row stores a WebAuthn credential (passkey) linked to a user record.
/// A user can have multiple passkeys. The `credential_id` (base64url-encoded)
/// is unique across all collections. The `credential_data` column stores the
/// full serialized `Passkey` struct from webauthn-rs as JSON.
fn create_webauthn_credentials_table_migration(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _webauthn_credentials (
            id              TEXT PRIMARY KEY NOT NULL,
            collection_id   TEXT NOT NULL,
            record_id       TEXT NOT NULL,
            name            TEXT NOT NULL DEFAULT '',
            credential_id   TEXT NOT NULL,
            credential_data TEXT NOT NULL,
            created         TEXT NOT NULL DEFAULT (datetime('now')),
            updated         TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_webauthn_credential_id
            ON _webauthn_credentials(credential_id);

        CREATE INDEX IF NOT EXISTS idx_webauthn_collection
            ON _webauthn_credentials(collection_id);

        CREATE INDEX IF NOT EXISTS idx_webauthn_record
            ON _webauthn_credentials(collection_id, record_id);",
    )
    .map_err(|e| {
        DbError::migration(format!("failed to create _webauthn_credentials table: {e}"))
    })?;

    Ok(())
}

/// Migration v4: Add `view_query` column to `_collections` for View collections.
///
/// View collections store their SQL query in this column. The column is nullable;
/// only view-type collections will have a non-NULL value.
fn add_view_query_column_migration(conn: &Connection) -> Result<()> {
    conn.execute_batch("ALTER TABLE _collections ADD COLUMN view_query TEXT;")
        .map_err(|e| DbError::migration(format!("failed to add view_query column: {e}")))?;

    Ok(())
}

/// Migration v5: create the `_logs` table for request audit logging.
fn create_logs_table_migration(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _logs (
            id          TEXT PRIMARY KEY NOT NULL,
            method      TEXT NOT NULL,
            url         TEXT NOT NULL,
            status      INTEGER NOT NULL DEFAULT 0,
            ip          TEXT NOT NULL DEFAULT '',
            auth_id     TEXT NOT NULL DEFAULT '',
            duration_ms INTEGER NOT NULL DEFAULT 0,
            user_agent  TEXT NOT NULL DEFAULT '',
            request_id  TEXT NOT NULL DEFAULT '',
            created     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );

        CREATE INDEX IF NOT EXISTS idx_logs_created ON _logs(created);
        CREATE INDEX IF NOT EXISTS idx_logs_method ON _logs(method);
        CREATE INDEX IF NOT EXISTS idx_logs_status ON _logs(status);
        CREATE INDEX IF NOT EXISTS idx_logs_auth_id ON _logs(auth_id);
        CREATE INDEX IF NOT EXISTS idx_logs_ip ON _logs(ip);",
    )
    .map_err(|e| DbError::migration(format!("failed to create _logs table: {e}")))?;

    Ok(())
}

/// Register system collections in `_collections` so they can be resolved
/// by the auth middleware's `SchemaLookup`.
fn register_system_collections(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO _collections (id, name, type, system) VALUES (?1, ?2, ?3, 1)",
        rusqlite::params![SUPERUSERS_COLLECTION_ID, "_superusers", "auth"],
    )
    .map_err(|e| DbError::migration(format!("failed to register _superusers collection: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations::{applied_migrations, current_version, run_migrations};

    fn in_memory_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                rusqlite::params![name],
                |row| row.get(0),
            )
            .unwrap();
        count == 1
    }

    fn index_exists(conn: &Connection, name: &str) -> bool {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
                rusqlite::params![name],
                |row| row.get(0),
            )
            .unwrap();
        count == 1
    }

    fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info('{table}')"))
            .unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        columns.contains(&column.to_string())
    }

    // ── System tables creation ────────────────────────────────────────────

    #[test]
    fn system_migration_creates_all_tables() {
        let conn = in_memory_conn();
        let migrations = system_migrations();
        run_migrations(&conn, &migrations).unwrap();

        assert!(table_exists(&conn, "_migrations"), "_migrations missing");
        assert!(table_exists(&conn, "_collections"), "_collections missing");
        assert!(table_exists(&conn, "_fields"), "_fields missing");
        assert!(table_exists(&conn, "_settings"), "_settings missing");
        assert!(table_exists(&conn, "_superusers"), "_superusers missing");
        assert!(
            table_exists(&conn, "_externalAuths"),
            "_externalAuths missing"
        );
    }

    #[test]
    fn system_migration_is_idempotent() {
        let conn = in_memory_conn();
        let migrations = system_migrations();

        run_migrations(&conn, &migrations).unwrap();
        run_migrations(&conn, &migrations).unwrap();

        assert_eq!(current_version(&conn).unwrap(), 5);
        assert!(table_exists(&conn, "_collections"));
        assert!(table_exists(&conn, "_externalAuths"));
        assert!(table_exists(&conn, "_webauthn_credentials"));
    }

    #[test]
    fn system_migration_records_version() {
        let conn = in_memory_conn();
        let migrations = system_migrations();
        run_migrations(&conn, &migrations).unwrap();

        let applied = applied_migrations(&conn).unwrap();
        assert_eq!(applied.len(), 5);
        assert_eq!(applied[0], (1, "create_system_tables".to_string()));
        assert_eq!(applied[1], (2, "create_external_auths_table".to_string()));
        assert_eq!(
            applied[2],
            (3, "create_webauthn_credentials_table".to_string())
        );
        assert_eq!(applied[3], (4, "add_view_query_to_collections".to_string()));
        assert_eq!(applied[4], (5, "create_logs_table".to_string()));
    }

    // ── _collections table structure ──────────────────────────────────────

    #[test]
    fn collections_table_has_expected_columns() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        for col in &[
            "id",
            "name",
            "type",
            "system",
            "list_rule",
            "view_rule",
            "create_rule",
            "update_rule",
            "delete_rule",
            "view_query",
            "created",
            "updated",
        ] {
            assert!(
                column_exists(&conn, "_collections", col),
                "_collections missing column: {col}"
            );
        }
    }

    #[test]
    fn collections_type_check_constraint() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        // Valid types should work.
        for valid_type in &["base", "auth", "view"] {
            conn.execute(
                "INSERT INTO _collections (id, name, type) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    format!("id_{valid_type}"),
                    format!("col_{valid_type}"),
                    valid_type,
                ],
            )
            .unwrap_or_else(|e| panic!("valid type '{valid_type}' should succeed: {e}"));
        }

        // Invalid type should fail.
        let result = conn.execute(
            "INSERT INTO _collections (id, name, type) VALUES ('id_bad', 'col_bad', 'invalid')",
            [],
        );
        assert!(
            result.is_err(),
            "invalid collection type should be rejected by CHECK"
        );
    }

    #[test]
    fn collections_name_is_unique() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('id1', 'unique_col')",
            [],
        )
        .unwrap();

        let result = conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('id2', 'unique_col')",
            [],
        );
        assert!(result.is_err(), "duplicate collection name should fail");
    }

    #[test]
    fn collections_indexes_created() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        assert!(
            index_exists(&conn, "idx_collections_name"),
            "idx_collections_name missing"
        );
        assert!(
            index_exists(&conn, "idx_collections_type"),
            "idx_collections_type missing"
        );
    }

    // ── _fields table structure ───────────────────────────────────────────

    #[test]
    fn fields_table_has_expected_columns() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        for col in &[
            "id",
            "collection_id",
            "name",
            "type",
            "required",
            "unique_field",
            "options",
            "sort_order",
            "created",
            "updated",
        ] {
            assert!(
                column_exists(&conn, "_fields", col),
                "_fields missing column: {col}"
            );
        }
    }

    #[test]
    fn fields_cascade_delete_with_collection() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        // Insert a collection and a field.
        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('col1', 'test_collection')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO _fields (id, collection_id, name, type) VALUES ('f1', 'col1', 'title', 'text')",
            [],
        )
        .unwrap();

        // Delete the collection.
        conn.execute("DELETE FROM _collections WHERE id = 'col1'", [])
            .unwrap();

        // Field should be cascade-deleted.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM _fields WHERE id = 'f1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            count, 0,
            "field should be deleted when collection is deleted"
        );
    }

    #[test]
    fn fields_unique_name_per_collection() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('col1', 'test_col')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO _fields (id, collection_id, name, type) VALUES ('f1', 'col1', 'title', 'text')",
            [],
        )
        .unwrap();

        // Same field name in same collection should fail.
        let result = conn.execute(
            "INSERT INTO _fields (id, collection_id, name, type) VALUES ('f2', 'col1', 'title', 'number')",
            [],
        );
        assert!(
            result.is_err(),
            "duplicate field name in same collection should fail"
        );
    }

    #[test]
    fn fields_same_name_different_collection_ok() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('col1', 'col_a')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('col2', 'col_b')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO _fields (id, collection_id, name, type) VALUES ('f1', 'col1', 'title', 'text')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO _fields (id, collection_id, name, type) VALUES ('f2', 'col2', 'title', 'text')",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM _fields WHERE name = 'title'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2, "same field name in different collections is ok");
    }

    #[test]
    fn fields_indexes_created() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        assert!(
            index_exists(&conn, "idx_fields_collection"),
            "idx_fields_collection missing"
        );
        assert!(
            index_exists(&conn, "idx_fields_type"),
            "idx_fields_type missing"
        );
    }

    // ── _settings table structure ─────────────────────────────────────────

    #[test]
    fn settings_table_has_expected_columns() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        for col in &["key", "value", "updated"] {
            assert!(
                column_exists(&conn, "_settings", col),
                "_settings missing column: {col}"
            );
        }
    }

    #[test]
    fn settings_key_is_unique() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _settings (key, value) VALUES ('app_name', '\"Zerobase\"')",
            [],
        )
        .unwrap();

        let result = conn.execute(
            "INSERT INTO _settings (key, value) VALUES ('app_name', '\"Other\"')",
            [],
        );
        assert!(result.is_err(), "duplicate settings key should fail");
    }

    #[test]
    fn settings_default_value_is_empty_json_object() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute("INSERT INTO _settings (key) VALUES ('test_key')", [])
            .unwrap();

        let value: String = conn
            .query_row(
                "SELECT value FROM _settings WHERE key = 'test_key'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, "{}");
    }

    // ── _superusers table structure ───────────────────────────────────────

    #[test]
    fn superusers_table_has_expected_columns() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        for col in &["id", "email", "password", "created", "updated"] {
            assert!(
                column_exists(&conn, "_superusers", col),
                "_superusers missing column: {col}"
            );
        }
    }

    #[test]
    fn superusers_email_is_unique() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _superusers (id, email, password) VALUES ('su1', 'admin@test.com', 'hash1')",
            [],
        )
        .unwrap();

        let result = conn.execute(
            "INSERT INTO _superusers (id, email, password) VALUES ('su2', 'admin@test.com', 'hash2')",
            [],
        );
        assert!(result.is_err(), "duplicate superuser email should fail");
    }

    #[test]
    fn superusers_index_created() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        assert!(
            index_exists(&conn, "idx_superusers_email"),
            "idx_superusers_email missing"
        );
    }

    // ── _externalAuths table structure ─────────────────────────────────────

    #[test]
    fn external_auths_table_has_expected_columns() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        for col in &[
            "id",
            "collection_id",
            "record_id",
            "provider",
            "provider_id",
            "created",
            "updated",
        ] {
            assert!(
                column_exists(&conn, "_externalAuths", col),
                "_externalAuths missing column: {col}"
            );
        }
    }

    #[test]
    fn external_auths_unique_record_provider() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _externalAuths (id, collection_id, record_id, provider, provider_id) VALUES ('ea1', 'col1', 'rec1', 'google', 'gid1')",
            [],
        )
        .unwrap();

        // Same record + provider should fail.
        let result = conn.execute(
            "INSERT INTO _externalAuths (id, collection_id, record_id, provider, provider_id) VALUES ('ea2', 'col1', 'rec1', 'google', 'gid2')",
            [],
        );
        assert!(result.is_err(), "duplicate record+provider should fail");
    }

    #[test]
    fn external_auths_unique_provider_id() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _externalAuths (id, collection_id, record_id, provider, provider_id) VALUES ('ea1', 'col1', 'rec1', 'google', 'gid1')",
            [],
        )
        .unwrap();

        // Same provider + provider_id but different record should fail.
        let result = conn.execute(
            "INSERT INTO _externalAuths (id, collection_id, record_id, provider, provider_id) VALUES ('ea2', 'col1', 'rec2', 'google', 'gid1')",
            [],
        );
        assert!(
            result.is_err(),
            "duplicate provider+provider_id should fail"
        );
    }

    #[test]
    fn external_auths_indexes_created() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        assert!(index_exists(&conn, "idx_externalAuths_record_provider"));
        assert!(index_exists(&conn, "idx_externalAuths_provider_id"));
        assert!(index_exists(&conn, "idx_externalAuths_collection"));
        assert!(index_exists(&conn, "idx_externalAuths_record"));
    }

    // ── Integration with Database pool ────────────────────────────────────

    #[test]
    fn system_migration_works_with_database_pool() {
        let db =
            crate::pool::Database::open_in_memory(&crate::pool::PoolConfig::default()).unwrap();

        db.with_write_conn(|conn| run_migrations(conn, &system_migrations()))
            .unwrap();

        // Verify via read connection.
        let conn = db.read_conn().unwrap();
        assert!(table_exists(&conn, "_collections"));
        assert!(table_exists(&conn, "_fields"));
        assert!(table_exists(&conn, "_settings"));
        assert!(table_exists(&conn, "_superusers"));
        assert!(table_exists(&conn, "_externalAuths"));
        assert!(table_exists(&conn, "_webauthn_credentials"));

        let version = current_version(&conn).unwrap();
        assert_eq!(version, 5);
    }

    #[test]
    fn system_migration_idempotent_with_database_pool() {
        let db =
            crate::pool::Database::open_in_memory(&crate::pool::PoolConfig::default()).unwrap();

        // Run twice.
        db.with_write_conn(|conn| run_migrations(conn, &system_migrations()))
            .unwrap();
        db.with_write_conn(|conn| run_migrations(conn, &system_migrations()))
            .unwrap();

        let conn = db.read_conn().unwrap();
        let version = current_version(&conn).unwrap();
        assert_eq!(version, 5);
    }

    // ── Column type and constraint verification helpers ─────────────────

    /// Column metadata from PRAGMA table_info.
    #[derive(Debug)]
    #[allow(dead_code)]
    struct ColumnInfo {
        name: String,
        col_type: String,
        notnull: bool,
        default_value: Option<String>,
        pk: bool,
    }

    fn get_columns(conn: &Connection, table: &str) -> Vec<ColumnInfo> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info('{table}')"))
            .unwrap();
        stmt.query_map([], |row| {
            Ok(ColumnInfo {
                name: row.get::<_, String>(1)?,
                col_type: row.get::<_, String>(2)?,
                notnull: row.get::<_, bool>(3)?,
                default_value: row.get::<_, Option<String>>(4)?,
                pk: row.get::<_, bool>(5)?,
            })
        })
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap()
    }

    fn find_column<'a>(columns: &'a [ColumnInfo], name: &str) -> &'a ColumnInfo {
        columns
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("column '{name}' not found"))
    }

    // ── _collections column types and constraints ───────────────────────

    #[test]
    fn collections_id_is_text_primary_key_not_null() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();
        let cols = get_columns(&conn, "_collections");
        let id = find_column(&cols, "id");
        assert!(id.pk, "_collections.id should be PK");
        assert_eq!(id.col_type, "TEXT");
        assert!(id.notnull, "_collections.id should be NOT NULL");
    }

    #[test]
    fn collections_name_is_text_not_null() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();
        let cols = get_columns(&conn, "_collections");
        let name = find_column(&cols, "name");
        assert_eq!(name.col_type, "TEXT");
        assert!(name.notnull, "_collections.name should be NOT NULL");
    }

    #[test]
    fn collections_type_defaults_to_base() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('t1', 'default_type_col')",
            [],
        )
        .unwrap();

        let col_type: String = conn
            .query_row("SELECT type FROM _collections WHERE id = 't1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(col_type, "base");
    }

    #[test]
    fn collections_system_defaults_to_zero() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('t1', 'sys_default_col')",
            [],
        )
        .unwrap();

        let system: i64 = conn
            .query_row(
                "SELECT system FROM _collections WHERE id = 't1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(system, 0);
    }

    #[test]
    fn collections_rules_are_nullable() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        // Insert without rules — they should default to NULL (locked).
        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('t1', 'nullable_rules_col')",
            [],
        )
        .unwrap();

        for rule_col in &[
            "list_rule",
            "view_rule",
            "create_rule",
            "update_rule",
            "delete_rule",
        ] {
            let is_null: bool = conn
                .query_row(
                    &format!("SELECT {rule_col} IS NULL FROM _collections WHERE id = 't1'"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(is_null, "{rule_col} should default to NULL");
        }
    }

    #[test]
    fn collections_timestamps_auto_set() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('t1', 'ts_col')",
            [],
        )
        .unwrap();

        let created: String = conn
            .query_row(
                "SELECT created FROM _collections WHERE id = 't1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let updated: String = conn
            .query_row(
                "SELECT updated FROM _collections WHERE id = 't1'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(!created.is_empty(), "created should be auto-set");
        assert!(!updated.is_empty(), "updated should be auto-set");
    }

    // ── _fields column types and constraints ────────────────────────────

    #[test]
    fn fields_required_defaults_to_zero() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('c1', 'fields_defaults_col')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO _fields (id, collection_id, name, type) VALUES ('f1', 'c1', 'title', 'text')",
            [],
        )
        .unwrap();

        let required: i64 = conn
            .query_row("SELECT required FROM _fields WHERE id = 'f1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(required, 0);
    }

    #[test]
    fn fields_unique_field_defaults_to_zero() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('c1', 'uniq_defaults_col')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO _fields (id, collection_id, name, type) VALUES ('f1', 'c1', 'title', 'text')",
            [],
        )
        .unwrap();

        let unique_field: i64 = conn
            .query_row(
                "SELECT unique_field FROM _fields WHERE id = 'f1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unique_field, 0);
    }

    #[test]
    fn fields_options_defaults_to_empty_json_object() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('c1', 'opts_defaults_col')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO _fields (id, collection_id, name, type) VALUES ('f1', 'c1', 'body', 'text')",
            [],
        )
        .unwrap();

        let options: String = conn
            .query_row("SELECT options FROM _fields WHERE id = 'f1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(options, "{}");
    }

    #[test]
    fn fields_sort_order_defaults_to_zero() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('c1', 'sort_defaults_col')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO _fields (id, collection_id, name, type) VALUES ('f1', 'c1', 'body', 'text')",
            [],
        )
        .unwrap();

        let sort_order: i64 = conn
            .query_row(
                "SELECT sort_order FROM _fields WHERE id = 'f1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(sort_order, 0);
    }

    #[test]
    fn fields_foreign_key_rejects_invalid_collection_id() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        let result = conn.execute(
            "INSERT INTO _fields (id, collection_id, name, type) VALUES ('f1', 'nonexistent', 'title', 'text')",
            [],
        );
        assert!(
            result.is_err(),
            "inserting field with nonexistent collection_id should fail"
        );
    }

    #[test]
    fn fields_id_is_text_primary_key() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();
        let cols = get_columns(&conn, "_fields");
        let id = find_column(&cols, "id");
        assert!(id.pk, "_fields.id should be PK");
        assert_eq!(id.col_type, "TEXT");
    }

    #[test]
    fn fields_timestamps_auto_set() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('c1', 'field_ts_col')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO _fields (id, collection_id, name, type) VALUES ('f1', 'c1', 'body', 'text')",
            [],
        )
        .unwrap();

        let created: String = conn
            .query_row("SELECT created FROM _fields WHERE id = 'f1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(!created.is_empty(), "fields.created should be auto-set");
    }

    // ── _settings column types and constraints ──────────────────────────

    #[test]
    fn settings_key_is_text_primary_key() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();
        let cols = get_columns(&conn, "_settings");
        let key = find_column(&cols, "key");
        assert!(key.pk, "_settings.key should be PK");
        assert_eq!(key.col_type, "TEXT");
        assert!(key.notnull, "_settings.key should be NOT NULL");
    }

    #[test]
    fn settings_updated_auto_set() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _settings (key, value) VALUES ('test', '\"val\"')",
            [],
        )
        .unwrap();

        let updated: String = conn
            .query_row(
                "SELECT updated FROM _settings WHERE key = 'test'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!updated.is_empty(), "settings.updated should be auto-set");
    }

    // ── _superusers column types and constraints ────────────────────────

    #[test]
    fn superusers_id_is_text_primary_key() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();
        let cols = get_columns(&conn, "_superusers");
        let id = find_column(&cols, "id");
        assert!(id.pk, "_superusers.id should be PK");
        assert_eq!(id.col_type, "TEXT");
    }

    #[test]
    fn superusers_email_is_text_not_null() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();
        let cols = get_columns(&conn, "_superusers");
        let email = find_column(&cols, "email");
        assert_eq!(email.col_type, "TEXT");
        assert!(email.notnull, "_superusers.email should be NOT NULL");
    }

    #[test]
    fn superusers_password_is_not_null() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();
        let cols = get_columns(&conn, "_superusers");
        let pw = find_column(&cols, "password");
        assert_eq!(pw.col_type, "TEXT");
        assert!(pw.notnull, "_superusers.password should be NOT NULL");
    }

    #[test]
    fn superusers_timestamps_auto_set() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        conn.execute(
            "INSERT INTO _superusers (id, email, password) VALUES ('su1', 'a@b.com', 'hash')",
            [],
        )
        .unwrap();

        let created: String = conn
            .query_row(
                "SELECT created FROM _superusers WHERE id = 'su1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let updated: String = conn
            .query_row(
                "SELECT updated FROM _superusers WHERE id = 'su1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!created.is_empty(), "superusers.created should be auto-set");
        assert!(!updated.is_empty(), "superusers.updated should be auto-set");
    }

    // ── _migrations table structure ─────────────────────────────────────

    #[test]
    fn migrations_table_has_expected_columns() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        for col in &["version", "name", "applied_at"] {
            assert!(
                column_exists(&conn, "_migrations", col),
                "_migrations missing column: {col}"
            );
        }
    }

    #[test]
    fn migrations_version_is_integer_primary_key() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();
        let cols = get_columns(&conn, "_migrations");
        let version = find_column(&cols, "version");
        assert!(version.pk, "_migrations.version should be PK");
        assert_eq!(version.col_type, "INTEGER");
    }

    // ── Cross-table integrity ───────────────────────────────────────────

    #[test]
    fn full_schema_round_trip() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        // Create a collection with fields, a setting, and a superuser.
        conn.execute(
            "INSERT INTO _collections (id, name, type) VALUES ('col1', 'posts', 'base')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO _fields (id, collection_id, name, type, required, options) \
             VALUES ('f1', 'col1', 'title', 'text', 1, '{\"min\": 1, \"max\": 200}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO _fields (id, collection_id, name, type, options) \
             VALUES ('f2', 'col1', 'body', 'editor', '{}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO _settings (key, value) VALUES ('app_name', '\"MyApp\"')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO _superusers (id, email, password) VALUES ('su1', 'admin@test.com', '$argon2id$hash')",
            [],
        )
        .unwrap();

        // Verify collection.
        let col_name: String = conn
            .query_row(
                "SELECT name FROM _collections WHERE id = 'col1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(col_name, "posts");

        // Verify fields for the collection.
        let field_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM _fields WHERE collection_id = 'col1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(field_count, 2);

        // Verify field options are valid JSON.
        let opts: String = conn
            .query_row("SELECT options FROM _fields WHERE id = 'f1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&opts).unwrap();
        assert_eq!(parsed["min"], 1);
        assert_eq!(parsed["max"], 200);

        // Verify setting.
        let app_name: String = conn
            .query_row(
                "SELECT value FROM _settings WHERE key = 'app_name'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(app_name, "\"MyApp\"");

        // Verify superuser.
        let su_email: String = conn
            .query_row(
                "SELECT email FROM _superusers WHERE id = 'su1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(su_email, "admin@test.com");
    }

    #[test]
    fn cascade_delete_removes_all_fields_for_collection() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        // Create two collections with fields each.
        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('c1', 'col_a')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO _collections (id, name) VALUES ('c2', 'col_b')",
            [],
        )
        .unwrap();

        for (fid, cid, name) in &[
            ("f1", "c1", "title"),
            ("f2", "c1", "body"),
            ("f3", "c2", "name"),
        ] {
            conn.execute(
                "INSERT INTO _fields (id, collection_id, name, type) VALUES (?1, ?2, ?3, 'text')",
                rusqlite::params![fid, cid, name],
            )
            .unwrap();
        }

        // Delete c1 — only c1's fields should be removed.
        conn.execute("DELETE FROM _collections WHERE id = 'c1'", [])
            .unwrap();

        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM _fields", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 1, "only c2's field should remain");

        let remaining_cid: String = conn
            .query_row(
                "SELECT collection_id FROM _fields WHERE id = 'f3'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining_cid, "c2");
    }

    #[test]
    fn total_table_count_after_system_migration() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        let tables: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
        };

        // Exactly 7 tables: _collections, _externalAuths, _fields, _migrations, _settings, _superusers, _webauthn_credentials
        assert_eq!(tables.len(), 8, "expected 8 system tables, got: {tables:?}");
        assert!(tables.contains(&"_collections".to_string()));
        assert!(tables.contains(&"_externalAuths".to_string()));
        assert!(tables.contains(&"_fields".to_string()));
        assert!(tables.contains(&"_migrations".to_string()));
        assert!(tables.contains(&"_settings".to_string()));
        assert!(tables.contains(&"_superusers".to_string()));
        assert!(tables.contains(&"_webauthn_credentials".to_string()));
    }

    #[test]
    fn total_index_count_after_system_migration() {
        let conn = in_memory_conn();
        run_migrations(&conn, &system_migrations()).unwrap();

        let indexes: Vec<String> = {
            let mut stmt = conn
                .prepare(
                    "SELECT name FROM sqlite_master WHERE type='index' AND name NOT LIKE 'sqlite_%' ORDER BY name",
                )
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
        };

        // 5 explicit indexes:
        //   idx_collections_name, idx_collections_type,
        //   idx_fields_collection, idx_fields_type,
        //   idx_superusers_email
        // Plus SQLite auto-indexes for UNIQUE constraints (autoindex for _collections.name,
        // _fields(collection_id, name), _superusers.email).
        for expected in &[
            "idx_collections_name",
            "idx_collections_type",
            "idx_fields_collection",
            "idx_fields_type",
            "idx_superusers_email",
            "idx_externalAuths_record_provider",
            "idx_externalAuths_provider_id",
            "idx_externalAuths_collection",
            "idx_externalAuths_record",
            "idx_webauthn_credential_id",
            "idx_webauthn_collection",
            "idx_webauthn_record",
        ] {
            assert!(
                indexes.contains(&expected.to_string()),
                "missing index: {expected}, found: {indexes:?}"
            );
        }
    }
}
