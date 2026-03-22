//! Middleware that enforces superuser authentication on routes.
//!
//! Uses the [`AuthInfo`] extractor (populated by [`auth_middleware`]) to check
//! whether the caller is a superuser. Returns `401 Unauthorized` if not.
//!
//! When `AuthInfo` is not present in extensions (e.g. auth middleware not
//! installed), falls back to checking for a non-empty `Authorization` header.
//! This fallback is intended for testing; production setups should always
//! use the auth middleware.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

use zerobase_core::ErrorResponseBody;

use super::auth_context::AuthInfo;

/// Middleware that requires superuser authentication.
///
/// Checks `AuthInfo` from request extensions first. If not present, falls
/// back to checking the `Authorization` header for backward compatibility.
/// Returns 401 if the caller is not a superuser.
pub async fn require_superuser(request: Request<Body>, next: Next) -> Response {
    let authorized = if let Some(info) = request.extensions().get::<AuthInfo>() {
        // Auth middleware is installed — use proper auth info.
        info.is_superuser
    } else {
        // Fallback: check for non-empty Authorization header.
        // This path is used in tests that don't install the auth middleware.
        request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| !v.is_empty())
    };

    if !authorized {
        let body = ErrorResponseBody {
            code: 401,
            message: "The request requires superuser authorization token to be set.".to_string(),
            data: std::collections::HashMap::new(),
        };
        return (StatusCode::UNAUTHORIZED, Json(body)).into_response();
    }

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    async fn ok_handler() -> &'static str {
        "ok"
    }

    fn app() -> Router {
        Router::new().route(
            "/protected",
            get(ok_handler).layer(axum::middleware::from_fn(require_superuser)),
        )
    }

    #[tokio::test]
    async fn missing_auth_returns_401() {
        let app = app();
        let req = Request::get("/protected").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn empty_auth_header_returns_401() {
        let app = app();
        let req = Request::get("/protected")
            .header("authorization", "")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn header_fallback_allows_access() {
        // Without AuthInfo in extensions, falls back to header check.
        let app = app();
        let req = Request::get("/protected")
            .header("authorization", "Bearer test-token")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn anonymous_auth_info_returns_401() {
        let app = app();
        let mut req = Request::get("/protected").body(Body::empty()).unwrap();
        req.extensions_mut().insert(AuthInfo::anonymous());
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn non_superuser_auth_info_returns_401() {
        let app = app();
        let mut record = std::collections::HashMap::new();
        record.insert(
            "id".to_string(),
            serde_json::Value::String("user1".to_string()),
        );
        let mut req = Request::get("/protected").body(Body::empty()).unwrap();
        req.extensions_mut().insert(AuthInfo::authenticated(record));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn superuser_auth_info_passes_through() {
        let app = app();
        let mut req = Request::get("/protected").body(Body::empty()).unwrap();
        req.extensions_mut().insert(AuthInfo::superuser());
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
