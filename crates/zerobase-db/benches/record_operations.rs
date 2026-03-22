//! Performance benchmarks for zerobase-db record operations.
//!
//! Benchmarks cover:
//! - Record CRUD operations (create, read, update, delete)
//! - Filtered queries with PocketBase-style expressions
//! - Paginated list queries
//! - Relation expansion (forward and back-relation)
//! - Concurrent read operations
//! - Filter parsing performance
//!
//! Run with: `cargo bench -p zerobase-db`

use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use serde_json::json;

use zerobase_core::services::record_service::{RecordQuery, RecordRepository, SortDirection};
use zerobase_db::{
    CollectionSchema, ColumnDef, Database, IndexDef, PoolConfig, SchemaRepository,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Create an in-memory database with system migrations applied.
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

/// Create a simple "posts" collection with common field types.
fn create_posts_collection(db: &Database) {
    let schema = CollectionSchema {
        name: "posts".to_string(),
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
                name: "author_id".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: false,
                default: None,
                unique: false,
            },
        ],
        indexes: vec![
            IndexDef {
                name: "idx_posts_status".to_string(),
                columns: vec!["status".to_string()],
                index_columns: vec![],
                unique: false,
            },
            IndexDef {
                name: "idx_posts_author_id".to_string(),
                columns: vec!["author_id".to_string()],
                index_columns: vec![],
                unique: false,
            },
            IndexDef {
                name: "idx_posts_views".to_string(),
                columns: vec!["views".to_string()],
                index_columns: vec![],
                unique: false,
            },
        ],
        searchable_fields: vec![],
        view_query: None,
    };
    db.create_collection(&schema)
        .expect("failed to create posts collection");
}

/// Create an "authors" collection for relation benchmarks.
fn create_authors_collection(db: &Database) {
    let schema = CollectionSchema {
        name: "authors".to_string(),
        collection_type: "base".to_string(),
        columns: vec![
            ColumnDef {
                name: "name".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: None,
                unique: false,
            },
            ColumnDef {
                name: "email".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: None,
                unique: true,
            },
        ],
        indexes: vec![],
        searchable_fields: vec![],
        view_query: None,
    };
    db.create_collection(&schema)
        .expect("failed to create authors collection");
}

/// Create a "comments" collection for back-relation benchmarks.
fn create_comments_collection(db: &Database) {
    let schema = CollectionSchema {
        name: "comments".to_string(),
        collection_type: "base".to_string(),
        columns: vec![
            ColumnDef {
                name: "body".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: None,
                unique: false,
            },
            ColumnDef {
                name: "post_id".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: None,
                unique: false,
            },
        ],
        indexes: vec![IndexDef {
            name: "idx_comments_post_id".to_string(),
            columns: vec!["post_id".to_string()],
            index_columns: vec![],
            unique: false,
        }],
        searchable_fields: vec![],
        view_query: None,
    };
    db.create_collection(&schema)
        .expect("failed to create comments collection");
}

fn now_str() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3fZ").to_string()
}

/// Seed authors and return their IDs.
fn seed_authors(db: &Database, count: usize) -> Vec<String> {
    let mut ids = Vec::with_capacity(count);
    for i in 0..count {
        let id = format!("author_{i:05}");
        let mut data = HashMap::new();
        data.insert("id".to_string(), json!(id));
        data.insert("name".to_string(), json!(format!("Author {i}")));
        data.insert("email".to_string(), json!(format!("author{i}@example.com")));
        data.insert("created".to_string(), json!(now_str()));
        data.insert("updated".to_string(), json!(now_str()));
        db.insert("authors", &data).expect("failed to insert author");
        ids.push(id);
    }
    ids
}

/// Seed posts with varying statuses and return their IDs.
fn seed_posts(db: &Database, count: usize, author_ids: &[String]) -> Vec<String> {
    let statuses = ["draft", "published", "archived"];
    let mut ids = Vec::with_capacity(count);
    for i in 0..count {
        let id = format!("post_{i:06}");
        let status = statuses[i % statuses.len()];
        let author_id = &author_ids[i % author_ids.len()];
        let mut data = HashMap::new();
        data.insert("id".to_string(), json!(id));
        data.insert("title".to_string(), json!(format!("Post Title Number {i}")));
        data.insert(
            "content".to_string(),
            json!(format!("This is the content of post {i}. It contains some text for benchmarking purposes.")),
        );
        data.insert("status".to_string(), json!(status));
        data.insert("views".to_string(), json!(i as i64 * 10));
        data.insert("author_id".to_string(), json!(author_id));
        data.insert("created".to_string(), json!(now_str()));
        data.insert("updated".to_string(), json!(now_str()));
        db.insert("posts", &data).expect("failed to insert post");
        ids.push(id);
    }
    ids
}

/// Seed comments for posts.
fn seed_comments(db: &Database, per_post: usize, post_ids: &[String]) {
    for (pi, post_id) in post_ids.iter().enumerate() {
        for ci in 0..per_post {
            let id = format!("comment_{pi:05}_{ci:03}");
            let mut data = HashMap::new();
            data.insert("id".to_string(), json!(id));
            data.insert(
                "body".to_string(),
                json!(format!("Comment {ci} on post {pi}")),
            );
            data.insert("post_id".to_string(), json!(post_id));
            data.insert("created".to_string(), json!(now_str()));
            data.insert("updated".to_string(), json!(now_str()));
            db.insert("comments", &data)
                .expect("failed to insert comment");
        }
    }
}

/// Set up a fully seeded database for benchmarks.
fn setup_seeded_db(post_count: usize, author_count: usize, comments_per_post: usize) -> (Database, Vec<String>, Vec<String>) {
    let db = setup_db();
    create_authors_collection(&db);
    create_posts_collection(&db);
    create_comments_collection(&db);
    let author_ids = seed_authors(&db, author_count);
    let post_ids = seed_posts(&db, post_count, &author_ids);
    seed_comments(&db, comments_per_post, &post_ids[..post_ids.len().min(100)]);
    (db, post_ids, author_ids)
}

// ── Benchmark Groups ─────────────────────────────────────────────────────────

/// Benchmark single-record read by ID.
fn bench_read_by_id(c: &mut Criterion) {
    let (db, post_ids, _) = setup_seeded_db(1000, 10, 0);

    c.bench_function("read_by_id", |b| {
        let mut idx = 0;
        b.iter(|| {
            let id = &post_ids[idx % post_ids.len()];
            idx += 1;
            let record = db.find_one("posts", black_box(id)).unwrap();
            black_box(record);
        });
    });
}

/// Benchmark record insertion.
fn bench_insert(c: &mut Criterion) {
    let db = setup_db();
    create_posts_collection(&db);

    let mut counter = 0u64;

    c.bench_function("insert_record", |b| {
        b.iter(|| {
            counter += 1;
            let id = format!("bench_insert_{counter:010}");
            let mut data = HashMap::new();
            data.insert("id".to_string(), json!(id));
            data.insert(
                "title".to_string(),
                json!(format!("Benchmark Post {counter}")),
            );
            data.insert("content".to_string(), json!("Benchmark content"));
            data.insert("status".to_string(), json!("draft"));
            data.insert("views".to_string(), json!(0));
            data.insert("created".to_string(), json!(now_str()));
            data.insert("updated".to_string(), json!(now_str()));
            db.insert("posts", black_box(&data)).unwrap();
        });
    });
}

/// Benchmark record update.
fn bench_update(c: &mut Criterion) {
    let (db, post_ids, _) = setup_seeded_db(1000, 10, 0);

    c.bench_function("update_record", |b| {
        let mut idx = 0;
        b.iter(|| {
            let id = &post_ids[idx % post_ids.len()];
            idx += 1;
            let mut data = HashMap::new();
            data.insert("title".to_string(), json!(format!("Updated Title {idx}")));
            data.insert("updated".to_string(), json!(now_str()));
            let result = db.update("posts", black_box(id), black_box(&data)).unwrap();
            black_box(result);
        });
    });
}

/// Benchmark record deletion.
fn bench_delete(c: &mut Criterion) {
    let db = setup_db();
    create_posts_collection(&db);
    let mut counter = 0u64;

    c.bench_function("delete_record", |b| {
        b.iter(|| {
            counter += 1;
            let id = format!("del_{counter:010}");
            // Insert a record, then delete it.
            let mut data = HashMap::new();
            data.insert("id".to_string(), json!(id));
            data.insert("title".to_string(), json!("Delete Me"));
            data.insert("status".to_string(), json!("draft"));
            data.insert("views".to_string(), json!(0));
            data.insert("created".to_string(), json!(now_str()));
            data.insert("updated".to_string(), json!(now_str()));
            db.insert("posts", &data).unwrap();
            let result = db.delete("posts", black_box(&id)).unwrap();
            black_box(result);
        });
    });
}

/// Benchmark filtered list queries.
fn bench_filtered_queries(c: &mut Criterion) {
    let (db, _, _) = setup_seeded_db(5000, 20, 0);

    let mut group = c.benchmark_group("filtered_queries");

    // Simple equality filter
    group.bench_function("equality_filter", |b| {
        b.iter(|| {
            let query = RecordQuery {
                filter: Some("status = \"published\"".to_string()),
                page: 1,
                per_page: 20,
                ..Default::default()
            };
            let result = db.find_many("posts", black_box(&query)).unwrap();
            black_box(result);
        });
    });

    // Comparison filter
    group.bench_function("comparison_filter", |b| {
        b.iter(|| {
            let query = RecordQuery {
                filter: Some("views > 5000".to_string()),
                page: 1,
                per_page: 20,
                ..Default::default()
            };
            let result = db.find_many("posts", black_box(&query)).unwrap();
            black_box(result);
        });
    });

    // Combined AND filter
    group.bench_function("combined_and_filter", |b| {
        b.iter(|| {
            let query = RecordQuery {
                filter: Some(
                    "status = \"published\" && views > 1000".to_string(),
                ),
                page: 1,
                per_page: 20,
                ..Default::default()
            };
            let result = db.find_many("posts", black_box(&query)).unwrap();
            black_box(result);
        });
    });

    // Combined OR filter
    group.bench_function("combined_or_filter", |b| {
        b.iter(|| {
            let query = RecordQuery {
                filter: Some(
                    "status = \"draft\" || status = \"archived\"".to_string(),
                ),
                page: 1,
                per_page: 20,
                ..Default::default()
            };
            let result = db.find_many("posts", black_box(&query)).unwrap();
            black_box(result);
        });
    });

    // Contains (LIKE) filter
    group.bench_function("contains_filter", |b| {
        b.iter(|| {
            let query = RecordQuery {
                filter: Some("title ~ \"Number 42\"".to_string()),
                page: 1,
                per_page: 20,
                ..Default::default()
            };
            let result = db.find_many("posts", black_box(&query)).unwrap();
            black_box(result);
        });
    });

    // No filter (full list with pagination)
    group.bench_function("no_filter_paginated", |b| {
        b.iter(|| {
            let query = RecordQuery {
                page: 1,
                per_page: 50,
                ..Default::default()
            };
            let result = db.find_many("posts", black_box(&query)).unwrap();
            black_box(result);
        });
    });

    group.finish();
}

/// Benchmark sorted queries.
fn bench_sorted_queries(c: &mut Criterion) {
    let (db, _, _) = setup_seeded_db(5000, 20, 0);

    let mut group = c.benchmark_group("sorted_queries");

    group.bench_function("sort_by_created_desc", |b| {
        b.iter(|| {
            let query = RecordQuery {
                sort: vec![("created".to_string(), SortDirection::Desc)],
                page: 1,
                per_page: 20,
                ..Default::default()
            };
            let result = db.find_many("posts", black_box(&query)).unwrap();
            black_box(result);
        });
    });

    group.bench_function("sort_by_views_desc", |b| {
        b.iter(|| {
            let query = RecordQuery {
                sort: vec![("views".to_string(), SortDirection::Desc)],
                page: 1,
                per_page: 20,
                ..Default::default()
            };
            let result = db.find_many("posts", black_box(&query)).unwrap();
            black_box(result);
        });
    });

    group.bench_function("multi_sort", |b| {
        b.iter(|| {
            let query = RecordQuery {
                sort: vec![
                    ("status".to_string(), SortDirection::Asc),
                    ("views".to_string(), SortDirection::Desc),
                ],
                page: 1,
                per_page: 20,
                ..Default::default()
            };
            let result = db.find_many("posts", black_box(&query)).unwrap();
            black_box(result);
        });
    });

    group.bench_function("filter_and_sort", |b| {
        b.iter(|| {
            let query = RecordQuery {
                filter: Some("status = \"published\"".to_string()),
                sort: vec![("views".to_string(), SortDirection::Desc)],
                page: 1,
                per_page: 20,
                ..Default::default()
            };
            let result = db.find_many("posts", black_box(&query)).unwrap();
            black_box(result);
        });
    });

    group.finish();
}

/// Benchmark pagination at different offsets.
fn bench_pagination(c: &mut Criterion) {
    let (db, _, _) = setup_seeded_db(5000, 20, 0);

    let mut group = c.benchmark_group("pagination");

    for page in [1, 10, 50, 100] {
        group.bench_with_input(BenchmarkId::new("page", page), &page, |b, &page| {
            b.iter(|| {
                let query = RecordQuery {
                    page,
                    per_page: 20,
                    ..Default::default()
                };
                let result = db.find_many("posts", black_box(&query)).unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark per-page sizes.
fn bench_per_page_sizes(c: &mut Criterion) {
    let (db, _, _) = setup_seeded_db(5000, 20, 0);

    let mut group = c.benchmark_group("per_page_sizes");

    for per_page in [10, 20, 50, 100, 200, 500] {
        group.bench_with_input(
            BenchmarkId::new("per_page", per_page),
            &per_page,
            |b, &per_page| {
                b.iter(|| {
                    let query = RecordQuery {
                        page: 1,
                        per_page,
                        ..Default::default()
                    };
                    let result = db.find_many("posts", black_box(&query)).unwrap();
                    black_box(result);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark the count operation.
fn bench_count(c: &mut Criterion) {
    let (db, _, _) = setup_seeded_db(5000, 20, 0);

    let mut group = c.benchmark_group("count");

    group.bench_function("count_all", |b| {
        b.iter(|| {
            let result = db.count("posts", black_box(None)).unwrap();
            black_box(result);
        });
    });

    group.bench_function("count_filtered", |b| {
        b.iter(|| {
            let result = db
                .count("posts", black_box(Some("status = \"published\"")))
                .unwrap();
            black_box(result);
        });
    });

    group.finish();
}

/// Benchmark forward relation lookups (find_referencing_records).
fn bench_relation_lookups(c: &mut Criterion) {
    let (db, post_ids, _) = setup_seeded_db(500, 10, 5);

    let mut group = c.benchmark_group("relation_lookups");

    // Find comments referencing a specific post
    group.bench_function("find_referencing_records", |b| {
        let mut idx = 0;
        b.iter(|| {
            let post_id = &post_ids[idx % post_ids.len().min(100)];
            idx += 1;
            let result = db
                .find_referencing_records("comments", "post_id", black_box(post_id))
                .unwrap();
            black_box(result);
        });
    });

    // Find referencing records with limit
    group.bench_function("find_referencing_limited", |b| {
        let mut idx = 0;
        b.iter(|| {
            let post_id = &post_ids[idx % post_ids.len().min(100)];
            idx += 1;
            let result = db
                .find_referencing_records_limited("comments", "post_id", black_box(post_id), 10)
                .unwrap();
            black_box(result);
        });
    });

    group.finish();
}

/// Benchmark filter parsing performance.
fn bench_filter_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("filter_parsing");

    let filters = [
        ("simple_eq", "status = \"published\""),
        ("comparison", "views > 1000"),
        ("combined_and", "status = \"published\" && views > 1000"),
        ("combined_or", "status = \"draft\" || status = \"archived\""),
        (
            "complex_nested",
            "(status = \"published\" && views > 100) || (status = \"draft\" && author_id = \"author_00001\")",
        ),
        ("contains", "title ~ \"hello world\""),
        ("null_check", "content != null"),
    ];

    for (name, filter) in &filters {
        group.bench_function(*name, |b| {
            b.iter(|| {
                let result =
                    zerobase_db::filter::parse_and_generate_sql(black_box(filter));
                let _ = black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark concurrent read operations from multiple threads.
fn bench_concurrent_reads(c: &mut Criterion) {
    let (db, post_ids, _) = setup_seeded_db(1000, 10, 0);
    let db = Arc::new(db);
    let post_ids = Arc::new(post_ids);

    let mut group = c.benchmark_group("concurrent_reads");

    for num_threads in [2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::new("threads", num_threads),
            &num_threads,
            |b, &num_threads| {
                b.iter(|| {
                    let handles: Vec<_> = (0..num_threads)
                        .map(|t| {
                            let db = Arc::clone(&db);
                            let ids = Arc::clone(&post_ids);
                            thread::spawn(move || {
                                // Each thread reads 50 records
                                for i in 0..50 {
                                    let idx = (t * 50 + i) % ids.len();
                                    let record = db.find_one("posts", &ids[idx]).unwrap();
                                    black_box(record);
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );
    }

    // Concurrent list queries
    group.bench_function("concurrent_list_4threads", |b| {
        b.iter(|| {
            let handles: Vec<_> = (0..4)
                .map(|t| {
                    let db = Arc::clone(&db);
                    thread::spawn(move || {
                        for page in 1..=5 {
                            let query = RecordQuery {
                                page: page + (t as u32 * 5),
                                per_page: 20,
                                ..Default::default()
                            };
                            let result = db.find_many("posts", &query).unwrap();
                            black_box(result);
                        }
                    })
                })
                .collect();

            for h in handles {
                h.join().unwrap();
            }
        });
    });

    // Mixed read workload: some find_one, some find_many
    group.bench_function("mixed_read_workload_4threads", |b| {
        b.iter(|| {
            let handles: Vec<_> = (0..4)
                .map(|t| {
                    let db = Arc::clone(&db);
                    let ids = Arc::clone(&post_ids);
                    thread::spawn(move || {
                        for i in 0..25 {
                            if i % 3 == 0 {
                                // List query
                                let query = RecordQuery {
                                    page: (t as u32 * 5 + i as u32 / 3) % 50 + 1,
                                    per_page: 20,
                                    filter: Some("status = \"published\"".to_string()),
                                    ..Default::default()
                                };
                                let result = db.find_many("posts", &query).unwrap();
                                black_box(result);
                            } else {
                                // Single read
                                let idx = (t * 25 + i) % ids.len();
                                let record = db.find_one("posts", &ids[idx]).unwrap();
                                black_box(record);
                            }
                        }
                    })
                })
                .collect();

            for h in handles {
                h.join().unwrap();
            }
        });
    });

    group.finish();
}

/// Benchmark scaling behavior: how performance changes with dataset size.
fn bench_dataset_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("dataset_scaling");
    group.sample_size(30);

    for size in [100, 500, 1000, 5000] {
        // Read scaling
        group.bench_with_input(
            BenchmarkId::new("read_by_id", size),
            &size,
            |b, &size| {
                let (db, post_ids, _) = setup_seeded_db(size, 10, 0);
                let mid_id = &post_ids[size / 2];
                b.iter(|| {
                    let record = db.find_one("posts", black_box(mid_id)).unwrap();
                    black_box(record);
                });
            },
        );

        // Filtered query scaling
        group.bench_with_input(
            BenchmarkId::new("filtered_list", size),
            &size,
            |b, &size| {
                let (db, _, _) = setup_seeded_db(size, 10, 0);
                b.iter(|| {
                    let query = RecordQuery {
                        filter: Some("status = \"published\"".to_string()),
                        page: 1,
                        per_page: 20,
                        ..Default::default()
                    };
                    let result = db.find_many("posts", black_box(&query)).unwrap();
                    black_box(result);
                });
            },
        );

        // Count scaling
        group.bench_with_input(
            BenchmarkId::new("count_filtered", size),
            &size,
            |b, &size| {
                let (db, _, _) = setup_seeded_db(size, 10, 0);
                b.iter(|| {
                    let result = db
                        .count("posts", black_box(Some("status = \"published\"")))
                        .unwrap();
                    black_box(result);
                });
            },
        );
    }

    group.finish();
}

// ── Criterion Configuration ──────────────────────────────────────────────────

criterion_group!(
    crud,
    bench_read_by_id,
    bench_insert,
    bench_update,
    bench_delete,
);

criterion_group!(
    queries,
    bench_filtered_queries,
    bench_sorted_queries,
    bench_pagination,
    bench_per_page_sizes,
    bench_count,
);

criterion_group!(
    relations,
    bench_relation_lookups,
);

criterion_group!(
    parsing,
    bench_filter_parsing,
);

criterion_group!(
    concurrency,
    bench_concurrent_reads,
);

criterion_group!(
    scaling,
    bench_dataset_scaling,
);

criterion_main!(crud, queries, relations, parsing, concurrency, scaling);
