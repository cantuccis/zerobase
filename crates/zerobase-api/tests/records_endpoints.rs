//! Integration tests for the record CRUD REST API.
//!
//! Tests exercise the full HTTP stack (router → handler → service → mock repo)
//! using in-memory mocks for both [`RecordRepository`] and [`SchemaLookup`].
//! Each test spawns an isolated server on a random port.

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
use zerobase_core::schema::{ApiRules, Collection, Field, FieldType, NumberOptions, TextOptions};
use zerobase_core::services::record_service::{
    RecordList, RecordQuery, RecordRepoError, RecordRepository, RecordService, SchemaLookup,
    SortDirection,
};

use zerobase_api::AuthInfo;

/// Test-only middleware that simulates auth by interpreting the Authorization header.
///
/// - `Bearer SUPERUSER` → `AuthInfo::superuser()`
/// - `Bearer <token>` → `AuthInfo::authenticated` with `id` = `<token>`
/// - No header / empty → `AuthInfo::anonymous()`
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

// ── In-memory mock: SchemaLookup ────────────────────────────────────────────

struct MockSchemaLookup {
    collections: Mutex<Vec<Collection>>,
}

impl MockSchemaLookup {
    fn new() -> Self {
        Self {
            collections: Mutex::new(Vec::new()),
        }
    }

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
}

// ── In-memory mock: RecordRepository ────────────────────────────────────────

struct MockRecordRepo {
    records: Mutex<HashMap<String, Vec<HashMap<String, Value>>>>,
}

impl MockRecordRepo {
    fn new() -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
        }
    }

    /// Seed records into a collection.
    fn with_records(collection: &str, records: Vec<HashMap<String, Value>>) -> Self {
        let mut map = HashMap::new();
        map.insert(collection.to_string(), records);
        Self {
            records: Mutex::new(map),
        }
    }

    /// Seed records into multiple collections.
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

        // Basic sort support for testing.
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

        // Basic filter support: `field = "value"` only.
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

        // Basic filter support: `field = "value"` only (mirrors find_many mock).
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

/// Very simple value comparison for test sorting.
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

/// Parse a trivial `field = "value"` filter expression for testing.
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

// ── Test infrastructure ─────────────────────────────────────────────────────

fn posts_fields() -> Vec<Field> {
    vec![
        Field::new(
            "title",
            FieldType::Text(TextOptions {
                min_length: 0,
                max_length: 200,
                pattern: None,
                searchable: false,
            }),
        ),
        Field::new(
            "status",
            FieldType::Text(TextOptions {
                min_length: 0,
                max_length: 50,
                pattern: None,
                searchable: false,
            }),
        ),
        Field::new(
            "views",
            FieldType::Number(NumberOptions {
                min: None,
                max: None,
                only_int: false,
            }),
        ),
    ]
}

fn make_posts_collection() -> Collection {
    let mut col = Collection::base("posts", posts_fields());
    col.rules = ApiRules::open();
    col
}

fn make_posts_collection_with_rules(rules: ApiRules) -> Collection {
    let mut col = Collection::base("posts", posts_fields());
    col.rules = rules;
    col
}

fn make_record(id: &str, title: &str, status: &str, views: i64) -> HashMap<String, Value> {
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!(id));
    record.insert("title".to_string(), json!(title));
    record.insert("status".to_string(), json!(status));
    record.insert("views".to_string(), json!(views));
    record.insert("created".to_string(), json!("2025-01-01T00:00:00Z"));
    record.insert("updated".to_string(), json!("2025-01-01T00:00:00Z"));
    record
}

/// Spawn a test server with record routes and in-memory mocks.
async fn spawn_record_app(
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

/// Spawn an app with a "posts" collection and optional seed records.
async fn spawn_posts_app(
    records: Vec<HashMap<String, Value>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let schema = MockSchemaLookup::with(vec![make_posts_collection()]);
    let repo = if records.is_empty() {
        MockRecordRepo::new()
    } else {
        MockRecordRepo::with_records("posts", records)
    };
    spawn_record_app(schema, repo).await
}

// ── List records tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn list_records_returns_200_with_empty_items() {
    let (addr, _handle) = spawn_posts_app(vec![]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 0);
    assert_eq!(body["page"], 1);
    assert_eq!(body["perPage"], 30);
    assert_eq!(body["totalPages"], 1);
    assert!(body["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn list_records_returns_seeded_records() {
    let records = vec![
        make_record("abc123456789012", "First Post", "published", 10),
        make_record("def456789012345", "Second Post", "draft", 5),
    ];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 2);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn list_records_with_pagination() {
    let records: Vec<_> = (0..5)
        .map(|i| {
            let id = format!("id_{i:014}");
            make_record(&id, &format!("Post {i}"), "published", i)
        })
        .collect();
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    // Page 1 with perPage=2
    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records?page=1&perPage=2"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 5);
    assert_eq!(body["page"], 1);
    assert_eq!(body["perPage"], 2);
    assert_eq!(body["totalPages"], 3);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);

    // Page 3 with perPage=2 (should have 1 item)
    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records?page=3&perPage=2"
        ))
        .send()
        .await
        .unwrap();

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn list_records_with_sort() {
    let records = vec![
        make_record("abc123456789012", "Beta", "published", 10),
        make_record("def456789012345", "Alpha", "published", 20),
        make_record("ghi789012345678", "Gamma", "published", 5),
    ];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    // Sort ascending by title
    let resp = client
        .get(format!("{addr}/api/collections/posts/records?sort=title"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items[0]["title"], "Alpha");
    assert_eq!(items[1]["title"], "Beta");
    assert_eq!(items[2]["title"], "Gamma");

    // Sort descending by title
    let resp = client
        .get(format!("{addr}/api/collections/posts/records?sort=-title"))
        .send()
        .await
        .unwrap();

    let body: Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items[0]["title"], "Gamma");
    assert_eq!(items[1]["title"], "Beta");
    assert_eq!(items[2]["title"], "Alpha");
}

#[tokio::test]
async fn list_records_with_filter() {
    let records = vec![
        make_record("abc123456789012", "Published Post", "published", 10),
        make_record("def456789012345", "Draft Post", "draft", 5),
        make_record("ghi789012345678", "Another Published", "published", 15),
    ];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records?filter=status%20%3D%20%22published%22"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 2);
    let items = body["items"].as_array().unwrap();
    for item in items {
        assert_eq!(item["status"], "published");
    }
}

#[tokio::test]
async fn list_records_nonexistent_collection_returns_404() {
    let schema = MockSchemaLookup::new();
    let repo = MockRecordRepo::new();
    let (addr, _handle) = spawn_record_app(schema, repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/nonexistent/records"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], 404);
}

#[tokio::test]
async fn list_records_with_invalid_sort_returns_400() {
    let (addr, _handle) = spawn_posts_app(vec![]).await;
    let client = reqwest::Client::new();

    // Invalid sort: field doesn't exist
    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records?sort=nonexistent_field"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── View record tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn view_record_returns_200() {
    let records = vec![make_record("abc123456789012", "My Post", "published", 42)];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/abc123456789012"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["id"], "abc123456789012");
    assert_eq!(body["title"], "My Post");
    assert_eq!(body["status"], "published");
    assert_eq!(body["views"], 42);
}

#[tokio::test]
async fn view_nonexistent_record_returns_404() {
    let (addr, _handle) = spawn_posts_app(vec![]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/nonexistent00000"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], 404);
}

#[tokio::test]
async fn view_record_in_nonexistent_collection_returns_404() {
    let schema = MockSchemaLookup::new();
    let repo = MockRecordRepo::new();
    let (addr, _handle) = spawn_record_app(schema, repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/nonexistent/records/abc123456789012"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn view_record_with_fields_projection() {
    let records = vec![make_record("abc123456789012", "My Post", "published", 42)];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/abc123456789012?fields=title"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    // id is always included
    assert!(body["id"].is_string());
    assert_eq!(body["title"], "My Post");
    // Other fields should not be present
    assert!(body.get("views").is_none() || body["views"].is_null());
}

// ── Create record tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn create_record_returns_200_with_created_body() {
    let (addr, _handle) = spawn_posts_app(vec![]).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/collections/posts/records"))
        .json(&json!({
            "title": "New Post",
            "status": "draft",
            "views": 0
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    // Auto-generated id
    assert!(body["id"].as_str().is_some_and(|id| !id.is_empty()));
    assert_eq!(body["title"], "New Post");
    assert_eq!(body["status"], "draft");
    // Auto-generated timestamps
    assert!(body["created"].is_string());
    assert!(body["updated"].is_string());
}

#[tokio::test]
async fn create_record_appears_in_list() {
    let (addr, _handle) = spawn_posts_app(vec![]).await;
    let client = reqwest::Client::new();

    // Create
    let resp = client
        .post(format!("{addr}/api/collections/posts/records"))
        .json(&json!({
            "title": "Listed Post",
            "status": "published"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let created: Value = resp.json().await.unwrap();
    let created_id = created["id"].as_str().unwrap();

    // List
    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .send()
        .await
        .unwrap();

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 1);
    assert_eq!(body["items"][0]["id"], created_id);
    assert_eq!(body["items"][0]["title"], "Listed Post");
}

#[tokio::test]
async fn create_record_in_nonexistent_collection_returns_404() {
    let schema = MockSchemaLookup::new();
    let repo = MockRecordRepo::new();
    let (addr, _handle) = spawn_record_app(schema, repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/collections/nonexistent/records"))
        .json(&json!({"title": "Test"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn create_record_with_invalid_body_returns_400() {
    let (addr, _handle) = spawn_posts_app(vec![]).await;
    let client = reqwest::Client::new();

    // Send a non-object body
    let resp = client
        .post(format!("{addr}/api/collections/posts/records"))
        .json(&json!("not an object"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── Update record tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn update_record_returns_200_with_updated_body() {
    let records = vec![make_record("abc123456789012", "Original", "draft", 0)];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/abc123456789012"
        ))
        .json(&json!({
            "title": "Updated Title",
            "status": "published"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["id"], "abc123456789012");
    assert_eq!(body["title"], "Updated Title");
    assert_eq!(body["status"], "published");
}

#[tokio::test]
async fn update_record_preserves_unmodified_fields() {
    let records = vec![make_record("abc123456789012", "Original", "draft", 42)];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    // Only update title
    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/abc123456789012"
        ))
        .json(&json!({ "title": "New Title" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "New Title");
    assert_eq!(body["status"], "draft"); // Preserved
    assert_eq!(body["views"], 42); // Preserved
}

#[tokio::test]
async fn update_nonexistent_record_returns_404() {
    let (addr, _handle) = spawn_posts_app(vec![]).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/nonexistent00000"
        ))
        .json(&json!({ "title": "Updated" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn update_record_cannot_change_id() {
    let records = vec![make_record("abc123456789012", "Original", "draft", 0)];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/abc123456789012"
        ))
        .json(&json!({ "id": "new_id_12345678" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], 400);
}

#[tokio::test]
async fn update_record_in_nonexistent_collection_returns_404() {
    let schema = MockSchemaLookup::new();
    let repo = MockRecordRepo::new();
    let (addr, _handle) = spawn_record_app(schema, repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!(
            "{addr}/api/collections/nonexistent/records/abc123456789012"
        ))
        .json(&json!({ "title": "Updated" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Delete record tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn delete_record_returns_204() {
    let records = vec![make_record("abc123456789012", "To Delete", "draft", 0)];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!(
            "{addr}/api/collections/posts/records/abc123456789012"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn delete_record_then_view_returns_404() {
    let records = vec![make_record("abc123456789012", "To Delete", "draft", 0)];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    // Delete
    let resp = client
        .delete(format!(
            "{addr}/api/collections/posts/records/abc123456789012"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // View — should be gone
    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/abc123456789012"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_record_disappears_from_list() {
    let records = vec![
        make_record("abc123456789012", "Keep", "published", 10),
        make_record("def456789012345", "Delete Me", "draft", 0),
    ];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    // Delete one
    client
        .delete(format!(
            "{addr}/api/collections/posts/records/def456789012345"
        ))
        .send()
        .await
        .unwrap();

    // List — should have 1 record
    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .send()
        .await
        .unwrap();

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 1);
    assert_eq!(body["items"][0]["title"], "Keep");
}

#[tokio::test]
async fn delete_nonexistent_record_returns_404() {
    let (addr, _handle) = spawn_posts_app(vec![]).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!(
            "{addr}/api/collections/posts/records/nonexistent00000"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_record_in_nonexistent_collection_returns_404() {
    let schema = MockSchemaLookup::new();
    let repo = MockRecordRepo::new();
    let (addr, _handle) = spawn_record_app(schema, repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!(
            "{addr}/api/collections/nonexistent/records/abc123456789012"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Response format tests ───────────────────────────────────────────────────

#[tokio::test]
async fn list_response_has_pocketbase_pagination_shape() {
    let (addr, _handle) = spawn_posts_app(vec![]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .send()
        .await
        .unwrap();

    let body: Value = resp.json().await.unwrap();
    assert!(body.get("page").is_some(), "response must have 'page'");
    assert!(
        body.get("perPage").is_some(),
        "response must have 'perPage'"
    );
    assert!(
        body.get("totalItems").is_some(),
        "response must have 'totalItems'"
    );
    assert!(
        body.get("totalPages").is_some(),
        "response must have 'totalPages'"
    );
    assert!(body.get("items").is_some(), "response must have 'items'");
}

#[tokio::test]
async fn error_responses_include_code_and_message() {
    let schema = MockSchemaLookup::new();
    let repo = MockRecordRepo::new();
    let (addr, _handle) = spawn_record_app(schema, repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/nonexistent/records/abc123456789012"
        ))
        .send()
        .await
        .unwrap();

    let body: Value = resp.json().await.unwrap();
    assert!(
        body["code"].is_number(),
        "error response should have 'code'"
    );
    assert!(
        body["message"].is_string(),
        "error response should have 'message'"
    );
}

// ── Full CRUD lifecycle test ────────────────────────────────────────────────

#[tokio::test]
async fn full_crud_lifecycle() {
    let (addr, _handle) = spawn_posts_app(vec![]).await;
    let client = reqwest::Client::new();

    // 1. Create
    let resp = client
        .post(format!("{addr}/api/collections/posts/records"))
        .json(&json!({
            "title": "Lifecycle Post",
            "status": "draft",
            "views": 0
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let created: Value = resp.json().await.unwrap();
    let record_id = created["id"].as_str().unwrap().to_string();

    // 2. View
    let resp = client
        .get(format!("{addr}/api/collections/posts/records/{record_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let viewed: Value = resp.json().await.unwrap();
    assert_eq!(viewed["title"], "Lifecycle Post");

    // 3. Update
    let resp = client
        .patch(format!("{addr}/api/collections/posts/records/{record_id}"))
        .json(&json!({
            "title": "Updated Lifecycle Post",
            "status": "published",
            "views": 100
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated: Value = resp.json().await.unwrap();
    assert_eq!(updated["title"], "Updated Lifecycle Post");
    assert_eq!(updated["status"], "published");
    assert_eq!(updated["views"], 100);

    // 4. List (should have 1 record)
    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 1);
    assert_eq!(body["items"][0]["title"], "Updated Lifecycle Post");

    // 5. Delete
    let resp = client
        .delete(format!("{addr}/api/collections/posts/records/{record_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 6. Verify deleted
    let resp = client
        .get(format!("{addr}/api/collections/posts/records/{record_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 7. List is empty again
    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 0);
}

// ── PocketBase format verification tests ──────────────────────────────────

/// Verify that list responses match PocketBase's exact JSON structure:
/// `{ page, perPage, totalPages, totalItems, items: [...] }`
#[tokio::test]
async fn pocketbase_format_list_response_structure() {
    let records = vec![make_record("abc123456789012", "Hello", "published", 5)];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .send()
        .await
        .unwrap();

    let body: Value = resp.json().await.unwrap();

    // Must have exactly these top-level keys
    let obj = body.as_object().unwrap();
    assert!(obj.contains_key("page"), "missing 'page'");
    assert!(obj.contains_key("perPage"), "missing 'perPage'");
    assert!(obj.contains_key("totalPages"), "missing 'totalPages'");
    assert!(obj.contains_key("totalItems"), "missing 'totalItems'");
    assert!(obj.contains_key("items"), "missing 'items'");

    // Types match PocketBase
    assert!(body["page"].is_u64());
    assert!(body["perPage"].is_u64());
    assert!(body["totalPages"].is_u64());
    assert!(body["totalItems"].is_u64());
    assert!(body["items"].is_array());
}

/// Verify individual records include `collectionId` and `collectionName`
/// alongside record data, matching PocketBase's flat record JSON format.
#[tokio::test]
async fn pocketbase_format_record_includes_collection_metadata() {
    let records = vec![make_record("abc123456789012", "Hello", "published", 5)];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    // Test via view endpoint
    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/abc123456789012"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    // Must have collectionId and collectionName
    assert!(body["collectionId"].is_string(), "missing 'collectionId'");
    assert_eq!(body["collectionName"], "posts");

    // Must also have standard record fields
    assert_eq!(body["id"], "abc123456789012");
    assert_eq!(body["title"], "Hello");
    assert!(body["created"].is_string());
    assert!(body["updated"].is_string());
}

/// Verify records inside list items also include collection metadata.
#[tokio::test]
async fn pocketbase_format_list_items_include_collection_metadata() {
    let records = vec![make_record("abc123456789012", "Hello", "published", 5)];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .send()
        .await
        .unwrap();

    let body: Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);

    let item = &items[0];
    assert!(item["collectionId"].is_string());
    assert_eq!(item["collectionName"], "posts");
    assert_eq!(item["id"], "abc123456789012");
    assert_eq!(item["title"], "Hello");
}

/// Verify create response includes collection metadata.
#[tokio::test]
async fn pocketbase_format_create_response_includes_collection_metadata() {
    let (addr, _handle) = spawn_posts_app(vec![]).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/collections/posts/records"))
        .json(&json!({
            "title": "New",
            "status": "draft",
            "views": 0
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert!(body["collectionId"].is_string());
    assert_eq!(body["collectionName"], "posts");
    assert!(body["id"].is_string());
    assert!(body["created"].is_string());
    assert!(body["updated"].is_string());
}

/// Verify update response includes collection metadata.
#[tokio::test]
async fn pocketbase_format_update_response_includes_collection_metadata() {
    let records = vec![make_record("abc123456789012", "Original", "draft", 0)];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/abc123456789012"
        ))
        .json(&json!({"title": "Updated"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert!(body["collectionId"].is_string());
    assert_eq!(body["collectionName"], "posts");
    assert_eq!(body["id"], "abc123456789012");
}

/// Verify error responses match PocketBase format:
/// `{ code, message, data: { fieldName: { code, message } } }`
#[tokio::test]
async fn pocketbase_format_error_response_structure() {
    let schema = MockSchemaLookup::new();
    let repo = MockRecordRepo::new();
    let (addr, _handle) = spawn_record_app(schema, repo).await;
    let client = reqwest::Client::new();

    // 404 error
    let resp = client
        .get(format!("{addr}/api/collections/nonexistent/records"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body: Value = resp.json().await.unwrap();

    let obj = body.as_object().unwrap();
    assert!(obj.contains_key("code"), "error missing 'code'");
    assert!(obj.contains_key("message"), "error missing 'message'");
    assert!(obj.contains_key("data"), "error missing 'data'");

    assert_eq!(body["code"], 404);
    assert!(body["message"].is_string());
    // data should be an object (empty for non-validation errors)
    assert!(body["data"].is_object());
}

/// Verify validation error data contains nested field errors with code and message.
#[tokio::test]
async fn pocketbase_format_validation_error_has_field_errors() {
    let (addr, _handle) = spawn_posts_app(vec![]).await;
    let client = reqwest::Client::new();

    // Send non-object body to trigger validation error
    let resp = client
        .post(format!("{addr}/api/collections/posts/records"))
        .json(&json!("not an object"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: Value = resp.json().await.unwrap();

    assert_eq!(body["code"], 400);
    assert!(body["message"].is_string());
    assert!(body["data"].is_object());
}

// ── Count records tests ───────────────────────────────────────────────────

#[tokio::test]
async fn count_records_returns_zero_for_empty_collection() {
    let (addr, _handle) = spawn_posts_app(vec![]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts/records/count"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 0);
}

#[tokio::test]
async fn count_records_returns_correct_total() {
    let records = vec![
        make_record("r1", "Post One", "published", 10),
        make_record("r2", "Post Two", "draft", 5),
        make_record("r3", "Post Three", "published", 20),
    ];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts/records/count"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 3);
}

#[tokio::test]
async fn count_records_with_filter() {
    let records = vec![
        make_record("r1", "Post One", "published", 10),
        make_record("r2", "Post Two", "draft", 5),
        make_record("r3", "Post Three", "published", 20),
    ];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/count?filter=status = \"published\""
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 2);
}

#[tokio::test]
async fn count_records_returns_404_for_unknown_collection() {
    let (addr, _handle) = spawn_posts_app(vec![]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/nonexistent/records/count"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn count_records_without_filter_returns_all() {
    let records = vec![
        make_record("r1", "A", "draft", 1),
        make_record("r2", "B", "draft", 2),
        make_record("r3", "C", "draft", 3),
        make_record("r4", "D", "draft", 4),
        make_record("r5", "E", "draft", 5),
    ];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts/records/count"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 5);
}

#[tokio::test]
async fn count_records_filter_matches_none() {
    let records = vec![
        make_record("r1", "Post One", "published", 10),
        make_record("r2", "Post Two", "published", 5),
    ];
    let (addr, _handle) = spawn_posts_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/count?filter=status = \"archived\""
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 0);
}

// ── Rule enforcement integration tests ──────────────────────────────────────

/// Spawn an app with a "posts" collection using specific rules.
async fn spawn_posts_app_with_rules(
    rules: ApiRules,
    records: Vec<HashMap<String, Value>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let schema = MockSchemaLookup::with(vec![make_posts_collection_with_rules(rules)]);
    let repo = if records.is_empty() {
        MockRecordRepo::new()
    } else {
        MockRecordRepo::with_records("posts", records)
    };
    spawn_record_app(schema, repo).await
}

// ── Null rules (locked): block non-superusers ───────────────────────────────

#[tokio::test]
async fn locked_list_returns_200_empty_for_anonymous() {
    let (addr, _handle) = spawn_posts_app_with_rules(
        ApiRules::locked(),
        vec![make_record("r1", "Title", "draft", 1)],
    )
    .await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 0);
    assert!(body["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn locked_view_returns_403_for_anonymous() {
    let (addr, _handle) = spawn_posts_app_with_rules(
        ApiRules::locked(),
        vec![make_record("r1_view_locked", "Title", "draft", 1)],
    )
    .await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/r1_view_locked"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn locked_create_returns_403_for_anonymous() {
    let (addr, _handle) = spawn_posts_app_with_rules(ApiRules::locked(), vec![]).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/collections/posts/records"))
        .json(&json!({"title": "New", "status": "draft", "views": 0}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn locked_update_returns_403_for_anonymous() {
    let (addr, _handle) = spawn_posts_app_with_rules(
        ApiRules::locked(),
        vec![make_record("r1_upd_locked", "Title", "draft", 1)],
    )
    .await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/r1_upd_locked"
        ))
        .json(&json!({"title": "Changed"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn locked_delete_returns_403_for_anonymous() {
    let (addr, _handle) = spawn_posts_app_with_rules(
        ApiRules::locked(),
        vec![make_record("r1_del_locked", "Title", "draft", 1)],
    )
    .await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!(
            "{addr}/api/collections/posts/records/r1_del_locked"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn locked_count_returns_200_zero_for_anonymous() {
    let (addr, _handle) = spawn_posts_app_with_rules(
        ApiRules::locked(),
        vec![make_record("r1", "Title", "draft", 1)],
    )
    .await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts/records/count"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 0);
}

// ── Null rules (locked): superuser bypasses ─────────────────────────────────

#[tokio::test]
async fn locked_list_allowed_for_superuser() {
    let (addr, _handle) = spawn_posts_app_with_rules(
        ApiRules::locked(),
        vec![make_record("r1_su_list", "Title", "draft", 1)],
    )
    .await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .header("Authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 1);
}

#[tokio::test]
async fn locked_view_allowed_for_superuser() {
    let (addr, _handle) = spawn_posts_app_with_rules(
        ApiRules::locked(),
        vec![make_record("r1_su_view", "Title", "draft", 1)],
    )
    .await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts/records/r1_su_view"))
        .header("Authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["id"], "r1_su_view");
}

#[tokio::test]
async fn locked_create_allowed_for_superuser() {
    let (addr, _handle) = spawn_posts_app_with_rules(ApiRules::locked(), vec![]).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/collections/posts/records"))
        .header("Authorization", "Bearer SUPERUSER")
        .json(&json!({"title": "New", "status": "draft", "views": 0}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn locked_delete_allowed_for_superuser() {
    let (addr, _handle) = spawn_posts_app_with_rules(
        ApiRules::locked(),
        vec![make_record("r1_su_del", "Title", "draft", 1)],
    )
    .await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{addr}/api/collections/posts/records/r1_su_del"))
        .header("Authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

// ── Empty rules (open): allow everyone ──────────────────────────────────────

#[tokio::test]
async fn open_list_allowed_for_anonymous() {
    let records = vec![make_record("r1_open", "Title", "draft", 1)];
    let (addr, _handle) = spawn_posts_app_with_rules(ApiRules::open(), records).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 1);
}

#[tokio::test]
async fn open_create_allowed_for_anonymous() {
    let (addr, _handle) = spawn_posts_app_with_rules(ApiRules::open(), vec![]).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/collections/posts/records"))
        .json(&json!({"title": "New", "status": "draft", "views": 0}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn open_view_allowed_for_anonymous() {
    let (addr, _handle) = spawn_posts_app_with_rules(
        ApiRules::open(),
        vec![make_record("r1_open_view", "Title", "draft", 1)],
    )
    .await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts/records/r1_open_view"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn open_delete_allowed_for_anonymous() {
    let (addr, _handle) = spawn_posts_app_with_rules(
        ApiRules::open(),
        vec![make_record("r1_open_del", "Title", "draft", 1)],
    )
    .await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{addr}/api/collections/posts/records/r1_open_del"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

// ── Expression rules ────────────────────────────────────────────────────────

#[tokio::test]
async fn expression_rule_allows_authenticated_user() {
    let rules = ApiRules {
        list_rule: Some(r#"@request.auth.id != """#.to_string()),
        view_rule: Some(r#"@request.auth.id != """#.to_string()),
        create_rule: Some(r#"@request.auth.id != """#.to_string()),
        update_rule: Some(r#"@request.auth.id != """#.to_string()),
        delete_rule: Some(r#"@request.auth.id != """#.to_string()),
        manage_rule: None,
    };
    let (addr, _handle) =
        spawn_posts_app_with_rules(rules, vec![make_record("r1_expr", "Title", "draft", 1)]).await;
    let client = reqwest::Client::new();

    // Authenticated user (token = user ID)
    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .header("Authorization", "Bearer user123")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 1);

    // View
    let resp = client
        .get(format!("{addr}/api/collections/posts/records/r1_expr"))
        .header("Authorization", "Bearer user123")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Create
    let resp = client
        .post(format!("{addr}/api/collections/posts/records"))
        .header("Authorization", "Bearer user123")
        .json(&json!({"title": "New", "status": "draft", "views": 0}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Delete
    let resp = client
        .delete(format!("{addr}/api/collections/posts/records/r1_expr"))
        .header("Authorization", "Bearer user123")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn expression_rule_denies_anonymous() {
    let rules = ApiRules {
        list_rule: Some(r#"@request.auth.id != """#.to_string()),
        view_rule: Some(r#"@request.auth.id != """#.to_string()),
        create_rule: Some(r#"@request.auth.id != """#.to_string()),
        update_rule: Some(r#"@request.auth.id != """#.to_string()),
        delete_rule: Some(r#"@request.auth.id != """#.to_string()),
        manage_rule: None,
    };
    let (addr, _handle) = spawn_posts_app_with_rules(
        rules,
        vec![make_record("r1_expr_deny", "Title", "draft", 1)],
    )
    .await;
    let client = reqwest::Client::new();

    // List returns 200 with empty items (PocketBase behaviour)
    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 0);

    // View returns 404 (hides existence, per PocketBase)
    let resp = client
        .get(format!("{addr}/api/collections/posts/records/r1_expr_deny"))
        .send()
        .await
        .unwrap();
    assert!(resp.status() == StatusCode::FORBIDDEN || resp.status() == StatusCode::NOT_FOUND);

    // Create returns 403
    let resp = client
        .post(format!("{addr}/api/collections/posts/records"))
        .json(&json!({"title": "New", "status": "draft", "views": 0}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Delete returns 404 (hides existence)
    let resp = client
        .delete(format!("{addr}/api/collections/posts/records/r1_expr_deny"))
        .send()
        .await
        .unwrap();
    assert!(resp.status() == StatusCode::FORBIDDEN || resp.status() == StatusCode::NOT_FOUND);
}

// ── Public-read rules ───────────────────────────────────────────────────────

#[tokio::test]
async fn public_read_allows_list_and_view_blocks_write() {
    let (addr, _handle) = spawn_posts_app_with_rules(
        ApiRules::public_read(),
        vec![make_record("r1_pr", "Title", "draft", 1)],
    )
    .await;
    let client = reqwest::Client::new();

    // List: open
    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 1);

    // View: open
    let resp = client
        .get(format!("{addr}/api/collections/posts/records/r1_pr"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Create: locked → 403
    let resp = client
        .post(format!("{addr}/api/collections/posts/records"))
        .json(&json!({"title": "New", "status": "draft", "views": 0}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Update: auth-required expression rule → 404 (hides existence for PATCH)
    let resp = client
        .patch(format!("{addr}/api/collections/posts/records/r1_pr"))
        .json(&json!({"title": "Changed"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Delete: auth-required expression rule → 404 (hides existence for DELETE)
    let resp = client
        .delete(format!("{addr}/api/collections/posts/records/r1_pr"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Superuser bypasses expression rules ─────────────────────────────────────

#[tokio::test]
async fn superuser_bypasses_expression_rules() {
    let rules = ApiRules {
        list_rule: Some(r#"@request.auth.id != """#.to_string()),
        view_rule: Some(r#"@request.auth.id != """#.to_string()),
        create_rule: Some(r#"@request.auth.id != """#.to_string()),
        update_rule: Some(r#"@request.auth.id != """#.to_string()),
        delete_rule: Some(r#"@request.auth.id != """#.to_string()),
        manage_rule: None,
    };
    let (addr, _handle) =
        spawn_posts_app_with_rules(rules, vec![make_record("r1_su_expr", "Title", "draft", 1)])
            .await;
    let client = reqwest::Client::new();

    // Superuser can list
    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .header("Authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 1);

    // Superuser can view
    let resp = client
        .get(format!("{addr}/api/collections/posts/records/r1_su_expr"))
        .header("Authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Superuser can create
    let resp = client
        .post(format!("{addr}/api/collections/posts/records"))
        .header("Authorization", "Bearer SUPERUSER")
        .json(&json!({"title": "New", "status": "draft", "views": 0}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ── Expand integration tests ────────────────────────────────────────────────

use zerobase_core::schema::RelationOptions;

/// Build a two-collection setup: "authors" and "articles" where articles.author is
/// a single relation pointing to the authors collection.
fn expand_test_collections() -> Vec<Collection> {
    let authors = {
        let mut col = Collection::base(
            "authors",
            vec![Field::new(
                "name",
                FieldType::Text(TextOptions {
                    min_length: 0,
                    max_length: 200,
                    pattern: None,
                    searchable: false,
                }),
            )],
        );
        col.rules = ApiRules::open();
        col
    };

    let articles = {
        let mut col = Collection::base(
            "articles",
            vec![
                Field::new(
                    "title",
                    FieldType::Text(TextOptions {
                        min_length: 0,
                        max_length: 200,
                        pattern: None,
                        searchable: false,
                    }),
                ),
                Field::new(
                    "author",
                    FieldType::Relation(RelationOptions {
                        collection_id: "authors".to_string(),
                        max_select: 1,
                        ..Default::default()
                    }),
                ),
            ],
        );
        col.rules = ApiRules::open();
        col
    };

    vec![authors, articles]
}

fn make_author(id: &str, name: &str) -> HashMap<String, Value> {
    let mut r = HashMap::new();
    r.insert("id".to_string(), json!(id));
    r.insert("name".to_string(), json!(name));
    r.insert("created".to_string(), json!("2025-01-01T00:00:00Z"));
    r.insert("updated".to_string(), json!("2025-01-01T00:00:00Z"));
    r
}

fn make_article(id: &str, title: &str, author_id: &str) -> HashMap<String, Value> {
    let mut r = HashMap::new();
    r.insert("id".to_string(), json!(id));
    r.insert("title".to_string(), json!(title));
    r.insert("author".to_string(), json!(author_id));
    r.insert("created".to_string(), json!("2025-01-01T00:00:00Z"));
    r.insert("updated".to_string(), json!("2025-01-01T00:00:00Z"));
    r
}

async fn spawn_expand_app() -> (String, tokio::task::JoinHandle<()>) {
    let schema = MockSchemaLookup::with(expand_test_collections());
    let repo = MockRecordRepo::with_multi_records(vec![
        (
            "authors",
            vec![
                make_author("author1_________", "Alice"),
                make_author("author2_________", "Bob"),
            ],
        ),
        (
            "articles",
            vec![
                make_article("article1________", "First Article", "author1_________"),
                make_article("article2________", "Second Article", "author2_________"),
            ],
        ),
    ]);
    spawn_record_app(schema, repo).await
}

#[tokio::test]
async fn expand_single_relation_on_view() {
    let (addr, _handle) = spawn_expand_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/articles/records/article1________?expand=author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "First Article");

    let expand = &body["expand"];
    assert!(expand.is_object(), "expand field should be present");
    let author_expand = &expand["author"];
    assert_eq!(author_expand["name"], "Alice");
    assert_eq!(author_expand["id"], "author1_________");
}

#[tokio::test]
async fn expand_single_relation_on_list() {
    let (addr, _handle) = spawn_expand_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/articles/records?expand=author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);

    // Each item should have expand.author
    for item in items {
        assert!(
            item["expand"]["author"].is_object(),
            "each item should have expanded author"
        );
    }
}

#[tokio::test]
async fn expand_back_relation_on_view() {
    let (addr, _handle) = spawn_expand_app().await;
    let client = reqwest::Client::new();

    // Expand articles that reference this author via the "author" field
    let resp = client
        .get(format!(
            "{addr}/api/collections/authors/records/author1_________?expand=articles_via_author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "Alice");

    let expand = &body["expand"];
    let articles = expand["articles_via_author"].as_array().unwrap();
    assert_eq!(articles.len(), 1);
    assert_eq!(articles[0]["title"], "First Article");
}

#[tokio::test]
async fn expand_no_expand_param_returns_no_expand_field() {
    let (addr, _handle) = spawn_expand_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/articles/records/article1________"
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

#[tokio::test]
async fn expand_nonexistent_field_is_ignored() {
    let (addr, _handle) = spawn_expand_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/articles/records/article1________?expand=nonexistent"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    // Should still return the record, just without expand data
    assert_eq!(body["title"], "First Article");
}

#[tokio::test]
async fn expand_back_relation_on_list() {
    let (addr, _handle) = spawn_expand_app().await;
    let client = reqwest::Client::new();

    // Expand articles_via_author on a list of authors
    let resp = client
        .get(format!(
            "{addr}/api/collections/authors/records?expand=articles_via_author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);

    // Each author should have their articles expanded
    for item in items {
        let name = item["name"].as_str().unwrap();
        let expand = &item["expand"];
        let articles = expand["articles_via_author"].as_array().unwrap();

        match name {
            "Alice" => {
                assert_eq!(articles.len(), 1);
                assert_eq!(articles[0]["title"], "First Article");
            }
            "Bob" => {
                assert_eq!(articles.len(), 1);
                assert_eq!(articles[0]["title"], "Second Article");
            }
            _ => panic!("unexpected author: {name}"),
        }
    }
}

#[tokio::test]
async fn expand_back_relation_includes_collection_metadata() {
    let (addr, _handle) = spawn_expand_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/authors/records/author1_________?expand=articles_via_author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let articles = body["expand"]["articles_via_author"].as_array().unwrap();

    for article in articles {
        assert!(
            article.get("collectionName").is_some(),
            "expanded back-relation records should have collectionName"
        );
        assert_eq!(article["collectionName"], "articles");
    }
}

#[tokio::test]
async fn expand_combined_forward_and_back_relation_on_view() {
    let (addr, _handle) = spawn_expand_app().await;
    let client = reqwest::Client::new();

    // On an article, expand author (forward) — articles_via_author would point
    // to another collection, but we can test combined expand params are accepted
    let resp = client
        .get(format!(
            "{addr}/api/collections/articles/records/article1________?expand=author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    // Forward relation
    assert!(body["expand"]["author"].is_object());
    assert_eq!(body["expand"]["author"]["name"], "Alice");
}

#[tokio::test]
async fn expand_back_relation_no_matches_no_expand_field() {
    // Create a setup where an author has no articles
    let schema = MockSchemaLookup::with(expand_test_collections());
    let repo = MockRecordRepo::with_multi_records(vec![
        ("authors", vec![make_author("author_lonely____", "Charlie")]),
        ("articles", vec![]), // No articles at all
    ]);
    let (addr, _handle) = spawn_record_app(schema, repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/collections/authors/records/author_lonely____?expand=articles_via_author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "Charlie");

    // No matching back-relations → expand field should not be present
    assert!(
        body.get("expand").is_none(),
        "no expand field when back-relation yields no results"
    );
}

// ── Expand view_rule enforcement integration tests ──────────────────────────

/// Build collections where `articles.author` is a relation to `authors`,
/// but `authors` has a restrictive view_rule.
fn expand_view_rule_test_collections(author_view_rule: Option<String>) -> Vec<Collection> {
    let mut authors = Collection::base(
        "authors",
        vec![
            Field::new(
                "name",
                FieldType::Text(TextOptions {
                    min_length: 0,
                    max_length: 200,
                    pattern: None,
                    searchable: false,
                }),
            ),
        ],
    );
    authors.rules = ApiRules {
        view_rule: author_view_rule,
        list_rule: Some(String::new()),
        create_rule: Some(String::new()),
        update_rule: Some(String::new()),
        delete_rule: Some(String::new()),
        manage_rule: None,
    };

    let mut articles = Collection::base(
        "articles",
        vec![
            Field::new(
                "title",
                FieldType::Text(TextOptions {
                    min_length: 0,
                    max_length: 200,
                    pattern: None,
                    searchable: false,
                }),
            ),
            Field::new(
                "author",
                FieldType::Relation(RelationOptions {
                    collection_id: "authors".to_string(),
                    max_select: 1,
                    ..Default::default()
                }),
            ),
        ],
    );
    articles.rules = ApiRules::open();

    vec![authors, articles]
}

async fn spawn_expand_view_rule_app(
    author_view_rule: Option<String>,
) -> (String, tokio::task::JoinHandle<()>) {
    let schema = MockSchemaLookup::with(expand_view_rule_test_collections(author_view_rule));
    let repo = MockRecordRepo::with_multi_records(vec![
        (
            "authors",
            vec![make_author("author1_________", "Alice")],
        ),
        (
            "articles",
            vec![make_article("article1________", "Test Article", "author1_________")],
        ),
    ]);
    spawn_record_app(schema, repo).await
}

#[tokio::test]
async fn expand_locked_view_rule_hides_relation_for_anonymous() {
    // authors has view_rule = None (locked → superusers only)
    let (addr, _handle) = spawn_expand_view_rule_app(None).await;
    let client = reqwest::Client::new();

    // Anonymous request (no auth header)
    let resp = client
        .get(format!(
            "{addr}/api/collections/articles/records/article1________?expand=author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "Test Article");

    // The expand should be absent or the author entry should not be present,
    // because the anonymous user cannot view authors.
    let expand = body.get("expand");
    let has_author = expand
        .and_then(|e| e.as_object())
        .map(|e| e.contains_key("author"))
        .unwrap_or(false);
    assert!(
        !has_author,
        "locked view_rule on target collection must hide expanded relation for anonymous user"
    );
}

#[tokio::test]
async fn expand_locked_view_rule_visible_to_superuser() {
    // authors has view_rule = None (locked → superusers only)
    let (addr, _handle) = spawn_expand_view_rule_app(None).await;
    let client = reqwest::Client::new();

    // Superuser request
    let resp = client
        .get(format!(
            "{addr}/api/collections/articles/records/article1________?expand=author"
        ))
        .header("Authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "Test Article");

    // Superuser should see the expanded author.
    let expand = body.get("expand").expect("superuser must see expand");
    assert_eq!(
        expand["author"]["name"], "Alice",
        "superuser must see expanded author record despite locked view_rule"
    );
}

#[tokio::test]
async fn expand_open_view_rule_visible_to_anonymous() {
    // authors has view_rule = Some("") (open to everyone)
    let (addr, _handle) = spawn_expand_view_rule_app(Some(String::new())).await;
    let client = reqwest::Client::new();

    // Anonymous request
    let resp = client
        .get(format!(
            "{addr}/api/collections/articles/records/article1________?expand=author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    let expand = body
        .get("expand")
        .expect("anonymous must see expand when view_rule is open");
    assert_eq!(expand["author"]["name"], "Alice");
}

#[tokio::test]
async fn expand_locked_view_rule_list_hides_relations_for_anonymous() {
    // Test that the list endpoint also respects view_rule on expansions.
    let (addr, _handle) = spawn_expand_view_rule_app(None).await;
    let client = reqwest::Client::new();

    // Anonymous list request
    let resp = client
        .get(format!(
            "{addr}/api/collections/articles/records?expand=author"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let items = body["items"].as_array().expect("items must be array");
    assert!(!items.is_empty());

    for item in items {
        let has_author = item
            .get("expand")
            .and_then(|e| e.as_object())
            .map(|e| e.contains_key("author"))
            .unwrap_or(false);
        assert!(
            !has_author,
            "list endpoint must not expand relations to locked collections for anonymous"
        );
    }
}
