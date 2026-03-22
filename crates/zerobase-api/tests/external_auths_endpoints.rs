//! Integration tests for external auth identity endpoints.
//!
//! Tests exercise the full HTTP stack (router → handler → mock repo) with
//! the auth middleware for authorization checks.
//!
//! Endpoints under test:
//! - `GET    /api/collections/:collection/records/:id/external-auths`
//! - `DELETE /api/collections/:collection/records/:id/external-auths/:provider`

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use zerobase_core::auth::{TokenClaims, TokenService, TokenType, ValidatedToken};
use zerobase_core::error::Result;
use zerobase_core::schema::Collection;
use zerobase_core::services::external_auth::{ExternalAuth, ExternalAuthRepository};
use zerobase_core::services::record_service::{
    RecordList, RecordQuery, RecordRepoError, RecordRepository, SchemaLookup,
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
    fn with_records(collection: &str, records: Vec<HashMap<String, Value>>) -> Self {
        let mut map = HashMap::new();
        map.insert(collection.to_string(), records);
        Self {
            records: Mutex::new(map),
        }
    }

    fn with_data(data: HashMap<String, Vec<HashMap<String, Value>>>) -> Self {
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

// ── Mock: ExternalAuthRepository ────────────────────────────────────────────

struct MockExternalAuthRepo {
    auths: Mutex<Vec<ExternalAuth>>,
}

impl MockExternalAuthRepo {
    fn new() -> Self {
        Self {
            auths: Mutex::new(Vec::new()),
        }
    }

    fn with_auths(auths: Vec<ExternalAuth>) -> Self {
        Self {
            auths: Mutex::new(auths),
        }
    }
}

impl ExternalAuthRepository for MockExternalAuthRepo {
    fn find_by_provider(&self, provider: &str, provider_id: &str) -> Result<Option<ExternalAuth>> {
        let auths = self.auths.lock().unwrap();
        Ok(auths
            .iter()
            .find(|a| a.provider == provider && a.provider_id == provider_id)
            .cloned())
    }

    fn find_by_record(&self, collection_id: &str, record_id: &str) -> Result<Vec<ExternalAuth>> {
        let auths = self.auths.lock().unwrap();
        Ok(auths
            .iter()
            .filter(|a| a.collection_id == collection_id && a.record_id == record_id)
            .cloned()
            .collect())
    }

    fn create(&self, auth: &ExternalAuth) -> Result<()> {
        let mut auths = self.auths.lock().unwrap();
        auths.push(auth.clone());
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<()> {
        let mut auths = self.auths.lock().unwrap();
        auths.retain(|a| a.id != id);
        Ok(())
    }

    fn delete_by_record(&self, collection_id: &str, record_id: &str) -> Result<()> {
        let mut auths = self.auths.lock().unwrap();
        auths.retain(|a| !(a.collection_id == collection_id && a.record_id == record_id));
        Ok(())
    }
}

// ── Test helpers ────────────────────────────────────────────────────────────

const TEST_COLLECTION_ID: &str = "test_col_id_001";
const USER_ID: &str = "user123456789012";
const OTHER_USER_ID: &str = "other12345678901";
const SUPERUSER_ID: &str = "admin12345678901";
const SUPERUSER_COL_ID: &str = "superuser_col_01";
const TOKEN_KEY: &str = "tk_abc123";

fn make_auth_collection() -> Collection {
    let mut collection = Collection::auth("users", vec![]);
    collection.id = TEST_COLLECTION_ID.to_string();
    collection
}

fn make_superusers_collection() -> Collection {
    let mut collection = Collection::base("_superusers", vec![]);
    collection.id = SUPERUSER_COL_ID.to_string();
    collection
}

fn make_user_record(id: &str, email: &str) -> HashMap<String, Value> {
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!(id));
    record.insert("email".to_string(), json!(email));
    record.insert("password".to_string(), json!("hashed:secret"));
    record.insert("tokenKey".to_string(), json!(TOKEN_KEY));
    record.insert("verified".to_string(), json!(true));
    record
}

fn make_superuser_record(id: &str, email: &str) -> HashMap<String, Value> {
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!(id));
    record.insert("email".to_string(), json!(email));
    record.insert("password".to_string(), json!("hashed:admin"));
    record
}

fn sample_external_auth(
    id: &str,
    collection_id: &str,
    record_id: &str,
    provider: &str,
    provider_id: &str,
) -> ExternalAuth {
    ExternalAuth {
        id: id.to_string(),
        collection_id: collection_id.to_string(),
        record_id: record_id.to_string(),
        provider: provider.to_string(),
        provider_id: provider_id.to_string(),
        created: "2025-01-01 00:00:00".to_string(),
        updated: "2025-01-01 00:00:00".to_string(),
    }
}

/// Token for the record owner (USER_ID).
fn owner_token() -> String {
    format!("valid:{USER_ID}:{TEST_COLLECTION_ID}:{TOKEN_KEY}")
}

/// Token for a different user (OTHER_USER_ID).
fn other_user_token() -> String {
    format!("valid:{OTHER_USER_ID}:{TEST_COLLECTION_ID}:{TOKEN_KEY}")
}

/// Token for a superuser.
fn superuser_token() -> String {
    format!("valid:{SUPERUSER_ID}:{SUPERUSER_COL_ID}:any")
}

type SpawnResult = (String, tokio::task::JoinHandle<()>);

async fn spawn_app(
    ext_repo: Arc<MockExternalAuthRepo>,
    extra_records: Vec<HashMap<String, Value>>,
) -> SpawnResult {
    let collections = vec![make_auth_collection(), make_superusers_collection()];
    let schema = Arc::new(MockSchemaLookup::with(collections));

    let mut all_records: HashMap<String, Vec<HashMap<String, Value>>> = HashMap::new();
    let mut user_records = vec![
        make_user_record(USER_ID, "owner@example.com"),
        make_user_record(OTHER_USER_ID, "other@example.com"),
    ];
    user_records.extend(extra_records);
    all_records.insert("users".to_string(), user_records);
    all_records.insert(
        "_superusers".to_string(),
        vec![make_superuser_record(SUPERUSER_ID, "admin@example.com")],
    );

    let repo = Arc::new(MockRecordRepo::with_data(all_records));
    let token_service: Arc<dyn TokenService> = Arc::new(MockTokenService);

    let auth_state = Arc::new(AuthMiddlewareState {
        token_service,
        record_repo: Arc::clone(&repo),
        schema_lookup: Arc::clone(&schema),
    });

    let external_auth_routes = zerobase_api::external_auth_routes(
        repo as Arc<MockRecordRepo>,
        schema as Arc<MockSchemaLookup>,
        ext_repo,
    );

    let app = external_auth_routes.layer(axum::middleware::from_fn_with_state(
        auth_state,
        zerobase_api::middleware::auth_context::auth_middleware::<MockRecordRepo, MockSchemaLookup>,
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

// ── Tests: list external auths ──────────────────────────────────────────────

#[tokio::test]
async fn list_external_auths_returns_linked_providers() {
    let ext_repo = Arc::new(MockExternalAuthRepo::with_auths(vec![
        sample_external_auth("ea1", TEST_COLLECTION_ID, USER_ID, "google", "gid1"),
        sample_external_auth("ea2", TEST_COLLECTION_ID, USER_ID, "microsoft", "msid1"),
    ]));
    let (addr, _handle) = spawn_app(ext_repo, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{addr}/api/collections/users/records/{USER_ID}/external-auths"
        ))
        .header("Authorization", format!("Bearer {}", owner_token()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(body.len(), 2);

    let providers: Vec<&str> = body
        .iter()
        .map(|a| a["provider"].as_str().unwrap())
        .collect();
    assert!(providers.contains(&"google"));
    assert!(providers.contains(&"microsoft"));
}

#[tokio::test]
async fn list_external_auths_returns_empty_when_none_linked() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_app(ext_repo, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{addr}/api/collections/users/records/{USER_ID}/external-auths"
        ))
        .header("Authorization", format!("Bearer {}", owner_token()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<Value> = resp.json().await.unwrap();
    assert!(body.is_empty());
}

#[tokio::test]
async fn list_external_auths_superuser_can_access_any_record() {
    let ext_repo = Arc::new(MockExternalAuthRepo::with_auths(vec![
        sample_external_auth("ea1", TEST_COLLECTION_ID, USER_ID, "google", "gid1"),
    ]));
    let (addr, _handle) = spawn_app(ext_repo, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{addr}/api/collections/users/records/{USER_ID}/external-auths"
        ))
        .header("Authorization", format!("Bearer {}", superuser_token()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(body.len(), 1);
}

#[tokio::test]
async fn list_external_auths_other_user_gets_403() {
    let ext_repo = Arc::new(MockExternalAuthRepo::with_auths(vec![
        sample_external_auth("ea1", TEST_COLLECTION_ID, USER_ID, "google", "gid1"),
    ]));
    let (addr, _handle) = spawn_app(ext_repo, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{addr}/api/collections/users/records/{USER_ID}/external-auths"
        ))
        .header("Authorization", format!("Bearer {}", other_user_token()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_external_auths_unauthenticated_gets_401() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_app(ext_repo, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{addr}/api/collections/users/records/{USER_ID}/external-auths"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn list_external_auths_nonexistent_record_returns_404() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_app(ext_repo, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{addr}/api/collections/users/records/nonexistent_id_123/external-auths"
        ))
        .header("Authorization", format!("Bearer {}", superuser_token()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_external_auths_nonexistent_collection_returns_404() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_app(ext_repo, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{addr}/api/collections/nonexistent/records/{USER_ID}/external-auths"
        ))
        .header("Authorization", format!("Bearer {}", superuser_token()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_external_auths_base_collection_returns_400() {
    // Spawn a separate app with a base collection named "posts"
    let collections = vec![
        {
            let mut c = Collection::base("posts", vec![]);
            c.id = "posts_col_id".to_string();
            c
        },
        make_superusers_collection(),
    ];
    let schema = Arc::new(MockSchemaLookup::with(collections));
    let mut all_records: HashMap<String, Vec<HashMap<String, Value>>> = HashMap::new();
    all_records.insert(
        "posts".to_string(),
        vec![{
            let mut r = HashMap::new();
            r.insert("id".to_string(), json!("post1"));
            r
        }],
    );
    all_records.insert(
        "_superusers".to_string(),
        vec![make_superuser_record(SUPERUSER_ID, "admin@example.com")],
    );
    let repo = Arc::new(MockRecordRepo::with_data(all_records));
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let token_service: Arc<dyn TokenService> = Arc::new(MockTokenService);

    let auth_state = Arc::new(AuthMiddlewareState {
        token_service,
        record_repo: Arc::clone(&repo),
        schema_lookup: Arc::clone(&schema),
    });

    let app = zerobase_api::external_auth_routes(
        repo as Arc<MockRecordRepo>,
        schema as Arc<MockSchemaLookup>,
        ext_repo,
    )
    .layer(axum::middleware::from_fn_with_state(
        auth_state,
        zerobase_api::middleware::auth_context::auth_middleware::<MockRecordRepo, MockSchemaLookup>,
    ));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let address = format!("http://127.0.0.1:{}", addr.port());

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{address}/api/collections/posts/records/post1/external-auths"
        ))
        .header("Authorization", format!("Bearer {}", superuser_token()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── Tests: unlink external auth ─────────────────────────────────────────────

#[tokio::test]
async fn unlink_external_auth_deletes_provider_link() {
    let ext_repo = Arc::new(MockExternalAuthRepo::with_auths(vec![
        sample_external_auth("ea1", TEST_COLLECTION_ID, USER_ID, "google", "gid1"),
        sample_external_auth("ea2", TEST_COLLECTION_ID, USER_ID, "microsoft", "msid1"),
    ]));
    let (addr, _handle) = spawn_app(ext_repo.clone(), vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .delete(format!(
            "{addr}/api/collections/users/records/{USER_ID}/external-auths/google"
        ))
        .header("Authorization", format!("Bearer {}", owner_token()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify the google link is removed.
    let remaining = ext_repo.auths.lock().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].provider, "microsoft");
}

#[tokio::test]
async fn unlink_external_auth_superuser_can_unlink() {
    let ext_repo = Arc::new(MockExternalAuthRepo::with_auths(vec![
        sample_external_auth("ea1", TEST_COLLECTION_ID, USER_ID, "google", "gid1"),
    ]));
    let (addr, _handle) = spawn_app(ext_repo.clone(), vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .delete(format!(
            "{addr}/api/collections/users/records/{USER_ID}/external-auths/google"
        ))
        .header("Authorization", format!("Bearer {}", superuser_token()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let remaining = ext_repo.auths.lock().unwrap();
    assert!(remaining.is_empty());
}

#[tokio::test]
async fn unlink_external_auth_other_user_gets_403() {
    let ext_repo = Arc::new(MockExternalAuthRepo::with_auths(vec![
        sample_external_auth("ea1", TEST_COLLECTION_ID, USER_ID, "google", "gid1"),
    ]));
    let (addr, _handle) = spawn_app(ext_repo.clone(), vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .delete(format!(
            "{addr}/api/collections/users/records/{USER_ID}/external-auths/google"
        ))
        .header("Authorization", format!("Bearer {}", other_user_token()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Verify nothing was deleted.
    let remaining = ext_repo.auths.lock().unwrap();
    assert_eq!(remaining.len(), 1);
}

#[tokio::test]
async fn unlink_external_auth_unauthenticated_gets_401() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_app(ext_repo, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .delete(format!(
            "{addr}/api/collections/users/records/{USER_ID}/external-auths/google"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unlink_external_auth_unknown_provider_returns_404() {
    let ext_repo = Arc::new(MockExternalAuthRepo::with_auths(vec![
        sample_external_auth("ea1", TEST_COLLECTION_ID, USER_ID, "google", "gid1"),
    ]));
    let (addr, _handle) = spawn_app(ext_repo, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .delete(format!(
            "{addr}/api/collections/users/records/{USER_ID}/external-auths/github"
        ))
        .header("Authorization", format!("Bearer {}", owner_token()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unlink_external_auth_nonexistent_record_returns_404() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_app(ext_repo, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .delete(format!(
            "{addr}/api/collections/users/records/nonexistent_id_123/external-auths/google"
        ))
        .header("Authorization", format!("Bearer {}", superuser_token()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
