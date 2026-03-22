//! Integration tests for the admin auth-with-password endpoint.
//!
//! Tests exercise the full HTTP stack (router → handler → service → mock repo)
//! using in-memory mocks. Each test spawns an isolated server on a random port.

mod common;

use std::collections::HashMap;
use std::sync::Mutex;

use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use std::sync::Arc;

use zerobase_core::auth::{PasswordHasher, TokenService, TokenType, ValidatedToken};
use zerobase_core::error::Result;
use zerobase_core::services::superuser_service::{
    SuperuserRepository, SuperuserService, SUPERUSERS_COLLECTION_ID,
};
use zerobase_core::ZerobaseError;

// ── Mock: SuperuserRepository ──────────────────────────────────────────────

struct MockSuperuserRepo {
    records: Mutex<Vec<HashMap<String, Value>>>,
}

impl MockSuperuserRepo {
    fn with_records(records: Vec<HashMap<String, Value>>) -> Self {
        Self {
            records: Mutex::new(records),
        }
    }
}

impl SuperuserRepository for MockSuperuserRepo {
    fn find_by_id(&self, id: &str) -> Result<Option<HashMap<String, Value>>> {
        let store = self.records.lock().unwrap();
        Ok(store
            .iter()
            .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
            .cloned())
    }

    fn find_by_email(&self, email: &str) -> Result<Option<HashMap<String, Value>>> {
        let store = self.records.lock().unwrap();
        Ok(store
            .iter()
            .find(|r| r.get("email").and_then(|v| v.as_str()) == Some(email))
            .cloned())
    }

    fn insert(&self, data: &HashMap<String, Value>) -> Result<()> {
        self.records.lock().unwrap().push(data.clone());
        Ok(())
    }

    fn update(&self, id: &str, data: &HashMap<String, Value>) -> Result<()> {
        let mut store = self.records.lock().unwrap();
        if let Some(record) = store
            .iter_mut()
            .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
        {
            for (k, v) in data {
                record.insert(k.clone(), v.clone());
            }
        }
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<bool> {
        let mut store = self.records.lock().unwrap();
        let before = store.len();
        store.retain(|r| r.get("id").and_then(|v| v.as_str()) != Some(id));
        Ok(store.len() < before)
    }

    fn list_all(&self) -> Result<Vec<HashMap<String, Value>>> {
        Ok(self.records.lock().unwrap().clone())
    }

    fn count(&self) -> Result<u64> {
        Ok(self.records.lock().unwrap().len() as u64)
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

// ── Test helpers ────────────────────────────────────────────────────────────

fn make_admin_record(id: &str, email: &str, password: &str) -> HashMap<String, Value> {
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!(id));
    record.insert("email".to_string(), json!(email));
    record.insert("password".to_string(), json!(format!("hashed:{password}")));
    record.insert("tokenKey".to_string(), json!("admin_token_key_12345"));
    record.insert("created".to_string(), json!("2025-01-01 00:00:00"));
    record.insert("updated".to_string(), json!("2025-01-01 00:00:00"));
    record
}

async fn spawn_admin_app(
    records: Vec<HashMap<String, Value>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let repo = MockSuperuserRepo::with_records(records);
    let superuser_service = Arc::new(SuperuserService::new(repo, TestHasher));
    let token_service: Arc<dyn TokenService> = Arc::new(TestTokenService);

    let app = zerobase_api::api_router()
        .merge(zerobase_api::admin_routes(superuser_service, token_service));

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
async fn admin_auth_returns_token_and_admin() {
    let admin = make_admin_record("su_123456789012", "admin@example.com", "secretpass");
    let (addr, _handle) = spawn_admin_app(vec![admin]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/_/api/admins/auth-with-password"))
        .json(&json!({
            "identity": "admin@example.com",
            "password": "secretpass"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert!(body["token"].is_string(), "response should contain token");
    let token = body["token"].as_str().unwrap();
    assert!(
        token.contains("su_123456789012"),
        "token should contain admin ID"
    );
    assert!(
        token.contains(SUPERUSERS_COLLECTION_ID),
        "token should contain superusers collection ID"
    );

    assert!(body["admin"].is_object(), "response should contain admin");
    assert_eq!(body["admin"]["email"], "admin@example.com");
    assert_eq!(body["admin"]["id"], "su_123456789012");
    assert_eq!(body["admin"]["collectionName"], "_superusers");
    assert_eq!(body["admin"]["collectionId"], SUPERUSERS_COLLECTION_ID);

    // Sensitive fields should not be in the response.
    assert!(
        body["admin"]["password"].is_null(),
        "password should be stripped"
    );
    assert!(
        body["admin"]["tokenKey"].is_null(),
        "tokenKey should be stripped"
    );
}

#[tokio::test]
async fn admin_auth_wrong_password_returns_400() {
    let admin = make_admin_record("su_123456789012", "admin@example.com", "secretpass");
    let (addr, _handle) = spawn_admin_app(vec![admin]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/_/api/admins/auth-with-password"))
        .json(&json!({
            "identity": "admin@example.com",
            "password": "wrongpassword"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], 400);
}

#[tokio::test]
async fn admin_auth_unknown_email_returns_400() {
    let (addr, _handle) = spawn_admin_app(vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/_/api/admins/auth-with-password"))
        .json(&json!({
            "identity": "nobody@example.com",
            "password": "somepassword"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn admin_auth_empty_identity_returns_400() {
    let (addr, _handle) = spawn_admin_app(vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/_/api/admins/auth-with-password"))
        .json(&json!({
            "identity": "",
            "password": "somepassword"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn admin_auth_empty_password_returns_400() {
    let (addr, _handle) = spawn_admin_app(vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/_/api/admins/auth-with-password"))
        .json(&json!({
            "identity": "admin@example.com",
            "password": ""
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn admin_auth_normalizes_email_case() {
    let admin = make_admin_record("su_123456789012", "admin@example.com", "secretpass");
    let (addr, _handle) = spawn_admin_app(vec![admin]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{addr}/_/api/admins/auth-with-password"))
        .json(&json!({
            "identity": "  Admin@Example.COM  ",
            "password": "secretpass"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert!(body["token"].is_string());
    assert_eq!(body["admin"]["email"], "admin@example.com");
}
