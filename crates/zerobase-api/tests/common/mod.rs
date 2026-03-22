//! Shared test utilities for `zerobase-api` integration tests.
//!
//! Provides [`TestApp`] to spin up a full application stack on a random port,
//! and [`TestClient`] for ergonomic HTTP assertions.
//!
//! # Usage
//!
//! ```rust,ignore
//! mod common;
//!
//! #[tokio::test]
//! async fn my_test() {
//!     let app = common::TestApp::spawn().await;
//!     let client = app.client();
//!     let resp = client.get_response("/api/health").await;
//!     common::assert_status(&resp, StatusCode::OK);
//! }
//! ```

use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// TestApp
// ---------------------------------------------------------------------------

/// A running application instance bound to an OS-assigned port.
///
/// Create via [`TestApp::spawn`]. The server runs in a background tokio task
/// and is automatically cleaned up when the `TestApp` is dropped.
pub struct TestApp {
    /// Base URL of the running server, e.g. `http://127.0.0.1:12345`.
    pub address: String,
    /// The port the server is listening on.
    pub port: u16,
    /// Handle to the background server task — aborted on drop.
    _server_handle: tokio::task::JoinHandle<()>,
}

impl TestApp {
    /// Boot the full API on a random port and return a ready-to-use [`TestApp`].
    ///
    /// The server uses the standard [`zerobase_api::api_router()`] and listens
    /// on `127.0.0.1:0` so the OS assigns an available port.
    pub async fn spawn() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind to random port");
        let addr = listener.local_addr().unwrap();
        let port = addr.port();
        let address = format!("http://127.0.0.1:{port}");

        let server_handle = tokio::spawn(async move {
            let app = zerobase_api::api_router();
            axum::serve(listener, app)
                .await
                .expect("server exited with error");
        });

        Self {
            address,
            port,
            _server_handle: server_handle,
        }
    }

    /// Return a [`TestClient`] bound to this app's base URL.
    pub fn client(&self) -> TestClient {
        TestClient::new(&self.address)
    }

    /// Build a full URL for the given `path` (which should start with `/`).
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.address, path)
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        self._server_handle.abort();
    }
}

// ---------------------------------------------------------------------------
// TestClient
// ---------------------------------------------------------------------------

/// Ergonomic HTTP client for integration tests.
///
/// Wraps [`reqwest::Client`] with convenience methods for every HTTP verb,
/// typed JSON deserialization, and common assertion patterns.
pub struct TestClient {
    inner: Client,
    base_url: String,
}

impl TestClient {
    /// Create a new client targeting the given base URL.
    pub fn new(base_url: &str) -> Self {
        Self {
            inner: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("failed to build reqwest client"),
            base_url: base_url.to_string(),
        }
    }

    // ---- HTTP verb helpers ------------------------------------------------

    /// Start building a GET request to `path`.
    pub fn get(&self, path: &str) -> RequestBuilder {
        self.inner.get(self.url(path))
    }

    /// Start building a POST request to `path`.
    pub fn post(&self, path: &str) -> RequestBuilder {
        self.inner.post(self.url(path))
    }

    /// Start building a PUT request to `path`.
    pub fn put(&self, path: &str) -> RequestBuilder {
        self.inner.put(self.url(path))
    }

    /// Start building a PATCH request to `path`.
    pub fn patch(&self, path: &str) -> RequestBuilder {
        self.inner.patch(self.url(path))
    }

    /// Start building a DELETE request to `path`.
    pub fn delete(&self, path: &str) -> RequestBuilder {
        self.inner.delete(self.url(path))
    }

    /// Start building a request with an arbitrary HTTP method.
    pub fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.inner.request(method, self.url(path))
    }

    // ---- Convenience "fire and forget" helpers ----------------------------

    /// Send a GET request and return the response (panics on network error).
    pub async fn get_response(&self, path: &str) -> Response {
        self.get(path)
            .send()
            .await
            .expect("failed to send GET request")
    }

    /// Send a POST with a JSON body and return the response.
    pub async fn post_json<T: Serialize>(&self, path: &str, body: &T) -> Response {
        self.post(path)
            .json(body)
            .send()
            .await
            .expect("failed to send POST request")
    }

    /// Send a PUT with a JSON body and return the response.
    pub async fn put_json<T: Serialize>(&self, path: &str, body: &T) -> Response {
        self.put(path)
            .json(body)
            .send()
            .await
            .expect("failed to send PUT request")
    }

    /// Send a PATCH with a JSON body and return the response.
    pub async fn patch_json<T: Serialize>(&self, path: &str, body: &T) -> Response {
        self.patch(path)
            .json(body)
            .send()
            .await
            .expect("failed to send PATCH request")
    }

    /// Send a DELETE request and return the response.
    pub async fn delete_response(&self, path: &str) -> Response {
        self.delete(path)
            .send()
            .await
            .expect("failed to send DELETE request")
    }

    // ---- URL builder ------------------------------------------------------

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

// ---------------------------------------------------------------------------
// Assertion helpers
// ---------------------------------------------------------------------------

/// Assert the response has the expected status code.
///
/// Returns a reference to the response for method chaining.
pub fn assert_status(response: &Response, expected: StatusCode) {
    assert_eq!(
        response.status(),
        expected,
        "expected HTTP {expected}, got HTTP {}",
        response.status(),
    );
}

/// Consume the response, assert its status, and deserialize the JSON body.
///
/// Panics if the status doesn't match or the body isn't valid JSON of type `T`.
pub async fn assert_json_response<T: DeserializeOwned>(
    response: Response,
    expected_status: StatusCode,
) -> T {
    assert_eq!(
        response.status(),
        expected_status,
        "expected HTTP {expected_status}, got HTTP {}",
        response.status(),
    );
    response
        .json::<T>()
        .await
        .expect("failed to deserialize response body as JSON")
}

/// Assert the response contains a header with exactly the given value.
pub fn assert_header(response: &Response, name: &str, expected_value: &str) {
    let actual = response
        .headers()
        .get(name)
        .unwrap_or_else(|| panic!("response missing header `{name}`"))
        .to_str()
        .unwrap_or_else(|_| panic!("header `{name}` is not valid UTF-8"));
    assert_eq!(
        actual, expected_value,
        "header `{name}`: expected `{expected_value}`, got `{actual}`",
    );
}

/// Assert the response contains the named header (any value).
pub fn assert_header_exists(response: &Response, name: &str) {
    assert!(
        response.headers().get(name).is_some(),
        "expected response to contain header `{name}`",
    );
}

/// Assert that `x-request-id` is present and is a valid UUID.
pub fn assert_request_id_is_uuid(response: &Response) {
    let id = response
        .headers()
        .get("x-request-id")
        .expect("response missing x-request-id header")
        .to_str()
        .expect("x-request-id is not valid UTF-8");
    assert!(
        uuid::Uuid::parse_str(id).is_ok(),
        "x-request-id should be a valid UUID, got: {id}",
    );
}
