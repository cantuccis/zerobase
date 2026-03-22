//! Request logging middleware.
//!
//! Captures HTTP request metadata (method, URL, status, duration, IP, auth user)
//! and writes a [`LogEntry`] to the [`LogService`] after the response is produced.

use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::middleware::Next;
use axum::response::Response;

use zerobase_core::id::generate_id;
use zerobase_core::services::log_service::{LogEntry, LogRepository, LogService};

use super::auth_context::AuthInfo;
use super::request_id::RequestId;

/// Axum middleware that logs every request to the [`LogService`].
///
/// Must be installed *after* the auth and request-id middleware so that
/// `AuthInfo` and `RequestId` are available in request extensions.
pub async fn request_logging_middleware<R: LogRepository + 'static>(
    State(service): State<Arc<LogService<R>>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().to_string();
    let url = request.uri().path().to_string();
    let user_agent = request
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let request_id = request
        .extensions()
        .get::<RequestId>()
        .map(|r| r.0.clone())
        .unwrap_or_default();

    let auth_id = request
        .extensions()
        .get::<AuthInfo>()
        .and_then(|info| {
            info.auth_record
                .get("id")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .unwrap_or_default();

    // Extract IP from ConnectInfo or X-Forwarded-For header.
    let ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
        .or_else(|| {
            request
                .extensions()
                .get::<ConnectInfo<std::net::SocketAddr>>()
                .map(|ci| ci.0.ip().to_string())
        })
        .unwrap_or_default();

    let start = Instant::now();
    let response = next.run(request).await;
    let duration_ms = start.elapsed().as_millis() as u64;

    let status = response.status().as_u16();

    let now: chrono::DateTime<chrono::Utc> = chrono::Utc::now();
    let created = now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();

    let entry = LogEntry {
        id: generate_id(),
        method,
        url,
        status,
        ip,
        auth_id,
        duration_ms,
        user_agent,
        request_id,
        created,
    };

    // Fire-and-forget: log creation errors are not propagated to the caller.
    if let Err(e) = service.create(&entry) {
        tracing::warn!(error = %e, "failed to persist request log");
    }

    response
}
