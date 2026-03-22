//! Admin dashboard — embedded static file serving.
//!
//! Embeds the AstroJS-built admin dashboard into the Rust binary using
//! `rust-embed`. Files are served at `/_/` to match the PocketBase convention.
//!
//! # Modes
//!
//! - **Production** (default, feature `embed-dashboard`): serves files compiled
//!   into the binary. Zero external dependencies at runtime.
//! - **Development** (feature `dev-proxy`): proxies requests to the AstroJS dev
//!   server (default `http://localhost:4321`) for hot-reload support.

use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use rust_embed::Embed;
use tracing::debug;

/// Embedded frontend assets from the AstroJS build output.
///
/// At compile time, `rust-embed` walks `frontend/dist` relative to the
/// workspace root and includes every file as `&[u8]`.
///
/// In debug builds the folder attribute points to the same path but files
/// are read from disk at runtime, which means you can rebuild the frontend
/// without recompiling the server.
#[derive(Embed)]
#[folder = "../../frontend/dist"]
struct DashboardAssets;

/// Build the admin dashboard [`Router`].
///
/// All paths under `/_/` are handled:
///
/// - Known static files (JS, CSS, images) are served with correct MIME types
///   and cache headers.
/// - Unknown paths fall back to `index.html` for client-side routing.
///
/// # Example
///
/// ```rust,ignore
/// let app = Router::new()
///     .merge(zerobase_admin::dashboard::dashboard_routes());
/// ```
pub fn dashboard_routes() -> Router {
    Router::new()
        // Serve the root `/_/` path
        .route("/_/", get(serve_index))
        // Catch-all for everything else under `/_/`
        .route("/_/{*path}", get(serve_dashboard_file))
}

/// Serve `index.html` for the dashboard root.
async fn serve_index() -> impl IntoResponse {
    serve_embedded_file("index.html")
}

/// Serve an embedded file or fall back to `index.html` for SPA routing.
async fn serve_dashboard_file(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    // First, try the exact path
    if let Some(resp) = try_serve_file(&path) {
        return resp;
    }

    // Try path/index.html (for Astro's directory-based routes)
    let index_path = if path.ends_with('/') {
        format!("{path}index.html")
    } else {
        format!("{path}/index.html")
    };
    if let Some(resp) = try_serve_file(&index_path) {
        return resp;
    }

    // SPA fallback: serve index.html for client-side routing
    debug!(path = %path, "SPA fallback to index.html");
    serve_embedded_file("index.html")
}

/// Try to serve a file from the embedded assets. Returns `None` if not found.
fn try_serve_file(path: &str) -> Option<Response> {
    DashboardAssets::get(path).map(|file| {
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();

        let cache_control = if path.contains("_astro/") {
            // Hashed assets are immutable — cache aggressively
            "public, max-age=31536000, immutable"
        } else {
            // HTML and other files — revalidate
            "public, max-age=0, must-revalidate"
        };

        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, cache_control)
            .body(Body::from(file.data.to_vec()))
            .unwrap()
    })
}

/// Serve a specific embedded file, returning 404 if missing.
fn serve_embedded_file(path: &str) -> Response {
    match try_serve_file(path) {
        Some(resp) => resp,
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Body::from("404 Not Found"))
            .unwrap(),
    }
}

/// Configuration for the dev proxy mode.
#[cfg(feature = "dev-proxy")]
#[derive(Debug, Clone)]
pub struct DevProxyConfig {
    /// URL of the AstroJS dev server (e.g. `http://localhost:4321`).
    pub upstream_url: String,
}

#[cfg(feature = "dev-proxy")]
impl Default for DevProxyConfig {
    fn default() -> Self {
        Self {
            upstream_url: "http://localhost:4321".to_string(),
        }
    }
}

/// Build a dev proxy router that forwards `/_/` requests to the AstroJS dev server.
///
/// This enables hot-reload during development: the Rust server proxies all
/// dashboard requests to `astro dev` running separately.
#[cfg(feature = "dev-proxy")]
pub fn dev_proxy_routes(config: DevProxyConfig) -> Router {
    let client = reqwest::Client::new();

    Router::new()
        .route("/_/", get(proxy_handler))
        .route("/_/{*path}", get(proxy_handler))
        .with_state(DevProxyState {
            client,
            upstream_url: config.upstream_url,
        })
}

#[cfg(feature = "dev-proxy")]
#[derive(Clone)]
struct DevProxyState {
    client: reqwest::Client,
    upstream_url: String,
}

#[cfg(feature = "dev-proxy")]
async fn proxy_handler(
    axum::extract::State(state): axum::extract::State<DevProxyState>,
    req: Request,
) -> Response {
    let path = req.uri().path();
    let query = req.uri().query().map(|q| format!("?{q}")).unwrap_or_default();
    let upstream = format!("{}{path}{query}", state.upstream_url);

    debug!(upstream = %upstream, "proxying to dev server");

    match state.client.get(&upstream).send().await {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut builder = Response::builder().status(status);

            for (name, value) in resp.headers() {
                if let Ok(name) = header::HeaderName::from_bytes(name.as_ref()) {
                    builder = builder.header(name, value.as_bytes());
                }
            }

            let body = resp.bytes().await.unwrap_or_default();
            builder.body(Body::from(body)).unwrap()
        }
        Err(e) => {
            tracing::warn!(error = %e, "dev proxy request failed");
            Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .header(header::CONTENT_TYPE, "text/plain")
                .body(Body::from(format!("Dev proxy error: {e}")))
                .unwrap()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_assets_contain_index_html() {
        let file = DashboardAssets::get("index.html");
        assert!(file.is_some(), "index.html must be embedded");
    }

    #[test]
    fn embedded_assets_contain_login_page() {
        let file = DashboardAssets::get("login/index.html");
        assert!(file.is_some(), "login/index.html must be embedded");
    }

    #[test]
    fn embedded_assets_contain_astro_bundles() {
        // At least one JS bundle should exist in _astro/
        let has_js = DashboardAssets::iter().any(|path| {
            path.starts_with("_astro/") && path.ends_with(".js")
        });
        assert!(has_js, "embedded assets must contain _astro/*.js bundles");
    }

    #[test]
    fn embedded_assets_contain_css() {
        let has_css = DashboardAssets::iter().any(|path| {
            path.ends_with(".css")
        });
        assert!(has_css, "embedded assets must contain CSS files");
    }

    #[test]
    fn serve_index_returns_html() {
        let resp = serve_embedded_file("index.html");
        assert_eq!(resp.status(), StatusCode::OK);
        let content_type = resp.headers().get(header::CONTENT_TYPE).unwrap().to_str().unwrap();
        assert!(content_type.contains("text/html"), "index.html should be served as text/html, got {content_type}");
    }

    #[test]
    fn serve_js_returns_correct_mime() {
        // Find any JS file to test
        let js_file = DashboardAssets::iter()
            .find(|p| p.ends_with(".js"))
            .expect("at least one JS file must be embedded");

        let resp = serve_embedded_file(&js_file);
        assert_eq!(resp.status(), StatusCode::OK);
        let content_type = resp.headers().get(header::CONTENT_TYPE).unwrap().to_str().unwrap();
        assert!(
            content_type.contains("javascript"),
            "JS files should have javascript MIME type, got {content_type}"
        );
    }

    #[test]
    fn serve_css_returns_correct_mime() {
        let css_file = DashboardAssets::iter()
            .find(|p| p.ends_with(".css"))
            .expect("at least one CSS file must be embedded");

        let resp = serve_embedded_file(&css_file);
        assert_eq!(resp.status(), StatusCode::OK);
        let content_type = resp.headers().get(header::CONTENT_TYPE).unwrap().to_str().unwrap();
        assert!(
            content_type.contains("css"),
            "CSS files should have css MIME type, got {content_type}"
        );
    }

    #[test]
    fn serve_missing_file_returns_404() {
        let resp = serve_embedded_file("nonexistent/file.xyz");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn hashed_assets_get_immutable_cache() {
        let js_file = DashboardAssets::iter()
            .find(|p| p.starts_with("_astro/") && p.ends_with(".js"))
            .expect("at least one _astro/*.js file must be embedded");

        let resp = try_serve_file(&js_file).unwrap();
        let cache = resp.headers().get(header::CACHE_CONTROL).unwrap().to_str().unwrap();
        assert!(
            cache.contains("immutable"),
            "hashed assets should have immutable cache-control, got {cache}"
        );
    }

    #[test]
    fn html_files_get_revalidate_cache() {
        let resp = try_serve_file("index.html").unwrap();
        let cache = resp.headers().get(header::CACHE_CONTROL).unwrap().to_str().unwrap();
        assert!(
            cache.contains("must-revalidate"),
            "HTML files should have must-revalidate cache-control, got {cache}"
        );
    }

    #[test]
    fn favicon_is_embedded() {
        assert!(DashboardAssets::get("favicon.svg").is_some(), "favicon.svg must be embedded");
    }

    #[test]
    fn all_embedded_files_are_listed() {
        let files: Vec<_> = DashboardAssets::iter().collect();
        assert!(
            files.len() >= 5,
            "expected at least 5 embedded files, found {}",
            files.len()
        );
    }
}
