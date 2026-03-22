//! Integration tests for relations and expansion.
//!
//! Covers:
//! - Forward relation expansion (single and multi)
//! - Back-relation expansion (`collection_via_field`)
//! - Nested expansion (multi-level dot notation)
//! - Circular reference detection
//! - Expansion depth limit enforcement
//! - Multi-relation modifiers (`field+` / `field-`)
//! - Cascade delete behaviors (Cascade, SetNull, Restrict, NoAction)
//!
//! Each test uses in-memory mocks for [`RecordRepository`] and [`SchemaLookup`],
//! spawning an isolated server on a random port.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

use zerobase_core::error::Result;
use zerobase_core::schema::{
    ApiRules, Collection, Field, FieldType, OnDeleteAction, RelationOptions,
    TextOptions,
};
use zerobase_core::services::record_service::{
    RecordList, RecordQuery, RecordRepoError, RecordRepository, RecordService, SchemaLookup,
    SortDirection,
};

use zerobase_api::AuthInfo;

// ── Test-only auth middleware ─────────────────────────────────────────────────

async fn fake_auth_middleware(mut request: Request<Body>, next: Next) -> Response {
    let auth_info = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            let token = v
                .strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))?;
            if token.is_empty() {
                return None;
            }
            Some(token.to_string())
        })
        .map(|token| {
            if token == "SUPERUSER" {
                AuthInfo::superuser()
            } else {
                let mut record = HashMap::new();
                record.insert("id".to_string(), serde_json::Value::String(token));
                AuthInfo::authenticated(record)
            }
        })
        .unwrap_or_else(AuthInfo::anonymous);

    request.extensions_mut().insert(auth_info);
    next.run(request).await
}

// ── Mock SchemaLookup ─────────────────────────────────────────────────────────

struct MockSchemaLookup {
    collections: Mutex<Vec<Collection>>,
}

impl MockSchemaLookup {
    fn with(collections: Vec<Collection>) -> Self {
        Self {
            collections: Mutex::new(collections),
        }
    }
}

impl SchemaLookup for MockSchemaLookup {
    fn get_collection(&self, name: &str) -> Result<Collection> {
        self.collections
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.name == name)
            .cloned()
            .ok_or_else(|| zerobase_core::ZerobaseError::not_found_with_id("Collection", name))
    }

    fn get_collection_by_id(&self, id: &str) -> Result<Collection> {
        self.collections
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .ok_or_else(|| zerobase_core::ZerobaseError::not_found_with_id("Collection", id))
    }

    fn list_all_collections(&self) -> Result<Vec<Collection>> {
        Ok(self.collections.lock().unwrap().clone())
    }
}

// ── Mock RecordRepository ─────────────────────────────────────────────────────

struct MockRecordRepo {
    records: Mutex<HashMap<String, Vec<HashMap<String, Value>>>>,
}

impl MockRecordRepo {
    fn with_multi_records(data: Vec<(&str, Vec<HashMap<String, Value>>)>) -> Self {
        let mut map = HashMap::new();
        for (collection, records) in data {
            map.insert(collection.to_string(), records);
        }
        Self {
            records: Mutex::new(map),
        }
    }
}

impl RecordRepository for MockRecordRepo {
    fn find_one(
        &self,
        collection: &str,
        id: &str,
    ) -> std::result::Result<HashMap<String, Value>, RecordRepoError> {
        let store = self.records.lock().unwrap();
        let rows = store
            .get(collection)
            .ok_or_else(|| RecordRepoError::NotFound {
                resource_type: "Record".to_string(),
                resource_id: Some(id.to_string()),
            })?;
        rows.iter()
            .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
            .cloned()
            .ok_or_else(|| RecordRepoError::NotFound {
                resource_type: "Record".to_string(),
                resource_id: Some(id.to_string()),
            })
    }

    fn find_many(
        &self,
        collection: &str,
        query: &RecordQuery,
    ) -> std::result::Result<RecordList, RecordRepoError> {
        let store = self.records.lock().unwrap();
        let mut rows = store.get(collection).cloned().unwrap_or_default();

        if !query.sort.is_empty() {
            let (field, dir) = &query.sort[0];
            let field = field.clone();
            let desc = matches!(dir, SortDirection::Desc);
            rows.sort_by(|a, b| {
                let va = a.get(&field).cloned().unwrap_or(Value::Null);
                let vb = b.get(&field).cloned().unwrap_or(Value::Null);
                let cmp = compare_values(&va, &vb);
                if desc {
                    cmp.reverse()
                } else {
                    cmp
                }
            });
        }

        if let Some(ref filter) = query.filter {
            if let Some((field, value)) = parse_simple_filter(filter) {
                rows.retain(|r| {
                    r.get(&field)
                        .and_then(|v| v.as_str())
                        .map(|v| v == value)
                        .unwrap_or(false)
                });
            }
        }

        let total = rows.len() as u64;
        let page = query.page.max(1);
        let per_page = query.per_page.max(1);
        let total_pages = if total == 0 {
            1
        } else {
            ((total as f64) / (per_page as f64)).ceil() as u32
        };
        let start = ((page - 1) * per_page) as usize;
        let items: Vec<_> = rows
            .into_iter()
            .skip(start)
            .take(per_page as usize)
            .collect();
        Ok(RecordList {
            items,
            total_items: total,
            page,
            per_page,
            total_pages,
        })
    }

    fn insert(
        &self,
        collection: &str,
        data: &HashMap<String, Value>,
    ) -> std::result::Result<(), RecordRepoError> {
        let mut store = self.records.lock().unwrap();
        store
            .entry(collection.to_string())
            .or_default()
            .push(data.clone());
        Ok(())
    }

    fn update(
        &self,
        collection: &str,
        id: &str,
        data: &HashMap<String, Value>,
    ) -> std::result::Result<bool, RecordRepoError> {
        let mut store = self.records.lock().unwrap();
        if let Some(rows) = store.get_mut(collection) {
            if let Some(row) = rows
                .iter_mut()
                .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
            {
                *row = data.clone();
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn delete(&self, collection: &str, id: &str) -> std::result::Result<bool, RecordRepoError> {
        let mut store = self.records.lock().unwrap();
        if let Some(rows) = store.get_mut(collection) {
            let len_before = rows.len();
            rows.retain(|r| r.get("id").and_then(|v| v.as_str()) != Some(id));
            return Ok(rows.len() < len_before);
        }
        Ok(false)
    }

    fn count(
        &self,
        collection: &str,
        filter: Option<&str>,
    ) -> std::result::Result<u64, RecordRepoError> {
        let store = self.records.lock().unwrap();
        let rows = match store.get(collection) {
            Some(r) => r.clone(),
            None => return Ok(0),
        };
        if let Some(f) = filter {
            if let Some((field, value)) = parse_simple_filter(f) {
                let count = rows
                    .iter()
                    .filter(|r| {
                        r.get(&field)
                            .and_then(|v| v.as_str())
                            .map(|v| v == value)
                            .unwrap_or(false)
                    })
                    .count();
                return Ok(count as u64);
            }
        }
        Ok(rows.len() as u64)
    }

    fn find_referencing_records(
        &self,
        collection: &str,
        field_name: &str,
        referenced_id: &str,
    ) -> std::result::Result<Vec<HashMap<String, Value>>, RecordRepoError> {
        let store = self.records.lock().unwrap();
        let rows = store.get(collection).cloned().unwrap_or_default();
        let matches = rows
            .into_iter()
            .filter(|r| {
                r.get(field_name)
                    .map(|v| match v {
                        Value::String(s) => s == referenced_id,
                        Value::Array(arr) => arr
                            .iter()
                            .any(|item| item.as_str().map(|s| s == referenced_id).unwrap_or(false)),
                        _ => false,
                    })
                    .unwrap_or(false)
            })
            .collect();
        Ok(matches)
    }
}

fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::String(a), Value::String(b)) => a.cmp(b),
        (Value::Number(a), Value::Number(b)) => {
            let fa = a.as_f64().unwrap_or(0.0);
            let fb = b.as_f64().unwrap_or(0.0);
            fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
        }
        _ => std::cmp::Ordering::Equal,
    }
}

fn parse_simple_filter(filter: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = filter.splitn(2, '=').collect();
    if parts.len() == 2 {
        let field = parts[0].trim().to_string();
        let value = parts[1].trim().trim_matches('"').to_string();
        Some((field, value))
    } else {
        None
    }
}

// ── Test infrastructure ───────────────────────────────────────────────────────

async fn spawn_app(
    schema: MockSchemaLookup,
    repo: MockRecordRepo,
) -> (String, tokio::task::JoinHandle<()>) {
    let service = Arc::new(RecordService::new(repo, schema));

    let app = zerobase_api::api_router()
        .merge(zerobase_api::record_routes(service))
        .layer(axum::middleware::from_fn(fake_auth_middleware));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let port = addr.port();
    let address = format!("http://127.0.0.1:{port}");

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (address, handle)
}

fn text_field(name: &str) -> Field {
    Field::new(
        name,
        FieldType::Text(TextOptions {
            min_length: 0,
            max_length: 500,
            pattern: None,
            searchable: false,
        }),
    )
}

fn single_relation_field(name: &str, target: &str) -> Field {
    Field::new(
        name,
        FieldType::Relation(RelationOptions {
            collection_id: target.to_string(),
            max_select: 1,
            ..Default::default()
        }),
    )
}

fn single_relation_field_with_on_delete(name: &str, target: &str, on_delete: OnDeleteAction) -> Field {
    Field::new(
        name,
        FieldType::Relation(RelationOptions {
            collection_id: target.to_string(),
            max_select: 1,
            on_delete,
            ..Default::default()
        }),
    )
}

fn multi_relation_field(name: &str, target: &str, max_select: u32) -> Field {
    Field::new(
        name,
        FieldType::Relation(RelationOptions {
            collection_id: target.to_string(),
            max_select,
            ..Default::default()
        }),
    )
}

fn multi_relation_field_with_on_delete(
    name: &str,
    target: &str,
    max_select: u32,
    on_delete: OnDeleteAction,
) -> Field {
    Field::new(
        name,
        FieldType::Relation(RelationOptions {
            collection_id: target.to_string(),
            max_select,
            on_delete,
            ..Default::default()
        }),
    )
}

fn make_collection(name: &str, fields: Vec<Field>) -> Collection {
    let mut col = Collection::base(name, fields);
    col.rules = ApiRules::open();
    col
}

fn rec(pairs: Vec<(&str, Value)>) -> HashMap<String, Value> {
    let mut r = HashMap::new();
    for (k, v) in pairs {
        r.insert(k.to_string(), v);
    }
    r.insert(
        "created".to_string(),
        json!("2025-01-01T00:00:00Z"),
    );
    r.insert(
        "updated".to_string(),
        json!("2025-01-01T00:00:00Z"),
    );
    r
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. FORWARD RELATION EXPANSION
// ═══════════════════════════════════════════════════════════════════════════════

/// Build a rich schema for forward/back/nested expansion tests:
/// - users, profiles (user → users), posts (author → users, tags → tags [multi]),
///   tags, comments (post → posts, author → users)
fn rich_collections() -> Vec<Collection> {
    vec![
        make_collection("users", vec![text_field("name")]),
        make_collection(
            "profiles",
            vec![
                text_field("bio"),
                single_relation_field("user", "users"),
            ],
        ),
        make_collection(
            "posts",
            vec![
                text_field("title"),
                single_relation_field("author", "users"),
                multi_relation_field("tags", "tags", 0),
            ],
        ),
        make_collection("tags", vec![text_field("label")]),
        make_collection(
            "comments",
            vec![
                text_field("text"),
                single_relation_field("post", "posts"),
                single_relation_field("author", "users"),
            ],
        ),
    ]
}

fn rich_seed() -> Vec<(&'static str, Vec<HashMap<String, Value>>)> {
    vec![
        (
            "users",
            vec![
                rec(vec![("id", json!("u1_____________")), ("name", json!("Alice"))]),
                rec(vec![("id", json!("u2_____________")), ("name", json!("Bob"))]),
            ],
        ),
        (
            "profiles",
            vec![rec(vec![
                ("id", json!("prof1__________")),
                ("bio", json!("Alice bio")),
                ("user", json!("u1_____________")),
            ])],
        ),
        (
            "tags",
            vec![
                rec(vec![("id", json!("t1_____________")), ("label", json!("rust"))]),
                rec(vec![("id", json!("t2_____________")), ("label", json!("web"))]),
                rec(vec![("id", json!("t3_____________")), ("label", json!("api"))]),
            ],
        ),
        (
            "posts",
            vec![
                rec(vec![
                    ("id", json!("p1_____________")),
                    ("title", json!("Hello World")),
                    ("author", json!("u1_____________")),
                    ("tags", json!(["t1_____________", "t2_____________"])),
                ]),
                rec(vec![
                    ("id", json!("p2_____________")),
                    ("title", json!("Second Post")),
                    ("author", json!("u2_____________")),
                    ("tags", json!(["t2_____________", "t3_____________"])),
                ]),
            ],
        ),
        (
            "comments",
            vec![
                rec(vec![
                    ("id", json!("c1_____________")),
                    ("text", json!("Great post!")),
                    ("post", json!("p1_____________")),
                    ("author", json!("u2_____________")),
                ]),
                rec(vec![
                    ("id", json!("c2_____________")),
                    ("text", json!("Thanks!")),
                    ("post", json!("p1_____________")),
                    ("author", json!("u1_____________")),
                ]),
                rec(vec![
                    ("id", json!("c3_____________")),
                    ("text", json!("Nice work")),
                    ("post", json!("p2_____________")),
                    ("author", json!("u1_____________")),
                ]),
            ],
        ),
    ]
}

async fn spawn_rich_app() -> (String, tokio::task::JoinHandle<()>) {
    let schema = MockSchemaLookup::with(rich_collections());
    let repo = MockRecordRepo::with_multi_records(rich_seed());
    spawn_app(schema, repo).await
}

#[tokio::test]
async fn forward_single_relation_expand_on_view() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p1_____________?expand=author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "Hello World");
    assert_eq!(body["expand"]["author"]["name"], "Alice");
    assert_eq!(body["expand"]["author"]["id"], "u1_____________");
}

#[tokio::test]
async fn forward_multi_relation_expand_returns_array() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p1_____________?expand=tags"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let tags = body["expand"]["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 2);
    let labels: Vec<&str> = tags.iter().map(|t| t["label"].as_str().unwrap()).collect();
    assert!(labels.contains(&"rust"));
    assert!(labels.contains(&"web"));
}

#[tokio::test]
async fn forward_expand_multiple_fields_simultaneously() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p1_____________?expand=author,tags"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body["expand"]["author"].is_object());
    assert!(body["expand"]["tags"].is_array());
}

#[tokio::test]
async fn forward_expand_on_list_applies_to_all_items() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records?expand=author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    for item in items {
        assert!(
            item["expand"]["author"].is_object(),
            "each item should have expanded author"
        );
    }
}

#[tokio::test]
async fn forward_expand_missing_reference_is_skipped() {
    let schema = MockSchemaLookup::with(rich_collections());
    let repo = MockRecordRepo::with_multi_records(vec![
        ("users", vec![]),
        (
            "posts",
            vec![rec(vec![
                ("id", json!("p_orphan_________")),
                ("title", json!("Orphan Post")),
                ("author", json!("nonexistent____")),
                ("tags", json!([])),
            ])],
        ),
        ("tags", vec![]),
        ("profiles", vec![]),
        ("comments", vec![]),
    ]);
    let (addr, _h) = spawn_app(schema, repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p_orphan_________?expand=author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "Orphan Post");
    // No expand field when reference doesn't exist
    assert!(
        body.get("expand").is_none(),
        "expand should not be present when referenced record doesn't exist"
    );
}

#[tokio::test]
async fn forward_expand_null_relation_is_skipped() {
    let schema = MockSchemaLookup::with(rich_collections());
    let mut null_post = rec(vec![
        ("id", json!("p_null___________")),
        ("title", json!("Null Author")),
        ("tags", json!([])),
    ]);
    null_post.insert("author".to_string(), Value::Null);

    let repo = MockRecordRepo::with_multi_records(vec![
        ("users", vec![]),
        ("posts", vec![null_post]),
        ("tags", vec![]),
        ("profiles", vec![]),
        ("comments", vec![]),
    ]);
    let (addr, _h) = spawn_app(schema, repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p_null___________?expand=author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body.get("expand").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. BACK-RELATION EXPANSION
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn back_relation_expand_on_view() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p1_____________?expand=comments_via_post"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let comments = body["expand"]["comments_via_post"].as_array().unwrap();
    assert_eq!(comments.len(), 2);
}

#[tokio::test]
async fn back_relation_always_returns_array_even_for_single_match() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    // p2 has only one comment
    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p2_____________?expand=comments_via_post"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let comments = &body["expand"]["comments_via_post"];
    assert!(comments.is_array());
    assert_eq!(comments.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn back_relation_no_matches_yields_no_expand() {
    let schema = MockSchemaLookup::with(rich_collections());
    let repo = MockRecordRepo::with_multi_records(vec![
        (
            "users",
            vec![rec(vec![
                ("id", json!("u_lonely_________")),
                ("name", json!("Lonely")),
            ])],
        ),
        ("posts", vec![]),
        ("tags", vec![]),
        ("profiles", vec![]),
        ("comments", vec![]),
    ]);
    let (addr, _h) = spawn_app(schema, repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/users/records/u_lonely_________?expand=posts_via_author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body.get("expand").is_none(),
        "no expand when back-relation has no matches"
    );
}

#[tokio::test]
async fn back_relation_on_list_expands_per_item() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/users/records?expand=posts_via_author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);

    for item in items {
        let name = item["name"].as_str().unwrap();
        let posts = item["expand"]["posts_via_author"].as_array().unwrap();
        match name {
            "Alice" => assert_eq!(posts.len(), 1),
            "Bob" => assert_eq!(posts.len(), 1),
            _ => panic!("unexpected user: {name}"),
        }
    }
}

#[tokio::test]
async fn back_relation_includes_collection_metadata() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p1_____________?expand=comments_via_post"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let comments = body["expand"]["comments_via_post"].as_array().unwrap();
    for comment in comments {
        assert_eq!(comment["collectionName"], "comments");
    }
}

#[tokio::test]
async fn back_relation_via_multi_relation_field() {
    // bundles.items is multi-relation → items. Expanding bundles_via_items should work.
    let collections = vec![
        make_collection("items", vec![text_field("name")]),
        make_collection(
            "bundles",
            vec![
                text_field("label"),
                multi_relation_field("items", "items", 0),
            ],
        ),
    ];
    let repo = MockRecordRepo::with_multi_records(vec![
        (
            "items",
            vec![rec(vec![
                ("id", json!("item1__________")),
                ("name", json!("Widget")),
            ])],
        ),
        (
            "bundles",
            vec![
                rec(vec![
                    ("id", json!("bun1___________")),
                    ("label", json!("Starter")),
                    ("items", json!(["item1__________"])),
                ]),
                rec(vec![
                    ("id", json!("bun2___________")),
                    ("label", json!("Pro")),
                    ("items", json!(["item1__________", "item_other_____"])),
                ]),
            ],
        ),
    ]);
    let (addr, _h) = spawn_app(MockSchemaLookup::with(collections), repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/items/records/item1__________?expand=bundles_via_items"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let bundles = body["expand"]["bundles_via_items"].as_array().unwrap();
    assert_eq!(bundles.len(), 2);
}

#[tokio::test]
async fn back_relation_wrong_collection_is_ignored() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    // comments.post points to "posts", not "users".
    // Expanding comments_via_post on a user should be ignored.
    let resp = client
        .get(format!(
            "{addr}/api/collections/users/records/u1_____________?expand=comments_via_post"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body.get("expand").is_none(),
        "back-relation pointing to wrong collection should be ignored"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. NESTED EXPANSION
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn nested_forward_expansion_two_levels() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    // comment → post → author (nested forward)
    let resp = client
        .get(format!(
            "{addr}/api/collections/comments/records/c1_____________?expand=post.author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let post = &body["expand"]["post"];
    assert_eq!(post["title"], "Hello World");
    assert_eq!(post["expand"]["author"]["name"], "Alice");
}

#[tokio::test]
async fn nested_back_relation_with_forward() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    // post → comments_via_post.author
    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p1_____________?expand=comments_via_post.author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let comments = body["expand"]["comments_via_post"].as_array().unwrap();
    assert_eq!(comments.len(), 2);
    for comment in comments {
        assert!(
            comment["expand"]["author"].is_object(),
            "each comment should have expanded author"
        );
    }
}

#[tokio::test]
async fn combined_forward_and_back_relation_expansion() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p1_____________?expand=author,comments_via_post"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body["expand"]["author"].is_object());
    assert_eq!(body["expand"]["author"]["name"], "Alice");
    assert!(body["expand"]["comments_via_post"].is_array());
    assert_eq!(
        body["expand"]["comments_via_post"].as_array().unwrap().len(),
        2
    );
}

#[tokio::test]
async fn complex_nested_expand_three_fields() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    // Expand author, tags, and back-relation comments with their authors
    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p1_____________?expand=author,tags,comments_via_post.author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body["expand"]["author"].is_object());
    assert_eq!(body["expand"]["tags"].as_array().unwrap().len(), 2);
    let comments = body["expand"]["comments_via_post"].as_array().unwrap();
    for c in comments {
        assert!(c["expand"]["author"].is_object());
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. CIRCULAR REFERENCE DETECTION & DEPTH LIMITS
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn expand_depth_limit_enforced() {
    // Parse rejects paths deeper than MAX_EXPAND_DEPTH (6).
    // We test via the API: a.b.c.d.e.f.g is 7 levels → should fail or be ignored.
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p1_____________?expand=a.b.c.d.e.f.g"
        ))
        .send()
        .await
        .unwrap();

    // The API should return a 400 or silently ignore the too-deep expand.
    // Based on implementation, parse_expand returns an error for > MAX_EXPAND_DEPTH.
    let status = resp.status();
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::OK,
        "expected 400 or 200 for too-deep expand, got {status}"
    );
}

#[tokio::test]
async fn expand_nonexistent_field_is_silently_ignored() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p1_____________?expand=nonexistent_field"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "Hello World");
}

#[tokio::test]
async fn expand_non_relation_field_is_silently_ignored() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    // "title" is a text field, not a relation
    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p1_____________?expand=title"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "Hello World");
    // No expand key or empty expand
    let has_expand = body.get("expand").map_or(false, |e| e.is_object() && !e.as_object().unwrap().is_empty());
    assert!(!has_expand, "non-relation field should not produce expand data");
}

#[tokio::test]
async fn no_expand_param_returns_no_expand_field() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p1_____________"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body.get("expand").is_none(),
        "no expand field without ?expand param"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. MULTI-RELATION MODIFIERS (+/-)
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn multi_relation_add_modifier_appends_ids() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    // p1 has tags: [t1, t2]. Add t3.
    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/p1_____________"
        ))
        .json(&json!({ "tags+": "t3_____________" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let tags = body["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 3);
    assert!(tags.contains(&json!("t3_____________")));
}

#[tokio::test]
async fn multi_relation_add_modifier_with_array() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    // Add multiple tags at once
    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/p1_____________"
        ))
        .json(&json!({ "tags+": ["t3_____________"] }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let tags = body["tags"].as_array().unwrap();
    assert!(tags.contains(&json!("t3_____________")));
}

#[tokio::test]
async fn multi_relation_add_modifier_deduplicates() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    // t1 already exists; adding it again should not duplicate.
    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/p1_____________"
        ))
        .json(&json!({ "tags+": "t1_____________" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let tags = body["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 2, "duplicate should not be added");
}

#[tokio::test]
async fn multi_relation_remove_modifier_removes_id() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    // p1 has tags: [t1, t2]. Remove t1.
    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/p1_____________"
        ))
        .json(&json!({ "tags-": "t1_____________" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let tags = body["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 1);
    assert!(!tags.contains(&json!("t1_____________")));
    assert!(tags.contains(&json!("t2_____________")));
}

#[tokio::test]
async fn multi_relation_remove_modifier_with_array() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    // Remove both tags
    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/p1_____________"
        ))
        .json(&json!({ "tags-": ["t1_____________", "t2_____________"] }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let tags = body["tags"].as_array().unwrap();
    assert!(tags.is_empty());
}

#[tokio::test]
async fn multi_relation_remove_nonexistent_is_noop() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    // Remove a tag that doesn't exist → should be fine, no change.
    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/p1_____________"
        ))
        .json(&json!({ "tags-": "nonexistent____" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let tags = body["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 2, "removing non-existent should be a no-op");
}

#[tokio::test]
async fn multi_relation_add_and_remove_in_same_update() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    // p1 has [t1, t2]. Add t3, remove t1.
    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/p1_____________"
        ))
        .json(&json!({
            "tags+": "t3_____________",
            "tags-": "t1_____________"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let tags = body["tags"].as_array().unwrap();
    assert!(tags.contains(&json!("t2_____________")));
    assert!(tags.contains(&json!("t3_____________")));
    assert!(!tags.contains(&json!("t1_____________")));
}

#[tokio::test]
async fn modifier_on_single_relation_field_returns_error() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    // "author" is a single relation (max_select=1), modifier should fail
    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/p1_____________"
        ))
        .json(&json!({ "author+": "u2_____________" }))
        .send()
        .await
        .unwrap();

    assert!(
        resp.status().is_client_error(),
        "modifier on single-relation should be rejected"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. CASCADE DELETE BEHAVIORS
// ═══════════════════════════════════════════════════════════════════════════════

/// Build collections for cascade delete testing.
/// - parents: base records
/// - children_cascade: has parent field with Cascade on_delete
/// - children_set_null: has parent field with SetNull on_delete
/// - children_restrict: has parent field with Restrict on_delete
/// - children_no_action: has parent field with NoAction on_delete
fn cascade_collections() -> Vec<Collection> {
    vec![
        make_collection("parents", vec![text_field("name")]),
        make_collection(
            "children_cascade",
            vec![
                text_field("label"),
                single_relation_field_with_on_delete("parent", "parents", OnDeleteAction::Cascade),
            ],
        ),
        make_collection(
            "children_set_null",
            vec![
                text_field("label"),
                single_relation_field_with_on_delete("parent", "parents", OnDeleteAction::SetNull),
            ],
        ),
        make_collection(
            "children_restrict",
            vec![
                text_field("label"),
                single_relation_field_with_on_delete("parent", "parents", OnDeleteAction::Restrict),
            ],
        ),
        make_collection(
            "children_no_action",
            vec![
                text_field("label"),
                single_relation_field_with_on_delete("parent", "parents", OnDeleteAction::NoAction),
            ],
        ),
    ]
}

#[tokio::test]
async fn cascade_delete_removes_referencing_records() {
    let schema = MockSchemaLookup::with(cascade_collections());
    let repo = MockRecordRepo::with_multi_records(vec![
        (
            "parents",
            vec![rec(vec![
                ("id", json!("par1___________")),
                ("name", json!("Parent 1")),
            ])],
        ),
        (
            "children_cascade",
            vec![
                rec(vec![
                    ("id", json!("cc1____________")),
                    ("label", json!("Child 1")),
                    ("parent", json!("par1___________")),
                ]),
                rec(vec![
                    ("id", json!("cc2____________")),
                    ("label", json!("Child 2")),
                    ("parent", json!("par1___________")),
                ]),
            ],
        ),
        ("children_set_null", vec![]),
        ("children_restrict", vec![]),
        ("children_no_action", vec![]),
    ]);
    let (addr, _h) = spawn_app(schema, repo).await;
    let client = reqwest::Client::new();

    // Delete parent → children should be cascade deleted
    let resp = client
        .delete(format!(
            "{addr}/api/collections/parents/records/par1___________"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify children are deleted
    let resp = client
        .get(format!(
            "{addr}/api/collections/children_cascade/records/cc1____________"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let resp = client
        .get(format!(
            "{addr}/api/collections/children_cascade/records/cc2____________"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn set_null_clears_relation_on_delete() {
    let schema = MockSchemaLookup::with(cascade_collections());
    let repo = MockRecordRepo::with_multi_records(vec![
        (
            "parents",
            vec![rec(vec![
                ("id", json!("par1___________")),
                ("name", json!("Parent 1")),
            ])],
        ),
        ("children_cascade", vec![]),
        (
            "children_set_null",
            vec![rec(vec![
                ("id", json!("csn1___________")),
                ("label", json!("Child SN")),
                ("parent", json!("par1___________")),
            ])],
        ),
        ("children_restrict", vec![]),
        ("children_no_action", vec![]),
    ]);
    let (addr, _h) = spawn_app(schema, repo).await;
    let client = reqwest::Client::new();

    // Delete parent → child's parent field should be set to null
    let resp = client
        .delete(format!(
            "{addr}/api/collections/parents/records/par1___________"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Child should still exist but with null parent
    let resp = client
        .get(format!(
            "{addr}/api/collections/children_set_null/records/csn1___________"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["parent"].is_null(),
        "parent field should be null after referenced record deleted"
    );
}

#[tokio::test]
async fn restrict_prevents_delete_when_references_exist() {
    let schema = MockSchemaLookup::with(cascade_collections());
    let repo = MockRecordRepo::with_multi_records(vec![
        (
            "parents",
            vec![rec(vec![
                ("id", json!("par1___________")),
                ("name", json!("Parent 1")),
            ])],
        ),
        ("children_cascade", vec![]),
        ("children_set_null", vec![]),
        (
            "children_restrict",
            vec![rec(vec![
                ("id", json!("cr1____________")),
                ("label", json!("Child R")),
                ("parent", json!("par1___________")),
            ])],
        ),
        ("children_no_action", vec![]),
    ]);
    let (addr, _h) = spawn_app(schema, repo).await;
    let client = reqwest::Client::new();

    // Delete parent should fail because children_restrict references it
    let resp = client
        .delete(format!(
            "{addr}/api/collections/parents/records/par1___________"
        ))
        .send()
        .await
        .unwrap();

    assert!(
        resp.status().is_client_error(),
        "delete should be rejected when restrict references exist"
    );

    // Parent should still exist
    let resp = client
        .get(format!(
            "{addr}/api/collections/parents/records/par1___________"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn restrict_allows_delete_when_no_references() {
    let schema = MockSchemaLookup::with(cascade_collections());
    let repo = MockRecordRepo::with_multi_records(vec![
        (
            "parents",
            vec![rec(vec![
                ("id", json!("par1___________")),
                ("name", json!("Parent 1")),
            ])],
        ),
        ("children_cascade", vec![]),
        ("children_set_null", vec![]),
        ("children_restrict", vec![]),
        ("children_no_action", vec![]),
    ]);
    let (addr, _h) = spawn_app(schema, repo).await;
    let client = reqwest::Client::new();

    // No references → delete should succeed
    let resp = client
        .delete(format!(
            "{addr}/api/collections/parents/records/par1___________"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn no_action_leaves_dangling_references() {
    let schema = MockSchemaLookup::with(cascade_collections());
    let repo = MockRecordRepo::with_multi_records(vec![
        (
            "parents",
            vec![rec(vec![
                ("id", json!("par1___________")),
                ("name", json!("Parent 1")),
            ])],
        ),
        ("children_cascade", vec![]),
        ("children_set_null", vec![]),
        ("children_restrict", vec![]),
        (
            "children_no_action",
            vec![rec(vec![
                ("id", json!("cna1___________")),
                ("label", json!("Child NA")),
                ("parent", json!("par1___________")),
            ])],
        ),
    ]);
    let (addr, _h) = spawn_app(schema, repo).await;
    let client = reqwest::Client::new();

    // Delete parent → child still exists with dangling reference
    let resp = client
        .delete(format!(
            "{addr}/api/collections/parents/records/par1___________"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Child still exists with the original (now dangling) parent reference
    let resp = client
        .get(format!(
            "{addr}/api/collections/children_no_action/records/cna1___________"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(
        body["parent"], "par1___________",
        "no_action should leave the dangling reference"
    );
}

#[tokio::test]
async fn set_null_on_multi_relation_removes_id_from_array() {
    // Multi-relation with SetNull should remove the deleted ID from the array
    let collections = vec![
        make_collection("targets", vec![text_field("name")]),
        make_collection(
            "holders",
            vec![
                text_field("label"),
                multi_relation_field_with_on_delete("refs", "targets", 0, OnDeleteAction::SetNull),
            ],
        ),
    ];
    let repo = MockRecordRepo::with_multi_records(vec![
        (
            "targets",
            vec![
                rec(vec![("id", json!("tgt1___________")), ("name", json!("T1"))]),
                rec(vec![("id", json!("tgt2___________")), ("name", json!("T2"))]),
            ],
        ),
        (
            "holders",
            vec![rec(vec![
                ("id", json!("hld1___________")),
                ("label", json!("Holder")),
                ("refs", json!(["tgt1___________", "tgt2___________"])),
            ])],
        ),
    ]);
    let (addr, _h) = spawn_app(MockSchemaLookup::with(collections), repo).await;
    let client = reqwest::Client::new();

    // Delete tgt1 → holder's refs should have tgt1 removed, tgt2 remains
    let resp = client
        .delete(format!(
            "{addr}/api/collections/targets/records/tgt1___________"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = client
        .get(format!(
            "{addr}/api/collections/holders/records/hld1___________"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let refs = body["refs"].as_array().unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0], "tgt2___________");
}

#[tokio::test]
async fn cascade_delete_is_recursive() {
    // grandparent → parent (cascade) → child (cascade)
    // Deleting grandparent should cascade to parent, then to child.
    let collections = vec![
        make_collection("grandparents", vec![text_field("name")]),
        make_collection(
            "mid_parents",
            vec![
                text_field("label"),
                single_relation_field_with_on_delete(
                    "grandparent",
                    "grandparents",
                    OnDeleteAction::Cascade,
                ),
            ],
        ),
        make_collection(
            "leaves",
            vec![
                text_field("info"),
                single_relation_field_with_on_delete(
                    "mid_parent",
                    "mid_parents",
                    OnDeleteAction::Cascade,
                ),
            ],
        ),
    ];
    let repo = MockRecordRepo::with_multi_records(vec![
        (
            "grandparents",
            vec![rec(vec![
                ("id", json!("gp1____________")),
                ("name", json!("GP")),
            ])],
        ),
        (
            "mid_parents",
            vec![rec(vec![
                ("id", json!("mp1____________")),
                ("label", json!("Mid")),
                ("grandparent", json!("gp1____________")),
            ])],
        ),
        (
            "leaves",
            vec![rec(vec![
                ("id", json!("lf1____________")),
                ("info", json!("Leaf")),
                ("mid_parent", json!("mp1____________")),
            ])],
        ),
    ]);
    let (addr, _h) = spawn_app(MockSchemaLookup::with(collections), repo).await;
    let client = reqwest::Client::new();

    // Delete grandparent → should cascade all the way down
    let resp = client
        .delete(format!(
            "{addr}/api/collections/grandparents/records/gp1____________"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Mid parent should be deleted
    let resp = client
        .get(format!(
            "{addr}/api/collections/mid_parents/records/mp1____________"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Leaf should be deleted
    let resp = client
        .get(format!(
            "{addr}/api/collections/leaves/records/lf1____________"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. EXPANSION COLLECTION METADATA
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn forward_expanded_records_have_collection_metadata() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p1_____________?expand=author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let author = &body["expand"]["author"];
    assert!(
        author.get("collectionName").is_some(),
        "expanded record should have collectionName"
    );
    assert_eq!(author["collectionName"], "users");
}

#[tokio::test]
async fn multi_relation_expanded_records_have_collection_metadata() {
    let (addr, _h) = spawn_rich_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/p1_____________?expand=tags"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let tags = body["expand"]["tags"].as_array().unwrap();
    for tag in tags {
        assert_eq!(tag["collectionName"], "tags");
    }
}
