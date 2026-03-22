//! Integration tests for the admin dashboard static file serving.
//!
//! Spins up a real HTTP server with the dashboard routes and verifies
//! that embedded files are served with correct status codes, MIME types,
//! cache headers, and SPA fallback routing.

use reqwest::{Client, StatusCode};
use tokio::net::TcpListener;

/// Spin up the dashboard router on a random port and return the base URL.
async fn spawn_dashboard_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://127.0.0.1:{}", addr.port());

    let handle = tokio::spawn(async move {
        let app = zerobase_admin::dashboard::dashboard_routes();
        axum::serve(listener, app).await.expect("server error");
    });

    base_url_ready(&base_url).await;
    (base_url, handle)
}

/// Wait until the server is accepting connections.
async fn base_url_ready(base_url: &str) {
    let client = Client::new();
    for _ in 0..50 {
        if client
            .get(format!("{base_url}/_/"))
            .send()
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("server did not become ready");
}

fn client() -> Client {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

#[tokio::test]
async fn dashboard_root_serves_index_html() {
    let (base_url, _handle) = spawn_dashboard_server().await;
    let resp = client()
        .get(format!("{base_url}/_/"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(ct.contains("text/html"), "expected text/html, got {ct}");

    let body = resp.text().await.unwrap();
    assert!(body.contains("<!DOCTYPE html>"), "body should contain HTML doctype");
    assert!(body.contains("Zerobase"), "body should mention Zerobase");
}

#[tokio::test]
async fn dashboard_login_page_is_served() {
    let (base_url, _handle) = spawn_dashboard_server().await;
    let resp = client()
        .get(format!("{base_url}/_/login"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(ct.contains("text/html"), "expected text/html, got {ct}");

    let body = resp.text().await.unwrap();
    assert!(body.contains("Sign In"), "login page should contain 'Sign In'");
}

#[tokio::test]
async fn dashboard_serves_js_with_correct_mime() {
    let (base_url, _handle) = spawn_dashboard_server().await;

    // Get the index page to find a JS asset reference
    let index_resp = client()
        .get(format!("{base_url}/_/"))
        .send()
        .await
        .unwrap();
    let body = index_resp.text().await.unwrap();

    // Extract a JS file path from the HTML (format: /_/_astro/Something.hash.js)
    let js_path = body
        .split("/_/")
        .filter_map(|segment| {
            let end = segment.find('"').or_else(|| segment.find('\''))?;
            let path = &segment[..end];
            if path.starts_with("_astro/") && path.ends_with(".js") {
                Some(path.to_string())
            } else {
                None
            }
        })
        .next()
        .expect("index.html should reference at least one JS bundle");

    let resp = client()
        .get(format!("{base_url}/_/{js_path}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(
        ct.contains("javascript"),
        "JS files should be served as javascript, got {ct}"
    );
}

#[tokio::test]
async fn dashboard_serves_css_with_correct_mime() {
    let (base_url, _handle) = spawn_dashboard_server().await;

    let index_resp = client()
        .get(format!("{base_url}/_/"))
        .send()
        .await
        .unwrap();
    let body = index_resp.text().await.unwrap();

    let css_path = body
        .split("/_/")
        .filter_map(|segment| {
            let end = segment.find('"').or_else(|| segment.find('\''))?;
            let path = &segment[..end];
            if path.ends_with(".css") {
                Some(path.to_string())
            } else {
                None
            }
        })
        .next()
        .expect("index.html should reference at least one CSS file");

    let resp = client()
        .get(format!("{base_url}/_/{css_path}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(ct.contains("css"), "CSS files should be served as text/css, got {ct}");
}

#[tokio::test]
async fn dashboard_hashed_assets_have_immutable_cache() {
    let (base_url, _handle) = spawn_dashboard_server().await;

    let index_resp = client()
        .get(format!("{base_url}/_/"))
        .send()
        .await
        .unwrap();
    let body = index_resp.text().await.unwrap();

    let asset_path = body
        .split("/_/")
        .filter_map(|segment| {
            let end = segment.find('"').or_else(|| segment.find('\''))?;
            let path = &segment[..end];
            if path.starts_with("_astro/") {
                Some(path.to_string())
            } else {
                None
            }
        })
        .next()
        .expect("index.html should reference _astro/ assets");

    let resp = client()
        .get(format!("{base_url}/_/{asset_path}"))
        .send()
        .await
        .unwrap();

    let cache = resp
        .headers()
        .get("cache-control")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        cache.contains("immutable"),
        "hashed assets should have immutable cache, got {cache}"
    );
}

#[tokio::test]
async fn dashboard_html_has_revalidate_cache() {
    let (base_url, _handle) = spawn_dashboard_server().await;
    let resp = client()
        .get(format!("{base_url}/_/"))
        .send()
        .await
        .unwrap();

    let cache = resp
        .headers()
        .get("cache-control")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        cache.contains("must-revalidate"),
        "HTML should have must-revalidate cache, got {cache}"
    );
}

#[tokio::test]
async fn dashboard_spa_fallback_serves_index() {
    let (base_url, _handle) = spawn_dashboard_server().await;

    // Request a path that doesn't exist as a file — should get index.html
    let resp = client()
        .get(format!("{base_url}/_/collections/some-dynamic-id"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(ct.contains("text/html"), "SPA fallback should serve HTML, got {ct}");

    let body = resp.text().await.unwrap();
    assert!(body.contains("<!DOCTYPE html>"), "SPA fallback should serve HTML content");
}

#[tokio::test]
async fn dashboard_favicon_is_served() {
    let (base_url, _handle) = spawn_dashboard_server().await;
    let resp = client()
        .get(format!("{base_url}/_/favicon.svg"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(
        ct.contains("svg") || ct.contains("xml"),
        "favicon.svg should be served as SVG, got {ct}"
    );
}

#[tokio::test]
async fn dashboard_settings_page_is_served() {
    let (base_url, _handle) = spawn_dashboard_server().await;
    let resp = client()
        .get(format!("{base_url}/_/settings"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(ct.contains("text/html"), "settings page should serve HTML, got {ct}");
}

#[tokio::test]
async fn dashboard_non_admin_paths_not_affected() {
    let (base_url, _handle) = spawn_dashboard_server().await;

    // Paths outside /_/ should not be handled by the dashboard
    let resp = client()
        .get(format!("{base_url}/api/health"))
        .send()
        .await
        .unwrap();

    // The dashboard router doesn't handle /api/ paths, so we expect 404
    // (since there's no other handler registered)
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
