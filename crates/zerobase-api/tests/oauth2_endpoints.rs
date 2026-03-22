//! Integration tests for OAuth2 authentication endpoints.
//!
//! Tests exercise the full HTTP stack (router → handler → service → mock repo)
//! using in-memory mocks with a mock OAuth2 provider.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use zerobase_core::auth::{PasswordHasher, TokenService, TokenType, ValidatedToken};
use zerobase_core::error::Result;
use zerobase_core::oauth::{
    AuthUrlResponse, OAuthProvider, OAuthProviderRegistry, OAuthToken, OAuthUserInfo,
};
use zerobase_core::schema::Collection;
use zerobase_core::services::external_auth::{ExternalAuth, ExternalAuthRepository};
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
        collection: &str,
        id: &str,
        data: &HashMap<String, Value>,
    ) -> std::result::Result<bool, RecordRepoError> {
        let mut store = self.records.lock().unwrap();
        if let Some(rows) = store.get_mut(collection) {
            if let Some(record) = rows
                .iter_mut()
                .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
            {
                for (k, v) in data {
                    record.insert(k.clone(), v.clone());
                }
                return Ok(true);
            }
        }
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

// ── Mock: OAuthProvider ─────────────────────────────────────────────────────

struct MockOAuthProvider {
    provider_name: String,
    user_info: OAuthUserInfo,
}

impl MockOAuthProvider {
    fn new(name: &str) -> Self {
        Self {
            provider_name: name.to_string(),
            user_info: OAuthUserInfo {
                id: "oauth-id-123".to_string(),
                email: Some("oauth-user@example.com".to_string()),
                email_verified: true,
                name: Some("OAuth User".to_string()),
                avatar_url: None,
                raw: None,
            },
        }
    }

    fn with_user_info(mut self, info: OAuthUserInfo) -> Self {
        self.user_info = info;
        self
    }
}

#[async_trait]
impl OAuthProvider for MockOAuthProvider {
    fn name(&self) -> &str {
        &self.provider_name
    }

    fn display_name(&self) -> &str {
        "Mock Provider"
    }

    fn auth_url(
        &self,
        state: &str,
        redirect_url: &str,
    ) -> std::result::Result<AuthUrlResponse, ZerobaseError> {
        Ok(AuthUrlResponse {
            url: format!(
                "https://mock.example.com/auth?state={}&redirect_uri={}",
                state, redirect_url
            ),
            state: state.to_string(),
            code_verifier: Some("mock-verifier".to_string()),
        })
    }

    async fn exchange_code(
        &self,
        _code: &str,
        _redirect_url: &str,
        _code_verifier: Option<&str>,
    ) -> std::result::Result<OAuthToken, ZerobaseError> {
        Ok(OAuthToken {
            access_token: "mock-access-token".to_string(),
            refresh_token: Some("mock-refresh-token".to_string()),
            expires_in: Some(3600),
        })
    }

    async fn get_user_info(
        &self,
        _token: &OAuthToken,
    ) -> std::result::Result<OAuthUserInfo, ZerobaseError> {
        Ok(self.user_info.clone())
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

fn make_oauth2_collection() -> Collection {
    let mut collection = Collection::auth("users", vec![]);
    collection.id = TEST_COLLECTION_ID.to_string();
    if let Some(ref mut opts) = collection.auth_options {
        opts.allow_oauth2_auth = true;
    }
    collection
}

fn make_non_oauth2_collection() -> Collection {
    Collection::auth("users", vec![])
}

fn make_user_record(id: &str, email: &str) -> HashMap<String, Value> {
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!(id));
    record.insert("email".to_string(), json!(email));
    record.insert("password".to_string(), json!("hashed:secret"));
    record.insert("tokenKey".to_string(), json!("random_token_key_12345"));
    record.insert("emailVisibility".to_string(), json!(false));
    record.insert("verified".to_string(), json!(false));
    record.insert("created".to_string(), json!("2025-01-01 00:00:00"));
    record.insert("updated".to_string(), json!("2025-01-01 00:00:00"));
    record
}

type SpawnResult = (String, tokio::task::JoinHandle<()>);

async fn spawn_oauth2_app(
    collections: Vec<Collection>,
    collection_name: &str,
    records: Vec<HashMap<String, Value>>,
    provider: MockOAuthProvider,
    external_auth_repo: Arc<MockExternalAuthRepo>,
) -> SpawnResult {
    let schema = MockSchemaLookup::with(collections);
    let repo = MockRecordRepo::with_records(collection_name, records);
    let record_service = Arc::new(RecordService::with_password_hasher(
        repo, schema, TestHasher,
    ));
    let token_service: Arc<dyn TokenService> = Arc::new(TestTokenService);

    let mut registry = OAuthProviderRegistry::new();
    registry.register(Arc::new(provider));

    let app = zerobase_api::api_router().merge(zerobase_api::oauth2_routes(
        record_service,
        token_service,
        external_auth_repo,
        Arc::new(registry),
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

// ── Tests: auth-methods endpoint ────────────────────────────────────────────

#[tokio::test]
async fn auth_methods_returns_structured_response_with_all_sections() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_oauth2_app(
        vec![make_oauth2_collection()],
        "users",
        vec![],
        MockOAuthProvider::new("google"),
        ext_repo,
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{addr}/api/collections/users/auth-methods"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();

    // Verify top-level structure has all four sections.
    assert!(body["password"].is_object(), "password section missing");
    assert!(body["oauth2"].is_object(), "oauth2 section missing");
    assert!(body["otp"].is_object(), "otp section missing");
    assert!(body["mfa"].is_object(), "mfa section missing");

    // Password: enabled by default with email identity field.
    assert_eq!(body["password"]["enabled"], true);
    let identity_fields = body["password"]["identityFields"].as_array().unwrap();
    assert!(identity_fields.contains(&json!("email")));

    // OAuth2: enabled with google provider.
    assert_eq!(body["oauth2"]["enabled"], true);
    let providers = body["oauth2"]["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0]["name"], "google");
    assert_eq!(providers[0]["displayName"], "Mock Provider");
    // Auth URL and state should be populated.
    assert!(
        providers[0]["authUrl"].as_str().unwrap().len() > 0,
        "authUrl should be non-empty"
    );
    assert!(
        providers[0]["state"].as_str().unwrap().len() > 0,
        "state should be non-empty"
    );

    // OTP: disabled by default.
    assert_eq!(body["otp"]["enabled"], false);

    // MFA: disabled by default.
    assert_eq!(body["mfa"]["enabled"], false);
    assert_eq!(body["mfa"]["duration"], 0);
}

#[tokio::test]
async fn auth_methods_default_auth_collection_password_only() {
    // Default auth collection: only password auth enabled.
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let collection = Collection::auth("users", vec![]);
    let schema = MockSchemaLookup::with(vec![collection]);
    let repo = MockRecordRepo::new();
    let record_service = Arc::new(RecordService::with_password_hasher(
        repo, schema, TestHasher,
    ));
    let token_service: Arc<dyn TokenService> = Arc::new(TestTokenService);
    let registry = Arc::new(OAuthProviderRegistry::new());

    let app = zerobase_api::api_router().merge(zerobase_api::oauth2_routes(
        record_service,
        token_service,
        ext_repo,
        registry,
    ));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let address = format!("http://127.0.0.1:{}", addr.port());

    let _handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{address}/api/collections/users/auth-methods"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["password"]["enabled"], true);
    assert_eq!(body["oauth2"]["enabled"], false);
    assert_eq!(body["oauth2"]["providers"].as_array().unwrap().len(), 0);
    assert_eq!(body["otp"]["enabled"], false);
    assert_eq!(body["mfa"]["enabled"], false);
}

#[tokio::test]
async fn auth_methods_with_otp_and_mfa_enabled() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let mut collection = Collection::auth("users", vec![]);
    collection.id = TEST_COLLECTION_ID.to_string();
    if let Some(ref mut opts) = collection.auth_options {
        opts.allow_otp_auth = true;
        opts.mfa_enabled = true;
        opts.mfa_duration = 1800;
    }

    let schema = MockSchemaLookup::with(vec![collection]);
    let repo = MockRecordRepo::new();
    let record_service = Arc::new(RecordService::with_password_hasher(
        repo, schema, TestHasher,
    ));
    let token_service: Arc<dyn TokenService> = Arc::new(TestTokenService);
    let registry = Arc::new(OAuthProviderRegistry::new());

    let app = zerobase_api::api_router().merge(zerobase_api::oauth2_routes(
        record_service,
        token_service,
        ext_repo,
        registry,
    ));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let address = format!("http://127.0.0.1:{}", addr.port());

    let _handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{address}/api/collections/users/auth-methods"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["password"]["enabled"], true);
    assert_eq!(body["otp"]["enabled"], true);
    assert_eq!(body["mfa"]["enabled"], true);
    assert_eq!(body["mfa"]["duration"], 1800);
}

#[tokio::test]
async fn auth_methods_oauth2_disabled_returns_empty_providers() {
    // OAuth2 disabled in auth options but providers registered in registry.
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let collection = Collection::auth("users", vec![]);
    // Default: allow_oauth2_auth = false
    let schema = MockSchemaLookup::with(vec![collection]);
    let repo = MockRecordRepo::new();
    let record_service = Arc::new(RecordService::with_password_hasher(
        repo, schema, TestHasher,
    ));
    let token_service: Arc<dyn TokenService> = Arc::new(TestTokenService);

    let mut registry = OAuthProviderRegistry::new();
    registry.register(Arc::new(MockOAuthProvider::new("google")));

    let app = zerobase_api::api_router().merge(zerobase_api::oauth2_routes(
        record_service,
        token_service,
        ext_repo,
        Arc::new(registry),
    ));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let address = format!("http://127.0.0.1:{}", addr.port());

    let _handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{address}/api/collections/users/auth-methods"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["oauth2"]["enabled"], false);
    // Even though providers are registered, they shouldn't appear when OAuth2 is disabled.
    assert_eq!(body["oauth2"]["providers"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn auth_methods_no_auth_required() {
    // Verify the endpoint works without any Authorization header.
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_oauth2_app(
        vec![make_oauth2_collection()],
        "users",
        vec![],
        MockOAuthProvider::new("google"),
        ext_repo,
    )
    .await;

    let client = reqwest::Client::new();
    // No Authorization header — should still work.
    let resp = client
        .get(format!("{addr}/api/collections/users/auth-methods"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn auth_methods_non_auth_collection_returns_400() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let base_collection = Collection::base("posts", vec![]);
    let schema = MockSchemaLookup::with(vec![base_collection]);
    let repo = MockRecordRepo::new();
    let record_service = Arc::new(RecordService::with_password_hasher(
        repo, schema, TestHasher,
    ));
    let token_service: Arc<dyn TokenService> = Arc::new(TestTokenService);
    let registry = Arc::new(OAuthProviderRegistry::new());

    let app = zerobase_api::api_router().merge(zerobase_api::oauth2_routes(
        record_service,
        token_service,
        ext_repo,
        registry,
    ));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let address = format!("http://127.0.0.1:{}", addr.port());

    let _handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{address}/api/collections/posts/auth-methods"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn auth_methods_nonexistent_collection_returns_404() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_oauth2_app(
        vec![make_oauth2_collection()],
        "users",
        vec![],
        MockOAuthProvider::new("google"),
        ext_repo,
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{addr}/api/collections/nonexistent/auth-methods"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn auth_methods_password_disabled() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let mut collection = Collection::auth("users", vec![]);
    collection.id = TEST_COLLECTION_ID.to_string();
    if let Some(ref mut opts) = collection.auth_options {
        opts.allow_email_auth = false;
    }

    let schema = MockSchemaLookup::with(vec![collection]);
    let repo = MockRecordRepo::new();
    let record_service = Arc::new(RecordService::with_password_hasher(
        repo, schema, TestHasher,
    ));
    let token_service: Arc<dyn TokenService> = Arc::new(TestTokenService);
    let registry = Arc::new(OAuthProviderRegistry::new());

    let app = zerobase_api::api_router().merge(zerobase_api::oauth2_routes(
        record_service,
        token_service,
        ext_repo,
        registry,
    ));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let address = format!("http://127.0.0.1:{}", addr.port());

    let _handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{address}/api/collections/users/auth-methods"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["password"]["enabled"], false);
}

// ── Tests: auth-with-oauth2 endpoint ────────────────────────────────────────

#[tokio::test]
async fn oauth2_creates_new_user_and_returns_token() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_oauth2_app(
        vec![make_oauth2_collection()],
        "users",
        vec![],
        MockOAuthProvider::new("google"),
        ext_repo.clone(),
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-oauth2"))
        .json(&json!({
            "provider": "google",
            "code": "auth-code-123",
            "redirectUrl": "http://localhost/callback"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert!(body["token"].is_string(), "response should contain token");
    assert!(!body["token"].as_str().unwrap().is_empty());
    assert!(body["record"].is_object(), "response should contain record");
    assert_eq!(body["record"]["collectionName"], "users");
    assert_eq!(body["meta"]["isNew"], true);
    // Sensitive fields should be stripped.
    assert!(
        body["record"]["password"].is_null(),
        "password should be stripped"
    );
    assert!(
        body["record"]["tokenKey"].is_null(),
        "tokenKey should be stripped"
    );

    // External auth link should be created.
    let auths = ext_repo.auths.lock().unwrap();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].provider, "google");
    assert_eq!(auths[0].provider_id, "oauth-id-123");
}

#[tokio::test]
async fn oauth2_links_existing_user_by_email() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let existing_user = make_user_record("abc123456789012", "oauth-user@example.com");

    let (addr, _handle) = spawn_oauth2_app(
        vec![make_oauth2_collection()],
        "users",
        vec![existing_user],
        MockOAuthProvider::new("google"),
        ext_repo.clone(),
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-oauth2"))
        .json(&json!({
            "provider": "google",
            "code": "auth-code-123",
            "redirectUrl": "http://localhost/callback"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["record"]["id"], "abc123456789012");
    assert_eq!(body["meta"]["isNew"], false);

    // External auth should be linked to the existing user.
    let auths = ext_repo.auths.lock().unwrap();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].record_id, "abc123456789012");
}

#[tokio::test]
async fn oauth2_returns_existing_linked_user() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let existing_user = make_user_record("abc123456789012", "oauth-user@example.com");

    // Pre-create an external auth link.
    ext_repo
        .create(&ExternalAuth {
            id: "ea1234567890123".to_string(),
            collection_id: TEST_COLLECTION_ID.to_string(),
            record_id: "abc123456789012".to_string(),
            provider: "google".to_string(),
            provider_id: "oauth-id-123".to_string(),
            created: "2025-01-01 00:00:00".to_string(),
            updated: "2025-01-01 00:00:00".to_string(),
        })
        .unwrap();

    let (addr, _handle) = spawn_oauth2_app(
        vec![make_oauth2_collection()],
        "users",
        vec![existing_user],
        MockOAuthProvider::new("google"),
        ext_repo,
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-oauth2"))
        .json(&json!({
            "provider": "google",
            "code": "auth-code-123",
            "redirectUrl": "http://localhost/callback"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["record"]["id"], "abc123456789012");
    // Existing linked user is not "new".
    assert_eq!(body["meta"]["isNew"], false);
}

#[tokio::test]
async fn oauth2_disabled_collection_returns_400() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_oauth2_app(
        vec![make_non_oauth2_collection()],
        "users",
        vec![],
        MockOAuthProvider::new("google"),
        ext_repo,
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-oauth2"))
        .json(&json!({
            "provider": "google",
            "code": "auth-code-123",
            "redirectUrl": "http://localhost/callback"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body: Value = resp.json().await.unwrap();
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("OAuth2 authentication is not enabled"));
}

#[tokio::test]
async fn oauth2_unknown_provider_returns_400() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_oauth2_app(
        vec![make_oauth2_collection()],
        "users",
        vec![],
        MockOAuthProvider::new("google"),
        ext_repo,
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-oauth2"))
        .json(&json!({
            "provider": "nonexistent",
            "code": "auth-code-123",
            "redirectUrl": "http://localhost/callback"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body: Value = resp.json().await.unwrap();
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("unknown OAuth2 provider"));
}

#[tokio::test]
async fn oauth2_missing_provider_returns_400() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_oauth2_app(
        vec![make_oauth2_collection()],
        "users",
        vec![],
        MockOAuthProvider::new("google"),
        ext_repo,
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-oauth2"))
        .json(&json!({
            "provider": "",
            "code": "auth-code-123",
            "redirectUrl": "http://localhost/callback"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn oauth2_missing_code_returns_400() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_oauth2_app(
        vec![make_oauth2_collection()],
        "users",
        vec![],
        MockOAuthProvider::new("google"),
        ext_repo,
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-oauth2"))
        .json(&json!({
            "provider": "google",
            "code": "",
            "redirectUrl": "http://localhost/callback"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn oauth2_with_pkce_code_verifier() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_oauth2_app(
        vec![make_oauth2_collection()],
        "users",
        vec![],
        MockOAuthProvider::new("google"),
        ext_repo,
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-oauth2"))
        .json(&json!({
            "provider": "google",
            "code": "auth-code-123",
            "redirectUrl": "http://localhost/callback",
            "codeVerifier": "pkce-verifier-value"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert!(body["token"].is_string());
}

#[tokio::test]
async fn oauth2_nonexistent_collection_returns_404() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let (addr, _handle) = spawn_oauth2_app(
        vec![make_oauth2_collection()],
        "users",
        vec![],
        MockOAuthProvider::new("google"),
        ext_repo,
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "{addr}/api/collections/nonexistent/auth-with-oauth2"
        ))
        .json(&json!({
            "provider": "google",
            "code": "auth-code-123",
            "redirectUrl": "http://localhost/callback"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn oauth2_user_without_email_creates_new_account() {
    let ext_repo = Arc::new(MockExternalAuthRepo::new());
    let provider = MockOAuthProvider::new("github").with_user_info(OAuthUserInfo {
        id: "github-id-456".to_string(),
        email: None,
        email_verified: false,
        name: Some("No Email User".to_string()),
        avatar_url: None,
        raw: None,
    });

    let (addr, _handle) = spawn_oauth2_app(
        vec![make_oauth2_collection()],
        "users",
        vec![],
        provider,
        ext_repo.clone(),
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/api/collections/users/auth-with-oauth2"))
        .json(&json!({
            "provider": "github",
            "code": "auth-code-123",
            "redirectUrl": "http://localhost/callback"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert!(body["token"].is_string());
    assert_eq!(body["meta"]["isNew"], true);

    let auths = ext_repo.auths.lock().unwrap();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].provider_id, "github-id-456");
}
