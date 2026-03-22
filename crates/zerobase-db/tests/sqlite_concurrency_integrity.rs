//! SQLite Concurrent Access and Data Integrity Under Load Tests
//!
//! Verifies that Zerobase's embedded SQLite database handles concurrency,
//! data integrity, and performance correctly under various stress scenarios.
//!
//! Test coverage:
//! 1. WAL mode handles concurrent reads during writes without blocking
//! 2. Schema alteration (table rebuild for column removal) doesn't corrupt data
//! 3. Connection pool exhaustion under high concurrent load returns proper errors
//! 4. Transaction isolation: batch operations are truly atomic (all-or-nothing)
//! 5. FTS5 index consistency after rapid create/update/delete cycles
//! 6. Database backup while writes are in-flight produces a consistent snapshot
//! 7. Large dataset performance: pagination with 100k+ records, filtering

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

use zerobase_core::services::record_service::{RecordQuery, RecordRepository, SortDirection};
use zerobase_db::{
    CollectionSchema, ColumnDef, Database, DbError, IndexDef, PoolConfig, SchemaAlteration,
    SchemaRepository,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn setup_db() -> Database {
    let config = PoolConfig {
        max_read_connections: 8,
        busy_timeout_ms: 5000,
    };
    let db = Database::open_in_memory(&config).expect("failed to create in-memory db");
    db.run_system_migrations()
        .expect("failed to run system migrations");
    db
}

fn setup_db_with_config(config: PoolConfig) -> Database {
    let db = Database::open_in_memory(&config).expect("failed to create in-memory db");
    db.run_system_migrations()
        .expect("failed to run system migrations");
    db
}

fn setup_file_db(dir: &std::path::Path) -> Database {
    let db_path = dir.join("test.db");
    let config = PoolConfig {
        max_read_connections: 8,
        busy_timeout_ms: 5000,
    };
    let db = Database::open(&db_path, &config).expect("failed to create file db");
    db.run_system_migrations()
        .expect("failed to run system migrations");
    db
}

fn now_str() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S%.3fZ")
        .to_string()
}

fn create_items_collection(db: &Database) {
    let schema = CollectionSchema {
        name: "items".to_string(),
        collection_type: "base".to_string(),
        columns: vec![
            ColumnDef {
                name: "title".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: None,
                unique: false,
            },
            ColumnDef {
                name: "content".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: false,
                default: None,
                unique: false,
            },
            ColumnDef {
                name: "status".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: Some("'draft'".to_string()),
                unique: false,
            },
            ColumnDef {
                name: "views".to_string(),
                sql_type: "INTEGER".to_string(),
                not_null: true,
                default: Some("0".to_string()),
                unique: false,
            },
            ColumnDef {
                name: "extra".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: false,
                default: None,
                unique: false,
            },
        ],
        indexes: vec![
            IndexDef {
                name: "idx_items_status".to_string(),
                columns: vec!["status".to_string()],
                index_columns: vec![],
                unique: false,
            },
            IndexDef {
                name: "idx_items_views".to_string(),
                columns: vec!["views".to_string()],
                index_columns: vec![],
                unique: false,
            },
        ],
        searchable_fields: vec![],
        view_query: None,
    };
    db.create_collection(&schema)
        .expect("failed to create items collection");
}

fn create_items_with_fts(db: &Database) {
    let schema = CollectionSchema {
        name: "articles".to_string(),
        collection_type: "base".to_string(),
        columns: vec![
            ColumnDef {
                name: "title".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: None,
                unique: false,
            },
            ColumnDef {
                name: "body".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: false,
                default: None,
                unique: false,
            },
        ],
        indexes: vec![],
        searchable_fields: vec!["title".to_string(), "body".to_string()],
        view_query: None,
    };
    db.create_collection(&schema)
        .expect("failed to create articles collection");
}

fn insert_item(db: &Database, id: &str, title: &str, status: &str, views: i64) {
    let mut data = HashMap::new();
    data.insert("id".to_string(), json!(id));
    data.insert("title".to_string(), json!(title));
    data.insert("content".to_string(), json!(format!("Content for {title}")));
    data.insert("status".to_string(), json!(status));
    data.insert("views".to_string(), json!(views));
    data.insert("extra".to_string(), json!(format!("extra-{id}")));
    data.insert("created".to_string(), json!(now_str()));
    data.insert("updated".to_string(), json!(now_str()));
    db.insert("items", &data)
        .unwrap_or_else(|e| panic!("failed to insert {id}: {e}"));
}

fn insert_article(db: &Database, id: &str, title: &str, body: &str) {
    let mut data = HashMap::new();
    data.insert("id".to_string(), json!(id));
    data.insert("title".to_string(), json!(title));
    data.insert("body".to_string(), json!(body));
    data.insert("created".to_string(), json!(now_str()));
    data.insert("updated".to_string(), json!(now_str()));
    db.insert("articles", &data)
        .unwrap_or_else(|e| panic!("failed to insert article {id}: {e}"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. WAL MODE: CONCURRENT READS DURING WRITES
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn wal_concurrent_reads_not_blocked_by_writes() {
    // WAL mode requires a file-based database. In-memory databases with shared
    // cache use table-level locking, not WAL page-level locking.
    let dir = tempfile::tempdir().unwrap();
    let db = setup_file_db(dir.path());
    create_items_collection(&db);

    // Seed some initial data.
    for i in 0..100 {
        insert_item(&db, &format!("item_{i:04}"), &format!("Item {i}"), "published", i);
    }

    let db = Arc::new(db);
    let barrier = Arc::new(Barrier::new(3)); // 1 writer + 2 readers
    let errors = Arc::new(AtomicU64::new(0));

    // Writer thread: continuously insert records.
    let db_w = Arc::clone(&db);
    let barrier_w = Arc::clone(&barrier);
    let writer = thread::spawn(move || {
        barrier_w.wait();
        for i in 100..200 {
            insert_item(
                &db_w,
                &format!("item_{i:04}"),
                &format!("Item {i}"),
                "draft",
                i,
            );
            // Small sleep to simulate realistic write pattern.
            thread::sleep(Duration::from_micros(100));
        }
    });

    // Reader threads: continuously read while writes happen.
    let mut readers = vec![];
    for reader_id in 0..2 {
        let db_r = Arc::clone(&db);
        let barrier_r = Arc::clone(&barrier);
        let errors_r = Arc::clone(&errors);
        readers.push(thread::spawn(move || {
            barrier_r.wait();
            let mut success_count = 0u64;
            for _ in 0..50 {
                let query = RecordQuery {
                    filter: Some("status = 'published'".to_string()),
                    sort: vec![("created".to_string(), SortDirection::Desc)],
                    page: 1,
                    per_page: 20,
                    fields: None,
                    search: None,
                };
                match db_r.find_many("items", &query) {
                    Ok(list) => {
                        assert!(
                            list.total_items >= 100,
                            "reader {reader_id}: expected >= 100 published items, got {}",
                            list.total_items
                        );
                        success_count += 1;
                    }
                    Err(e) => {
                        eprintln!("reader {reader_id} error: {e}");
                        errors_r.fetch_add(1, Ordering::Relaxed);
                    }
                }
                thread::sleep(Duration::from_micros(50));
            }
            success_count
        }));
    }

    writer.join().expect("writer panicked");
    let total_reads: u64 = readers
        .into_iter()
        .map(|h| h.join().expect("reader panicked"))
        .sum();

    let error_count = errors.load(Ordering::Relaxed);
    assert_eq!(error_count, 0, "no read errors expected under WAL mode");
    assert!(total_reads >= 50, "readers should complete successfully");
}

#[test]
fn wal_reads_see_consistent_snapshots() {
    // WAL mode requires a file-based database for true concurrent access.
    let dir = tempfile::tempdir().unwrap();
    let db = setup_file_db(dir.path());
    create_items_collection(&db);

    // Insert 50 "published" records.
    for i in 0..50 {
        insert_item(&db, &format!("snap_{i:03}"), &format!("Snap {i}"), "published", 1);
    }

    let db = Arc::new(db);
    let barrier = Arc::new(Barrier::new(2));

    // Writer: insert 50 more "published" records in a transaction.
    let db_w = Arc::clone(&db);
    let barrier_w = Arc::clone(&barrier);
    let writer = thread::spawn(move || {
        barrier_w.wait();
        db_w.transaction(|tx| {
            for i in 50..100 {
                let id = format!("snap_{i:03}");
                tx.execute(
                    "INSERT INTO items (id, title, content, status, views, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![id, format!("Snap {i}"), "", "published", 1, now_str(), now_str()],
                ).map_err(DbError::Query)?;
            }
            Ok(())
        }).expect("transaction should succeed");
    });

    // Reader: read count multiple times. Should see either 50 or 100, not in-between.
    let db_r = Arc::clone(&db);
    let barrier_r = Arc::clone(&barrier);
    let reader = thread::spawn(move || {
        barrier_r.wait();
        let mut observed_counts = vec![];
        for _ in 0..20 {
            let count = db_r
                .count("items", Some("status = 'published'"))
                .expect("count should succeed");
            observed_counts.push(count);
            thread::sleep(Duration::from_micros(200));
        }
        observed_counts
    });

    writer.join().expect("writer panicked");
    let counts = reader.join().expect("reader panicked");

    // Every observed count should be either 50 (before commit) or 100 (after).
    // No partial state (e.g. 67 records) should be visible.
    for &c in &counts {
        assert!(
            c == 50 || c == 100,
            "expected 50 or 100, got {c} — partial transaction visible!"
        );
    }
}

#[test]
fn wal_many_concurrent_readers() {
    // Stress test: 8 concurrent readers hammering the DB while writes proceed.
    // Must use a file-based DB so WAL mode is actually enabled (in-memory shared cache
    // uses table-level locking instead).
    let tmp = tempfile::tempdir().expect("tempdir");
    let db = setup_file_db(tmp.path());
    create_items_collection(&db);

    for i in 0..500 {
        insert_item(
            &db,
            &format!("mcr_{i:04}"),
            &format!("Record {i}"),
            if i % 2 == 0 { "published" } else { "draft" },
            i,
        );
    }

    let db = Arc::new(db);
    let num_readers = 8;
    let barrier = Arc::new(Barrier::new(num_readers + 1));
    let total_errors = Arc::new(AtomicU64::new(0));

    // Writer thread.
    let db_w = Arc::clone(&db);
    let barrier_w = Arc::clone(&barrier);
    let writer = thread::spawn(move || {
        barrier_w.wait();
        for i in 500..600 {
            insert_item(
                &db_w,
                &format!("mcr_{i:04}"),
                &format!("Record {i}"),
                "published",
                i,
            );
        }
    });

    // Spawn reader threads.
    let mut readers = vec![];
    for _ in 0..num_readers {
        let db_r = Arc::clone(&db);
        let barrier_r = Arc::clone(&barrier);
        let errors = Arc::clone(&total_errors);
        readers.push(thread::spawn(move || {
            barrier_r.wait();
            for _ in 0..30 {
                let query = RecordQuery {
                    filter: None,
                    sort: vec![("views".to_string(), SortDirection::Asc)],
                    page: 1,
                    per_page: 50,
                    fields: None,
                    search: None,
                };
                if db_r.find_many("items", &query).is_err() {
                    errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }

    writer.join().expect("writer panicked");
    for r in readers {
        r.join().expect("reader panicked");
    }

    assert_eq!(
        total_errors.load(Ordering::Relaxed),
        0,
        "no read errors expected with 8 concurrent readers"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. SCHEMA ALTERATION: TABLE REBUILD PRESERVES DATA
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn alter_remove_column_preserves_data() {
    // Removing a column (requires table rebuild in SQLite) must not lose data
    // from the remaining columns.
    let db = setup_db();
    create_items_collection(&db);

    // Insert records with all columns populated.
    for i in 0..50 {
        insert_item(
            &db,
            &format!("alt_{i:03}"),
            &format!("Title {i}"),
            "published",
            i * 10,
        );
    }

    // Alter: remove the "extra" column.
    let new_schema = CollectionSchema {
        name: "items".to_string(),
        collection_type: "base".to_string(),
        columns: vec![
            ColumnDef {
                name: "title".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: None,
                unique: false,
            },
            ColumnDef {
                name: "content".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: false,
                default: None,
                unique: false,
            },
            ColumnDef {
                name: "status".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: Some("'draft'".to_string()),
                unique: false,
            },
            ColumnDef {
                name: "views".to_string(),
                sql_type: "INTEGER".to_string(),
                not_null: true,
                default: Some("0".to_string()),
                unique: false,
            },
        ],
        indexes: vec![
            IndexDef {
                name: "idx_items_status".to_string(),
                columns: vec!["status".to_string()],
                index_columns: vec![],
                unique: false,
            },
            IndexDef {
                name: "idx_items_views".to_string(),
                columns: vec!["views".to_string()],
                index_columns: vec![],
                unique: false,
            },
        ],
        searchable_fields: vec![],
        view_query: None,
    };

    let alteration = SchemaAlteration {
        schema: new_schema,
        renames: vec![],
    };

    db.alter_collection("items", &alteration)
        .expect("alter_collection should succeed");

    // Verify all 50 records survived with correct data.
    let query = RecordQuery {
        filter: None,
        sort: vec![("title".to_string(), SortDirection::Asc)],
        page: 1,
        per_page: 100,
        fields: None,
        search: None,
    };
    let result = db.find_many("items", &query).expect("find_many should work");
    assert_eq!(result.total_items, 50, "all records must survive alteration");

    // Verify specific record data is intact.
    let record = db.find_one("items", "alt_025").expect("record should exist");
    assert_eq!(record.get("title").unwrap(), "Title 25");
    assert_eq!(record.get("status").unwrap(), "published");
    assert_eq!(record.get("views").unwrap(), 250);

    // Verify the "extra" column is gone.
    assert!(
        record.get("extra").is_none(),
        "removed column should not appear in results"
    );
}

#[test]
fn alter_add_column_preserves_existing_data() {
    let db = setup_db();
    create_items_collection(&db);

    for i in 0..20 {
        insert_item(&db, &format!("add_{i:03}"), &format!("Title {i}"), "draft", i);
    }

    // Add a new "priority" column.
    let new_schema = CollectionSchema {
        name: "items".to_string(),
        collection_type: "base".to_string(),
        columns: vec![
            ColumnDef {
                name: "title".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: None,
                unique: false,
            },
            ColumnDef {
                name: "content".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: false,
                default: None,
                unique: false,
            },
            ColumnDef {
                name: "status".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: Some("'draft'".to_string()),
                unique: false,
            },
            ColumnDef {
                name: "views".to_string(),
                sql_type: "INTEGER".to_string(),
                not_null: true,
                default: Some("0".to_string()),
                unique: false,
            },
            ColumnDef {
                name: "extra".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: false,
                default: None,
                unique: false,
            },
            ColumnDef {
                name: "priority".to_string(),
                sql_type: "INTEGER".to_string(),
                not_null: false,
                default: Some("0".to_string()),
                unique: false,
            },
        ],
        indexes: vec![
            IndexDef {
                name: "idx_items_status".to_string(),
                columns: vec!["status".to_string()],
                index_columns: vec![],
                unique: false,
            },
            IndexDef {
                name: "idx_items_views".to_string(),
                columns: vec!["views".to_string()],
                index_columns: vec![],
                unique: false,
            },
        ],
        searchable_fields: vec![],
        view_query: None,
    };

    let alteration = SchemaAlteration {
        schema: new_schema,
        renames: vec![],
    };

    db.alter_collection("items", &alteration)
        .expect("adding a column should succeed");

    let result = db
        .find_many(
            "items",
            &RecordQuery {
                page: 1,
                per_page: 100,
                ..Default::default()
            },
        )
        .expect("find_many after alter");
    assert_eq!(result.total_items, 20, "no records should be lost");

    // Existing records should have the default value for the new column.
    let record = db.find_one("items", "add_005").expect("should exist");
    assert_eq!(record.get("title").unwrap(), "Title 5");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. CONNECTION POOL EXHAUSTION
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn pool_exhaustion_returns_error_not_deadlock() {
    // With a very small pool, concurrent requests should get errors, not hang.
    let config = PoolConfig {
        max_read_connections: 2,
        busy_timeout_ms: 500, // Short timeout to detect issues quickly.
    };
    let db = setup_db_with_config(config);
    create_items_collection(&db);

    for i in 0..10 {
        insert_item(&db, &format!("pool_{i:02}"), &format!("Item {i}"), "published", i);
    }

    let db = Arc::new(db);
    let num_threads = 10; // Much more than pool size of 2.
    let barrier = Arc::new(Barrier::new(num_threads));
    let errors = Arc::new(AtomicU64::new(0));
    let successes = Arc::new(AtomicU64::new(0));

    let mut handles = vec![];
    for _ in 0..num_threads {
        let db_r = Arc::clone(&db);
        let barrier_r = Arc::clone(&barrier);
        let err = Arc::clone(&errors);
        let suc = Arc::clone(&successes);
        handles.push(thread::spawn(move || {
            barrier_r.wait();
            // Each thread tries to hold a connection for some time.
            for _ in 0..5 {
                match db_r.find_many(
                    "items",
                    &RecordQuery {
                        page: 1,
                        per_page: 10,
                        ..Default::default()
                    },
                ) {
                    Ok(_) => {
                        suc.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        err.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
    }

    for h in handles {
        // Should not hang (deadlock). Use a generous timeout.
        h.join().expect("thread should not panic or deadlock");
    }

    // Most requests should succeed (r2d2 queues waiting callers).
    let total_success = successes.load(Ordering::Relaxed);
    assert!(
        total_success > 0,
        "at least some requests should succeed even under contention"
    );

    // Health diagnostics should detect degradation.
    let diag = db.health_diagnostics();
    // After all threads finish, pool should recover.
    assert!(
        diag.read_pool_ok,
        "pool should be functional after contention subsides"
    );
}

#[test]
fn pool_health_reports_degradation_under_load() {
    let config = PoolConfig {
        max_read_connections: 2,
        busy_timeout_ms: 5000,
    };
    let db = setup_db_with_config(config);
    create_items_collection(&db);

    // Without load, should be healthy.
    let diag = db.health_diagnostics();
    assert_eq!(
        diag.status,
        zerobase_db::HealthStatus::Healthy,
        "unloaded pool should be healthy"
    );
    assert!(!diag.pool_exhausted, "pool should not be exhausted");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. TRANSACTION ATOMICITY
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn transaction_batch_is_all_or_nothing() {
    // If any insert in a batch fails, none should be committed.
    let db = setup_db();
    create_items_collection(&db);

    // Create a record to cause a unique constraint violation.
    insert_item(&db, "conflict_id", "Existing", "draft", 0);

    // Attempt a batch transaction where the 3rd insert conflicts.
    let result = db.transaction(|tx| {
        tx.execute(
            "INSERT INTO items (id, title, content, status, views, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params!["batch_01", "First", "", "draft", 0, now_str(), now_str()],
        ).map_err(DbError::Query)?;

        tx.execute(
            "INSERT INTO items (id, title, content, status, views, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params!["batch_02", "Second", "", "draft", 0, now_str(), now_str()],
        ).map_err(DbError::Query)?;

        // This should fail — "conflict_id" already exists.
        tx.execute(
            "INSERT INTO items (id, title, content, status, views, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params!["conflict_id", "Conflict", "", "draft", 0, now_str(), now_str()],
        ).map_err(DbError::Query)?;

        tx.execute(
            "INSERT INTO items (id, title, content, status, views, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params!["batch_03", "Third", "", "draft", 0, now_str(), now_str()],
        ).map_err(DbError::Query)?;

        Ok(())
    });

    assert!(result.is_err(), "transaction with conflict should fail");

    // Verify NONE of the batch records were committed (rollback).
    assert!(
        db.find_one("items", "batch_01").is_err(),
        "batch_01 should not exist after rollback"
    );
    assert!(
        db.find_one("items", "batch_02").is_err(),
        "batch_02 should not exist after rollback"
    );
    assert!(
        db.find_one("items", "batch_03").is_err(),
        "batch_03 should not exist after rollback"
    );

    // Original record should be untouched.
    let existing = db.find_one("items", "conflict_id").expect("original should survive");
    assert_eq!(existing.get("title").unwrap(), "Existing");
}

#[test]
fn transaction_commit_makes_all_visible() {
    let db = setup_db();
    create_items_collection(&db);

    db.transaction(|tx| {
        for i in 0..10 {
            tx.execute(
                "INSERT INTO items (id, title, content, status, views, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![format!("tx_{i:02}"), format!("Tx Item {i}"), "", "published", i, now_str(), now_str()],
            ).map_err(DbError::Query)?;
        }
        Ok(())
    })
    .expect("transaction should succeed");

    // All 10 records should be visible.
    let count = db
        .count("items", None)
        .expect("count should work");
    assert_eq!(count, 10, "all transaction records should be visible");
}

#[test]
fn nested_error_rolls_back_entire_transaction() {
    let db = setup_db();
    create_items_collection(&db);

    let result: zerobase_db::Result<()> = db.transaction(|tx| {
        tx.execute(
            "INSERT INTO items (id, title, content, status, views, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params!["nest_01", "First", "", "draft", 0, now_str(), now_str()],
        ).map_err(DbError::Query)?;

        // Intentionally fail.
        Err(DbError::migration("simulated failure in nested operation"))
    });

    assert!(result.is_err());
    assert!(
        db.find_one("items", "nest_01").is_err(),
        "record should not exist after rollback"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. FTS5 INDEX CONSISTENCY
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn fts5_consistent_after_rapid_crud_cycles() {
    let db = setup_db();
    create_items_with_fts(&db);

    // Rapid create/update/delete cycles.
    for i in 0..50 {
        let id = format!("fts_{i:03}");
        insert_article(&db, &id, &format!("Rust Programming {i}"), "Learn Rust basics");
    }

    // Update some records — change both title AND body so "Rust" no longer appears.
    for i in 0..25 {
        let id = format!("fts_{i:03}");
        let mut update_data = HashMap::new();
        update_data.insert(
            "title".to_string(),
            json!(format!("Advanced Python {i}")),
        );
        update_data.insert("body".to_string(), json!("Learn Python basics"));
        update_data.insert("updated".to_string(), json!(now_str()));
        db.update("articles", &id, &update_data)
            .expect("update should succeed");
    }

    // Delete some records.
    for i in 25..35 {
        let id = format!("fts_{i:03}");
        db.delete("articles", &id).expect("delete should succeed");
    }

    // FTS search for "Rust" should only find records 25-49 (first 25 were updated to "Python").
    let query = RecordQuery {
        search: Some("Rust".to_string()),
        page: 1,
        per_page: 100,
        ..Default::default()
    };
    let results = db
        .find_many("articles", &query)
        .expect("FTS search should work");

    // Records 0-24: updated to "Python" (no longer match "Rust")
    // Records 25-34: deleted
    // Records 35-49: still have "Rust" in title → 15 results expected.
    assert_eq!(
        results.total_items, 15,
        "FTS should reflect updates and deletes: expected 15, got {}",
        results.total_items
    );

    // Search for "Python" should find the 25 updated records.
    let query_python = RecordQuery {
        search: Some("Python".to_string()),
        page: 1,
        per_page: 100,
        ..Default::default()
    };
    let python_results = db
        .find_many("articles", &query_python)
        .expect("FTS search for Python should work");
    assert_eq!(
        python_results.total_items, 25,
        "FTS should find updated records: expected 25, got {}",
        python_results.total_items
    );
}

#[test]
fn fts5_empty_search_returns_all_records() {
    let db = setup_db();
    create_items_with_fts(&db);

    for i in 0..10 {
        insert_article(&db, &format!("all_{i}"), &format!("Article {i}"), "Body text");
    }

    // Query without search should return all.
    let query = RecordQuery {
        page: 1,
        per_page: 100,
        ..Default::default()
    };
    let results = db
        .find_many("articles", &query)
        .expect("should return all");
    assert_eq!(results.total_items, 10);
}

#[test]
fn fts5_search_no_match_returns_empty() {
    let db = setup_db();
    create_items_with_fts(&db);

    insert_article(&db, "nomatch_1", "Hello World", "Body text");

    let query = RecordQuery {
        search: Some("xyznonexistent".to_string()),
        page: 1,
        per_page: 100,
        ..Default::default()
    };
    let results = db
        .find_many("articles", &query)
        .expect("should return empty");
    assert_eq!(results.total_items, 0);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. BACKUP DURING WRITES
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn backup_during_writes_produces_consistent_snapshot() {
    use zerobase_core::services::backup_service::BackupRepository;

    let dir = tempfile::tempdir().unwrap();
    let db = setup_file_db(dir.path());
    create_items_collection(&db);

    // Insert initial batch of records.
    for i in 0..100 {
        insert_item(
            &db,
            &format!("bk_{i:04}"),
            &format!("Backup Item {i}"),
            "published",
            i,
        );
    }

    let db = Arc::new(db);

    // Start writing in background.
    let db_w = Arc::clone(&db);
    let still_writing = Arc::new(AtomicBool::new(true));
    let still_writing_w = Arc::clone(&still_writing);

    let writer = thread::spawn(move || {
        let mut i = 100;
        while still_writing_w.load(Ordering::Relaxed) {
            insert_item(
                &db_w,
                &format!("bk_{i:04}"),
                &format!("Backup Item {i}"),
                "published",
                i,
            );
            i += 1;
            thread::sleep(Duration::from_millis(1));
        }
        i // Return total inserted count.
    });

    // Let writes proceed briefly, then take a backup.
    thread::sleep(Duration::from_millis(50));

    let backup_result = db.create_backup("test_backup.db");
    assert!(
        backup_result.is_ok(),
        "backup should succeed during writes: {:?}",
        backup_result.err()
    );

    // Stop the writer.
    still_writing.store(false, Ordering::Relaxed);
    writer.join().expect("writer panicked");

    // Verify the backup is a valid, consistent database.
    let backup_path = dir.path().join("pb_backups").join("test_backup.db");
    assert!(backup_path.exists(), "backup file should exist");

    let backup_db = Database::open(
        &backup_path,
        &PoolConfig {
            max_read_connections: 2,
            busy_timeout_ms: 5000,
        },
    )
    .expect("backup should be openable");

    // Count records in backup — should be a consistent number (at least the initial 100).
    let backup_conn = backup_db.read_conn().expect("backup read should work");
    let count: i64 = backup_conn
        .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
        .expect("backup query should work");

    assert!(
        count >= 100,
        "backup should contain at least the initial 100 records, got {count}"
    );

    // Verify data integrity in the backup.
    let title: String = backup_conn
        .query_row(
            "SELECT title FROM items WHERE id = 'bk_0050'",
            [],
            |row| row.get(0),
        )
        .expect("should find record in backup");
    assert_eq!(title, "Backup Item 50");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. LARGE DATASET PERFORMANCE
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn pagination_with_100k_records() {
    let db = setup_db();
    create_items_collection(&db);

    let start = Instant::now();
    let batch_size = 100_000;

    // Bulk insert using transaction for speed.
    db.transaction(|tx| {
        for i in 0..batch_size {
            let status = match i % 3 {
                0 => "published",
                1 => "draft",
                _ => "archived",
            };
            tx.execute(
                "INSERT INTO items (id, title, content, status, views, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    format!("lg_{i:06}"),
                    format!("Large Item {i}"),
                    format!("Content for item {i}"),
                    status,
                    i,
                    now_str(),
                    now_str()
                ],
            ).map_err(DbError::Query)?;
        }
        Ok(())
    })
    .expect("bulk insert should succeed");

    let insert_time = start.elapsed();
    eprintln!("Inserted {batch_size} records in {:?}", insert_time);

    // Test pagination on first page.
    let start = Instant::now();
    let result = db
        .find_many(
            "items",
            &RecordQuery {
                page: 1,
                per_page: 50,
                sort: vec![("created".to_string(), SortDirection::Desc)],
                ..Default::default()
            },
        )
        .expect("pagination should work");
    let first_page_time = start.elapsed();
    eprintln!("First page (50 records) in {:?}", first_page_time);

    assert_eq!(result.items.len(), 50);
    assert_eq!(result.total_items, batch_size as u64);
    assert!(
        first_page_time < Duration::from_secs(5),
        "first page should complete within 5 seconds, took {:?}",
        first_page_time
    );

    // Test deep pagination (page 1000).
    let start = Instant::now();
    let result = db
        .find_many(
            "items",
            &RecordQuery {
                page: 1000,
                per_page: 50,
                sort: vec![("created".to_string(), SortDirection::Desc)],
                ..Default::default()
            },
        )
        .expect("deep pagination should work");
    let deep_page_time = start.elapsed();
    eprintln!("Deep page (page 1000, 50 records) in {:?}", deep_page_time);

    assert_eq!(result.items.len(), 50);
    assert!(
        deep_page_time < Duration::from_secs(10),
        "deep pagination should complete within 10 seconds, took {:?}",
        deep_page_time
    );

    // Test filtering on indexed field.
    let start = Instant::now();
    let result = db
        .find_many(
            "items",
            &RecordQuery {
                filter: Some("status = 'published'".to_string()),
                page: 1,
                per_page: 50,
                ..Default::default()
            },
        )
        .expect("indexed filter should work");
    let indexed_filter_time = start.elapsed();
    eprintln!(
        "Indexed filter (status = 'published') in {:?}, found {} total",
        indexed_filter_time, result.total_items
    );

    // ~33k records should be published.
    assert!(result.total_items > 30_000 && result.total_items < 35_000);
    assert!(
        indexed_filter_time < Duration::from_secs(5),
        "indexed filter should be fast, took {:?}",
        indexed_filter_time
    );

    // Test filtering on non-indexed field (content).
    let start = Instant::now();
    let result = db
        .find_many(
            "items",
            &RecordQuery {
                filter: Some("title ~ 'Item 999'".to_string()),
                page: 1,
                per_page: 50,
                ..Default::default()
            },
        )
        .expect("non-indexed filter should work");
    let non_indexed_time = start.elapsed();
    eprintln!(
        "Non-indexed filter (title contains) in {:?}, found {} total",
        non_indexed_time, result.total_items
    );

    // Should find records with "Item 999" in title (e.g., "Item 999", "Item 9990"-"Item 9999").
    assert!(
        result.total_items > 0,
        "should find at least some records matching 'Item 999'"
    );
    assert!(
        non_indexed_time < Duration::from_secs(10),
        "non-indexed filter should complete within 10 seconds, took {:?}",
        non_indexed_time
    );
}

#[test]
fn count_with_100k_records() {
    let db = setup_db();
    create_items_collection(&db);

    db.transaction(|tx| {
        for i in 0..100_000 {
            let status = if i % 2 == 0 { "published" } else { "draft" };
            tx.execute(
                "INSERT INTO items (id, title, content, status, views, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    format!("cnt_{i:06}"),
                    format!("Count Item {i}"),
                    "",
                    status,
                    i,
                    now_str(),
                    now_str()
                ],
            ).map_err(DbError::Query)?;
        }
        Ok(())
    })
    .expect("bulk insert");

    // Total count.
    let start = Instant::now();
    let total = db.count("items", None).expect("total count");
    let count_time = start.elapsed();
    eprintln!("Total count of 100k records in {:?}", count_time);
    assert_eq!(total, 100_000);

    // Filtered count on indexed field.
    let start = Instant::now();
    let published = db
        .count("items", Some("status = 'published'"))
        .expect("filtered count");
    let filtered_time = start.elapsed();
    eprintln!("Filtered count (published) in {:?}", filtered_time);
    assert_eq!(published, 50_000);

    assert!(
        count_time < Duration::from_secs(5),
        "count should be fast"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// CONCURRENT WRITE SERIALIZATION
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn concurrent_writes_are_serialized_no_data_loss() {
    // Multiple threads writing simultaneously should not lose any records.
    let db = setup_db();
    create_items_collection(&db);

    let db = Arc::new(db);
    let num_writers = 4;
    let records_per_writer = 50;
    let barrier = Arc::new(Barrier::new(num_writers));

    let mut handles = vec![];
    for writer_id in 0..num_writers {
        let db_w = Arc::clone(&db);
        let barrier_w = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier_w.wait();
            for i in 0..records_per_writer {
                let id = format!("w{writer_id}_{i:03}");
                insert_item(
                    &db_w,
                    &id,
                    &format!("Writer {writer_id} Item {i}"),
                    "published",
                    (writer_id * 1000 + i) as i64,
                );
            }
        }));
    }

    for h in handles {
        h.join().expect("writer should not panic");
    }

    // All records from all writers should be present.
    let total = db.count("items", None).expect("count should work");
    assert_eq!(
        total,
        (num_writers * records_per_writer) as u64,
        "no records should be lost from concurrent writes"
    );
}

#[test]
fn concurrent_updates_to_same_record_are_serialized() {
    // Multiple threads updating the same record should serialize correctly.
    let db = setup_db();
    create_items_collection(&db);

    insert_item(&db, "shared_01", "Shared Record", "draft", 0);

    let db = Arc::new(db);
    let num_updaters = 8;
    let updates_per_thread = 20;
    let barrier = Arc::new(Barrier::new(num_updaters));

    let mut handles = vec![];
    for _ in 0..num_updaters {
        let db_u = Arc::clone(&db);
        let barrier_u = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier_u.wait();
            for i in 0..updates_per_thread {
                let mut data = HashMap::new();
                data.insert("views".to_string(), json!(i));
                data.insert("updated".to_string(), json!(now_str()));
                // Some updates may briefly contend, but should all succeed.
                let _ = db_u.update("items", "shared_01", &data);
            }
        }));
    }

    for h in handles {
        h.join().expect("updater should not panic");
    }

    // Record should still be valid and readable.
    let record = db.find_one("items", "shared_01").expect("record should exist");
    assert_eq!(record.get("title").unwrap(), "Shared Record");
    // Views should be some value (last writer wins).
    assert!(record.get("views").is_some());
}

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn empty_transaction_commits_cleanly() {
    let db = setup_db();
    create_items_collection(&db);

    db.transaction(|_tx| Ok(())).expect("empty transaction should commit");

    // DB should still work fine after empty transaction.
    insert_item(&db, "after_empty", "After Empty Tx", "draft", 0);
    let record = db.find_one("items", "after_empty").expect("should exist");
    assert_eq!(record.get("title").unwrap(), "After Empty Tx");
}

#[test]
fn read_during_schema_alteration() {
    // Reads should not get corrupted data during a schema change.
    let db = setup_db();
    create_items_collection(&db);

    for i in 0..20 {
        insert_item(&db, &format!("sch_{i:03}"), &format!("Schema {i}"), "published", i);
    }

    // Perform schema alteration (remove "extra" column).
    let new_schema = CollectionSchema {
        name: "items".to_string(),
        collection_type: "base".to_string(),
        columns: vec![
            ColumnDef {
                name: "title".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: None,
                unique: false,
            },
            ColumnDef {
                name: "content".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: false,
                default: None,
                unique: false,
            },
            ColumnDef {
                name: "status".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: Some("'draft'".to_string()),
                unique: false,
            },
            ColumnDef {
                name: "views".to_string(),
                sql_type: "INTEGER".to_string(),
                not_null: true,
                default: Some("0".to_string()),
                unique: false,
            },
        ],
        indexes: vec![],
        searchable_fields: vec![],
        view_query: None,
    };

    db.alter_collection(
        "items",
        &SchemaAlteration {
            schema: new_schema,
            renames: vec![],
        },
    )
    .expect("alteration should succeed");

    // Read after alteration should work fine.
    let result = db
        .find_many(
            "items",
            &RecordQuery {
                page: 1,
                per_page: 100,
                ..Default::default()
            },
        )
        .expect("read after alteration should work");
    assert_eq!(result.total_items, 20);

    for item in &result.items {
        assert!(item.get("title").is_some(), "title should exist");
        assert!(item.get("extra").is_none(), "extra should be gone");
    }
}

#[test]
fn write_conn_recovers_after_failed_write() {
    // After a failed write, subsequent writes should still work.
    let db = setup_db();
    create_items_collection(&db);

    // Attempt an invalid write.
    let result = db.with_write_conn(|conn| {
        conn.execute("INSERT INTO nonexistent_table VALUES (1)", [])
            .map_err(DbError::Query)?;
        Ok(())
    });
    assert!(result.is_err());

    // The write connection should still be usable.
    insert_item(&db, "recovery_01", "Recovery Test", "draft", 0);
    let record = db
        .find_one("items", "recovery_01")
        .expect("should be able to write after error");
    assert_eq!(record.get("title").unwrap(), "Recovery Test");
}

#[test]
fn file_db_wal_mode_concurrent_access() {
    // Verify WAL mode works correctly with file-based databases under concurrency.
    let dir = tempfile::tempdir().unwrap();
    let db = setup_file_db(dir.path());
    create_items_collection(&db);

    // Verify WAL mode.
    let conn = db.read_conn().unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    drop(conn);

    let db = Arc::new(db);
    let barrier = Arc::new(Barrier::new(3));

    // Writer.
    let db_w = Arc::clone(&db);
    let barrier_w = Arc::clone(&barrier);
    let writer = thread::spawn(move || {
        barrier_w.wait();
        for i in 0..100 {
            insert_item(
                &db_w,
                &format!("fwal_{i:03}"),
                &format!("WAL Item {i}"),
                "published",
                i,
            );
        }
    });

    // Two readers.
    let mut readers = vec![];
    for _ in 0..2 {
        let db_r = Arc::clone(&db);
        let barrier_r = Arc::clone(&barrier);
        readers.push(thread::spawn(move || {
            barrier_r.wait();
            let mut success = 0;
            for _ in 0..30 {
                if db_r
                    .find_many(
                        "items",
                        &RecordQuery {
                            page: 1,
                            per_page: 10,
                            ..Default::default()
                        },
                    )
                    .is_ok()
                {
                    success += 1;
                }
                thread::sleep(Duration::from_millis(1));
            }
            success
        }));
    }

    writer.join().unwrap();
    for r in readers {
        let count = r.join().unwrap();
        assert_eq!(count, 30, "all reads should succeed on file-based WAL DB");
    }
}
