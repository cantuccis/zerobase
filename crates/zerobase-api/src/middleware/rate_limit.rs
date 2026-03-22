//! Rate-limiting middleware for the Zerobase API.
//!
//! Uses a token-bucket algorithm per client IP with configurable limits
//! per route category. When a client exceeds the limit the middleware
//! returns **429 Too Many Requests** with a `Retry-After` header.
//!
//! # Route categories
//!
//! Different endpoint groups can have independent rate limits:
//!
//! | Category   | Default          | Typical endpoints                         |
//! |------------|------------------|-------------------------------------------|
//! | `Auth`     | 10 req / 60 s    | login, OTP, password reset, OAuth2        |
//! | `Default`  | 100 req / 60 s   | CRUD, files, settings, etc.               |
//!
//! # Usage
//!
//! ```rust,ignore
//! use zerobase_api::middleware::rate_limit::{RateLimiter, RateLimitConfig, RouteCategory};
//!
//! let limiter = RateLimiter::new(RateLimitConfig::default());
//!
//! let app = Router::new()
//!     .route("/api/health", get(health))
//!     .layer(axum::middleware::from_fn_with_state(
//!         Arc::new(limiter),
//!         rate_limit_middleware,
//!     ));
//! ```

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Identifies which rate-limit bucket a request falls into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RouteCategory {
    /// Authentication-related endpoints (login, OTP, password reset, …).
    Auth,
    /// Everything else.
    Default,
}

/// Per-category rate-limit parameters.
#[derive(Debug, Clone, Copy)]
pub struct CategoryLimit {
    /// Maximum number of requests allowed within the window.
    pub max_requests: u32,
    /// Rolling time window.
    pub window: Duration,
}

/// Top-level configuration for the rate limiter.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Whether rate limiting is globally enabled.
    pub enabled: bool,
    /// Per-category limits. Any category missing from the map uses `default_limit`.
    pub category_limits: HashMap<RouteCategory, CategoryLimit>,
    /// Fallback limit when a category is not explicitly configured.
    pub default_limit: CategoryLimit,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        let mut category_limits = HashMap::new();
        category_limits.insert(
            RouteCategory::Auth,
            CategoryLimit {
                max_requests: 10,
                window: Duration::from_secs(60),
            },
        );
        Self {
            enabled: true,
            category_limits,
            default_limit: CategoryLimit {
                max_requests: 100,
                window: Duration::from_secs(60),
            },
        }
    }
}

impl RateLimitConfig {
    /// Look up the limit for a given category.
    pub fn limit_for(&self, category: RouteCategory) -> &CategoryLimit {
        self.category_limits
            .get(&category)
            .unwrap_or(&self.default_limit)
    }
}

// ---------------------------------------------------------------------------
// Token-bucket state (per client+category)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Bucket {
    /// Remaining tokens (requests) in this window.
    tokens: u32,
    /// When the current window started.
    window_start: Instant,
}

/// Composite key: (client IP, route category).
type BucketKey = (IpAddr, RouteCategory);

// ---------------------------------------------------------------------------
// RateLimiter
// ---------------------------------------------------------------------------

/// Thread-safe, in-memory rate limiter.
///
/// Internally uses a `DashMap` so concurrent requests from different clients
/// never contend on a single lock.
pub struct RateLimiter {
    config: RateLimitConfig,
    buckets: dashmap::DashMap<BucketKey, Bucket>,
}

impl std::fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateLimiter")
            .field("config", &self.config)
            .field("buckets_len", &self.buckets.len())
            .finish()
    }
}

impl RateLimiter {
    /// Create a new limiter with the given configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: dashmap::DashMap::new(),
        }
    }

    /// Check whether the request from `ip` in `category` should be allowed.
    ///
    /// Returns `Ok(remaining)` on success or `Err(retry_after_secs)` when the
    /// client has exceeded the limit.
    pub fn check(&self, ip: IpAddr, category: RouteCategory) -> Result<u32, u64> {
        if !self.config.enabled {
            return Ok(u32::MAX);
        }

        let limit = self.config.limit_for(category);
        let now = Instant::now();
        let key = (ip, category);

        let mut entry = self.buckets.entry(key).or_insert_with(|| Bucket {
            tokens: limit.max_requests,
            window_start: now,
        });

        let bucket = entry.value_mut();

        // If the window has elapsed, reset the bucket.
        if now.duration_since(bucket.window_start) >= limit.window {
            bucket.tokens = limit.max_requests;
            bucket.window_start = now;
        }

        if bucket.tokens > 0 {
            bucket.tokens -= 1;
            Ok(bucket.tokens)
        } else {
            let elapsed = now.duration_since(bucket.window_start);
            let retry_after = limit
                .window
                .checked_sub(elapsed)
                .unwrap_or(Duration::ZERO)
                .as_secs()
                .max(1);
            Err(retry_after)
        }
    }

    /// Remove stale entries that have not been touched for longer than their
    /// window. Call periodically in a background task to prevent unbounded
    /// memory growth.
    pub fn cleanup(&self) {
        let now = Instant::now();
        self.buckets.retain(|(_ip, cat), bucket| {
            let limit = self.config.limit_for(*cat);
            // Keep entries that are still within 2× their window (grace).
            now.duration_since(bucket.window_start) < limit.window * 2
        });
    }
}

// ---------------------------------------------------------------------------
// Route classification
// ---------------------------------------------------------------------------

/// Classify a request URI into a [`RouteCategory`].
///
/// The classification is intentionally simple: any path that contains an
/// auth-related segment is categorised as [`RouteCategory::Auth`].
pub fn classify_route(uri_path: &str) -> RouteCategory {
    let path = uri_path.to_ascii_lowercase();

    // Auth-related path segments
    const AUTH_SEGMENTS: &[&str] = &[
        "/auth-with-password",
        "/auth-with-otp",
        "/auth-with-mfa",
        "/auth-with-passkey",
        "/auth-with-oauth2",
        "/auth-refresh",
        "/auth-methods",
        "/request-otp",
        "/request-password-reset",
        "/confirm-password-reset",
        "/request-verification",
        "/confirm-verification",
        "/request-email-change",
        "/confirm-email-change",
        "/request-passkey-register",
        "/confirm-passkey-register",
        "/request-mfa-setup",
        "/confirm-mfa",
        "/admins/auth-with-password",
    ];

    for segment in AUTH_SEGMENTS {
        if path.contains(segment) {
            return RouteCategory::Auth;
        }
    }

    RouteCategory::Default
}

// ---------------------------------------------------------------------------
// Axum middleware function
// ---------------------------------------------------------------------------

/// Axum middleware that enforces rate limits.
///
/// Must be used with [`axum::middleware::from_fn_with_state`] and an
/// `Arc<RateLimiter>` state.
pub async fn rate_limit_middleware(
    State(limiter): State<Arc<RateLimiter>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let ip = extract_client_ip(&request);
    let category = classify_route(request.uri().path());

    match limiter.check(ip, category) {
        Ok(remaining) => {
            let mut response = next.run(request).await;
            // Inform client of remaining budget.
            let limit = limiter.config.limit_for(category);
            if let Ok(v) = HeaderValue::from_str(&remaining.to_string()) {
                response
                    .headers_mut()
                    .insert("x-ratelimit-remaining", v);
            }
            if let Ok(v) = HeaderValue::from_str(&limit.max_requests.to_string()) {
                response
                    .headers_mut()
                    .insert("x-ratelimit-limit", v);
            }
            response
        }
        Err(retry_after) => {
            tracing::warn!(
                ip = %ip,
                category = ?category,
                retry_after,
                "rate limit exceeded"
            );
            too_many_requests(retry_after)
        }
    }
}

/// Build a 429 response with `Retry-After` header.
fn too_many_requests(retry_after_secs: u64) -> Response {
    let body = serde_json::json!({
        "code": 429,
        "message": "Too many requests. Please try again later.",
        "data": {}
    });

    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        axum::Json(body),
    )
        .into_response();

    if let Ok(v) = HeaderValue::from_str(&retry_after_secs.to_string()) {
        response.headers_mut().insert("retry-after", v);
    }

    response
}

/// Extract the client IP from the request.
///
/// Checks (in order):
/// 1. `x-forwarded-for` header (first IP)
/// 2. `x-real-ip` header
/// 3. [`ConnectInfo`] socket address
/// 4. Falls back to `127.0.0.1`
fn extract_client_ip(request: &Request<Body>) -> IpAddr {
    // 1. x-forwarded-for
    if let Some(forwarded) = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(first) = forwarded.split(',').next() {
            if let Ok(ip) = first.trim().parse::<IpAddr>() {
                return ip;
            }
        }
    }

    // 2. x-real-ip
    if let Some(real_ip) = request
        .headers()
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
    {
        if let Ok(ip) = real_ip.trim().parse::<IpAddr>() {
            return ip;
        }
    }

    // 3. ConnectInfo (only available when server uses `into_make_service_with_connect_info`)
    if let Some(connect_info) = request
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
    {
        return connect_info.0.ip();
    }

    // 4. Fallback
    IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    /// Helper: build a test app with rate limiting.
    fn test_app(config: RateLimitConfig) -> Router {
        let limiter = Arc::new(RateLimiter::new(config));
        Router::new()
            .route("/api/health", get(|| async { "ok" }))
            .route(
                "/api/collections/users/auth-with-password",
                get(|| async { "auth" }),
            )
            .layer(axum::middleware::from_fn_with_state(
                limiter,
                rate_limit_middleware,
            ))
    }

    fn make_request(uri: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .header("x-forwarded-for", "10.0.0.1")
            .body(Body::empty())
            .unwrap()
    }

    // --- classify_route tests ---

    #[test]
    fn classify_auth_with_password() {
        assert_eq!(
            classify_route("/api/collections/users/auth-with-password"),
            RouteCategory::Auth,
        );
    }

    #[test]
    fn classify_auth_refresh() {
        assert_eq!(
            classify_route("/api/collections/users/auth-refresh"),
            RouteCategory::Auth,
        );
    }

    #[test]
    fn classify_request_otp() {
        assert_eq!(
            classify_route("/api/collections/users/request-otp"),
            RouteCategory::Auth,
        );
    }

    #[test]
    fn classify_admin_login() {
        assert_eq!(
            classify_route("/_/api/admins/auth-with-password"),
            RouteCategory::Auth,
        );
    }

    #[test]
    fn classify_default_records() {
        assert_eq!(
            classify_route("/api/collections/posts/records"),
            RouteCategory::Default,
        );
    }

    #[test]
    fn classify_default_health() {
        assert_eq!(classify_route("/api/health"), RouteCategory::Default);
    }

    #[test]
    fn classify_password_reset() {
        assert_eq!(
            classify_route("/api/collections/users/request-password-reset"),
            RouteCategory::Auth,
        );
        assert_eq!(
            classify_route("/api/collections/users/confirm-password-reset"),
            RouteCategory::Auth,
        );
    }

    #[test]
    fn classify_mfa_routes() {
        assert_eq!(
            classify_route("/api/collections/users/auth-with-mfa"),
            RouteCategory::Auth,
        );
        assert_eq!(
            classify_route("/api/collections/users/records/abc/request-mfa-setup"),
            RouteCategory::Auth,
        );
        assert_eq!(
            classify_route("/api/collections/users/records/abc/confirm-mfa"),
            RouteCategory::Auth,
        );
    }

    #[test]
    fn classify_passkey_routes() {
        assert_eq!(
            classify_route("/api/collections/users/auth-with-passkey-begin"),
            RouteCategory::Auth,
        );
        assert_eq!(
            classify_route("/api/collections/users/request-passkey-register"),
            RouteCategory::Auth,
        );
    }

    #[test]
    fn classify_oauth2_routes() {
        assert_eq!(
            classify_route("/api/collections/users/auth-with-oauth2"),
            RouteCategory::Auth,
        );
        assert_eq!(
            classify_route("/api/collections/users/auth-methods"),
            RouteCategory::Auth,
        );
    }

    #[test]
    fn classify_verification_routes() {
        assert_eq!(
            classify_route("/api/collections/users/request-verification"),
            RouteCategory::Auth,
        );
        assert_eq!(
            classify_route("/api/collections/users/confirm-verification"),
            RouteCategory::Auth,
        );
    }

    #[test]
    fn classify_email_change_routes() {
        assert_eq!(
            classify_route("/api/collections/users/request-email-change"),
            RouteCategory::Auth,
        );
        assert_eq!(
            classify_route("/api/collections/users/confirm-email-change"),
            RouteCategory::Auth,
        );
    }

    // --- RateLimiter unit tests ---

    #[test]
    fn allows_requests_within_limit() {
        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 3,
                window: Duration::from_secs(60),
            },
            category_limits: HashMap::new(),
        };
        let limiter = RateLimiter::new(config);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();

        assert_eq!(limiter.check(ip, RouteCategory::Default), Ok(2));
        assert_eq!(limiter.check(ip, RouteCategory::Default), Ok(1));
        assert_eq!(limiter.check(ip, RouteCategory::Default), Ok(0));
    }

    #[test]
    fn rejects_when_limit_exceeded() {
        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 2,
                window: Duration::from_secs(60),
            },
            category_limits: HashMap::new(),
        };
        let limiter = RateLimiter::new(config);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();

        assert!(limiter.check(ip, RouteCategory::Default).is_ok());
        assert!(limiter.check(ip, RouteCategory::Default).is_ok());
        let result = limiter.check(ip, RouteCategory::Default);
        assert!(result.is_err());
        // retry_after should be at least 1
        assert!(result.unwrap_err() >= 1);
    }

    #[test]
    fn different_ips_have_independent_limits() {
        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 1,
                window: Duration::from_secs(60),
            },
            category_limits: HashMap::new(),
        };
        let limiter = RateLimiter::new(config);

        let ip1: IpAddr = "10.0.0.1".parse().unwrap();
        let ip2: IpAddr = "10.0.0.2".parse().unwrap();

        assert!(limiter.check(ip1, RouteCategory::Default).is_ok());
        assert!(limiter.check(ip1, RouteCategory::Default).is_err());

        // ip2 still has its own budget
        assert!(limiter.check(ip2, RouteCategory::Default).is_ok());
    }

    #[test]
    fn different_categories_have_independent_limits() {
        let mut category_limits = HashMap::new();
        category_limits.insert(
            RouteCategory::Auth,
            CategoryLimit {
                max_requests: 1,
                window: Duration::from_secs(60),
            },
        );

        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 5,
                window: Duration::from_secs(60),
            },
            category_limits,
        };
        let limiter = RateLimiter::new(config);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();

        // Exhaust auth limit
        assert!(limiter.check(ip, RouteCategory::Auth).is_ok());
        assert!(limiter.check(ip, RouteCategory::Auth).is_err());

        // Default should still work
        assert!(limiter.check(ip, RouteCategory::Default).is_ok());
    }

    #[test]
    fn disabled_limiter_always_allows() {
        let config = RateLimitConfig {
            enabled: false,
            default_limit: CategoryLimit {
                max_requests: 0,
                window: Duration::from_secs(60),
            },
            category_limits: HashMap::new(),
        };
        let limiter = RateLimiter::new(config);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();

        // Even with max_requests=0, disabled limiter should allow
        assert!(limiter.check(ip, RouteCategory::Default).is_ok());
    }

    #[test]
    fn window_resets_after_duration() {
        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 1,
                window: Duration::from_millis(1), // 1ms window for testing
            },
            category_limits: HashMap::new(),
        };
        let limiter = RateLimiter::new(config);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();

        assert!(limiter.check(ip, RouteCategory::Default).is_ok());
        assert!(limiter.check(ip, RouteCategory::Default).is_err());

        // Wait for the window to expire
        std::thread::sleep(Duration::from_millis(5));

        // Should be allowed again
        assert!(limiter.check(ip, RouteCategory::Default).is_ok());
    }

    #[test]
    fn cleanup_removes_stale_entries() {
        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 10,
                window: Duration::from_millis(1),
            },
            category_limits: HashMap::new(),
        };
        let limiter = RateLimiter::new(config);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();

        limiter.check(ip, RouteCategory::Default).unwrap();
        assert_eq!(limiter.buckets.len(), 1);

        // Wait for 2× window to elapse
        std::thread::sleep(Duration::from_millis(5));
        limiter.cleanup();

        assert_eq!(limiter.buckets.len(), 0);
    }

    // --- Middleware integration tests ---

    #[tokio::test]
    async fn middleware_allows_request_within_limit() {
        let app = test_app(RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 5,
                window: Duration::from_secs(60),
            },
            category_limits: HashMap::new(),
        });

        let response = app.oneshot(make_request("/api/health")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Check rate limit headers
        let remaining = response
            .headers()
            .get("x-ratelimit-remaining")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(remaining, "4");

        let limit = response
            .headers()
            .get("x-ratelimit-limit")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(limit, "5");
    }

    #[tokio::test]
    async fn middleware_returns_429_when_exceeded() {
        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 2,
                window: Duration::from_secs(60),
            },
            category_limits: HashMap::new(),
        };
        let limiter = Arc::new(RateLimiter::new(config));
        let app = Router::new()
            .route("/api/health", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&limiter),
                rate_limit_middleware,
            ));

        // Use same IP for all requests
        let ip = "10.0.0.1";

        // First two succeed
        let req1 = Request::builder()
            .uri("/api/health")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        let r1 = app.clone().oneshot(req1).await.unwrap();
        assert_eq!(r1.status(), StatusCode::OK);

        let req2 = Request::builder()
            .uri("/api/health")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        let r2 = app.clone().oneshot(req2).await.unwrap();
        assert_eq!(r2.status(), StatusCode::OK);

        // Third request should be rate-limited
        let req3 = Request::builder()
            .uri("/api/health")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        let r3 = app.clone().oneshot(req3).await.unwrap();
        assert_eq!(r3.status(), StatusCode::TOO_MANY_REQUESTS);

        // Check Retry-After header
        let retry_after = r3
            .headers()
            .get("retry-after")
            .expect("should have retry-after header")
            .to_str()
            .unwrap();
        let retry_secs: u64 = retry_after.parse().unwrap();
        assert!(retry_secs >= 1);

        // Check response body is valid JSON
        let body = to_bytes(r3.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], 429);
        assert!(json["message"].as_str().unwrap().contains("Too many requests"));
    }

    #[tokio::test]
    async fn middleware_applies_auth_limit_to_auth_endpoints() {
        let mut category_limits = HashMap::new();
        category_limits.insert(
            RouteCategory::Auth,
            CategoryLimit {
                max_requests: 1,
                window: Duration::from_secs(60),
            },
        );
        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 100,
                window: Duration::from_secs(60),
            },
            category_limits,
        };
        let limiter = Arc::new(RateLimiter::new(config));
        let app = Router::new()
            .route(
                "/api/collections/users/auth-with-password",
                get(|| async { "auth" }),
            )
            .route("/api/health", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&limiter),
                rate_limit_middleware,
            ));

        let ip = "10.0.0.1";

        // Auth endpoint: first request ok
        let req = Request::builder()
            .uri("/api/collections/users/auth-with-password")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        let r = app.clone().oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);

        // Auth endpoint: second request rate-limited
        let req = Request::builder()
            .uri("/api/collections/users/auth-with-password")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        let r = app.clone().oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);

        // Default endpoint should still work (independent bucket)
        let req = Request::builder()
            .uri("/api/health")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        let r = app.clone().oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn middleware_disabled_allows_all() {
        let config = RateLimitConfig {
            enabled: false,
            default_limit: CategoryLimit {
                max_requests: 0,
                window: Duration::from_secs(60),
            },
            category_limits: HashMap::new(),
        };
        let app = test_app(config);

        let response = app.oneshot(make_request("/api/health")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn different_ips_tracked_independently_in_middleware() {
        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 1,
                window: Duration::from_secs(60),
            },
            category_limits: HashMap::new(),
        };
        let limiter = Arc::new(RateLimiter::new(config));
        let app = Router::new()
            .route("/api/health", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&limiter),
                rate_limit_middleware,
            ));

        // IP 1: first request ok
        let req = Request::builder()
            .uri("/api/health")
            .header("x-forwarded-for", "10.0.0.1")
            .body(Body::empty())
            .unwrap();
        let r = app.clone().oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);

        // IP 1: second request rate-limited
        let req = Request::builder()
            .uri("/api/health")
            .header("x-forwarded-for", "10.0.0.1")
            .body(Body::empty())
            .unwrap();
        let r = app.clone().oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);

        // IP 2: first request ok (independent bucket)
        let req = Request::builder()
            .uri("/api/health")
            .header("x-forwarded-for", "10.0.0.2")
            .body(Body::empty())
            .unwrap();
        let r = app.clone().oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
    }

    // --- IP extraction tests ---

    #[test]
    fn extract_ip_from_x_forwarded_for() {
        let req = Request::builder()
            .header("x-forwarded-for", "1.2.3.4, 5.6.7.8")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "1.2.3.4".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_ip_from_x_real_ip() {
        let req = Request::builder()
            .header("x-real-ip", "9.8.7.6")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "9.8.7.6".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_ip_fallback_to_localhost() {
        let req = Request::builder().body(Body::empty()).unwrap();
        assert_eq!(
            extract_client_ip(&req),
            IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
        );
    }

    #[test]
    fn extract_ip_x_forwarded_for_takes_precedence() {
        let req = Request::builder()
            .header("x-forwarded-for", "1.2.3.4")
            .header("x-real-ip", "5.6.7.8")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "1.2.3.4".parse::<IpAddr>().unwrap());
    }

    // --- Config tests ---

    #[test]
    fn default_config_values() {
        let config = RateLimitConfig::default();
        assert!(config.enabled);
        assert_eq!(config.default_limit.max_requests, 100);
        assert_eq!(config.default_limit.window, Duration::from_secs(60));

        let auth_limit = config.limit_for(RouteCategory::Auth);
        assert_eq!(auth_limit.max_requests, 10);
        assert_eq!(auth_limit.window, Duration::from_secs(60));
    }

    #[test]
    fn config_falls_back_to_default_for_unknown_category() {
        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 42,
                window: Duration::from_secs(30),
            },
            category_limits: HashMap::new(),
        };
        let limit = config.limit_for(RouteCategory::Default);
        assert_eq!(limit.max_requests, 42);
        assert_eq!(limit.window, Duration::from_secs(30));
    }

    // --- Security: Auth endpoint rate limiting (10 req/60s) ---

    #[tokio::test]
    async fn security_login_endpoint_enforces_auth_rate_limit() {
        let mut category_limits = HashMap::new();
        category_limits.insert(
            RouteCategory::Auth,
            CategoryLimit {
                max_requests: 3,
                window: Duration::from_secs(60),
            },
        );
        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 100,
                window: Duration::from_secs(60),
            },
            category_limits,
        };
        let limiter = Arc::new(RateLimiter::new(config));
        let app = Router::new()
            .route(
                "/api/collections/users/auth-with-password",
                get(|| async { "ok" }),
            )
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&limiter),
                rate_limit_middleware,
            ));

        let ip = "10.0.0.1";

        // First 3 requests succeed.
        for i in 0..3 {
            let req = Request::builder()
                .uri("/api/collections/users/auth-with-password")
                .header("x-forwarded-for", ip)
                .body(Body::empty())
                .unwrap();
            let r = app.clone().oneshot(req).await.unwrap();
            assert_eq!(r.status(), StatusCode::OK, "request {i} should succeed");
        }

        // 4th request rate-limited with Retry-After header.
        let req = Request::builder()
            .uri("/api/collections/users/auth-with-password")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        let r = app.clone().oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(
            r.headers().get("retry-after").is_some(),
            "429 should include Retry-After header"
        );
    }

    #[tokio::test]
    async fn security_otp_and_login_share_auth_bucket() {
        let mut category_limits = HashMap::new();
        category_limits.insert(
            RouteCategory::Auth,
            CategoryLimit {
                max_requests: 2,
                window: Duration::from_secs(60),
            },
        );
        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 100,
                window: Duration::from_secs(60),
            },
            category_limits,
        };
        let limiter = Arc::new(RateLimiter::new(config));
        let app = Router::new()
            .route(
                "/api/collections/users/request-otp",
                get(|| async { "ok" }),
            )
            .route(
                "/api/collections/users/auth-with-otp",
                get(|| async { "ok" }),
            )
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&limiter),
                rate_limit_middleware,
            ));

        let ip = "10.0.0.1";

        // Consume both with different auth endpoints.
        let req = Request::builder()
            .uri("/api/collections/users/request-otp")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::OK);

        let req = Request::builder()
            .uri("/api/collections/users/auth-with-otp")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::OK);

        // Third auth-category request rate-limited.
        let req = Request::builder()
            .uri("/api/collections/users/request-otp")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    #[tokio::test]
    async fn security_password_reset_shares_auth_bucket() {
        let mut category_limits = HashMap::new();
        category_limits.insert(
            RouteCategory::Auth,
            CategoryLimit {
                max_requests: 1,
                window: Duration::from_secs(60),
            },
        );
        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 100,
                window: Duration::from_secs(60),
            },
            category_limits,
        };
        let limiter = Arc::new(RateLimiter::new(config));
        let app = Router::new()
            .route(
                "/api/collections/users/request-password-reset",
                get(|| async { "ok" }),
            )
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&limiter),
                rate_limit_middleware,
            ));

        let ip = "10.0.0.1";

        let req = Request::builder()
            .uri("/api/collections/users/request-password-reset")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::OK);

        let req = Request::builder()
            .uri("/api/collections/users/request-password-reset")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    #[tokio::test]
    async fn security_auth_rate_limit_per_ip_isolation() {
        let mut category_limits = HashMap::new();
        category_limits.insert(
            RouteCategory::Auth,
            CategoryLimit {
                max_requests: 1,
                window: Duration::from_secs(60),
            },
        );
        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 100,
                window: Duration::from_secs(60),
            },
            category_limits,
        };
        let limiter = Arc::new(RateLimiter::new(config));
        let app = Router::new()
            .route(
                "/api/collections/users/auth-with-password",
                get(|| async { "ok" }),
            )
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&limiter),
                rate_limit_middleware,
            ));

        // IP 1 exhausts its quota.
        let req = Request::builder()
            .uri("/api/collections/users/auth-with-password")
            .header("x-forwarded-for", "10.0.0.1")
            .body(Body::empty())
            .unwrap();
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::OK);

        let req = Request::builder()
            .uri("/api/collections/users/auth-with-password")
            .header("x-forwarded-for", "10.0.0.1")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::TOO_MANY_REQUESTS
        );

        // IP 2 is unaffected.
        let req = Request::builder()
            .uri("/api/collections/users/auth-with-password")
            .header("x-forwarded-for", "10.0.0.2")
            .body(Body::empty())
            .unwrap();
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn security_exhausted_auth_bucket_does_not_block_default_routes() {
        let mut category_limits = HashMap::new();
        category_limits.insert(
            RouteCategory::Auth,
            CategoryLimit {
                max_requests: 1,
                window: Duration::from_secs(60),
            },
        );
        let config = RateLimitConfig {
            enabled: true,
            default_limit: CategoryLimit {
                max_requests: 100,
                window: Duration::from_secs(60),
            },
            category_limits,
        };
        let limiter = Arc::new(RateLimiter::new(config));
        let app = Router::new()
            .route(
                "/api/collections/users/auth-with-password",
                get(|| async { "auth" }),
            )
            .route("/api/health", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&limiter),
                rate_limit_middleware,
            ));

        let ip = "10.0.0.1";

        // Exhaust auth bucket.
        let req = Request::builder()
            .uri("/api/collections/users/auth-with-password")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::OK);

        let req = Request::builder()
            .uri("/api/collections/users/auth-with-password")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::TOO_MANY_REQUESTS
        );

        // Default bucket still works.
        let req = Request::builder()
            .uri("/api/health")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::OK);
    }
}
