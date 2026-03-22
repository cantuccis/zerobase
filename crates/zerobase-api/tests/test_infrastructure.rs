//! Tests that validate the test infrastructure itself.
//!
//! These serve double duty: they verify that [`TestApp`], [`TestClient`], and
//! the assertion helpers work correctly, and they act as living documentation
//! for how to write integration tests in this project.

mod common;

use common::{
    assert_header, assert_header_exists, assert_json_response, assert_request_id_is_uuid,
    assert_status, TestApp,
};
use reqwest::StatusCode;
use serde_json::Value;

// ---------------------------------------------------------------------------
// TestApp basics
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_app_spawns_on_random_port() {
    let app = TestApp::spawn().await;

    assert!(app.port > 0, "port should be non-zero");
    assert!(
        app.address.starts_with("http://127.0.0.1:"),
        "address should be a localhost URL"
    );
}

#[tokio::test]
async fn test_app_url_helper_builds_correct_urls() {
    let app = TestApp::spawn().await;

    let url = app.url("/api/health");
    assert!(url.starts_with("http://127.0.0.1:"));
    assert!(url.ends_with("/api/health"));
}

#[tokio::test]
async fn test_multiple_apps_bind_to_different_ports() {
    let app1 = TestApp::spawn().await;
    let app2 = TestApp::spawn().await;

    assert_ne!(
        app1.port, app2.port,
        "two TestApp instances should bind to different ports"
    );
}

#[tokio::test]
async fn test_multiple_apps_are_independently_reachable() {
    let app1 = TestApp::spawn().await;
    let app2 = TestApp::spawn().await;

    let r1 = app1.client().get_response("/api/health").await;
    let r2 = app2.client().get_response("/api/health").await;

    assert_status(&r1, StatusCode::OK);
    assert_status(&r2, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// TestClient HTTP verb helpers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_client_get() {
    let app = TestApp::spawn().await;
    let client = app.client();

    let response = client.get_response("/api/health").await;
    assert_status(&response, StatusCode::OK);
}

#[tokio::test]
async fn test_client_post_json() {
    let app = TestApp::spawn().await;
    let client = app.client();

    // POST to health endpoint should return 405 (Method Not Allowed)
    let response = client
        .post_json("/api/health", &serde_json::json!({"key": "value"}))
        .await;
    assert_status(&response, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn test_client_put_json() {
    let app = TestApp::spawn().await;
    let client = app.client();

    let response = client
        .put_json("/api/health", &serde_json::json!({"key": "value"}))
        .await;
    assert_status(&response, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn test_client_patch_json() {
    let app = TestApp::spawn().await;
    let client = app.client();

    let response = client
        .patch_json("/api/health", &serde_json::json!({"key": "value"}))
        .await;
    assert_status(&response, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn test_client_delete() {
    let app = TestApp::spawn().await;
    let client = app.client();

    let response = client.delete_response("/api/health").await;
    assert_status(&response, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn test_client_arbitrary_method() {
    let app = TestApp::spawn().await;
    let client = app.client();

    let response = client
        .request(reqwest::Method::OPTIONS, "/api/health")
        .header("origin", "http://example.com")
        .header("access-control-request-method", "GET")
        .send()
        .await
        .unwrap();

    assert_header_exists(&response, "access-control-allow-origin");
}

// ---------------------------------------------------------------------------
// Assertion helpers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_assert_json_response_deserializes_body() {
    let app = TestApp::spawn().await;
    let client = app.client();

    let body: Value =
        assert_json_response(client.get_response("/api/health").await, StatusCode::OK).await;

    assert_eq!(body["status"], "healthy");
}

#[tokio::test]
async fn test_assert_header_checks_value() {
    let app = TestApp::spawn().await;
    let client = app.client();

    let custom_id = "test-header-check-42";
    let response = client
        .get("/api/health")
        .header("x-request-id", custom_id)
        .send()
        .await
        .unwrap();

    assert_header(&response, "x-request-id", custom_id);
}

#[tokio::test]
async fn test_assert_request_id_is_uuid_passes_for_auto_id() {
    let app = TestApp::spawn().await;
    let client = app.client();

    let response = client.get_response("/api/health").await;
    assert_request_id_is_uuid(&response);
}

#[tokio::test]
async fn test_unknown_route_returns_404() {
    let app = TestApp::spawn().await;
    let client = app.client();

    let response = client.get_response("/api/nonexistent").await;
    assert_status(&response, StatusCode::NOT_FOUND);
}
