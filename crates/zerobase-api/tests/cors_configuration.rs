//! Integration tests for configurable CORS middleware.
//!
//! Verifies that CORS headers are set correctly based on settings, that
//! the default permissive mode works, and that restrictive configurations
//! block unauthorized origins.

use std::collections::HashMap;
use std::sync::Arc;

use reqwest::StatusCode;
use serde_json::json;
use tokio::net::TcpListener;

use zerobase_core::services::settings_service::{
    CorsSettingsDto, SettingsRepoError, SettingsRepository,
};
use zerobase_core::SettingsService;

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

    fn with_cors(settings: &CorsSettingsDto) -> Self {
        let value = serde_json::to_string(settings).unwrap();
        let mut store = HashMap::new();
        store.insert("cors".to_string(), value);
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

/// Spawn a test server with CORS configured from settings.
async fn spawn_with_cors(cors: &CorsSettingsDto) -> (String, tokio::task::JoinHandle<()>) {
    let cors_layer = zerobase_api::build_cors_layer(cors);
    let app = zerobase_api::api_router_with_rate_limit_and_cors(
        zerobase_api::RateLimitConfig::default(),
        cors_layer,
    );

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let address = format!("http://127.0.0.1:{}", addr.port());

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (address, handle)
}

/// Spawn a test server with default (disabled) CORS settings.
async fn spawn_default() -> (String, tokio::task::JoinHandle<()>) {
    spawn_with_cors(&CorsSettingsDto::default()).await
}

// ── Tests: Default (disabled) CORS — permissive ────────────────────────────

#[tokio::test]
async fn default_cors_allows_any_origin() {
    let (addr, _handle) = spawn_default().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/health"))
        .header("origin", "https://evil.example.com")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let acao = resp.headers().get("access-control-allow-origin");
    assert!(acao.is_some(), "ACAO header missing");
    assert_eq!(acao.unwrap().to_str().unwrap(), "*");
}

#[tokio::test]
async fn default_cors_preflight_allows_any() {
    let (addr, _handle) = spawn_default().await;
    let client = reqwest::Client::new();

    let resp = client
        .request(reqwest::Method::OPTIONS, format!("{addr}/api/health"))
        .header("origin", "https://example.com")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "content-type")
        .send()
        .await
        .unwrap();

    let acao = resp.headers().get("access-control-allow-origin");
    assert!(acao.is_some(), "preflight ACAO missing");
    assert_eq!(acao.unwrap().to_str().unwrap(), "*");
}

// ── Tests: Enabled with specific origins ────────────────────────────────────

#[tokio::test]
async fn specific_origins_allows_matching_origin() {
    let cors = CorsSettingsDto {
        enabled: true,
        allowed_origins: vec!["https://app.example.com".to_string()],
        ..Default::default()
    };
    let (addr, _handle) = spawn_with_cors(&cors).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/health"))
        .header("origin", "https://app.example.com")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let acao = resp
        .headers()
        .get("access-control-allow-origin")
        .expect("ACAO header missing");
    assert_eq!(acao.to_str().unwrap(), "https://app.example.com");
}

#[tokio::test]
async fn specific_origins_blocks_non_matching_origin() {
    let cors = CorsSettingsDto {
        enabled: true,
        allowed_origins: vec!["https://app.example.com".to_string()],
        ..Default::default()
    };
    let (addr, _handle) = spawn_with_cors(&cors).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/health"))
        .header("origin", "https://evil.example.com")
        .send()
        .await
        .unwrap();

    // The request itself succeeds (CORS is enforced by the browser),
    // but the ACAO header should NOT be present for non-matching origins.
    let acao = resp.headers().get("access-control-allow-origin");
    assert!(
        acao.is_none(),
        "ACAO header should not be set for non-matching origin, got: {:?}",
        acao
    );
}

// ── Tests: Allowed methods ─────────────────────────────────────────────────

#[tokio::test]
async fn specific_methods_reflected_in_preflight() {
    let cors = CorsSettingsDto {
        enabled: true,
        allowed_origins: vec!["*".to_string()],
        allowed_methods: vec!["GET".to_string(), "POST".to_string()],
        ..Default::default()
    };
    let (addr, _handle) = spawn_with_cors(&cors).await;
    let client = reqwest::Client::new();

    let resp = client
        .request(reqwest::Method::OPTIONS, format!("{addr}/api/health"))
        .header("origin", "https://example.com")
        .header("access-control-request-method", "GET")
        .send()
        .await
        .unwrap();

    let acam = resp
        .headers()
        .get("access-control-allow-methods")
        .expect("ACAM header missing");
    let methods = acam.to_str().unwrap().to_uppercase();
    assert!(methods.contains("GET"), "GET not in allowed methods");
    assert!(methods.contains("POST"), "POST not in allowed methods");
}

// ── Tests: Credentials ──────────────────────────────────────────────────────

#[tokio::test]
async fn credentials_flag_set_when_enabled() {
    let cors = CorsSettingsDto {
        enabled: true,
        allowed_origins: vec!["https://app.example.com".to_string()],
        allow_credentials: true,
        ..Default::default()
    };
    let (addr, _handle) = spawn_with_cors(&cors).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/health"))
        .header("origin", "https://app.example.com")
        .send()
        .await
        .unwrap();

    let acac = resp
        .headers()
        .get("access-control-allow-credentials")
        .expect("ACAC header missing");
    assert_eq!(acac.to_str().unwrap(), "true");
}

#[tokio::test]
async fn no_credentials_header_when_disabled() {
    let cors = CorsSettingsDto {
        enabled: true,
        allowed_origins: vec!["*".to_string()],
        allow_credentials: false,
        ..Default::default()
    };
    let (addr, _handle) = spawn_with_cors(&cors).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/health"))
        .header("origin", "https://example.com")
        .send()
        .await
        .unwrap();

    let acac = resp.headers().get("access-control-allow-credentials");
    assert!(
        acac.is_none(),
        "ACAC header should not be set when credentials disabled"
    );
}

// ── Tests: Max age ─────────────────────────────────────────────────────────

#[tokio::test]
async fn max_age_reflected_in_preflight() {
    let cors = CorsSettingsDto {
        enabled: true,
        allowed_origins: vec!["*".to_string()],
        max_age: 3600,
        ..Default::default()
    };
    let (addr, _handle) = spawn_with_cors(&cors).await;
    let client = reqwest::Client::new();

    let resp = client
        .request(reqwest::Method::OPTIONS, format!("{addr}/api/health"))
        .header("origin", "https://example.com")
        .header("access-control-request-method", "GET")
        .send()
        .await
        .unwrap();

    let acma = resp
        .headers()
        .get("access-control-max-age")
        .expect("ACMA header missing");
    assert_eq!(acma.to_str().unwrap(), "3600");
}

// ── Tests: Exposed headers ──────────────────────────────────────────────────

#[tokio::test]
async fn exposed_headers_set_correctly() {
    let cors = CorsSettingsDto {
        enabled: true,
        allowed_origins: vec!["*".to_string()],
        exposed_headers: vec!["x-request-id".to_string(), "x-custom".to_string()],
        ..Default::default()
    };
    let (addr, _handle) = spawn_with_cors(&cors).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/health"))
        .header("origin", "https://example.com")
        .send()
        .await
        .unwrap();

    let aceh = resp
        .headers()
        .get("access-control-expose-headers")
        .expect("ACEH header missing");
    let headers = aceh.to_str().unwrap().to_lowercase();
    assert!(
        headers.contains("x-request-id"),
        "x-request-id not in exposed headers"
    );
    assert!(
        headers.contains("x-custom"),
        "x-custom not in exposed headers"
    );
}

// ── Tests: Settings validation ─────────────────────────────────────────────

#[tokio::test]
async fn settings_api_validates_credentials_with_wildcard() {
    let repo = InMemorySettingsRepo::new();
    let service = Arc::new(SettingsService::new(repo));
    let email_svc: Arc<dyn zerobase_core::email::EmailService> =
        Arc::new(zerobase_core::email::NoopEmailService);

    let app = zerobase_api::api_router()
        .merge(zerobase_api::settings_routes(service, email_svc));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();

    // Try to set credentials=true with wildcard origins — should fail
    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header("authorization", "Bearer test-superuser-token")
        .json(&json!({
            "cors": {
                "enabled": true,
                "allowedOrigins": ["*"],
                "allowCredentials": true
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    let msg = body["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("allowCredentials"),
        "error should mention allowCredentials: {msg}"
    );
}

#[tokio::test]
async fn settings_api_validates_invalid_method() {
    let repo = InMemorySettingsRepo::new();
    let service = Arc::new(SettingsService::new(repo));
    let email_svc: Arc<dyn zerobase_core::email::EmailService> =
        Arc::new(zerobase_core::email::NoopEmailService);

    let app = zerobase_api::api_router()
        .merge(zerobase_api::settings_routes(service, email_svc));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();

    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header("authorization", "Bearer test-superuser-token")
        .json(&json!({
            "cors": {
                "allowedMethods": ["GET", "INVALID"]
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── Tests: Settings persistence ─────────────────────────────────────────────

#[tokio::test]
async fn cors_settings_persisted_and_retrieved() {
    let repo = InMemorySettingsRepo::new();
    let service = Arc::new(SettingsService::new(repo));
    let email_svc: Arc<dyn zerobase_core::email::EmailService> =
        Arc::new(zerobase_core::email::NoopEmailService);

    let app = zerobase_api::api_router()
        .merge(zerobase_api::settings_routes(service, email_svc));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();

    // Update CORS settings
    let resp = client
        .patch(format!("{addr}/api/settings"))
        .header("authorization", "Bearer test-superuser-token")
        .json(&json!({
            "cors": {
                "enabled": true,
                "allowedOrigins": ["https://example.com"],
                "allowCredentials": true,
                "maxAge": 7200
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    // Read back
    let resp = client
        .get(format!("{addr}/api/settings/cors"))
        .header("authorization", "Bearer test-superuser-token")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["enabled"], true);
    assert_eq!(body["allowedOrigins"], json!(["https://example.com"]));
    assert_eq!(body["allowCredentials"], true);
    assert_eq!(body["maxAge"], 7200);
}

// ── Tests: Default settings include CORS ────────────────────────────────────

#[tokio::test]
async fn get_all_settings_includes_cors_defaults() {
    let repo = InMemorySettingsRepo::new();
    let service = Arc::new(SettingsService::new(repo));
    let email_svc: Arc<dyn zerobase_core::email::EmailService> =
        Arc::new(zerobase_core::email::NoopEmailService);

    let app = zerobase_api::api_router()
        .merge(zerobase_api::settings_routes(service, email_svc));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/settings"))
        .header("authorization", "Bearer test-superuser-token")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();

    // CORS should be present with defaults
    let cors = &body["cors"];
    assert_eq!(cors["enabled"], false);
    assert_eq!(cors["allowedOrigins"], json!(["*"]));
    assert_eq!(cors["allowCredentials"], false);
    assert_eq!(cors["maxAge"], 86400);
}

// ── Tests: Reset to defaults ────────────────────────────────────────────────

#[tokio::test]
async fn delete_cors_setting_resets_to_default() {
    let cors = CorsSettingsDto {
        enabled: true,
        allowed_origins: vec!["https://example.com".to_string()],
        ..Default::default()
    };
    let repo = InMemorySettingsRepo::with_cors(&cors);
    let service = Arc::new(SettingsService::new(repo));
    let email_svc: Arc<dyn zerobase_core::email::EmailService> =
        Arc::new(zerobase_core::email::NoopEmailService);

    let app = zerobase_api::api_router()
        .merge(zerobase_api::settings_routes(service, email_svc));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();

    // Delete (reset) CORS settings
    let resp = client
        .delete(format!("{addr}/api/settings/cors"))
        .header("authorization", "Bearer test-superuser-token")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Read back — should be defaults
    let resp = client
        .get(format!("{addr}/api/settings/cors"))
        .header("authorization", "Bearer test-superuser-token")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["enabled"], false);
    assert_eq!(body["allowedOrigins"], json!(["*"]));
}
