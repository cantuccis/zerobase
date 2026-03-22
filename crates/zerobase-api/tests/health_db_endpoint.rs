//! Integration tests for the health endpoint with database diagnostics.
//!
//! These tests boot a server that includes a real in-memory [`Database`] and
//! verify that `/api/health` returns detailed database health information.

use std::sync::Arc;

use reqwest::Client;
use tokio::net::TcpListener;

use zerobase_db::{Database, PoolConfig};

/// Spawn the API with a database-backed health endpoint.
async fn spawn_app_with_db() -> (String, Arc<Database>) {
    let db = Arc::new(
        Database::open_in_memory(&PoolConfig::default()).expect("failed to open in-memory DB"),
    );
    let db_clone = Arc::clone(&db);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        let app = zerobase_api::api_router_with_db(db_clone);
        axum::serve(listener, app).await.unwrap();
    });

    (base_url, db)
}

#[tokio::test]
async fn health_with_db_returns_200_and_healthy_status() {
    let (base_url, _db) = spawn_app_with_db().await;
    let client = Client::new();

    let response = client
        .get(format!("{base_url}/api/health"))
        .send()
        .await
        .expect("failed to send request");

    assert_eq!(response.status(), 200);

    let json: serde_json::Value = response.json().await.expect("invalid JSON");
    assert_eq!(json["status"], "healthy");
}

#[tokio::test]
async fn health_with_db_includes_database_diagnostics() {
    let (base_url, _db) = spawn_app_with_db().await;
    let client = Client::new();

    let response = client
        .get(format!("{base_url}/api/health"))
        .send()
        .await
        .expect("failed to send request");

    let json: serde_json::Value = response.json().await.expect("invalid JSON");

    // Database section should be present.
    let db = json
        .get("database")
        .expect("response should include 'database' field");

    assert_eq!(db["read_pool_ok"], true);
    assert_eq!(db["write_conn_ok"], true);
    assert_eq!(db["pool_exhausted"], false);

    // Pool stats should be present.
    let pool = db.get("pool").expect("database should include 'pool'");
    assert!(pool["max_size"].as_u64().unwrap() > 0);
    assert!(pool["total_connections"].as_u64().unwrap() > 0);

    // Latencies should be non-negative numbers.
    assert!(db["read_latency_us"].as_u64().is_some());
    assert!(db["write_latency_us"].as_u64().is_some());

    // Utilization should be a number.
    assert!(db["pool_utilization_pct"].as_f64().is_some());
}

#[tokio::test]
async fn health_without_db_returns_200_and_no_database_field() {
    // Use the simple api_router (no database).
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        let app = zerobase_api::api_router();
        axum::serve(listener, app).await.unwrap();
    });

    let client = Client::new();
    let response = client
        .get(format!("{base_url}/api/health"))
        .send()
        .await
        .expect("failed to send request");

    assert_eq!(response.status(), 200);

    let json: serde_json::Value = response.json().await.expect("invalid JSON");
    assert_eq!(json["status"], "healthy");
    // database field should NOT be present.
    assert!(
        json.get("database").is_none() || json["database"].is_null(),
        "simple health check should not include database diagnostics"
    );
}

#[tokio::test]
async fn health_with_db_pool_stats_reflect_configuration() {
    // Use a custom pool size.
    let config = PoolConfig {
        max_read_connections: 4,
        busy_timeout_ms: 5000,
    };
    let db = Arc::new(Database::open_in_memory(&config).expect("failed to open DB"));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    let db_clone = Arc::clone(&db);
    tokio::spawn(async move {
        let app = zerobase_api::api_router_with_db(db_clone);
        axum::serve(listener, app).await.unwrap();
    });

    let client = Client::new();
    let response = client
        .get(format!("{base_url}/api/health"))
        .send()
        .await
        .expect("failed to send request");

    let json: serde_json::Value = response.json().await.expect("invalid JSON");
    let pool = &json["database"]["pool"];
    assert_eq!(pool["max_size"], 4);
}

#[tokio::test]
async fn health_response_includes_request_id() {
    let (base_url, _db) = spawn_app_with_db().await;
    let client = Client::new();

    let response = client
        .get(format!("{base_url}/api/health"))
        .send()
        .await
        .expect("failed to send request");

    let request_id = response
        .headers()
        .get("x-request-id")
        .expect("response should include x-request-id header");

    let id_str = request_id.to_str().unwrap();
    assert!(
        uuid::Uuid::parse_str(id_str).is_ok(),
        "auto-generated request-id should be a valid UUID, got: {id_str}"
    );
}
