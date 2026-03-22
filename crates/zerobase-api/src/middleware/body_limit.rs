//! Request body size limiting middleware for the Zerobase API.
//!
//! Enforces configurable maximum request body sizes. File upload endpoints
//! (multipart/form-data) use a separate, higher limit than regular JSON
//! endpoints. When a request exceeds the configured limit, the middleware
//! returns **413 Payload Too Large**.
//!
//! # Configuration
//!
//! | Setting                  | Default   | Description                          |
//! |--------------------------|-----------|--------------------------------------|
//! | `server.body_limit`      | 10 MiB    | Max body size for regular requests   |
//! | `server.body_limit_upload`| 100 MiB  | Max body size for multipart uploads  |
//!
//! # Usage
//!
//! ```rust,ignore
//! use zerobase_api::middleware::body_limit::{BodyLimitConfig, body_limit_middleware};
//!
//! let config = BodyLimitConfig::default();
//!
//! let app = Router::new()
//!     .route("/api/health", get(health))
//!     .layer(axum::middleware::from_fn_with_state(
//!         Arc::new(config),
//!         body_limit_middleware,
//!     ));
//! ```

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for request body size limits.
#[derive(Debug, Clone)]
pub struct BodyLimitConfig {
    /// Maximum body size in bytes for regular (non-multipart) requests.
    pub max_body_size: usize,
    /// Maximum body size in bytes for multipart/file upload requests.
    pub max_upload_size: usize,
}

impl Default for BodyLimitConfig {
    fn default() -> Self {
        Self {
            max_body_size: 10 * 1024 * 1024,      // 10 MiB
            max_upload_size: 100 * 1024 * 1024,    // 100 MiB
        }
    }
}

impl BodyLimitConfig {
    /// Create a new config with the given limits.
    pub fn new(max_body_size: usize, max_upload_size: usize) -> Self {
        Self {
            max_body_size,
            max_upload_size,
        }
    }

    /// Get the applicable limit for a request based on its content type.
    pub fn limit_for_content_type(&self, content_type: Option<&str>) -> usize {
        match content_type {
            Some(ct) if ct.starts_with("multipart/") => self.max_upload_size,
            _ => self.max_body_size,
        }
    }
}

// ---------------------------------------------------------------------------
// Axum middleware function
// ---------------------------------------------------------------------------

/// Axum middleware that enforces request body size limits.
///
/// Checks the `Content-Length` header against the configured limit for the
/// request type (multipart vs regular). If the declared content length
/// exceeds the limit, returns 413 immediately without reading the body.
///
/// Must be used with [`axum::middleware::from_fn_with_state`] and an
/// `Arc<BodyLimitConfig>` state.
pub async fn body_limit_middleware(
    State(config): State<Arc<BodyLimitConfig>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let limit = config.limit_for_content_type(content_type.as_deref());

    // Check Content-Length header if present
    if let Some(content_length) = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
    {
        if content_length > limit {
            tracing::warn!(
                content_length,
                limit,
                content_type = content_type.as_deref().unwrap_or("-"),
                "request body too large"
            );
            return payload_too_large(limit);
        }
    }

    next.run(request).await
}

/// Build a 413 response.
fn payload_too_large(limit: usize) -> Response {
    let body = serde_json::json!({
        "code": 413,
        "message": format!(
            "Request body too large. Maximum allowed size is {} bytes.",
            limit
        ),
        "data": {}
    });

    let mut response = (StatusCode::PAYLOAD_TOO_LARGE, axum::Json(body)).into_response();

    // Add max-size hint header
    if let Ok(v) = HeaderValue::from_str(&limit.to_string()) {
        response.headers_mut().insert("x-max-body-size", v);
    }

    response
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::routing::{get, post};
    use axum::Router;
    use tower::ServiceExt;

    /// Helper: build a test app with body limit middleware.
    fn test_app(config: BodyLimitConfig) -> Router {
        let config = Arc::new(config);
        Router::new()
            .route("/api/health", get(|| async { "ok" }))
            .route("/api/upload", post(|| async { "uploaded" }))
            .route("/api/records", post(|| async { "created" }))
            .layer(axum::middleware::from_fn_with_state(
                config,
                body_limit_middleware,
            ))
    }

    // --- Configuration tests ---

    #[test]
    fn default_config_values() {
        let config = BodyLimitConfig::default();
        assert_eq!(config.max_body_size, 10 * 1024 * 1024);
        assert_eq!(config.max_upload_size, 100 * 1024 * 1024);
    }

    #[test]
    fn limit_for_json_content_type() {
        let config = BodyLimitConfig::new(1000, 5000);
        assert_eq!(
            config.limit_for_content_type(Some("application/json")),
            1000
        );
    }

    #[test]
    fn limit_for_multipart_content_type() {
        let config = BodyLimitConfig::new(1000, 5000);
        assert_eq!(
            config.limit_for_content_type(Some(
                "multipart/form-data; boundary=----WebKitFormBoundary"
            )),
            5000
        );
    }

    #[test]
    fn limit_for_no_content_type() {
        let config = BodyLimitConfig::new(1000, 5000);
        assert_eq!(config.limit_for_content_type(None), 1000);
    }

    // --- Middleware integration tests ---

    #[tokio::test]
    async fn allows_request_within_limit() {
        let app = test_app(BodyLimitConfig::new(1000, 5000));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/records")
                    .header("content-type", "application/json")
                    .header("content-length", "500")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rejects_oversized_json_request() {
        let app = test_app(BodyLimitConfig::new(1000, 5000));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/records")
                    .header("content-type", "application/json")
                    .header("content-length", "2000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

        // Check response body
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], 413);
        assert!(json["message"]
            .as_str()
            .unwrap()
            .contains("too large"));
    }

    #[tokio::test]
    async fn allows_multipart_within_upload_limit() {
        let app = test_app(BodyLimitConfig::new(1000, 5000));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/upload")
                    .header("content-type", "multipart/form-data; boundary=abc")
                    .header("content-length", "3000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // 3000 bytes > 1000 (json limit) but < 5000 (upload limit) → allowed
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rejects_oversized_multipart_request() {
        let app = test_app(BodyLimitConfig::new(1000, 5000));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/upload")
                    .header("content-type", "multipart/form-data; boundary=abc")
                    .header("content-length", "6000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn allows_get_requests_without_content_length() {
        let app = test_app(BodyLimitConfig::new(100, 100));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn allows_request_exactly_at_limit() {
        let app = test_app(BodyLimitConfig::new(1000, 5000));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/records")
                    .header("content-type", "application/json")
                    .header("content-length", "1000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rejects_request_one_byte_over_limit() {
        let app = test_app(BodyLimitConfig::new(1000, 5000));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/records")
                    .header("content-type", "application/json")
                    .header("content-length", "1001")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn response_includes_max_body_size_header() {
        let app = test_app(BodyLimitConfig::new(1000, 5000));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/records")
                    .header("content-type", "application/json")
                    .header("content-length", "2000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let max_size = response
            .headers()
            .get("x-max-body-size")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(max_size, "1000");
    }

    #[tokio::test]
    async fn no_content_type_uses_default_body_limit() {
        let app = test_app(BodyLimitConfig::new(500, 5000));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/records")
                    .header("content-length", "600")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // No content-type → uses default (500), 600 > 500 → rejected
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
