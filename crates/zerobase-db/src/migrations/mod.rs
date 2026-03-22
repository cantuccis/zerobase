//! Database migration system.
//!
//! Migrations are forward-only operations identified by a version number.
//! Each migration can be either a raw SQL string or a Rust function for
//! type-safe schema manipulation. The runner tracks applied versions in a
//! `_migrations` table and applies pending migrations inside transactions.
//!
//! # Design
//!
//! Migrations are Rust functions (not SQL files) for type safety. This enables
//! compile-time checked schema operations, complex data transformations, and
//! conditional logic that raw SQL cannot express.

pub mod system;

use rusqlite::Connection;
use tracing::{debug, info, warn};

use crate::error::{DbError, Result};

/// The action a migration performs.
///
/// Migrations can be either raw SQL (for simple schema changes) or Rust
/// functions (for complex, type-safe operations). Using Rust functions is
/// preferred as it enables compile-time safety and richer logic.
pub enum MigrationAction {
    /// Raw SQL to execute via `execute_batch`.
    Sql(&'static str),
    /// A Rust function receiving the connection within a transaction.
    Function(fn(&Connection) -> Result<()>),
}

impl std::fmt::Debug for MigrationAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sql(sql) => f
                .debug_tuple("Sql")
                .field(&sql.chars().take(60).collect::<String>())
                .finish(),
            Self::Function(_) => f.debug_tuple("Function").field(&"<fn>").finish(),
        }
    }
}

/// A single database migration.
///
/// Migrations are identified by a monotonically increasing version number and
/// applied in version order. Each migration runs inside a transaction — if it
/// fails, the transaction is rolled back and the version is not recorded.
#[derive(Debug)]
pub struct Migration {
    /// Monotonically increasing version number (e.g., 1, 2, 3).
    pub version: u32,
    /// Human-readable name (e.g., "create_system_tables").
    pub name: &'static str,
    /// The action to perform: SQL string or Rust function.
    pub action: MigrationAction,
}

impl Migration {
    /// Create a migration backed by a raw SQL string.
    pub fn sql(version: u32, name: &'static str, sql: &'static str) -> Self {
        Self {
            version,
            name,
            action: MigrationAction::Sql(sql),
        }
    }

    /// Create a migration backed by a Rust function.
    pub fn function(version: u32, name: &'static str, f: fn(&Connection) -> Result<()>) -> Self {
        Self {
            version,
            name,
            action: MigrationAction::Function(f),
        }
    }
}

/// Run all pending migrations against the given connection.
///
/// This function:
/// 1. Creates the `_migrations` tracking table if it doesn't exist.
/// 2. Determines which migrations have already been applied.
/// 3. Applies pending migrations in version order, each inside a transaction.
///
/// # Errors
///
/// Returns `DbError::Migration` if any migration fails. Already-applied
/// migrations are skipped, so this function is idempotent.
pub fn run_migrations(conn: &Connection, migrations: &[Migration]) -> Result<()> {
    ensure_migrations_table(conn)?;

    let applied = get_applied_versions(conn)?;

    let mut pending: Vec<&Migration> = migrations
        .iter()
        .filter(|m| !applied.contains(&m.version))
        .collect();

    pending.sort_by_key(|m| m.version);

    if pending.is_empty() {
        debug!("No pending migrations");
        return Ok(());
    }

    info!(
        count = pending.len(),
        "Applying {} pending migration(s)",
        pending.len()
    );

    for migration in &pending {
        apply_migration(conn, migration)?;
    }

    Ok(())
}

/// Create the `_migrations` table if it doesn't already exist.
fn ensure_migrations_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            version    INTEGER PRIMARY KEY,
            name       TEXT    NOT NULL,
            applied_at TEXT    NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .map_err(|e| DbError::migration(format!("failed to create _migrations table: {e}")))?;
    Ok(())
}

/// Return the set of already-applied migration versions.
fn get_applied_versions(conn: &Connection) -> Result<Vec<u32>> {
    let mut stmt = conn
        .prepare("SELECT version FROM _migrations ORDER BY version")
        .map_err(|e| DbError::migration(format!("failed to query _migrations: {e}")))?;

    let versions = stmt
        .query_map([], |row| row.get::<_, u32>(0))
        .map_err(|e| DbError::migration(format!("failed to read migration versions: {e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| DbError::migration(format!("failed to collect migration versions: {e}")))?;

    Ok(versions)
}

/// Apply a single migration inside a transaction.
fn apply_migration(conn: &Connection, migration: &Migration) -> Result<()> {
    info!(
        version = migration.version,
        name = migration.name,
        "Applying migration v{}:{}",
        migration.version,
        migration.name
    );

    // We use a savepoint to allow per-migration rollback while the caller
    // may have its own outer transaction.
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| DbError::migration(format!("failed to begin transaction: {e}")))?;

    // Execute the migration action.
    match &migration.action {
        MigrationAction::Sql(sql) => {
            tx.execute_batch(sql).map_err(|e| {
                warn!(
                    version = migration.version,
                    name = migration.name,
                    error = %e,
                    "Migration v{}:{} failed (SQL)",
                    migration.version,
                    migration.name
                );
                DbError::migration(format!(
                    "migration v{}:{} failed: {e}",
                    migration.version, migration.name
                ))
            })?;
        }
        MigrationAction::Function(f) => {
            f(&tx).map_err(|e| {
                warn!(
                    version = migration.version,
                    name = migration.name,
                    error = %e,
                    "Migration v{}:{} failed (function)",
                    migration.version,
                    migration.name
                );
                DbError::migration(format!(
                    "migration v{}:{} failed: {e}",
                    migration.version, migration.name
                ))
            })?;
        }
    }

    // Record the migration.
    tx.execute(
        "INSERT INTO _migrations (version, name) VALUES (?1, ?2)",
        rusqlite::params![migration.version, migration.name],
    )
    .map_err(|e| {
        DbError::migration(format!(
            "failed to record migration v{}:{}: {e}",
            migration.version, migration.name
        ))
    })?;

    tx.commit().map_err(|e| {
        DbError::migration(format!(
            "failed to commit migration v{}:{}: {e}",
            migration.version, migration.name
        ))
    })?;

    info!(
        version = migration.version,
        name = migration.name,
        "Migration v{}:{} applied successfully",
        migration.version,
        migration.name
    );

    Ok(())
}

/// Return the current schema version (highest applied migration), or 0 if none.
pub fn current_version(conn: &Connection) -> Result<u32> {
    ensure_migrations_table(conn)?;

    let version: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM _migrations",
            [],
            |row| row.get(0),
        )
        .map_err(|e| DbError::migration(format!("failed to query current version: {e}")))?;

    Ok(version)
}

/// Return the list of all applied migration names in version order.
pub fn applied_migrations(conn: &Connection) -> Result<Vec<(u32, String)>> {
    ensure_migrations_table(conn)?;

    let mut stmt = conn
        .prepare("SELECT version, name FROM _migrations ORDER BY version")
        .map_err(|e| DbError::migration(format!("failed to query _migrations: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| DbError::migration(format!("failed to read migrations: {e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| DbError::migration(format!("failed to collect migrations: {e}")))?;

    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn
    }

    fn sample_sql_migrations() -> Vec<Migration> {
        vec![
            Migration::sql(
                1,
                "create_users",
                "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
            ),
            Migration::sql(
                2,
                "create_posts",
                "CREATE TABLE posts (
                    id INTEGER PRIMARY KEY,
                    user_id INTEGER NOT NULL REFERENCES users(id),
                    title TEXT NOT NULL
                );",
            ),
            Migration::sql(
                3,
                "add_email_to_users",
                "ALTER TABLE users ADD COLUMN email TEXT;",
            ),
        ]
    }

    fn sample_function_migration() -> Migration {
        Migration::function(1, "create_items_via_fn", |conn| {
            conn.execute_batch(
                "CREATE TABLE items (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    quantity INTEGER NOT NULL DEFAULT 0
                );
                INSERT INTO items (name, quantity) VALUES ('seed_item', 10);",
            )
            .map_err(|e| DbError::migration(format!("function migration failed: {e}")))?;
            Ok(())
        })
    }

    // ── Migration table creation ──────────────────────────────────────────

    #[test]
    fn creates_migrations_table() {
        let conn = in_memory_conn();
        ensure_migrations_table(&conn).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='_migrations'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn ensure_migrations_table_is_idempotent() {
        let conn = in_memory_conn();
        ensure_migrations_table(&conn).unwrap();
        ensure_migrations_table(&conn).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='_migrations'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    // ── SQL migration tests ───────────────────────────────────────────────

    #[test]
    fn runs_all_sql_migrations() {
        let conn = in_memory_conn();
        let migrations = sample_sql_migrations();

        run_migrations(&conn, &migrations).unwrap();

        let tables: Vec<String> = {
            let mut stmt = conn
                .prepare(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE '\\_%' ESCAPE '\\' ORDER BY name",
                )
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
        };
        assert!(tables.contains(&"users".to_string()));
        assert!(tables.contains(&"posts".to_string()));
    }

    #[test]
    fn idempotent_rerun() {
        let conn = in_memory_conn();
        let migrations = sample_sql_migrations();

        run_migrations(&conn, &migrations).unwrap();
        run_migrations(&conn, &migrations).unwrap();

        let version = current_version(&conn).unwrap();
        assert_eq!(version, 3);
    }

    #[test]
    fn applies_only_pending() {
        let conn = in_memory_conn();
        let migrations = sample_sql_migrations();

        // Apply first two.
        run_migrations(&conn, &migrations[..2]).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 2);

        // Apply all three — only v3 should run.
        run_migrations(&conn, &migrations).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 3);
    }

    #[test]
    fn current_version_returns_zero_when_empty() {
        let conn = in_memory_conn();
        let version = current_version(&conn).unwrap();
        assert_eq!(version, 0);
    }

    #[test]
    fn bad_sql_migration_returns_error() {
        let conn = in_memory_conn();
        let bad = vec![Migration::sql(1, "bad_sql", "THIS IS NOT VALID SQL;")];

        let result = run_migrations(&conn, &bad);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("bad_sql"));
    }

    #[test]
    fn failed_migration_does_not_record_version() {
        let conn = in_memory_conn();
        let migrations = vec![
            Migration::sql(
                1,
                "good",
                "CREATE TABLE good_table (id INTEGER PRIMARY KEY);",
            ),
            Migration::sql(2, "bad", "INVALID SQL HERE;"),
        ];

        let _ = run_migrations(&conn, &migrations);

        let version = current_version(&conn).unwrap();
        assert_eq!(
            version, 1,
            "only the successful migration should be recorded"
        );
    }

    #[test]
    fn migrations_applied_in_version_order() {
        let conn = in_memory_conn();
        // Provide migrations out of order.
        let migrations = vec![
            Migration::sql(3, "third", "CREATE TABLE third (id INTEGER PRIMARY KEY);"),
            Migration::sql(1, "first", "CREATE TABLE first (id INTEGER PRIMARY KEY);"),
            Migration::sql(2, "second", "CREATE TABLE second (id INTEGER PRIMARY KEY);"),
        ];

        run_migrations(&conn, &migrations).unwrap();

        let applied = get_applied_versions(&conn).unwrap();
        assert_eq!(applied, vec![1, 2, 3]);
    }

    // ── Function migration tests ──────────────────────────────────────────

    #[test]
    fn runs_function_migration() {
        let conn = in_memory_conn();
        let migrations = vec![sample_function_migration()];

        run_migrations(&conn, &migrations).unwrap();

        // Table should exist.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='items'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Seed data should exist.
        let name: String = conn
            .query_row("SELECT name FROM items WHERE id = 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(name, "seed_item");

        let qty: i64 = conn
            .query_row("SELECT quantity FROM items WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(qty, 10);
    }

    #[test]
    fn function_migration_is_idempotent() {
        let conn = in_memory_conn();
        let migrations = vec![sample_function_migration()];

        run_migrations(&conn, &migrations).unwrap();
        run_migrations(&conn, &migrations).unwrap();

        let version = current_version(&conn).unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn failed_function_migration_rolls_back() {
        let conn = in_memory_conn();
        let migrations = vec![Migration::function(1, "failing_fn", |conn| {
            conn.execute_batch("CREATE TABLE will_rollback (id INTEGER PRIMARY KEY);")
                .map_err(|e| DbError::migration(format!("{e}")))?;
            Err(DbError::migration("intentional failure"))
        })];

        let result = run_migrations(&conn, &migrations);
        assert!(result.is_err());

        // Table should NOT exist because transaction rolled back.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='will_rollback'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "failed function migration should roll back");

        // Version should not be recorded.
        let version = current_version(&conn).unwrap();
        assert_eq!(version, 0);
    }

    // ── Mixed migration tests ─────────────────────────────────────────────

    #[test]
    fn mixed_sql_and_function_migrations() {
        let conn = in_memory_conn();
        let migrations = vec![
            Migration::sql(
                1,
                "create_categories",
                "CREATE TABLE categories (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
            ),
            Migration::function(2, "seed_categories", |conn| {
                conn.execute(
                    "INSERT INTO categories (name) VALUES (?1)",
                    rusqlite::params!["default"],
                )
                .map_err(|e| DbError::migration(format!("{e}")))?;
                Ok(())
            }),
            Migration::sql(
                3,
                "add_description",
                "ALTER TABLE categories ADD COLUMN description TEXT;",
            ),
        ];

        run_migrations(&conn, &migrations).unwrap();

        assert_eq!(current_version(&conn).unwrap(), 3);

        // Verify seed data from function migration.
        let name: String = conn
            .query_row("SELECT name FROM categories WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(name, "default");
    }

    // ── Applied migrations listing ────────────────────────────────────────

    #[test]
    fn applied_migrations_returns_all_applied() {
        let conn = in_memory_conn();
        let migrations = sample_sql_migrations();
        run_migrations(&conn, &migrations).unwrap();

        let applied = applied_migrations(&conn).unwrap();
        assert_eq!(applied.len(), 3);
        assert_eq!(applied[0], (1, "create_users".to_string()));
        assert_eq!(applied[1], (2, "create_posts".to_string()));
        assert_eq!(applied[2], (3, "add_email_to_users".to_string()));
    }

    #[test]
    fn applied_migrations_empty_when_no_migrations_run() {
        let conn = in_memory_conn();
        let applied = applied_migrations(&conn).unwrap();
        assert!(applied.is_empty());
    }

    // ── Edge cases ────────────────────────────────────────────────────────

    #[test]
    fn empty_migration_list_is_no_op() {
        let conn = in_memory_conn();
        run_migrations(&conn, &[]).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 0);
    }

    #[test]
    fn duplicate_versions_second_is_skipped() {
        let conn = in_memory_conn();
        let migrations = vec![Migration::sql(
            1,
            "first_v1",
            "CREATE TABLE dup_test (id INTEGER PRIMARY KEY);",
        )];
        run_migrations(&conn, &migrations).unwrap();

        // Run again with a different migration at the same version.
        let migrations2 = vec![Migration::sql(
            1,
            "second_v1",
            "CREATE TABLE should_not_exist (id INTEGER PRIMARY KEY);",
        )];
        run_migrations(&conn, &migrations2).unwrap();

        // The second migration should not have been applied.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='should_not_exist'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "duplicate version should be skipped");
    }

    #[test]
    fn migration_debug_format() {
        let sql_m = Migration::sql(1, "test", "CREATE TABLE t (id INTEGER);");
        let fn_m = Migration::function(2, "test_fn", |_| Ok(()));

        let sql_debug = format!("{:?}", sql_m);
        assert!(sql_debug.contains("Sql"));
        assert!(sql_debug.contains("test"));

        let fn_debug = format!("{:?}", fn_m);
        assert!(fn_debug.contains("Function"));
        assert!(fn_debug.contains("test_fn"));
    }
}
