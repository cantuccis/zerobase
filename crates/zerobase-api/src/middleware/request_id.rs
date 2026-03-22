//! Request-ID propagation middleware.
//!
//! Assigns a unique request ID to every incoming HTTP request and makes it
//! available in three places:
//!
//! 1. **tracing span** — every log line emitted while handling the request
//!    includes a `request_id` field.
//! 2. **Response header** — the `x-request-id` header is echoed back to the
//!    caller so they can reference it in bug reports.
//! 3. **Request extensions** — downstream handlers can extract the ID via
//!    `req.extensions().get::<RequestId>()`.
//!
//! If the incoming request already carries an `x-request-id` header the
//! middleware reuses that value (useful when a reverse proxy injects one).
//! Otherwise a new UUID v4 is generated.

use axum::{
    body::Body,
    extract::Request,
    http::{header::HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use tracing::Span;
use uuid::Uuid;

/// Header name used for request ID propagation.
pub static REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// Newtype stored in request extensions so handlers can access the ID.
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

/// Axum middleware function that assigns / propagates a request ID.
///
/// Usage with `axum::middleware::from_fn`:
///
/// ```rust,ignore
/// use axum::{Router, middleware};
/// use zerobase_api::middleware::request_id::request_id_middleware;
///
/// let app = Router::new()
///     .layer(middleware::from_fn(request_id_middleware));
/// ```
pub async fn request_id_middleware(mut request: Request<Body>, next: Next) -> Response {
    // Reuse existing header or generate a new UUID.
    let id = request
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Store in extensions for downstream handlers.
    request.extensions_mut().insert(RequestId(id.clone()));

    // Record in the current tracing span so every log line includes it.
    Span::current().record("request_id", id.as_str());

    let mut response = next.run(request).await;

    // Echo back to the caller.
    if let Ok(value) = HeaderValue::from_str(&id) {
        response
            .headers_mut()
            .insert(REQUEST_ID_HEADER.clone(), value);
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::{routing::get, Router};
    use tower::ServiceExt;

    async fn echo_request_id(request: Request<Body>) -> String {
        request
            .extensions()
            .get::<RequestId>()
            .map(|rid| rid.0.clone())
            .unwrap_or_else(|| "missing".to_string())
    }

    fn test_app() -> Router {
        Router::new()
            .route("/test", get(echo_request_id))
            .layer(axum::middleware::from_fn(request_id_middleware))
    }

    #[tokio::test]
    async fn generates_request_id_when_absent() {
        let app = test_app();

        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Response must have the header.
        let id_str = response
            .headers()
            .get("x-request-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        // Must be a valid UUID v4.
        let parsed = Uuid::parse_str(&id_str);
        assert!(parsed.is_ok(), "expected valid UUID, got: {id_str}");

        // Body should echo the same ID (proves extensions work).
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(std::str::from_utf8(&body).unwrap(), id_str);
    }

    #[tokio::test]
    async fn reuses_existing_request_id_header() {
        let app = test_app();

        let custom_id = "my-custom-request-id-123";
        let request = Request::builder()
            .uri("/test")
            .header("x-request-id", custom_id)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        let header = response
            .headers()
            .get("x-request-id")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(header, custom_id);

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(std::str::from_utf8(&body).unwrap(), custom_id);
    }
}
