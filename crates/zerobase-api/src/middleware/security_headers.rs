//! Security headers middleware.
//!
//! Adds standard security headers to all HTTP responses to mitigate common
//! web vulnerabilities like MIME-sniffing, clickjacking, and XSS.

use axum::body::Body;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

/// Middleware that adds security headers to every response.
///
/// Headers added:
/// - `X-Content-Type-Options: nosniff` — prevents MIME-type sniffing
/// - `X-Frame-Options: SAMEORIGIN` — prevents clickjacking via iframes
/// - `X-XSS-Protection: 0` — disables legacy browser XSS auditor (can cause issues)
/// - `Referrer-Policy: strict-origin-when-cross-origin` — limits referrer leakage
/// - `Permissions-Policy: interest-cohort=()` — opts out of FLoC/Topics
pub async fn security_headers_middleware(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    headers.insert(
        "x-content-type-options",
        "nosniff".parse().unwrap(),
    );
    headers.insert(
        "x-frame-options",
        "SAMEORIGIN".parse().unwrap(),
    );
    // Disable legacy XSS auditor — it can introduce vulnerabilities.
    headers.insert(
        "x-xss-protection",
        "0".parse().unwrap(),
    );
    headers.insert(
        "referrer-policy",
        "strict-origin-when-cross-origin".parse().unwrap(),
    );
    headers.insert(
        "permissions-policy",
        "interest-cohort=()".parse().unwrap(),
    );

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::middleware;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    async fn ok_handler() -> &'static str {
        "ok"
    }

    #[tokio::test]
    async fn adds_security_headers() {
        let app = Router::new()
            .route("/test", get(ok_handler))
            .layer(middleware::from_fn(security_headers_middleware));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("x-content-type-options").unwrap(),
            "nosniff"
        );
        assert_eq!(
            response.headers().get("x-frame-options").unwrap(),
            "SAMEORIGIN"
        );
        assert_eq!(
            response.headers().get("x-xss-protection").unwrap(),
            "0"
        );
        assert_eq!(
            response.headers().get("referrer-policy").unwrap(),
            "strict-origin-when-cross-origin"
        );
        assert_eq!(
            response.headers().get("permissions-policy").unwrap(),
            "interest-cohort=()"
        );
    }
}
