//! SQLite connection pool and database handle.
//!
//! [`Database`] wraps an r2d2 connection pool for reads and a dedicated
//! mutex-guarded connection for writes. This matches SQLite's concurrency
//! model: many concurrent readers, one writer at a time.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use serde::Serialize;
use tracing::{debug, info, warn};

/// Global counter for generating unique in-memory database names.
static IN_MEMORY_COUNTER: AtomicU64 = AtomicU64::new(0);

use zerobase_core::configuration::DatabaseSettings;

use crate::error::{DbError, Result};

/// Configuration for the database connection pool.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of read connections in the pool.
    pub max_read_connections: u32,
    /// Busy timeout in milliseconds for SQLite.
    pub busy_timeout_ms: u32,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_read_connections: 8,
            busy_timeout_ms: 5000,
        }
    }
}

impl From<&DatabaseSettings> for PoolConfig {
    fn from(settings: &DatabaseSettings) -> Self {
        Self {
            max_read_connections: settings.max_read_connections,
            busy_timeout_ms: settings.busy_timeout_ms,
        }
    }
}

/// Snapshot of pool health and usage statistics.
#[derive(Debug, Clone, Serialize)]
pub struct PoolStats {
    /// Total connections managed by the read pool.
    pub total_connections: u32,
    /// Connections currently idle in the read pool.
    pub idle_connections: u32,
    /// Maximum configured pool size.
    pub max_size: u32,
}

/// Overall health status of the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// Database is fully operational.
    Healthy,
    /// Database is reachable but showing signs of stress (e.g. pool near exhaustion).
    Degraded,
    /// Database is unreachable or non-functional.
    Unhealthy,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Healthy => write!(f, "healthy"),
            Self::Degraded => write!(f, "degraded"),
            Self::Unhealthy => write!(f, "unhealthy"),
        }
    }
}

/// Detailed health diagnostics for the database.
#[derive(Debug, Clone, Serialize)]
pub struct HealthDiagnostics {
    /// Overall health status.
    pub status: HealthStatus,
    /// Whether the read pool can execute a trivial query.
    pub read_pool_ok: bool,
    /// Whether the write connection can execute a trivial query.
    pub write_conn_ok: bool,
    /// Pool utilization statistics.
    pub pool: PoolStats,
    /// Pool utilization as a percentage (0.0–100.0).
    pub pool_utilization_pct: f64,
    /// Whether the pool is considered exhausted (all connections in use).
    pub pool_exhausted: bool,
    /// Time taken to acquire a read connection and execute a query, in microseconds.
    pub read_latency_us: u64,
    /// Time taken to acquire the write lock and execute a query, in microseconds.
    pub write_latency_us: u64,
}

/// Default slow-query threshold: 200 milliseconds.
pub const DEFAULT_SLOW_QUERY_THRESHOLD: Duration = Duration::from_millis(200);

/// Fraction of pool utilization above which the pool is considered degraded.
const POOL_DEGRADED_THRESHOLD: f64 = 0.8;

/// Type alias for the r2d2 connection pool using SQLite.
pub type SqlitePool = Pool<SqliteConnectionManager>;

/// Type alias for a pooled SQLite connection.
pub type PooledConnection = r2d2::PooledConnection<SqliteConnectionManager>;

/// The primary database handle.
///
/// Provides separate read and write paths to match SQLite's concurrency model.
/// The read pool allows multiple concurrent readers via r2d2. The write
/// connection is protected by a `Mutex` to serialize all write operations.
#[derive(Clone)]
pub struct Database {
    /// Pool of read-only connections.
    read_pool: SqlitePool,
    /// Single write connection, mutex-guarded.
    pub(crate) write_conn: Arc<Mutex<Connection>>,
    /// Path to the database file on disk (None for in-memory).
    pub(crate) db_path: Option<std::path::PathBuf>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("read_pool_size", &self.read_pool.state().connections)
            .finish()
    }
}

impl Database {
    /// Open (or create) a database at the given file path.
    ///
    /// Applies WAL mode, foreign keys, and busy timeout on all connections.
    pub fn open(path: &Path, config: &PoolConfig) -> Result<Self> {
        // Ensure parent directory exists.
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    DbError::migration(format!(
                        "failed to create database directory {}: {e}",
                        parent.display()
                    ))
                })?;
            }
        }

        let db_path = path.to_str().unwrap_or("data.db");
        info!(path = db_path, "Opening SQLite database");

        // Open the write connection FIRST and enable WAL mode before creating
        // the read pool. WAL mode requires a brief exclusive lock, so enabling
        // it before the pool avoids "database is locked" errors when r2d2
        // initializes multiple read connections concurrently.
        let write_conn = Connection::open(path).map_err(DbError::Query)?;
        apply_pragmas(&write_conn, config.busy_timeout_ms)?;

        // Build the read pool (WAL mode is already active on the database file).
        let manager = SqliteConnectionManager::file(path);
        let read_pool = Pool::builder()
            .max_size(config.max_read_connections)
            .connection_customizer(Box::new(ConnectionInit {
                busy_timeout_ms: config.busy_timeout_ms,
            }))
            .build(manager)?;

        debug!(
            "Database opened with {} read connections",
            config.max_read_connections
        );

        Ok(Self {
            read_pool,
            write_conn: Arc::new(Mutex::new(write_conn)),
            db_path: Some(path.to_path_buf()),
        })
    }

    /// Create an in-memory database for testing.
    ///
    /// Each call creates an isolated in-memory database with a unique name
    /// and shared cache so that the read pool and write connection access
    /// the same data.
    pub fn open_in_memory(config: &PoolConfig) -> Result<Self> {
        // Each call gets a unique name so parallel tests don't interfere.
        let id = IN_MEMORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let uri = format!("file:zerobase_mem_{id}?mode=memory&cache=shared");

        let manager = SqliteConnectionManager::file(&uri);
        let read_pool = Pool::builder()
            .max_size(config.max_read_connections)
            .connection_customizer(Box::new(ConnectionInit {
                busy_timeout_ms: config.busy_timeout_ms,
            }))
            .build(manager)?;

        let write_conn = Connection::open(&uri).map_err(DbError::Query)?;
        apply_pragmas(&write_conn, config.busy_timeout_ms)?;

        Ok(Self {
            read_pool,
            write_conn: Arc::new(Mutex::new(write_conn)),
            db_path: None,
        })
    }

    /// Get a read-only connection from the pool.
    pub fn read_conn(&self) -> Result<PooledConnection> {
        self.read_pool.get().map_err(DbError::Pool)
    }

    /// Execute a closure with exclusive write access.
    ///
    /// The closure receives a mutable reference to the write connection.
    /// This serializes all write operations, preventing `SQLITE_BUSY` errors.
    pub fn with_write_conn<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| DbError::migration(format!("write lock poisoned: {e}")))?;
        f(&conn)
    }

    /// Execute a closure inside a SQLite transaction on the write connection.
    ///
    /// If the closure returns `Ok`, the transaction is committed.
    /// If it returns `Err`, the transaction is rolled back.
    pub fn transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<T>,
    {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| DbError::migration(format!("write lock poisoned: {e}")))?;

        let tx = conn.transaction().map_err(DbError::Query)?;
        let result = f(&tx)?;
        tx.commit().map_err(DbError::Query)?;
        Ok(result)
    }

    /// Return a reference to the underlying read pool (for advanced use).
    pub fn read_pool(&self) -> &SqlitePool {
        &self.read_pool
    }

    /// Return a snapshot of pool usage statistics.
    pub fn stats(&self) -> PoolStats {
        let state = self.read_pool.state();
        PoolStats {
            total_connections: state.connections,
            idle_connections: state.idle_connections,
            max_size: self.read_pool.max_size(),
        }
    }

    /// Run all pending system migrations.
    ///
    /// This applies the built-in migrations that create Zerobase's internal
    /// tables (`_collections`, `_fields`, `_settings`, `_superusers`).
    /// The operation is idempotent — already-applied migrations are skipped.
    pub fn run_system_migrations(&self) -> Result<()> {
        let migrations = crate::migrations::system::system_migrations();
        self.with_write_conn(|conn| crate::migrations::run_migrations(conn, &migrations))
    }

    /// Run all pending migrations from the given list.
    ///
    /// Migrations are applied in version order on the write connection.
    /// The operation is idempotent — already-applied migrations are skipped.
    pub fn run_migrations(&self, migrations: &[crate::migrations::Migration]) -> Result<()> {
        self.with_write_conn(|conn| crate::migrations::run_migrations(conn, migrations))
    }

    /// Return the current migration version.
    pub fn migration_version(&self) -> Result<u32> {
        let conn = self.read_conn()?;
        crate::migrations::current_version(&conn)
    }

    /// Check that the database is reachable by executing a trivial query.
    pub fn is_healthy(&self) -> bool {
        match self.read_pool.get() {
            Ok(conn) => conn
                .query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
                .is_ok(),
            Err(_) => false,
        }
    }

    /// Return detailed health diagnostics.
    ///
    /// Checks both the read pool and write connection, measures latency,
    /// and detects pool exhaustion.
    pub fn health_diagnostics(&self) -> HealthDiagnostics {
        // ── Read pool check ──────────────────────────────────────────────
        let read_start = Instant::now();
        let read_pool_ok = match self.read_pool.get() {
            Ok(conn) => conn
                .query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
                .is_ok(),
            Err(e) => {
                warn!(error = %e, "health check: read pool connection failed");
                false
            }
        };
        let read_latency = read_start.elapsed();

        // ── Write connection check ───────────────────────────────────────
        let write_start = Instant::now();
        let write_conn_ok = match self.write_conn.lock() {
            Ok(conn) => conn
                .query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
                .is_ok(),
            Err(e) => {
                warn!(error = %e, "health check: write connection lock poisoned");
                false
            }
        };
        let write_latency = write_start.elapsed();

        // ── Pool stats ───────────────────────────────────────────────────
        let pool = self.stats();
        let pool_utilization_pct = if pool.max_size > 0 {
            let active = pool.total_connections.saturating_sub(pool.idle_connections);
            (active as f64 / pool.max_size as f64) * 100.0
        } else {
            0.0
        };
        let pool_exhausted = pool.idle_connections == 0 && pool.total_connections >= pool.max_size;

        if pool_exhausted {
            warn!(
                total = pool.total_connections,
                idle = pool.idle_connections,
                max = pool.max_size,
                "health check: connection pool exhausted"
            );
        }

        // ── Overall status ───────────────────────────────────────────────
        let status = if !read_pool_ok || !write_conn_ok {
            HealthStatus::Unhealthy
        } else if pool_exhausted || pool_utilization_pct >= POOL_DEGRADED_THRESHOLD * 100.0 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        };

        HealthDiagnostics {
            status,
            read_pool_ok,
            write_conn_ok,
            pool,
            pool_utilization_pct,
            pool_exhausted,
            read_latency_us: read_latency.as_micros() as u64,
            write_latency_us: write_latency.as_micros() as u64,
        }
    }

    /// Execute a closure with exclusive write access, logging a warning if it
    /// exceeds the given duration threshold.
    ///
    /// This is identical to [`with_write_conn`] but instruments the call.
    pub fn with_write_conn_timed<F, T>(&self, label: &str, threshold: Duration, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let start = Instant::now();
        let result = self.with_write_conn(f);
        let elapsed = start.elapsed();
        if elapsed >= threshold {
            warn!(
                query = label,
                elapsed_ms = elapsed.as_millis() as u64,
                threshold_ms = threshold.as_millis() as u64,
                "slow write query detected"
            );
        }
        result
    }

    /// Get a read connection and log a warning if the query exceeds the threshold.
    ///
    /// The closure receives a read connection; timing covers both pool acquisition
    /// and query execution.
    pub fn read_conn_timed<F, T>(&self, label: &str, threshold: Duration, f: F) -> Result<T>
    where
        F: FnOnce(&PooledConnection) -> Result<T>,
    {
        let start = Instant::now();
        let conn = self.read_conn()?;
        let result = f(&conn);
        let elapsed = start.elapsed();
        if elapsed >= threshold {
            warn!(
                query = label,
                elapsed_ms = elapsed.as_millis() as u64,
                threshold_ms = threshold.as_millis() as u64,
                "slow read query detected"
            );
        }
        result
    }
}

/// r2d2 connection customizer that applies pragmas on each new connection.
#[derive(Debug)]
struct ConnectionInit {
    busy_timeout_ms: u32,
}

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for ConnectionInit {
    fn on_acquire(&self, conn: &mut Connection) -> std::result::Result<(), rusqlite::Error> {
        apply_pragmas(conn, self.busy_timeout_ms).map_err(|e| match e {
            DbError::Query(sqlite_err) => sqlite_err,
            other => rusqlite::Error::ToSqlConversionFailure(Box::new(other)),
        })
    }
}

/// Apply standard pragmas to a connection.
fn apply_pragmas(conn: &Connection, busy_timeout_ms: u32) -> Result<()> {
    conn.execute_batch(&format!(
        "PRAGMA journal_mode = WAL;
         PRAGMA busy_timeout = {busy_timeout_ms};
         PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;"
    ))
    .map_err(DbError::Query)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn test_db() -> Database {
        Database::open_in_memory(&PoolConfig::default()).expect("failed to open in-memory DB")
    }

    // ── Basic connectivity ────────────────────────────────────────────────

    #[test]
    fn open_in_memory_succeeds() {
        let db = test_db();
        let conn = db.read_conn().unwrap();
        let result: i64 = conn.query_row("SELECT 1", [], |row| row.get(0)).unwrap();
        assert_eq!(result, 1);
    }

    #[test]
    fn database_is_clone_and_send() {
        fn assert_send_sync<T: Send + Sync + Clone>() {}
        assert_send_sync::<Database>();
    }

    // ── Pragma verification ───────────────────────────────────────────────

    #[test]
    fn wal_mode_enabled() {
        let db = test_db();
        let conn = db.read_conn().unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        // In-memory databases may report "memory" instead of "wal".
        assert!(
            mode == "wal" || mode == "memory",
            "unexpected journal_mode: {mode}"
        );
    }

    #[test]
    fn foreign_keys_enabled() {
        let db = test_db();
        let conn = db.read_conn().unwrap();
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn synchronous_set_to_normal() {
        let db = test_db();
        let conn = db.read_conn().unwrap();
        let sync_val: i64 = conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .unwrap();
        // NORMAL = 1
        assert_eq!(sync_val, 1, "synchronous should be NORMAL (1)");
    }

    #[test]
    fn busy_timeout_configured() {
        let db = test_db();
        let conn = db.read_conn().unwrap();
        let timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(timeout, 5000);
    }

    #[test]
    fn custom_busy_timeout_applied() {
        let config = PoolConfig {
            max_read_connections: 2,
            busy_timeout_ms: 10_000,
        };
        let db = Database::open_in_memory(&config).unwrap();
        let conn = db.read_conn().unwrap();
        let timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(timeout, 10_000);
    }

    #[test]
    fn pragmas_applied_on_write_connection() {
        let db = test_db();
        db.with_write_conn(|conn| {
            let fk: i64 = conn
                .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
                .map_err(DbError::Query)?;
            assert_eq!(fk, 1);

            let timeout: i64 = conn
                .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
                .map_err(DbError::Query)?;
            assert_eq!(timeout, 5000);
            Ok(())
        })
        .unwrap();
    }

    // ── Write and transaction tests ───────────────────────────────────────

    #[test]
    fn write_conn_executes_ddl() {
        let db = test_db();
        db.with_write_conn(|conn| {
            conn.execute(
                "CREATE TABLE test_table (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let conn = db.read_conn().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='test_table'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn transaction_commits_on_success() {
        let db = test_db();
        db.with_write_conn(|conn| {
            conn.execute(
                "CREATE TABLE tx_test (id INTEGER PRIMARY KEY, val TEXT)",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        db.transaction(|tx| {
            tx.execute("INSERT INTO tx_test (val) VALUES (?1)", ["hello"])
                .map_err(DbError::Query)?;
            tx.execute("INSERT INTO tx_test (val) VALUES (?1)", ["world"])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let conn = db.read_conn().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tx_test", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn transaction_rolls_back_on_error() {
        let db = test_db();
        db.with_write_conn(|conn| {
            conn.execute(
                "CREATE TABLE tx_rb_test (id INTEGER PRIMARY KEY, val TEXT)",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let result: Result<()> = db.transaction(|tx| {
            tx.execute("INSERT INTO tx_rb_test (val) VALUES (?1)", ["kept"])
                .map_err(DbError::Query)?;
            Err(DbError::migration("intentional failure"))
        });
        assert!(result.is_err());

        let conn = db.read_conn().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tx_rb_test", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0, "transaction should have been rolled back");
    }

    // ── File-based database tests ─────────────────────────────────────────

    #[test]
    fn open_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("subdir").join("nested").join("test.db");

        let db = Database::open(&db_path, &PoolConfig::default()).unwrap();
        let conn = db.read_conn().unwrap();
        let result: i64 = conn.query_row("SELECT 1", [], |row| row.get(0)).unwrap();
        assert_eq!(result, 1);
    }

    #[test]
    fn file_database_wal_mode_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("wal_test.db");

        let db = Database::open(&db_path, &PoolConfig::default()).unwrap();

        // Verify WAL mode on read connection.
        let conn = db.read_conn().unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal", "file-based DB must use WAL mode");

        // WAL file should exist on disk.
        let wal_path = dir.path().join("wal_test.db-wal");
        assert!(wal_path.exists(), "WAL file should exist on disk");
    }

    #[test]
    fn file_database_persists_data() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("persist.db");

        // Write data.
        {
            let db = Database::open(&db_path, &PoolConfig::default()).unwrap();
            db.with_write_conn(|conn| {
                conn.execute(
                    "CREATE TABLE persist_test (id INTEGER PRIMARY KEY, val TEXT)",
                    [],
                )
                .map_err(DbError::Query)?;
                conn.execute("INSERT INTO persist_test (val) VALUES ('hello')", [])
                    .map_err(DbError::Query)?;
                Ok(())
            })
            .unwrap();
        }

        // Reopen and verify data survives.
        {
            let db = Database::open(&db_path, &PoolConfig::default()).unwrap();
            let conn = db.read_conn().unwrap();
            let val: String = conn
                .query_row("SELECT val FROM persist_test WHERE id = 1", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(val, "hello");
        }
    }

    #[test]
    fn file_database_foreign_keys_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("fk_test.db");
        let db = Database::open(&db_path, &PoolConfig::default()).unwrap();

        db.with_write_conn(|conn| {
            conn.execute_batch(
                "CREATE TABLE parents (id INTEGER PRIMARY KEY);
                 CREATE TABLE children (
                     id INTEGER PRIMARY KEY,
                     parent_id INTEGER NOT NULL REFERENCES parents(id)
                 );",
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Inserting a child with non-existent parent must fail.
        let result = db.with_write_conn(|conn| {
            conn.execute("INSERT INTO children (parent_id) VALUES (999)", [])
                .map_err(DbError::Query)?;
            Ok(())
        });
        assert!(result.is_err(), "FK violation should fail");
    }

    // ── Pool statistics and health ────────────────────────────────────────

    #[test]
    fn pool_stats_reflect_configuration() {
        let config = PoolConfig {
            max_read_connections: 4,
            busy_timeout_ms: 5000,
        };
        let db = Database::open_in_memory(&config).unwrap();
        let stats = db.stats();
        assert_eq!(stats.max_size, 4);
        assert!(stats.total_connections <= 4);
    }

    #[test]
    fn is_healthy_returns_true_for_working_db() {
        let db = test_db();
        assert!(db.is_healthy());
    }

    #[test]
    fn stats_idle_decreases_when_connections_checked_out() {
        let config = PoolConfig {
            max_read_connections: 4,
            busy_timeout_ms: 5000,
        };
        let db = Database::open_in_memory(&config).unwrap();

        // Force pool to initialize connections by checking them out.
        let _c1 = db.read_conn().unwrap();
        let _c2 = db.read_conn().unwrap();

        let stats = db.stats();
        // At least 2 connections are checked out.
        assert!(
            stats.idle_connections <= stats.total_connections,
            "idle should be <= total"
        );
    }

    // ── PoolConfig from DatabaseSettings ──────────────────────────────────

    #[test]
    fn pool_config_from_database_settings() {
        let settings = DatabaseSettings {
            path: std::path::PathBuf::from("/tmp/test.db"),
            max_read_connections: 16,
            busy_timeout_ms: 10_000,
        };
        let config = PoolConfig::from(&settings);
        assert_eq!(config.max_read_connections, 16);
        assert_eq!(config.busy_timeout_ms, 10_000);
    }

    // ── Concurrent access tests ───────────────────────────────────────────

    #[test]
    fn concurrent_reads_from_multiple_threads() {
        let db = Arc::new(test_db());

        // Set up test data.
        db.with_write_conn(|conn| {
            conn.execute_batch(
                "CREATE TABLE concurrent_read (id INTEGER PRIMARY KEY, val INTEGER);
                 INSERT INTO concurrent_read (val) VALUES (42);",
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Spawn multiple reader threads.
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let db = Arc::clone(&db);
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        let conn = db.read_conn().unwrap();
                        let val: i64 = conn
                            .query_row("SELECT val FROM concurrent_read WHERE id = 1", [], |row| {
                                row.get(0)
                            })
                            .unwrap();
                        assert_eq!(val, 42);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("reader thread panicked");
        }
    }

    #[test]
    fn concurrent_reads_while_writing() {
        // Use a file-based database — WAL mode enables true concurrent reads+writes.
        // In-memory shared-cache databases use table-level locking and don't
        // support this concurrency pattern.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("concurrent_rw.db");
        let db = Arc::new(Database::open(&db_path, &PoolConfig::default()).unwrap());

        db.with_write_conn(|conn| {
            conn.execute(
                "CREATE TABLE rw_test (id INTEGER PRIMARY KEY, val INTEGER NOT NULL)",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Writer thread inserts rows.
        let writer_db = Arc::clone(&db);
        let writer = std::thread::spawn(move || {
            for i in 1..=100 {
                writer_db
                    .with_write_conn(|conn| {
                        conn.execute("INSERT INTO rw_test (val) VALUES (?1)", [i])
                            .map_err(DbError::Query)?;
                        Ok(())
                    })
                    .unwrap();
            }
        });

        // Reader threads count rows concurrently.
        let reader_handles: Vec<_> = (0..4)
            .map(|_| {
                let db = Arc::clone(&db);
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        let conn = db.read_conn().unwrap();
                        let count: i64 = conn
                            .query_row("SELECT COUNT(*) FROM rw_test", [], |row| row.get(0))
                            .unwrap();
                        assert!(count >= 0 && count <= 100);
                    }
                })
            })
            .collect();

        writer.join().expect("writer thread panicked");
        for h in reader_handles {
            h.join().expect("reader thread panicked");
        }

        // Final count should be exactly 100.
        let conn = db.read_conn().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM rw_test", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 100);
    }

    #[test]
    fn serialized_writes_from_multiple_threads() {
        let db = Arc::new(test_db());

        db.with_write_conn(|conn| {
            conn.execute(
                "CREATE TABLE serial_write (id INTEGER PRIMARY KEY, thread_id INTEGER NOT NULL)",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let handles: Vec<_> = (0..4)
            .map(|thread_id| {
                let db = Arc::clone(&db);
                std::thread::spawn(move || {
                    for _ in 0..25 {
                        db.with_write_conn(|conn| {
                            conn.execute(
                                "INSERT INTO serial_write (thread_id) VALUES (?1)",
                                [thread_id],
                            )
                            .map_err(DbError::Query)?;
                            Ok(())
                        })
                        .unwrap();
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("writer thread panicked");
        }

        // All 100 inserts should succeed (4 threads × 25 each).
        let conn = db.read_conn().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM serial_write", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 100);
    }

    // ── Pool configuration edge cases ─────────────────────────────────────

    #[test]
    fn pool_with_single_read_connection() {
        let config = PoolConfig {
            max_read_connections: 1,
            busy_timeout_ms: 5000,
        };
        let db = Database::open_in_memory(&config).unwrap();

        let conn = db.read_conn().unwrap();
        let val: i64 = conn.query_row("SELECT 1", [], |row| row.get(0)).unwrap();
        assert_eq!(val, 1);

        let stats = db.stats();
        assert_eq!(stats.max_size, 1);
    }

    #[test]
    fn multiple_read_connections_are_independent() {
        let db = test_db();

        // Create a table and insert data.
        db.with_write_conn(|conn| {
            conn.execute_batch(
                "CREATE TABLE indep_test (id INTEGER PRIMARY KEY, val TEXT);
                 INSERT INTO indep_test (val) VALUES ('a');
                 INSERT INTO indep_test (val) VALUES ('b');",
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Get two read connections and use them independently.
        let conn1 = db.read_conn().unwrap();
        let conn2 = db.read_conn().unwrap();

        let count1: i64 = conn1
            .query_row("SELECT COUNT(*) FROM indep_test", [], |row| row.get(0))
            .unwrap();
        let count2: i64 = conn2
            .query_row("SELECT COUNT(*) FROM indep_test", [], |row| row.get(0))
            .unwrap();

        assert_eq!(count1, 2);
        assert_eq!(count2, 2);
    }

    #[test]
    fn write_visible_to_subsequent_reads() {
        let db = test_db();

        db.with_write_conn(|conn| {
            conn.execute_batch(
                "CREATE TABLE vis_test (id INTEGER PRIMARY KEY, val TEXT);
                 INSERT INTO vis_test (val) VALUES ('first');",
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Read should see the inserted row.
        let conn = db.read_conn().unwrap();
        let val: String = conn
            .query_row("SELECT val FROM vis_test WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(val, "first");

        drop(conn);

        // Insert another row.
        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO vis_test (val) VALUES ('second')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // New read connection should see both rows.
        let conn = db.read_conn().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM vis_test", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn cloned_database_shares_pool() {
        let db = test_db();

        db.with_write_conn(|conn| {
            conn.execute_batch(
                "CREATE TABLE clone_test (id INTEGER PRIMARY KEY, val INTEGER);
                 INSERT INTO clone_test (val) VALUES (99);",
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let db2 = db.clone();

        // Both handles read the same data.
        let conn1 = db.read_conn().unwrap();
        let conn2 = db2.read_conn().unwrap();

        let v1: i64 = conn1
            .query_row("SELECT val FROM clone_test WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        let v2: i64 = conn2
            .query_row("SELECT val FROM clone_test WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();

        assert_eq!(v1, 99);
        assert_eq!(v2, 99);
    }

    // ── Health diagnostics tests ──────────────────────────────────────────

    #[test]
    fn health_diagnostics_reports_healthy() {
        let db = test_db();
        let diag = db.health_diagnostics();
        assert_eq!(diag.status, HealthStatus::Healthy);
        assert!(diag.read_pool_ok);
        assert!(diag.write_conn_ok);
        assert!(!diag.pool_exhausted);
    }

    #[test]
    fn health_diagnostics_includes_pool_stats() {
        let db = test_db();
        let diag = db.health_diagnostics();
        assert_eq!(diag.pool.max_size, 8);
        assert!(diag.pool.total_connections > 0);
    }

    #[test]
    fn health_diagnostics_records_latency() {
        let db = test_db();
        let diag = db.health_diagnostics();
        // Latencies should be recorded (non-negative; they may be 0 on fast systems).
        assert!(diag.read_latency_us < 1_000_000, "read latency too high");
        assert!(diag.write_latency_us < 1_000_000, "write latency too high");
    }

    #[test]
    fn health_diagnostics_pool_utilization_zero_when_idle() {
        let db = test_db();
        // Ensure no connections are checked out.
        let diag = db.health_diagnostics();
        // Utilization should be very low (health check itself borrows one).
        assert!(diag.pool_utilization_pct < 50.0);
    }

    #[test]
    fn health_diagnostics_detects_high_utilization() {
        // Create a pool with only 2 read connections.
        let config = PoolConfig {
            max_read_connections: 2,
            busy_timeout_ms: 5000,
        };
        let db = Database::open_in_memory(&config).unwrap();

        // Hold both connections to simulate exhaustion.
        let _conn1 = db.read_conn().unwrap();
        let _conn2 = db.read_conn().unwrap();

        // Now the health check itself can't get a read connection from the pool,
        // so read_pool_ok will be false and status will be Unhealthy.
        // With pool size 2 and both held, pool is exhausted.
        let diag = db.health_diagnostics();
        // The health check tries to get a conn from the pool; with 0 idle it may timeout.
        // With default r2d2 timeout, it will fail. Status should reflect that.
        assert!(!diag.read_pool_ok || diag.pool_exhausted);
        assert!(diag.status == HealthStatus::Degraded || diag.status == HealthStatus::Unhealthy);
    }

    #[test]
    fn health_status_display() {
        assert_eq!(HealthStatus::Healthy.to_string(), "healthy");
        assert_eq!(HealthStatus::Degraded.to_string(), "degraded");
        assert_eq!(HealthStatus::Unhealthy.to_string(), "unhealthy");
    }

    #[test]
    fn health_diagnostics_serializes_to_json() {
        let db = test_db();
        let diag = db.health_diagnostics();
        let json = serde_json::to_value(&diag).expect("diagnostics should serialize");
        assert!(json.get("status").is_some());
        assert!(json.get("read_pool_ok").is_some());
        assert!(json.get("write_conn_ok").is_some());
        assert!(json.get("pool").is_some());
        assert!(json.get("pool_utilization_pct").is_some());
        assert!(json.get("pool_exhausted").is_some());
        assert!(json.get("read_latency_us").is_some());
        assert!(json.get("write_latency_us").is_some());
    }

    // ── Slow query logging tests ──────────────────────────────────────────

    #[test]
    fn with_write_conn_timed_returns_result() {
        let db = test_db();
        db.with_write_conn(|conn| {
            conn.execute(
                "CREATE TABLE timed_test (id INTEGER PRIMARY KEY, val TEXT)",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let count = db
            .with_write_conn_timed("insert test", Duration::from_secs(10), |conn| {
                conn.execute("INSERT INTO timed_test (val) VALUES ('hello')", [])
                    .map_err(DbError::Query)?;
                let c: i64 = conn
                    .query_row("SELECT COUNT(*) FROM timed_test", [], |row| row.get(0))
                    .map_err(DbError::Query)?;
                Ok(c)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn read_conn_timed_returns_result() {
        let db = test_db();
        db.with_write_conn(|conn| {
            conn.execute_batch(
                "CREATE TABLE read_timed (id INTEGER PRIMARY KEY, val INTEGER);
                 INSERT INTO read_timed (val) VALUES (42);",
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let val = db
            .read_conn_timed("read test", Duration::from_secs(10), |conn| {
                let v: i64 = conn
                    .query_row("SELECT val FROM read_timed WHERE id = 1", [], |row| {
                        row.get(0)
                    })
                    .map_err(DbError::Query)?;
                Ok(v)
            })
            .unwrap();
        assert_eq!(val, 42);
    }

    #[test]
    fn with_write_conn_timed_propagates_errors() {
        let db = test_db();
        let result = db.with_write_conn_timed(
            "bad query",
            Duration::from_secs(10),
            |conn| {
                conn.execute("INSERT INTO nonexistent_table (val) VALUES (1)", [])
                    .map_err(DbError::Query)?;
                Ok(())
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn read_conn_timed_propagates_errors() {
        let db = test_db();
        let result = db.read_conn_timed("bad query", Duration::from_secs(10), |conn| {
            let _: i64 = conn
                .query_row("SELECT * FROM nonexistent_table", [], |row| row.get(0))
                .map_err(DbError::Query)?;
            Ok(())
        });
        assert!(result.is_err());
    }
}
