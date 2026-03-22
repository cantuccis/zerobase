//! Integration tests for the data export endpoint.
//!
//! Tests exercise the full HTTP stack (router → handler → service → mock repo)
//! using in-memory mocks. Each test spawns an isolated server on a random port.

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
use zerobase_core::schema::{Collection, Field, FieldType, TextOptions};
use zerobase_core::services::record_service::{
    RecordList, RecordQuery, RecordRepoError, RecordRepository, RecordService, SchemaLookup,
    SortDirection,
};

use zerobase_api::AuthInfo;

// ── Fake auth middleware ─────────────────────────────────────────────────────

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
                record.insert("id".to_string(), Value::String(token));
                AuthInfo::authenticated(record)
            }
        })
        .unwrap_or_else(AuthInfo::anonymous);

    request.extensions_mut().insert(auth_info);
    next.run(request).await
}

// ── In-memory mock: SchemaLookup ─────────────────────────────────────────────

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
}

// ── In-memory mock: RecordRepository ─────────────────────────────────────────

struct MockRecordRepo {
    records: Mutex<HashMap<String, Vec<HashMap<String, Value>>>>,
}

impl MockRecordRepo {
    fn new() -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
        }
    }

    fn with_records(collection: &str, records: Vec<HashMap<String, Value>>) -> Self {
        let mut map = HashMap::new();
        map.insert(collection.to_string(), records);
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
                let cmp = match (&va, &vb) {
                    (Value::String(a), Value::String(b)) => a.cmp(b),
                    (Value::Number(a), Value::Number(b)) => {
                        let fa = a.as_f64().unwrap_or(0.0);
                        let fb = b.as_f64().unwrap_or(0.0);
                        fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
                    }
                    _ => std::cmp::Ordering::Equal,
                };
                if desc { cmp.reverse() } else { cmp }
            });
        }

        if let Some(ref filter) = query.filter {
            let parts: Vec<&str> = filter.splitn(2, '=').collect();
            if parts.len() == 2 {
                let field = parts[0].trim().to_string();
                let value = parts[1].trim().trim_matches('"').to_string();
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
        let items: Vec<_> = rows.into_iter().skip(start).take(per_page as usize).collect();
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
        store.entry(collection.to_string()).or_default().push(data.clone());
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
            if let Some(row) = rows.iter_mut().find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id)) {
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
            let parts: Vec<&str> = f.splitn(2, '=').collect();
            if parts.len() == 2 {
                let field = parts[0].trim().to_string();
                let value = parts[1].trim().trim_matches('"').to_string();
                let count = rows.iter().filter(|r| {
                    r.get(&field).and_then(|v| v.as_str()).map(|v| v == value).unwrap_or(false)
                }).count();
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
        let matches = rows.into_iter().filter(|r| {
            r.get(field_name).map(|v| match v {
                Value::String(s) => s == referenced_id,
                Value::Array(arr) => arr.iter().any(|item| item.as_str().map(|s| s == referenced_id).unwrap_or(false)),
                _ => false,
            }).unwrap_or(false)
        }).collect();
        Ok(matches)
    }
}

// ── Test infrastructure ──────────────────────────────────────────────────────

fn text_field(name: &str) -> Field {
    Field::new(
        name,
        FieldType::Text(TextOptions {
            min_length: 0,
            max_length: 200,
            pattern: None,
            searchable: false,
        }),
    )
}

fn make_posts_collection() -> Collection {
    Collection::base(
        "posts",
        vec![text_field("title"), text_field("status")],
    )
}

fn make_record(id: &str, title: &str, status: &str) -> HashMap<String, Value> {
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!(id));
    record.insert("title".to_string(), json!(title));
    record.insert("status".to_string(), json!(status));
    record.insert("created".to_string(), json!("2025-01-01T00:00:00Z"));
    record.insert("updated".to_string(), json!("2025-01-01T00:00:00Z"));
    record
}

async fn spawn_export_app(
    schema: MockSchemaLookup,
    repo: MockRecordRepo,
) -> (String, tokio::task::JoinHandle<()>) {
    let service = Arc::new(RecordService::new(repo, schema));

    let app = zerobase_api::api_router()
        .merge(zerobase_api::export_routes(service))
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

async fn spawn_posts_export_app(
    records: Vec<HashMap<String, Value>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let schema = MockSchemaLookup::with(vec![make_posts_collection()]);
    let repo = if records.is_empty() {
        MockRecordRepo::new()
    } else {
        MockRecordRepo::with_records("posts", records)
    };
    spawn_export_app(schema, repo).await
}

// ── Tests: JSON export ───────────────────────────────────────────────────────

#[tokio::test]
async fn export_json_empty_collection() {
    let (addr, _handle) = spawn_posts_export_app(vec![]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/collections/posts/export"))
        .header("authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "application/json; charset=utf-8"
    );
    assert_eq!(
        resp.headers().get("content-disposition").unwrap(),
        "attachment; filename=\"posts_export.json\""
    );

    let body: Value = resp.json().await.unwrap();
    assert!(body.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn export_json_with_records() {
    let records = vec![
        make_record("r1", "First Post", "active"),
        make_record("r2", "Second Post", "draft"),
    ];
    let (addr, _handle) = spawn_posts_export_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/collections/posts/export?format=json"))
        .header("authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["id"], "r1");
    assert_eq!(arr[1]["id"], "r2");
    assert_eq!(arr[0]["title"], "First Post");
}

// ── Tests: CSV export ────────────────────────────────────────────────────────

#[tokio::test]
async fn export_csv_empty_collection() {
    let (addr, _handle) = spawn_posts_export_app(vec![]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/collections/posts/export?format=csv"))
        .header("authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/csv; charset=utf-8"
    );
    assert_eq!(
        resp.headers().get("content-disposition").unwrap(),
        "attachment; filename=\"posts_export.csv\""
    );

    let body = resp.text().await.unwrap();
    // Should contain header row only.
    let lines: Vec<&str> = body.trim().lines().collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("id"));
    assert!(lines[0].contains("title"));
}

#[tokio::test]
async fn export_csv_with_records() {
    let records = vec![
        make_record("r1", "First Post", "active"),
        make_record("r2", "Second Post", "draft"),
    ];
    let (addr, _handle) = spawn_posts_export_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/collections/posts/export?format=csv"))
        .header("authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.text().await.unwrap();
    let lines: Vec<&str> = body.trim().lines().collect();
    // Header + 2 data rows.
    assert_eq!(lines.len(), 3);
    // Data rows should contain our record IDs.
    assert!(body.contains("r1"));
    assert!(body.contains("r2"));
    assert!(body.contains("First Post"));
    assert!(body.contains("Second Post"));
}

// ── Tests: Auth / access control ─────────────────────────────────────────────

#[tokio::test]
async fn export_requires_superuser_auth() {
    let (addr, _handle) = spawn_posts_export_app(vec![]).await;
    let client = reqwest::Client::new();

    // No auth → should be rejected.
    let resp = client
        .get(format!("{addr}/_/api/collections/posts/export"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Regular user auth → should also be rejected (not a superuser).
    let resp = client
        .get(format!("{addr}/_/api/collections/posts/export"))
        .header("authorization", "Bearer user123")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Tests: Error cases ───────────────────────────────────────────────────────

#[tokio::test]
async fn export_nonexistent_collection_returns_404() {
    let schema = MockSchemaLookup::with(vec![]);
    let repo = MockRecordRepo::new();
    let (addr, _handle) = spawn_export_app(schema, repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/collections/nonexistent/export"))
        .header("authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Tests: Filtering ─────────────────────────────────────────────────────────

#[tokio::test]
async fn export_json_with_filter() {
    let records = vec![
        make_record("r1", "Active Post", "active"),
        make_record("r2", "Draft Post", "draft"),
        make_record("r3", "Another Active", "active"),
    ];
    let (addr, _handle) = spawn_posts_export_app(records).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/_/api/collections/posts/export?format=json&filter=status%20%3D%20%22active%22"
        ))
        .header("authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert!(arr.iter().all(|r| r["status"] == "active"));
}

// ── Tests: Auth collection headers ───────────────────────────────────────────

#[tokio::test]
async fn export_csv_auth_collection_includes_auth_headers() {
    let fields = vec![text_field("name")];
    let collection = Collection::auth("users", fields);
    let schema = MockSchemaLookup::with(vec![collection]);

    let mut record = HashMap::new();
    record.insert("id".to_string(), json!("u1"));
    record.insert("email".to_string(), json!("user@test.com"));
    record.insert("verified".to_string(), json!(true));
    record.insert("emailVisibility".to_string(), json!(false));
    record.insert("name".to_string(), json!("Test User"));
    record.insert("created".to_string(), json!("2025-01-01T00:00:00Z"));
    record.insert("updated".to_string(), json!("2025-01-01T00:00:00Z"));

    let repo = MockRecordRepo::with_records("users", vec![record]);
    let (addr, _handle) = spawn_export_app(schema, repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/collections/users/export?format=csv"))
        .header("authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.text().await.unwrap();
    let header_line = body.lines().next().unwrap();
    // Auth collections should include email, verified, emailVisibility in headers.
    assert!(header_line.contains("email"));
    assert!(header_line.contains("verified"));
    assert!(header_line.contains("emailVisibility"));
    assert!(header_line.contains("name"));
    // Data should include the actual values.
    assert!(body.contains("user@test.com"));
    assert!(body.contains("Test User"));
}
