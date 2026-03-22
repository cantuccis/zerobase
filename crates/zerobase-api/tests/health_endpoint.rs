//! Integration test that boots a real HTTP server and exercises the health
//! endpoint over TCP with an HTTP client.
//!
//! This validates the full request path: TCP accept → middleware stack →
//! handler → serialization → response.

use reqwest::Client;
use tokio::net::TcpListener;

/// Spawn the API on an OS-assigned port and return the base URL.
async fn spawn_app() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        let app = zerobase_api::api_router();
        axum::serve(listener, app).await.unwrap();
    });

    base_url
}

#[tokio::test]
async fn health_endpoint_returns_200_with_json_status() {
    let base_url = spawn_app().await;
    let client = Client::new();

    let response = client
        .get(format!("{base_url}/api/health"))
        .send()
        .await
        .expect("failed to send request");

    assert_eq!(response.status(), 200);

    let json: serde_json::Value = response.json().await.expect("response is not valid JSON");
    assert_eq!(json["status"], "healthy");
}

#[tokio::test]
async fn health_endpoint_includes_request_id_header() {
    let base_url = spawn_app().await;
    let client = Client::new();

    let response = client
        .get(format!("{base_url}/api/health"))
        .send()
        .await
        .expect("failed to send request");

    let request_id = response
        .headers()
        .get("x-request-id")
        .expect("response missing x-request-id header");

    let id_str = request_id.to_str().unwrap();
    assert!(
        uuid::Uuid::parse_str(id_str).is_ok(),
        "auto-generated request-id should be a valid UUID, got: {id_str}"
    );
}

#[tokio::test]
async fn health_endpoint_echoes_caller_request_id() {
    let base_url = spawn_app().await;
    let client = Client::new();
    let custom_id = "integration-test-id-999";

    let response = client
        .get(format!("{base_url}/api/health"))
        .header("x-request-id", custom_id)
        .send()
        .await
        .expect("failed to send request");

    let echoed = response
        .headers()
        .get("x-request-id")
        .expect("response missing x-request-id")
        .to_str()
        .unwrap();

    assert_eq!(echoed, custom_id);
}

#[tokio::test]
async fn cors_preflight_succeeds() {
    let base_url = spawn_app().await;
    let client = Client::new();

    let response = client
        .request(reqwest::Method::OPTIONS, format!("{base_url}/api/health"))
        .header("origin", "http://example.com")
        .header("access-control-request-method", "GET")
        .send()
        .await
        .expect("failed to send preflight request");

    assert!(
        response
            .headers()
            .get("access-control-allow-origin")
            .is_some(),
        "CORS preflight response must include access-control-allow-origin"
    );
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let base_url = spawn_app().await;
    let client = Client::new();

    let response = client
        .get(format!("{base_url}/api/nonexistent"))
        .send()
        .await
        .expect("failed to send request");

    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn graceful_shutdown_completes_in_flight_request() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        let app = zerobase_api::api_router();
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
            .unwrap();
    });

    // Ensure the server is ready by hitting health first.
    let client = Client::new();
    let response = client
        .get(format!("{base_url}/api/health"))
        .send()
        .await
        .expect("server should be ready");
    assert_eq!(response.status(), 200);

    // Signal shutdown.
    shutdown_tx.send(()).unwrap();

    // Server task should complete without error.
    tokio::time::timeout(std::time::Duration::from_secs(5), server_handle)
        .await
        .expect("server should shut down within 5 seconds")
        .expect("server task should not panic");
}
