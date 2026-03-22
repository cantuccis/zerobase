//! Integration tests for the auth-refresh endpoint.
//!
//! Tests exercise the full HTTP stack: auth middleware → auth-refresh handler.
//! Each test spawns an isolated server on a random port with in-memory mocks.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use zerobase_core::auth::{PasswordHasher, TokenClaims, TokenService, TokenType, ValidatedToken};
use zerobase_core::error::Result;
use zerobase_core::schema::Collection;
use zerobase_core::services::record_service::{
    RecordList, RecordQuery, RecordRepoError, RecordRepository, RecordService, SchemaLookup,
};
use zerobase_core::ZerobaseError;

use zerobase_api::AuthMiddlewareState;

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

    fn get_collection_by_id(&self, id: &str) -> Result<Collection> {
        self.collections
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .ok_or_else(|| ZerobaseError::not_found_with_id("Collection", id))
    }
}

// ── Mock: RecordRepository ──────────────────────────────────────────────────

struct MockRecordRepo {
    records: Mutex<HashMap<String, Vec<HashMap<String, Value>>>>,
}

impl MockRecordRepo {
    fn with_records(data: HashMap<String, Vec<HashMap<String, Value>>>) -> Self {
        Self {
            records: Mutex::new(data),
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

/// Test token service that:
/// - `generate`: returns `test-token:<user_id>:<collection_id>`
/// - `validate`: parses `valid:<user_id>:<collection_id>:<token_key>` format
/// - Tokens starting with `expired:` simulate expired tokens
struct MockTokenService;

impl TokenService for MockTokenService {
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
        token: &str,
        _expected_type: TokenType,
    ) -> std::result::Result<ValidatedToken, ZerobaseError> {
        if token.starts_with("expired:") {
            return Err(ZerobaseError::auth("token expired"));
        }

        // Format: valid:<user_id>:<collection_id>:<token_key>
        if let Some(rest) = token.strip_prefix("valid:") {
            let parts: Vec<&str> = rest.splitn(3, ':').collect();
            if parts.len() == 3 {
                return Ok(ValidatedToken {
                    claims: TokenClaims {
                        id: parts[0].to_string(),
                        collection_id: parts[1].to_string(),
                        token_type: TokenType::Auth,
                        token_key: parts[2].to_string(),
                        new_email: None,
                        iat: 0,
                        exp: u64::MAX,
                    },
                });
            }
        }

        Err(ZerobaseError::auth("invalid token"))
    }
}

// ── Test helpers ────────────────────────────────────────────────────────────

fn make_users_collection() -> Collection {
    Collection::auth("users", vec![])
}

fn make_user_record(id: &str, email: &str, token_key: &str) -> HashMap<String, Value> {
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!(id));
    record.insert("email".to_string(), json!(email));
    record.insert("password".to_string(), json!(format!("hashed:secret")));
    record.insert("tokenKey".to_string(), json!(token_key));
    record.insert("emailVisibility".to_string(), json!(false));
    record.insert("verified".to_string(), json!(true));
    record.insert("created".to_string(), json!("2025-01-01T00:00:00Z"));
    record.insert("updated".to_string(), json!("2025-01-01T00:00:00Z"));
    record
}

/// Spawn a test server with auth middleware + auth routes (including auth-refresh).
async fn spawn_app(
    collections: Vec<Collection>,
    records: HashMap<String, Vec<HashMap<String, Value>>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let schema = Arc::new(MockSchemaLookup::with(collections));
    let repo = Arc::new(MockRecordRepo::with_records(records));
    let token_service: Arc<dyn TokenService> = Arc::new(MockTokenService);

    let record_service = Arc::new(RecordService::with_password_hasher(
        // We need a RecordService that uses the same repo and schema.
        // Since RecordService owns its repo/schema, we need separate instances
        // that share the same underlying data.
        MockRecordRepo::with_records(repo.records.lock().unwrap().clone()),
        MockSchemaLookup::with(schema.collections.lock().unwrap().clone()),
        TestHasher,
    ));

    let auth_middleware_state = Arc::new(AuthMiddlewareState {
        token_service: Arc::clone(&token_service),
        record_repo: repo,
        schema_lookup: schema,
    });

    // Build auth routes, then apply auth middleware so RequireAuth works.
    let app = zerobase_api::auth_routes(record_service, token_service).layer(
        axum::middleware::from_fn_with_state(
            auth_middleware_state,
            zerobase_api::middleware::auth_context::auth_middleware::<
                MockRecordRepo,
                MockSchemaLookup,
            >,
        ),
    );

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
async fn auth_refresh_returns_new_token_and_record() {
    let collection = make_users_collection();
    let collection_id = collection.id.clone();
    let user = make_user_record("user123", "test@example.com", "tk_key1");

    let mut records = HashMap::new();
    records.insert("users".to_string(), vec![user]);

    let (addr, _handle) = spawn_app(vec![collection], records).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-refresh"))
        .header(
            "Authorization",
            format!("Bearer valid:user123:{collection_id}:tk_key1"),
        )
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert!(body["token"].is_string(), "response should contain token");
    assert!(!body["token"].as_str().unwrap().is_empty());
    assert!(body["record"].is_object(), "response should contain record");
    assert_eq!(body["record"]["email"], "test@example.com");
    assert_eq!(body["record"]["id"], "user123");
    assert_eq!(body["record"]["collectionName"], "users");
    // Sensitive fields should be stripped.
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
async fn auth_refresh_without_token_returns_401() {
    let collection = make_users_collection();
    let user = make_user_record("user123", "test@example.com", "tk_key1");

    let mut records = HashMap::new();
    records.insert("users".to_string(), vec![user]);

    let (addr, _handle) = spawn_app(vec![collection], records).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-refresh"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_refresh_with_expired_token_returns_401() {
    let collection = make_users_collection();
    let user = make_user_record("user123", "test@example.com", "tk_key1");

    let mut records = HashMap::new();
    records.insert("users".to_string(), vec![user]);

    let (addr, _handle) = spawn_app(vec![collection], records).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-refresh"))
        .header("Authorization", "Bearer expired:user123:col_abc:tk_key1")
        .send()
        .await
        .unwrap();

    // Expired tokens are rejected by the middleware → anonymous → RequireAuth → 401
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_refresh_with_invalid_token_returns_401() {
    let collection = make_users_collection();
    let user = make_user_record("user123", "test@example.com", "tk_key1");

    let mut records = HashMap::new();
    records.insert("users".to_string(), vec![user]);

    let (addr, _handle) = spawn_app(vec![collection], records).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-refresh"))
        .header("Authorization", "Bearer garbage-token-value")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_refresh_with_changed_token_key_returns_401() {
    // The user's tokenKey in the database has been rotated since the token was issued.
    let collection = make_users_collection();
    let collection_id = collection.id.clone();

    // User record has token_key "new_key", but token was issued with "old_key"
    let user = make_user_record("user123", "test@example.com", "new_key");

    let mut records = HashMap::new();
    records.insert("users".to_string(), vec![user]);

    let (addr, _handle) = spawn_app(vec![collection], records).await;

    let client = reqwest::Client::new();
    // Token has token_key "old_key" but the database record has "new_key"
    // The middleware checks tokenKey and rejects mismatches → anonymous → 401
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-refresh"))
        .header(
            "Authorization",
            format!("Bearer valid:user123:{collection_id}:old_key"),
        )
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_refresh_for_nonexistent_collection_returns_401() {
    // Token references a valid user but against a collection that doesn't exist
    // in the path. The middleware might authenticate, but collection lookup fails.
    let collection = make_users_collection();
    let collection_id = collection.id.clone();
    let user = make_user_record("user123", "test@example.com", "tk_key1");

    let mut records = HashMap::new();
    records.insert("users".to_string(), vec![user]);

    let (addr, _handle) = spawn_app(vec![collection], records).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/nonexistent/auth-refresh"))
        .header(
            "Authorization",
            format!("Bearer valid:user123:{collection_id}:tk_key1"),
        )
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn auth_refresh_collection_mismatch_returns_400() {
    // Token was issued for "users" collection but request is against "other" collection.
    let users_collection = make_users_collection();
    let users_collection_id = users_collection.id.clone();
    let other_collection = Collection::auth("other", vec![]);

    let user = make_user_record("user123", "test@example.com", "tk_key1");

    let mut records = HashMap::new();
    records.insert("users".to_string(), vec![user.clone()]);
    records.insert("other".to_string(), vec![]);

    let (addr, _handle) = spawn_app(vec![users_collection, other_collection], records).await;

    let client = reqwest::Client::new();
    // Token has users_collection_id but we're hitting /other/auth-refresh
    let resp = client
        .post(format!("{addr}/api/collections/other/auth-refresh"))
        .header(
            "Authorization",
            format!("Bearer valid:user123:{users_collection_id}:tk_key1"),
        )
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn auth_refresh_returns_fresh_token_with_correct_format() {
    let collection = make_users_collection();
    let collection_id = collection.id.clone();
    let user = make_user_record("user456", "refresh@example.com", "tk_abc");

    let mut records = HashMap::new();
    records.insert("users".to_string(), vec![user]);

    let (addr, _handle) = spawn_app(vec![collection], records).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-refresh"))
        .header(
            "Authorization",
            format!("Bearer valid:user456:{collection_id}:tk_abc"),
        )
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let token = body["token"].as_str().unwrap();
    // Our MockTokenService returns `test-token:<user_id>:<collection_id>`
    assert!(
        token.starts_with("test-token:user456:"),
        "new token should contain user ID: got {token}"
    );
    assert!(
        token.contains(&collection_id),
        "new token should contain collection ID"
    );
}

#[tokio::test]
async fn auth_refresh_returns_latest_record_data() {
    let collection = make_users_collection();
    let collection_id = collection.id.clone();

    // User record with specific field values
    let mut user = make_user_record("user789", "latest@example.com", "tk_xyz");
    user.insert("name".to_string(), json!("Latest Name"));

    let mut records = HashMap::new();
    records.insert("users".to_string(), vec![user]);

    let (addr, _handle) = spawn_app(vec![collection], records).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-refresh"))
        .header(
            "Authorization",
            format!("Bearer valid:user789:{collection_id}:tk_xyz"),
        )
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    // Record should contain the latest data from the database.
    assert_eq!(body["record"]["id"], "user789");
    assert_eq!(body["record"]["email"], "latest@example.com");
    assert_eq!(body["record"]["name"], "Latest Name");
    assert_eq!(body["record"]["collectionId"], collection_id);
}
