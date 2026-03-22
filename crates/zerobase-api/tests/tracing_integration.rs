//! Integration tests for structured logging and request-ID propagation.
//!
//! These tests verify:
//! 1. JSON log output contains expected structured fields.
//! 2. Request IDs propagate from middleware into log spans.
//! 3. The `x-request-id` response header is set correctly.

use axum::body::Body;
use axum::http::Request;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter};

/// A thread-safe in-memory writer for capturing log output.
#[derive(Clone)]
struct TestWriter {
    buf: Arc<Mutex<Vec<u8>>>,
}

impl TestWriter {
    fn new() -> Self {
        Self {
            buf: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn contents(&self) -> String {
        let buf = self.buf.lock().unwrap();
        String::from_utf8_lossy(&buf).to_string()
    }
}

impl std::io::Write for TestWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buf.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for TestWriter {
    type Writer = TestWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Helper: build the API router and a scoped JSON tracing subscriber that
/// writes to a buffer.  Returns `(Router, TestWriter, DefaultGuard)`.
fn setup_traced_app() -> (axum::Router, TestWriter, tracing::subscriber::DefaultGuard) {
    let writer = TestWriter::new();

    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::new("zerobase_api=trace,tower_http=trace,info"))
        .with(
            fmt::layer()
                .json()
                .with_target(true)
                .with_writer(writer.clone()),
        );

    let guard = tracing::subscriber::set_default(subscriber);
    let app = zerobase_api::api_router();

    (app, writer, guard)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn health_check_returns_json_ok() {
    let app = zerobase_api::api_router();

    let request = Request::builder()
        .uri("/api/health")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "healthy");
}

#[tokio::test]
async fn response_contains_request_id_header() {
    let app = zerobase_api::api_router();

    let request = Request::builder()
        .uri("/api/health")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    let header = response.headers().get("x-request-id");
    assert!(
        header.is_some(),
        "response must include x-request-id header"
    );

    // Should be a valid UUID when no incoming header was set.
    let id = header.unwrap().to_str().unwrap();
    assert!(
        uuid::Uuid::parse_str(id).is_ok(),
        "auto-generated request id should be a valid UUID, got: {id}"
    );
}

#[tokio::test]
async fn caller_supplied_request_id_is_echoed() {
    let app = zerobase_api::api_router();
    let custom_id = "caller-trace-abc-123";

    let request = Request::builder()
        .uri("/api/health")
        .header("x-request-id", custom_id)
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    let echoed = response
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(echoed, custom_id);
}

#[tokio::test]
async fn cors_headers_present_on_preflight() {
    let app = zerobase_api::api_router();

    let request = Request::builder()
        .method("OPTIONS")
        .uri("/api/health")
        .header("origin", "http://example.com")
        .header("access-control-request-method", "GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert!(
        response
            .headers()
            .get("access-control-allow-origin")
            .is_some(),
        "CORS preflight must include access-control-allow-origin"
    );
}

#[tokio::test]
async fn json_log_output_contains_structured_fields() {
    let (app, writer, _guard) = setup_traced_app();

    let request = Request::builder()
        .uri("/api/health")
        .header("x-request-id", "test-trace-id-42")
        .body(Body::empty())
        .unwrap();

    let _response = app.oneshot(request).await.unwrap();

    let output = writer.contents();

    // There should be at least one JSON log line.
    assert!(!output.is_empty(), "expected log output, got nothing");

    // Each non-empty line should be valid JSON.
    let mut found_request_id = false;
    for line in output.lines().filter(|l| !l.is_empty()) {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| {
            panic!("log line is not valid JSON: {e}\nline: {line}");
        });

        // Structured fields we expect from tracing-subscriber JSON formatter.
        assert!(
            parsed.get("timestamp").is_some() || parsed.get("ts").is_some(),
            "log line should have a timestamp: {line}"
        );
        assert!(
            parsed.get("level").is_some(),
            "log line should have a level: {line}"
        );

        // Check if this line carries our request_id in its span data.
        let line_str = line.to_string();
        if line_str.contains("test-trace-id-42") {
            found_request_id = true;
        }
    }

    assert!(
        found_request_id,
        "at least one log line should contain the request_id 'test-trace-id-42'.\nFull output:\n{output}"
    );
}

#[tokio::test]
async fn json_log_lines_are_valid_json() {
    let (app, writer, _guard) = setup_traced_app();

    // Fire a few requests.
    let app_clone = app.clone();
    let req1 = Request::builder()
        .uri("/api/health")
        .body(Body::empty())
        .unwrap();
    let _ = app_clone.oneshot(req1).await.unwrap();

    let req2 = Request::builder()
        .uri("/api/health")
        .body(Body::empty())
        .unwrap();
    let _ = app.oneshot(req2).await.unwrap();

    let output = writer.contents();
    let lines: Vec<&str> = output.lines().filter(|l| !l.is_empty()).collect();
    assert!(
        !lines.is_empty(),
        "should have produced at least one log line"
    );

    for line in &lines {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(
            parsed.is_ok(),
            "every log line must be valid JSON.\nInvalid line: {line}"
        );
    }
}
