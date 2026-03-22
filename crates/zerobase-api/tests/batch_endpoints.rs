//! Integration tests for the batch operation endpoint.
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
use zerobase_core::schema::{ApiRules, Collection, Field, FieldType, NumberOptions, TextOptions};
use zerobase_core::services::record_service::{
    RecordList, RecordQuery, RecordRepoError, RecordRepository, RecordService, SchemaLookup,
    SortDirection,
};

use zerobase_api::AuthInfo;

// ── Shared fake auth middleware ───────────────────────────────────────────────

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

// ── In-memory mock: SchemaLookup ──────────────────────────────────────────────

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

// ── In-memory mock: RecordRepository ──────────────────────────────────────────

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
        let rows = store.get(collection).cloned().unwrap_or_default();
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
        _filter: Option<&str>,
    ) -> std::result::Result<u64, RecordRepoError> {
        let store = self.records.lock().unwrap();
        Ok(store.get(collection).map(|r| r.len() as u64).unwrap_or(0))
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

// ── Test infrastructure ───────────────────────────────────────────────────────

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

/// Spawn a test server with batch + record routes and in-memory mocks.
async fn spawn_batch_app(
    schema: MockSchemaLookup,
    repo: MockRecordRepo,
) -> (String, tokio::task::JoinHandle<()>) {
    let service = Arc::new(RecordService::new(repo, schema));

    let app = zerobase_api::api_router()
        .merge(zerobase_api::batch_routes(service.clone()))
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

async fn spawn_posts_batch_app(
    records: Vec<HashMap<String, Value>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let schema = MockSchemaLookup::with(vec![make_posts_collection()]);
    let repo = if records.is_empty() {
        MockRecordRepo::new()
    } else {
        MockRecordRepo::with_records("posts", records)
    };
    spawn_batch_app(schema, repo).await
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn batch_create_single_record() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "POST",
                "url": "/api/collections/posts/records",
                "body": { "title": "First post", "status": "draft", "views": 0 }
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let json: Value = resp.json().await.unwrap();
    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], 200);
    assert_eq!(results[0]["body"]["title"], "First post");
    assert!(results[0]["body"]["id"].is_string());
}

#[tokio::test]
async fn batch_multiple_creates() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "POST",
                "url": "/api/collections/posts/records",
                "body": { "title": "Post 1", "status": "draft", "views": 0 }
            },
            {
                "method": "POST",
                "url": "/api/collections/posts/records",
                "body": { "title": "Post 2", "status": "published", "views": 10 }
            },
            {
                "method": "POST",
                "url": "/api/collections/posts/records",
                "body": { "title": "Post 3", "status": "archived", "views": 100 }
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let json: Value = resp.json().await.unwrap();
    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);

    for result in results {
        assert_eq!(result["status"], 200);
    }

    assert_eq!(results[0]["body"]["title"], "Post 1");
    assert_eq!(results[1]["body"]["title"], "Post 2");
    assert_eq!(results[2]["body"]["title"], "Post 3");

    // All records should have unique IDs.
    let ids: Vec<&str> = results
        .iter()
        .map(|r| r["body"]["id"].as_str().unwrap())
        .collect();
    let unique: std::collections::HashSet<&str> = ids.iter().cloned().collect();
    assert_eq!(ids.len(), unique.len(), "all record IDs should be unique");
}

#[tokio::test]
async fn batch_update_existing_record() {
    let records = vec![make_record("rec1aaaaaaaaaaa", "Original", "draft", 0)];
    let (addr, _handle) = spawn_posts_batch_app(records).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "PATCH",
                "url": "/api/collections/posts/records/rec1aaaaaaaaaaa",
                "body": { "title": "Updated title", "views": 42 }
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let json: Value = resp.json().await.unwrap();

    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], 200);
    assert_eq!(results[0]["body"]["title"], "Updated title");
    assert_eq!(results[0]["body"]["views"], 42);
}

#[tokio::test]
async fn batch_delete_existing_record() {
    let records = vec![make_record("rec1bbbbbbbbbbb", "To delete", "draft", 0)];
    let (addr, _handle) = spawn_posts_batch_app(records).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "DELETE",
                "url": "/api/collections/posts/records/rec1bbbbbbbbbbb"
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let json: Value = resp.json().await.unwrap();
    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], 204);

    // Verify the record is gone by trying to fetch it.
    let get_resp = client
        .get(format!("{addr}/api/collections/posts/records/rec1bbbbbbbbbbb"))
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn batch_mixed_operations() {
    let records = vec![
        make_record("existing1aaaaaa", "Existing post", "draft", 5),
        make_record("to_delete_aaaaa", "Will be deleted", "old", 0),
    ];
    let (addr, _handle) = spawn_posts_batch_app(records).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "POST",
                "url": "/api/collections/posts/records",
                "body": { "title": "Brand new", "status": "draft", "views": 0 }
            },
            {
                "method": "PATCH",
                "url": "/api/collections/posts/records/existing1aaaaaa",
                "body": { "title": "Updated existing", "views": 99 }
            },
            {
                "method": "DELETE",
                "url": "/api/collections/posts/records/to_delete_aaaaa"
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let json: Value = resp.json().await.unwrap();
    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);

    // Create succeeded.
    assert_eq!(results[0]["status"], 200);
    assert_eq!(results[0]["body"]["title"], "Brand new");

    // Update succeeded.
    assert_eq!(results[1]["status"], 200);
    assert_eq!(results[1]["body"]["title"], "Updated existing");
    assert_eq!(results[1]["body"]["views"], 99);

    // Delete succeeded.
    assert_eq!(results[2]["status"], 204);
}

#[tokio::test]
async fn batch_rollback_on_failure_deletes_created_records() {
    // Start with one existing record. The batch will:
    // 1. Create a new record (succeeds)
    // 2. Update a non-existent record (fails) → triggers rollback
    let records = vec![make_record("existing1aaaaaa", "Existing", "draft", 0)];
    let (addr, _handle) = spawn_posts_batch_app(records).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "POST",
                "url": "/api/collections/posts/records",
                "body": { "title": "Will be rolled back", "status": "draft", "views": 0 }
            },
            {
                "method": "PATCH",
                "url": "/api/collections/posts/records/nonexistent",
                "body": { "title": "This will fail" }
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    // Batch should fail.
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let json: Value = resp.json().await.unwrap();
    assert!(json["message"].as_str().unwrap().contains("batch failed at operation 1"));
}

#[tokio::test]
async fn batch_empty_request_returns_400() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    let body = json!({ "requests": [] });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json: Value = resp.json().await.unwrap();
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("at least one operation"));
}

#[tokio::test]
async fn batch_exceeds_max_size_returns_400() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    // Create 51 operations (exceeds MAX_BATCH_SIZE of 50).
    let ops: Vec<Value> = (0..51)
        .map(|i| {
            json!({
                "method": "POST",
                "url": "/api/collections/posts/records",
                "body": { "title": format!("Post {i}"), "status": "draft", "views": 0 }
            })
        })
        .collect();

    let body = json!({ "requests": ops });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json: Value = resp.json().await.unwrap();
    assert!(json["message"].as_str().unwrap().contains("maximum"));
}

#[tokio::test]
async fn batch_invalid_method_returns_400() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "GET",
                "url": "/api/collections/posts/records"
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json: Value = resp.json().await.unwrap();
    assert!(json["message"].as_str().unwrap().contains("unsupported"));
}

#[tokio::test]
async fn batch_invalid_url_returns_400() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "POST",
                "url": "/invalid/path",
                "body": { "title": "test" }
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn batch_nonexistent_collection_returns_error() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "POST",
                "url": "/api/collections/nonexistent/records",
                "body": { "title": "test" }
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json: Value = resp.json().await.unwrap();
    assert!(json["message"].as_str().unwrap().contains("batch failed"));
}

#[tokio::test]
async fn batch_post_without_body_returns_400() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "POST",
                "url": "/api/collections/posts/records"
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn batch_patch_without_body_returns_400() {
    let records = vec![make_record("rec1ccccccccccc", "Title", "draft", 0)];
    let (addr, _handle) = spawn_posts_batch_app(records).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "PATCH",
                "url": "/api/collections/posts/records/rec1ccccccccccc"
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn batch_post_with_record_id_in_url_returns_400() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "POST",
                "url": "/api/collections/posts/records/some_id",
                "body": { "title": "test" }
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json: Value = resp.json().await.unwrap();
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("should not include a record ID"));
}

#[tokio::test]
async fn batch_delete_without_record_id_returns_400() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "DELETE",
                "url": "/api/collections/posts/records"
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json: Value = resp.json().await.unwrap();
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("require a record ID"));
}

#[tokio::test]
async fn batch_with_locked_rules_requires_superuser() {
    // Create a collection with locked (null) create_rule.
    let mut col = Collection::base("posts", posts_fields());
    col.rules = ApiRules::locked();
    let schema = MockSchemaLookup::with(vec![col]);
    let repo = MockRecordRepo::new();
    let (addr, _handle) = spawn_batch_app(schema, repo).await;
    let client = reqwest::Client::new();

    // Anonymous request should fail.
    let body = json!({
        "requests": [
            {
                "method": "POST",
                "url": "/api/collections/posts/records",
                "body": { "title": "test", "status": "draft", "views": 0 }
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    // Should fail due to locked rule.
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Superuser request should succeed.
    let resp = client
        .post(format!("{addr}/api/batch"))
        .header("Authorization", "Bearer SUPERUSER")
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn batch_create_and_verify_records_persisted() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    // Create two records via batch.
    let batch_body = json!({
        "requests": [
            {
                "method": "POST",
                "url": "/api/collections/posts/records",
                "body": { "title": "Batch Post 1", "status": "published", "views": 10 }
            },
            {
                "method": "POST",
                "url": "/api/collections/posts/records",
                "body": { "title": "Batch Post 2", "status": "draft", "views": 0 }
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&batch_body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = resp.json().await.unwrap();
    let results = json["results"].as_array().unwrap();

    // Extract IDs from batch response.
    let id1 = results[0]["body"]["id"].as_str().unwrap();
    let id2 = results[1]["body"]["id"].as_str().unwrap();

    // Verify records exist via individual GET requests.
    let get1 = client
        .get(format!("{addr}/api/collections/posts/records/{id1}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get1.status(), StatusCode::OK);
    let record1: Value = get1.json().await.unwrap();
    assert_eq!(record1["title"], "Batch Post 1");

    let get2 = client
        .get(format!("{addr}/api/collections/posts/records/{id2}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get2.status(), StatusCode::OK);
    let record2: Value = get2.json().await.unwrap();
    assert_eq!(record2["title"], "Batch Post 2");
}

#[tokio::test]
async fn batch_response_includes_collection_metadata() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "POST",
                "url": "/api/collections/posts/records",
                "body": { "title": "Metadata test", "status": "draft", "views": 0 }
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = resp.json().await.unwrap();
    let result = &json["results"][0]["body"];

    // RecordResponse includes collection metadata.
    assert_eq!(result["collectionName"], "posts");
    assert!(result["collectionId"].is_string());
}

#[tokio::test]
async fn batch_across_multiple_collections() {
    // Set up two collections: posts and comments.
    let comments_fields = vec![Field::new(
        "text",
        FieldType::Text(TextOptions {
            min_length: 0,
            max_length: 500,
            pattern: None,
            searchable: false,
        }),
    )];
    let mut comments_col = Collection::base("comments", comments_fields);
    comments_col.rules = ApiRules::open();

    let schema = MockSchemaLookup::with(vec![make_posts_collection(), comments_col]);
    let repo = MockRecordRepo::new();
    let (addr, _handle) = spawn_batch_app(schema, repo).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "POST",
                "url": "/api/collections/posts/records",
                "body": { "title": "A post", "status": "published", "views": 0 }
            },
            {
                "method": "POST",
                "url": "/api/collections/comments/records",
                "body": { "text": "A comment" }
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let json: Value = resp.json().await.unwrap();
    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);

    assert_eq!(results[0]["body"]["collectionName"], "posts");
    assert_eq!(results[0]["body"]["title"], "A post");

    assert_eq!(results[1]["body"]["collectionName"], "comments");
    assert_eq!(results[1]["body"]["text"], "A comment");
}

#[tokio::test]
async fn batch_update_nonexistent_record_fails() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "PATCH",
                "url": "/api/collections/posts/records/ghost",
                "body": { "title": "nope" }
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json: Value = resp.json().await.unwrap();
    assert!(json["message"].as_str().unwrap().contains("batch failed"));
}

#[tokio::test]
async fn batch_delete_nonexistent_record_fails() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "DELETE",
                "url": "/api/collections/posts/records/ghost"
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn batch_case_insensitive_method() {
    let (addr, _handle) = spawn_posts_batch_app(vec![]).await;
    let client = reqwest::Client::new();

    let body = json!({
        "requests": [
            {
                "method": "post",
                "url": "/api/collections/posts/records",
                "body": { "title": "lowercase method", "status": "draft", "views": 0 }
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/batch"))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["results"][0]["body"]["title"], "lowercase method");
}
