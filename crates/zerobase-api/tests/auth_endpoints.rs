//! Integration tests for the auth-with-password endpoint.
//!
//! Tests exercise the full HTTP stack (router → handler → service → mock repo)
//! using in-memory mocks. Each test spawns an isolated server on a random port.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use zerobase_core::auth::{PasswordHasher, TokenService, TokenType, ValidatedToken};
use zerobase_core::error::Result;
use zerobase_core::schema::Collection;
use zerobase_core::services::record_service::{
    RecordList, RecordQuery, RecordRepoError, RecordRepository, RecordService, SchemaLookup,
};
use zerobase_core::ZerobaseError;

// ── Mock: SchemaLookup ──────────────────────────────────────────────────────

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
            .ok_or_else(|| ZerobaseError::not_found_with_id("Collection", name))
    }
}

// ── Mock: RecordRepository ──────────────────────────────────────────────────

struct MockRecordRepo {
    records: Mutex<HashMap<String, Vec<HashMap<String, Value>>>>,
}

impl MockRecordRepo {
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
        _collection: &str,
        _id: &str,
        _data: &HashMap<String, Value>,
    ) -> std::result::Result<bool, RecordRepoError> {
        Ok(false)
    }

    fn delete(&self, _collection: &str, _id: &str) -> std::result::Result<bool, RecordRepoError> {
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
        _collection: &str,
        _field_name: &str,
        _referenced_id: &str,
    ) -> std::result::Result<Vec<HashMap<String, Value>>, RecordRepoError> {
        Ok(Vec::new())
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

// ── Mock: PasswordHasher ────────────────────────────────────────────────────

/// Simple test hasher: hash is `hashed:<plain>`, verify checks equality.
struct TestHasher;

impl PasswordHasher for TestHasher {
    fn hash(&self, plain: &str) -> Result<String> {
        Ok(format!("hashed:{plain}"))
    }

    fn verify(&self, plain: &str, hash: &str) -> Result<bool> {
        Ok(hash == format!("hashed:{plain}"))
    }
}

// ── Mock: TokenService ──────────────────────────────────────────────────────

/// Simple test token service: generates `test-token:<user_id>:<collection_id>`.
struct TestTokenService;

impl TokenService for TestTokenService {
    fn generate(
        &self,
        user_id: &str,
        collection_id: &str,
        _token_type: TokenType,
        _token_key: &str,
        _duration_secs: Option<u64>,
    ) -> std::result::Result<String, ZerobaseError> {
        Ok(format!("test-token:{user_id}:{collection_id}"))
    }

    fn validate(
        &self,
        _token: &str,
        _expected_type: TokenType,
    ) -> std::result::Result<ValidatedToken, ZerobaseError> {
        Err(ZerobaseError::auth("not implemented in test"))
    }
}

// ── Test helpers ────────────────────────────────────────────────────────────

fn make_users_collection() -> Collection {
    Collection::auth("users", vec![])
}

fn make_user_record(id: &str, email: &str, password: &str) -> HashMap<String, Value> {
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!(id));
    record.insert("email".to_string(), json!(email));
    record.insert("password".to_string(), json!(format!("hashed:{password}")));
    record.insert("tokenKey".to_string(), json!("random_token_key_12345"));
    record.insert("emailVisibility".to_string(), json!(false));
    record.insert("verified".to_string(), json!(true));
    record.insert("created".to_string(), json!("2025-01-01T00:00:00Z"));
    record.insert("updated".to_string(), json!("2025-01-01T00:00:00Z"));
    record
}

async fn spawn_auth_app(
    collections: Vec<Collection>,
    collection_name: &str,
    records: Vec<HashMap<String, Value>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let schema = MockSchemaLookup::with(collections);
    let repo = MockRecordRepo::with_records(collection_name, records);
    let record_service = Arc::new(RecordService::with_password_hasher(
        repo, schema, TestHasher,
    ));
    let token_service: Arc<dyn TokenService> = Arc::new(TestTokenService);

    let app =
        zerobase_api::api_router().merge(zerobase_api::auth_routes(record_service, token_service));

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

// ── Tests ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn auth_with_password_returns_token_and_record() {
    let user = make_user_record("abc123456789012", "test@example.com", "secret123");
    let (addr, _handle) = spawn_auth_app(vec![make_users_collection()], "users", vec![user]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-password"))
        .json(&json!({
            "identity": "test@example.com",
            "password": "secret123"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert!(body["token"].is_string(), "response should contain token");
    assert!(!body["token"].as_str().unwrap().is_empty());
    assert!(body["record"].is_object(), "response should contain record");
    assert_eq!(body["record"]["email"], "test@example.com");
    assert_eq!(body["record"]["id"], "abc123456789012");
    assert_eq!(body["record"]["collectionName"], "users");
    // Password and tokenKey should not be in the response.
    assert!(
        body["record"]["password"].is_null(),
        "password should be stripped"
    );
    assert!(
        body["record"]["tokenKey"].is_null(),
        "tokenKey should be stripped"
    );
}

#[tokio::test]
async fn auth_with_password_wrong_password_returns_400() {
    let user = make_user_record("abc123456789012", "test@example.com", "secret123");
    let (addr, _handle) = spawn_auth_app(vec![make_users_collection()], "users", vec![user]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-password"))
        .json(&json!({
            "identity": "test@example.com",
            "password": "wrong_password"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], 400);
    assert!(body["message"].as_str().unwrap().contains("authenticate"));
}

#[tokio::test]
async fn auth_with_password_unknown_email_returns_400() {
    let user = make_user_record("abc123456789012", "test@example.com", "secret123");
    let (addr, _handle) = spawn_auth_app(vec![make_users_collection()], "users", vec![user]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-password"))
        .json(&json!({
            "identity": "unknown@example.com",
            "password": "secret123"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], 400);
}

#[tokio::test]
async fn auth_with_password_non_auth_collection_returns_400() {
    let base_collection = Collection::base("posts", vec![]);
    let (addr, _handle) = spawn_auth_app(vec![base_collection], "posts", vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/posts/auth-with-password"))
        .json(&json!({
            "identity": "test@example.com",
            "password": "secret123"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn auth_with_password_nonexistent_collection_returns_404() {
    let (addr, _handle) = spawn_auth_app(vec![make_users_collection()], "users", vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "{addr}/api/collections/nonexistent/auth-with-password"
        ))
        .json(&json!({
            "identity": "test@example.com",
            "password": "secret123"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn auth_with_password_empty_identity_returns_400() {
    let user = make_user_record("abc123456789012", "test@example.com", "secret123");
    let (addr, _handle) = spawn_auth_app(vec![make_users_collection()], "users", vec![user]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-password"))
        .json(&json!({
            "identity": "",
            "password": "secret123"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn auth_with_password_empty_password_returns_400() {
    let user = make_user_record("abc123456789012", "test@example.com", "secret123");
    let (addr, _handle) = spawn_auth_app(vec![make_users_collection()], "users", vec![user]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-password"))
        .json(&json!({
            "identity": "test@example.com",
            "password": ""
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn auth_with_password_token_contains_user_id() {
    let user = make_user_record("abc123456789012", "test@example.com", "secret123");
    let collection = make_users_collection();
    let collection_id = collection.id.clone();

    let (addr, _handle) = spawn_auth_app(vec![collection], "users", vec![user]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-password"))
        .json(&json!({
            "identity": "test@example.com",
            "password": "secret123"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let token = body["token"].as_str().unwrap();
    // Our test token service generates `test-token:<user_id>:<collection_id>`.
    assert!(
        token.starts_with("test-token:abc123456789012:"),
        "token should contain user ID"
    );
    assert!(
        token.contains(&collection_id),
        "token should contain collection ID"
    );
}
