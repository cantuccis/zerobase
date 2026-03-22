//! Integration tests for the JWT auth middleware.
//!
//! Tests exercise the auth middleware pipeline: JWT validation → record loading
//! → tokenKey checking → AuthInfo injection into request extensions.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use zerobase_core::auth::{TokenClaims, TokenService, TokenType, ValidatedToken};
use zerobase_core::error::Result;
use zerobase_core::schema::Collection;
use zerobase_core::services::record_service::{
    RecordList, RecordQuery, RecordRepoError, RecordRepository, SchemaLookup,
};
use zerobase_core::ZerobaseError;

use zerobase_api::{AuthInfo, AuthMiddlewareState};

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
        _collection: &str,
        _query: &RecordQuery,
    ) -> std::result::Result<RecordList, RecordRepoError> {
        Ok(RecordList {
            items: vec![],
            total_items: 0,
            page: 1,
            per_page: 20,
            total_pages: 1,
        })
    }

    fn insert(
        &self,
        _collection: &str,
        _data: &HashMap<String, Value>,
    ) -> std::result::Result<(), RecordRepoError> {
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
        _collection: &str,
        _filter: Option<&str>,
    ) -> std::result::Result<u64, RecordRepoError> {
        Ok(0)
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

// ── Mock: TokenService ──────────────────────────────────────────────────────

/// Configurable test token service.
///
/// - `validate` parses tokens in the format `valid:<user_id>:<collection_id>:<token_key>`
///   and returns matching claims.
/// - Tokens starting with `expired:` return an error (simulates expired JWTs).
/// - Any other token returns an error (invalid).
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

fn make_superusers_collection() -> Collection {
    // The _superusers collection has a fixed name that the auth middleware checks.
    Collection::base("_superusers", vec![])
}

fn make_user_record(id: &str, email: &str, token_key: &str) -> HashMap<String, Value> {
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!(id));
    record.insert("email".to_string(), json!(email));
    record.insert("password".to_string(), json!("hashed:secret"));
    record.insert("tokenKey".to_string(), json!(token_key));
    record.insert("verified".to_string(), json!(true));
    record
}

fn make_superuser_record(id: &str, email: &str) -> HashMap<String, Value> {
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!(id));
    record.insert("email".to_string(), json!(email));
    record.insert("password".to_string(), json!("hashed:admin"));
    // _superusers have no tokenKey column
    record
}

/// Handler that returns the AuthInfo as JSON for testing.
async fn auth_info_handler(auth: AuthInfo) -> axum::Json<Value> {
    axum::Json(json!({
        "is_superuser": auth.is_superuser,
        "is_authenticated": auth.is_authenticated(),
        "auth_record": auth.auth_record,
        "has_token": auth.token.is_some(),
    }))
}

/// Handler that requires auth (returns 401 for unauthenticated).
async fn require_auth_handler(auth: zerobase_api::RequireAuth) -> axum::Json<Value> {
    axum::Json(json!({
        "is_superuser": auth.is_superuser,
        "auth_record": auth.auth_record,
    }))
}

async fn spawn_auth_middleware_app(
    collections: Vec<Collection>,
    records: HashMap<String, Vec<HashMap<String, Value>>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let schema = Arc::new(MockSchemaLookup::with(collections));
    let repo = Arc::new(MockRecordRepo::with_records(records));
    let token_service: Arc<dyn TokenService> = Arc::new(MockTokenService);

    let auth_state = Arc::new(AuthMiddlewareState {
        token_service,
        record_repo: repo,
        schema_lookup: schema,
    });

    use axum::routing::get;

    // Build test routes and health route together, then apply auth middleware
    // to all of them so AuthInfo is available in every handler.
    let app = axum::Router::new()
        .route(
            "/api/health",
            get(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
        )
        .route("/test/auth-info", get(auth_info_handler))
        .route("/test/require-auth", get(require_auth_handler))
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            zerobase_api::middleware::auth_context::auth_middleware::<
                MockRecordRepo,
                MockSchemaLookup,
            >,
        ));

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
async fn no_auth_header_returns_anonymous() {
    let (addr, _handle) = spawn_auth_middleware_app(vec![], HashMap::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/test/auth-info"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_superuser"], false);
    assert_eq!(body["is_authenticated"], false);
    assert_eq!(body["has_token"], false);
}

#[tokio::test]
async fn empty_bearer_returns_anonymous() {
    let (addr, _handle) = spawn_auth_middleware_app(vec![], HashMap::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/test/auth-info"))
        .header("Authorization", "Bearer ")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_authenticated"], false);
}

#[tokio::test]
async fn invalid_token_returns_anonymous() {
    let (addr, _handle) = spawn_auth_middleware_app(vec![], HashMap::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/test/auth-info"))
        .header("Authorization", "Bearer garbage-token")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_authenticated"], false);
    assert_eq!(body["has_token"], false);
}

#[tokio::test]
async fn expired_token_returns_anonymous() {
    let (addr, _handle) = spawn_auth_middleware_app(vec![], HashMap::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/test/auth-info"))
        .header("Authorization", "Bearer expired:user1:col1:key1")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_authenticated"], false);
}

#[tokio::test]
async fn valid_token_authenticates_user() {
    let users_col = make_users_collection();
    let col_id = users_col.id.clone();
    let user = make_user_record("user1", "alice@example.com", "mykey");

    let mut records = HashMap::new();
    records.insert("users".to_string(), vec![user]);

    let (addr, _handle) = spawn_auth_middleware_app(vec![users_col], records).await;
    let client = reqwest::Client::new();

    let token = format!("valid:user1:{col_id}:mykey");
    let resp = client
        .get(format!("{addr}/test/auth-info"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_authenticated"], true);
    assert_eq!(body["is_superuser"], false);
    assert_eq!(body["has_token"], true);
    assert_eq!(body["auth_record"]["id"], "user1");
    assert_eq!(body["auth_record"]["email"], "alice@example.com");
    // Password and tokenKey should be stripped.
    assert!(body["auth_record"]["password"].is_null());
    assert!(body["auth_record"]["tokenKey"].is_null());
    // Collection context should be injected.
    assert_eq!(body["auth_record"]["collectionName"], "users");
    assert!(body["auth_record"]["collectionId"].is_string());
}

#[tokio::test]
async fn superuser_token_sets_is_superuser() {
    let su_col = make_superusers_collection();
    let col_id = su_col.id.clone();
    let admin = make_superuser_record("admin1", "admin@example.com");

    let mut records = HashMap::new();
    records.insert("_superusers".to_string(), vec![admin]);

    let (addr, _handle) = spawn_auth_middleware_app(vec![su_col], records).await;
    let client = reqwest::Client::new();

    // _superusers have no tokenKey, so any token_key value should work.
    let token = format!("valid:admin1:{col_id}:anykey");
    let resp = client
        .get(format!("{addr}/test/auth-info"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_authenticated"], true);
    assert_eq!(body["is_superuser"], true);
    assert_eq!(body["has_token"], true);
}

#[tokio::test]
async fn revoked_token_key_returns_anonymous() {
    let users_col = make_users_collection();
    let col_id = users_col.id.clone();
    // User's stored tokenKey is "current-key"
    let user = make_user_record("user1", "alice@example.com", "current-key");

    let mut records = HashMap::new();
    records.insert("users".to_string(), vec![user]);

    let (addr, _handle) = spawn_auth_middleware_app(vec![users_col], records).await;
    let client = reqwest::Client::new();

    // Token claims have old/wrong tokenKey
    let token = format!("valid:user1:{col_id}:old-revoked-key");
    let resp = client
        .get(format!("{addr}/test/auth-info"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_authenticated"], false);
    assert_eq!(body["has_token"], false);
}

#[tokio::test]
async fn token_for_nonexistent_user_returns_anonymous() {
    let users_col = make_users_collection();
    let col_id = users_col.id.clone();

    let (addr, _handle) = spawn_auth_middleware_app(vec![users_col], HashMap::new()).await;
    let client = reqwest::Client::new();

    let token = format!("valid:ghost:{col_id}:key");
    let resp = client
        .get(format!("{addr}/test/auth-info"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_authenticated"], false);
}

#[tokio::test]
async fn token_for_nonexistent_collection_returns_anonymous() {
    let (addr, _handle) = spawn_auth_middleware_app(vec![], HashMap::new()).await;
    let client = reqwest::Client::new();

    let token = "valid:user1:no-such-collection-id:key";
    let resp = client
        .get(format!("{addr}/test/auth-info"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_authenticated"], false);
}

#[tokio::test]
async fn require_auth_returns_401_for_anonymous() {
    let (addr, _handle) = spawn_auth_middleware_app(vec![], HashMap::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/test/require-auth"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], 401);
}

#[tokio::test]
async fn require_auth_passes_for_valid_user() {
    let users_col = make_users_collection();
    let col_id = users_col.id.clone();
    let user = make_user_record("user1", "alice@example.com", "mykey");

    let mut records = HashMap::new();
    records.insert("users".to_string(), vec![user]);

    let (addr, _handle) = spawn_auth_middleware_app(vec![users_col], records).await;
    let client = reqwest::Client::new();

    let token = format!("valid:user1:{col_id}:mykey");
    let resp = client
        .get(format!("{addr}/test/require-auth"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_superuser"], false);
    assert_eq!(body["auth_record"]["id"], "user1");
}

#[tokio::test]
async fn require_auth_returns_401_for_invalid_token() {
    let (addr, _handle) = spawn_auth_middleware_app(vec![], HashMap::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/test/require-auth"))
        .header("Authorization", "Bearer garbage")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn case_insensitive_bearer_prefix() {
    let users_col = make_users_collection();
    let col_id = users_col.id.clone();
    let user = make_user_record("user1", "alice@example.com", "key1");

    let mut records = HashMap::new();
    records.insert("users".to_string(), vec![user]);

    let (addr, _handle) = spawn_auth_middleware_app(vec![users_col], records).await;
    let client = reqwest::Client::new();

    // Use lowercase "bearer" prefix.
    let token = format!("valid:user1:{col_id}:key1");
    let resp = client
        .get(format!("{addr}/test/auth-info"))
        .header("Authorization", format!("bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_authenticated"], true);
}

#[tokio::test]
async fn raw_token_without_bearer_prefix() {
    let users_col = make_users_collection();
    let col_id = users_col.id.clone();
    let user = make_user_record("user1", "alice@example.com", "key1");

    let mut records = HashMap::new();
    records.insert("users".to_string(), vec![user]);

    let (addr, _handle) = spawn_auth_middleware_app(vec![users_col], records).await;
    let client = reqwest::Client::new();

    // Pass raw token without "Bearer " prefix — should still work.
    let token = format!("valid:user1:{col_id}:key1");
    let resp = client
        .get(format!("{addr}/test/auth-info"))
        .header("Authorization", &token)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_authenticated"], true);
}

#[tokio::test]
async fn health_check_works_with_auth_middleware() {
    let (addr, _handle) = spawn_auth_middleware_app(vec![], HashMap::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/health"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}
