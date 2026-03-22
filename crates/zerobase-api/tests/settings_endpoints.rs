//! Integration tests for the settings management REST API.
//!
//! Tests exercise the full HTTP stack (router → middleware → handler → service)
//! using an in-memory [`SettingsRepository`] mock. Each test spawns an isolated
//! server on a random port.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use reqwest::StatusCode;
use serde_json::json;
use tokio::net::TcpListener;

use zerobase_core::email::{EmailMessage, EmailService};
use zerobase_core::error::ZerobaseError;
use zerobase_core::services::settings_service::{SettingsRepoError, SettingsRepository};
use zerobase_core::SettingsService;

// ── Mock email service ──────────────────────────────────────────────────

struct MockEmailService {
    sent: std::sync::Mutex<Vec<EmailMessage>>,
}

impl MockEmailService {
    fn new() -> Self {
        Self {
            sent: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl EmailService for MockEmailService {
    fn send(&self, message: &EmailMessage) -> Result<(), ZerobaseError> {
        self.sent.lock().unwrap().push(message.clone());
        Ok(())
    }
}

struct FailingEmailService;

impl EmailService for FailingEmailService {
    fn send(&self, _message: &EmailMessage) -> Result<(), ZerobaseError> {
        Err(ZerobaseError::internal("SMTP connection failed"))
    }
}

// ── In-memory mock repository ───────────────────────────────────────────────

struct InMemorySettingsRepo {
    store: std::sync::Mutex<HashMap<String, String>>,
}

impl InMemorySettingsRepo {
    fn new() -> Self {
        Self {
            store: std::sync::Mutex::new(HashMap::new()),
        }
    }

    fn with(entries: Vec<(&str, &str)>) -> Self {
        let mut store = HashMap::new();
        for (k, v) in entries {
            store.insert(k.to_string(), v.to_string());
        }
        Self {
            store: std::sync::Mutex::new(store),
        }
    }
}

impl SettingsRepository for InMemorySettingsRepo {
    fn get_setting(&self, key: &str) -> Result<Option<String>, SettingsRepoError> {
        Ok(self.store.lock().unwrap().get(key).cloned())
    }

    fn get_all_settings(&self) -> Result<Vec<(String, String)>, SettingsRepoError> {
        Ok(self
            .store
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<(), SettingsRepoError> {
        self.store
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete_setting(&self, key: &str) -> Result<(), SettingsRepoError> {
        self.store.lock().unwrap().remove(key);
        Ok(())
    }
}

// ── Test infrastructure ─────────────────────────────────────────────────────

/// Spawn a test server with settings routes and an in-memory repo.
async fn spawn_app(repo: InMemorySettingsRepo) -> (String, u16, tokio::task::JoinHandle<()>) {
    let email_svc: Arc<dyn EmailService> = Arc::new(MockEmailService::new());
    spawn_app_with_email(repo, email_svc).await
}

/// Spawn a test server with settings routes and a custom email service.
async fn spawn_app_with_email(
    repo: InMemorySettingsRepo,
    email_service: Arc<dyn EmailService>,
) -> (String, u16, tokio::task::JoinHandle<()>) {
    let service = Arc::new(SettingsService::new(repo));

    let app = zerobase_api::api_router().merge(zerobase_api::settings_routes(service, email_service));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let port = addr.port();
    let address = format!("http://127.0.0.1:{port}");

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (address, port, handle)
}

fn auth_header() -> (&'static str, &'static str) {
    ("authorization", "Bearer test-superuser-token")
}

// ── GET /api/settings ──────────────────────────────────────────────────────

#[tokio::test]
async fn get_all_settings_returns_200_with_defaults() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    // All known keys should be present
    assert!(body.get("meta").is_some(), "meta key missing");
    assert!(body.get("smtp").is_some(), "smtp key missing");
    assert!(body.get("s3").is_some(), "s3 key missing");
    assert!(body.get("backups").is_some(), "backups key missing");
    assert!(body.get("auth").is_some(), "auth key missing");

    // Default values
    assert_eq!(body["meta"]["appName"], "");
    assert_eq!(body["smtp"]["enabled"], false);
    assert_eq!(body["auth"]["tokenDuration"], 1_209_600);
    assert_eq!(body["auth"]["minPasswordLength"], 8);
}

#[tokio::test]
async fn get_all_settings_returns_stored_values() {
    let repo = InMemorySettingsRepo::with(vec![(
        "meta",
        r#"{"appName":"MyApp","appUrl":"https://my.app","senderName":"","senderAddress":""}"#,
    )]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["meta"]["appName"], "MyApp");
    assert_eq!(body["meta"]["appUrl"], "https://my.app");
}

#[tokio::test]
async fn get_all_settings_without_auth_returns_401() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/settings"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── PATCH /api/settings ────────────────────────────────────────────────────

#[tokio::test]
async fn update_settings_returns_200_with_merged_settings() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "meta": { "appName": "TestApp", "appUrl": "https://test.app" }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["meta"]["appName"], "TestApp");
    assert_eq!(body["meta"]["appUrl"], "https://test.app");
    // Other keys should still have defaults
    assert!(body.get("smtp").is_some());
    assert!(body.get("auth").is_some());
}

#[tokio::test]
async fn update_settings_merges_with_existing() {
    let repo = InMemorySettingsRepo::with(vec![(
        "meta",
        r#"{"appName":"OldName","appUrl":"https://old.app","senderName":"Bob","senderAddress":"bob@test.com"}"#,
    )]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "meta": { "appName": "NewName" }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["meta"]["appName"], "NewName");
    // Existing fields preserved
    assert_eq!(body["meta"]["appUrl"], "https://old.app");
    assert_eq!(body["meta"]["senderName"], "Bob");
}

#[tokio::test]
async fn update_settings_persists_across_reads() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    // Update
    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "meta": { "appName": "Persisted" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Read back
    let resp = client
        .get(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["meta"]["appName"], "Persisted");
}

#[tokio::test]
async fn update_settings_rejects_unknown_keys() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "bogus_key": { "foo": "bar" }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], 400);
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("unknown setting key"));
}

#[tokio::test]
async fn update_settings_rejects_non_object_values() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "meta": "not an object"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn update_settings_validates_smtp_host_when_enabled() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "smtp": { "enabled": true, "host": "" }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["message"].as_str().unwrap().contains("smtp.host"));
}

#[tokio::test]
async fn update_settings_validates_s3_when_enabled() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "s3": { "enabled": true, "bucket": "", "region": "us-east-1" }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn update_settings_without_auth_returns_401() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!("{addr}/api/settings"))
        .json(&json!({ "meta": { "appName": "X" } }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn update_multiple_settings_at_once() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "meta": { "appName": "Multi" },
            "smtp": { "enabled": false, "host": "smtp.test.com" },
            "auth": { "minPasswordLength": 12 }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["meta"]["appName"], "Multi");
    assert_eq!(body["smtp"]["host"], "smtp.test.com");
    assert_eq!(body["auth"]["minPasswordLength"], 12);
}

// ── GET /api/settings/:key ─────────────────────────────────────────────────

#[tokio::test]
async fn get_single_setting_returns_200_with_default() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/settings/smtp"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["enabled"], false);
    assert_eq!(body["host"], "");
}

#[tokio::test]
async fn get_single_setting_returns_stored_value() {
    let repo = InMemorySettingsRepo::with(vec![(
        "meta",
        r#"{"appName":"Stored","appUrl":"","senderName":"","senderAddress":""}"#,
    )]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/settings/meta"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["appName"], "Stored");
}

#[tokio::test]
async fn get_single_setting_without_auth_returns_401() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/settings/meta"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── DELETE /api/settings/:key ──────────────────────────────────────────────

#[tokio::test]
async fn delete_setting_returns_204_and_resets_to_default() {
    let repo = InMemorySettingsRepo::with(vec![("meta", r#"{"appName":"ToBeDeleted"}"#)]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    // Delete
    let resp = client
        .delete(format!("{addr}/api/settings/meta"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Read back — should return default
    let resp = client
        .get(format!("{addr}/api/settings/meta"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["appName"], "");
}

#[tokio::test]
async fn delete_setting_without_auth_returns_401() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{addr}/api/settings/meta"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── End-to-end flow ────────────────────────────────────────────────────────

#[tokio::test]
async fn full_settings_lifecycle() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    // 1. Read defaults
    let resp = client
        .get(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["meta"]["appName"], "");

    // 2. Update
    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "meta": { "appName": "LifecycleApp", "appUrl": "https://lifecycle.app" },
            "smtp": { "host": "smtp.lifecycle.com", "port": 465 }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 3. Verify via single key read
    let resp = client
        .get(format!("{addr}/api/settings/meta"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let meta: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(meta["appName"], "LifecycleApp");
    assert_eq!(meta["appUrl"], "https://lifecycle.app");

    // 4. Update again (merge)
    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "meta": { "appName": "UpdatedApp" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["meta"]["appName"], "UpdatedApp");
    // URL preserved
    assert_eq!(body["meta"]["appUrl"], "https://lifecycle.app");

    // 5. Delete meta → reset to default
    let resp = client
        .delete(format!("{addr}/api/settings/meta"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 6. Verify reset
    let resp = client
        .get(format!("{addr}/api/settings/meta"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let meta: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(meta["appName"], "");

    // 7. SMTP setting still persists
    let resp = client
        .get(format!("{addr}/api/settings/smtp"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let smtp: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(smtp["host"], "smtp.lifecycle.com");
    assert_eq!(smtp["port"], 465);
}

// ── Auth settings integration tests ─────────────────────────────────────

#[tokio::test]
async fn get_auth_settings_returns_defaults() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/settings/auth"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["tokenDuration"], 1_209_600);
    assert_eq!(body["refreshTokenDuration"], 604_800);
    assert_eq!(body["allowEmailAuth"], false);
    assert_eq!(body["allowOauth2Auth"], false);
    assert_eq!(body["allowMfa"], false);
    assert_eq!(body["minPasswordLength"], 8);
    assert_eq!(body["mfa"]["duration"], 300);
    assert_eq!(body["otp"]["length"], 6);
}

#[tokio::test]
async fn toggle_auth_methods_via_api() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    // Enable auth methods
    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "auth": {
                "allowEmailAuth": true,
                "allowOauth2Auth": true,
                "allowMfa": true,
                "allowOtpAuth": true,
                "allowPasskeyAuth": true
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["auth"]["allowEmailAuth"], true);
    assert_eq!(body["auth"]["allowOauth2Auth"], true);
    assert_eq!(body["auth"]["allowMfa"], true);
    assert_eq!(body["auth"]["allowOtpAuth"], true);
    assert_eq!(body["auth"]["allowPasskeyAuth"], true);

    // Verify persistence via GET
    let resp = client
        .get(format!("{addr}/api/settings/auth"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    let auth: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(auth["allowEmailAuth"], true);
    assert_eq!(auth["allowOauth2Auth"], true);

    // Toggle off a single method — others preserved
    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({"auth": {"allowEmailAuth": false}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["auth"]["allowEmailAuth"], false);
    assert_eq!(body["auth"]["allowOauth2Auth"], true);
}

#[tokio::test]
async fn configure_oauth2_provider_credentials() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    // Store provider credentials
    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "auth": {
                "oauth2Providers": {
                    "google": {
                        "enabled": true,
                        "clientId": "google-client-id",
                        "clientSecret": "google-secret-xyz",
                        "displayName": "Google"
                    }
                }
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();

    // Client secret masked in response
    assert_eq!(
        body["auth"]["oauth2Providers"]["google"]["clientId"],
        "google-client-id"
    );
    assert_eq!(
        body["auth"]["oauth2Providers"]["google"]["clientSecret"],
        ""
    );
    assert_eq!(
        body["auth"]["oauth2Providers"]["google"]["displayName"],
        "Google"
    );

    // Read back — still masked
    let resp = client
        .get(format!("{addr}/api/settings/auth"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    let auth: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        auth["oauth2Providers"]["google"]["clientId"],
        "google-client-id"
    );
    assert_eq!(auth["oauth2Providers"]["google"]["clientSecret"], "");
}

#[tokio::test]
async fn oauth2_provider_secret_preserved_on_empty_update() {
    let repo = InMemorySettingsRepo::with(vec![(
        "auth",
        r#"{"oauth2Providers":{"google":{"enabled":true,"clientId":"id-1","clientSecret":"real-secret","displayName":"G"}}}"#,
    )]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    // Update with empty clientSecret — should preserve existing secret
    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "auth": {
                "oauth2Providers": {
                    "google": {
                        "enabled": true,
                        "clientId": "id-1",
                        "clientSecret": "",
                        "displayName": "Updated Google"
                    }
                }
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["auth"]["oauth2Providers"]["google"]["displayName"],
        "Updated Google"
    );
    // Secret is masked but was preserved internally
    assert_eq!(
        body["auth"]["oauth2Providers"]["google"]["clientSecret"],
        ""
    );
}

#[tokio::test]
async fn auth_validation_rejects_invalid_token_duration() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({"auth": {"tokenDuration": 0}}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["message"].as_str().unwrap().contains("tokenDuration"));
}

#[tokio::test]
async fn auth_validation_rejects_short_password() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({"auth": {"minPasswordLength": 3}}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("minPasswordLength"));
}

#[tokio::test]
async fn auth_validation_rejects_oauth2_without_client_id() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "auth": {
                "oauth2Providers": {
                    "github": {"enabled": true, "clientId": ""}
                }
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["message"].as_str().unwrap().contains("clientId"));
}

#[tokio::test]
async fn auth_settings_full_lifecycle() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    // 1. Configure auth settings
    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "auth": {
                "allowEmailAuth": true,
                "allowOauth2Auth": true,
                "tokenDuration": 86400,
                "minPasswordLength": 10,
                "mfa": {"required": true, "duration": 600},
                "otp": {"length": 8, "duration": 120},
                "oauth2Providers": {
                    "google": {
                        "enabled": true,
                        "clientId": "g-id",
                        "clientSecret": "g-secret",
                        "displayName": "Google"
                    },
                    "github": {
                        "enabled": false,
                        "clientId": "gh-id",
                        "clientSecret": "gh-secret",
                        "displayName": "GitHub"
                    }
                }
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. Read back and verify
    let resp = client
        .get(format!("{addr}/api/settings/auth"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    let auth: serde_json::Value = resp.json().await.unwrap();

    assert_eq!(auth["allowEmailAuth"], true);
    assert_eq!(auth["allowOauth2Auth"], true);
    assert_eq!(auth["tokenDuration"], 86400);
    assert_eq!(auth["minPasswordLength"], 10);
    assert_eq!(auth["mfa"]["required"], true);
    assert_eq!(auth["mfa"]["duration"], 600);
    assert_eq!(auth["otp"]["length"], 8);
    assert_eq!(auth["otp"]["duration"], 120);
    assert_eq!(auth["oauth2Providers"]["google"]["clientId"], "g-id");
    assert_eq!(auth["oauth2Providers"]["google"]["clientSecret"], "");
    assert_eq!(auth["oauth2Providers"]["github"]["enabled"], false);

    // 3. Partial update — merge preserves existing
    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({"auth": {"allowPasskeyAuth": true}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["auth"]["allowPasskeyAuth"], true);
    assert_eq!(body["auth"]["allowEmailAuth"], true);
    assert_eq!(body["auth"]["tokenDuration"], 86400);

    // 4. Delete resets to defaults
    let resp = client
        .delete(format!("{addr}/api/settings/auth"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = client
        .get(format!("{addr}/api/settings/auth"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    let auth: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(auth["allowEmailAuth"], false);
    assert_eq!(auth["tokenDuration"], 1_209_600);
}

// ── POST /api/settings/test-email ────────────────────────────────────

#[tokio::test]
async fn test_email_sends_when_smtp_enabled() {
    let repo = InMemorySettingsRepo::with(vec![(
        "smtp",
        r#"{"enabled":true,"host":"smtp.test.com","port":587,"username":"user","password":"pass","tls":true}"#,
    )]);
    let email_svc = Arc::new(MockEmailService::new());
    let email_svc_clone = Arc::clone(&email_svc);
    let (addr, _, _handle) =
        spawn_app_with_email(repo, email_svc_clone as Arc<dyn EmailService>).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/settings/test-email"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({ "to": "test@example.com" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);

    // Verify the email was sent
    let sent = email_svc.sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "test@example.com");
    assert!(sent[0].subject.contains("Test Email"));
}

#[tokio::test]
async fn test_email_rejects_when_smtp_disabled() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/settings/test-email"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({ "to": "test@example.com" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["message"].as_str().unwrap().contains("not enabled"));
}

#[tokio::test]
async fn test_email_rejects_empty_recipient() {
    let repo = InMemorySettingsRepo::with(vec![(
        "smtp",
        r#"{"enabled":true,"host":"smtp.test.com","port":587,"username":"","password":"","tls":true}"#,
    )]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/settings/test-email"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({ "to": "" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_email_returns_error_on_send_failure() {
    let repo = InMemorySettingsRepo::with(vec![(
        "smtp",
        r#"{"enabled":true,"host":"smtp.test.com","port":587,"username":"","password":"","tls":true}"#,
    )]);
    let email_svc: Arc<dyn EmailService> = Arc::new(FailingEmailService);
    let (addr, _, _handle) = spawn_app_with_email(repo, email_svc).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/settings/test-email"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({ "to": "test@example.com" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_email_without_auth_returns_401() {
    let (addr, _, _handle) = spawn_app(InMemorySettingsRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/settings/test-email"))
        .json(&json!({ "to": "test@example.com" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
