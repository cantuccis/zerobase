//! Async webhook HTTP dispatcher with HMAC signing and retry.
//!
//! The dispatcher sends webhook payloads via HTTP POST with:
//! - JSON body containing the event, collection, and record data
//! - `Content-Type: application/json` header
//! - `X-Webhook-Event` header with the event name
//! - `X-Webhook-Signature` header with HMAC-SHA256 signature (when secret is set)
//!
//! Failed deliveries are retried up to 3 times with exponential backoff
//! (1s, 2s, 4s delays between attempts).

use std::sync::Arc;
use std::time::Duration;

use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::model::{
    Webhook, WebhookDeliveryLog, WebhookDeliveryStatus, WebhookEvent, WebhookPayload,
};
use super::service::WebhookRepository;
use crate::id::generate_id;

type HmacSha256 = Hmac<Sha256>;

/// Maximum number of delivery attempts per webhook invocation.
pub const MAX_ATTEMPTS: u8 = 3;

/// Base delay between retry attempts (doubles each time).
const BASE_RETRY_DELAY: Duration = Duration::from_secs(1);

/// Trait abstracting HTTP POST calls for testability.
///
/// The default implementation uses `reqwest`, but tests can provide
/// a mock to capture requests without network access.
#[async_trait::async_trait]
pub trait HttpSender: Send + Sync {
    /// Send an HTTP POST request and return the status code.
    ///
    /// Returns `Ok(status_code)` on any HTTP response, or `Err(message)` for
    /// network/connection failures.
    async fn post(
        &self,
        url: &str,
        body: &str,
        headers: Vec<(String, String)>,
    ) -> std::result::Result<u16, String>;
}

/// Default HTTP sender using reqwest.
#[derive(Clone)]
pub struct ReqwestSender {
    client: reqwest::Client,
}

impl ReqwestSender {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("failed to build reqwest client"),
        }
    }
}

impl Default for ReqwestSender {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl HttpSender for ReqwestSender {
    async fn post(
        &self,
        url: &str,
        body: &str,
        headers: Vec<(String, String)>,
    ) -> std::result::Result<u16, String> {
        let mut request = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .body(body.to_string());

        for (key, value) in &headers {
            request = request.header(key.as_str(), value.as_str());
        }

        match request.send().await {
            Ok(response) => Ok(response.status().as_u16()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Async webhook dispatcher that delivers payloads and logs results.
///
/// The dispatcher is designed to be spawned as a background task. It:
///
/// 1. Serializes the webhook payload to JSON
/// 2. Computes HMAC-SHA256 signature if a secret is configured
/// 3. Sends the HTTP POST request
/// 4. Retries on failure with exponential backoff (up to 3 attempts)
/// 5. Logs every delivery attempt to the repository
pub struct WebhookDispatcher<S: HttpSender, R: WebhookRepository> {
    sender: Arc<S>,
    repo: Arc<R>,
}

impl<S: HttpSender, R: WebhookRepository> WebhookDispatcher<S, R> {
    pub fn new(sender: Arc<S>, repo: Arc<R>) -> Self {
        Self { sender, repo }
    }

    /// Dispatch a webhook payload asynchronously.
    ///
    /// This method attempts delivery up to [`MAX_ATTEMPTS`] times with
    /// exponential backoff. Each attempt is logged to the repository.
    ///
    /// Returns the final delivery status.
    pub async fn dispatch(
        &self,
        webhook: &Webhook,
        event: WebhookEvent,
        collection: &str,
        record_id: &str,
        record: serde_json::Value,
    ) -> WebhookDeliveryStatus {
        let payload = WebhookPayload {
            event,
            collection: collection.to_string(),
            record,
        };

        let body = match serde_json::to_string(&payload) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(webhook_id = %webhook.id, "failed to serialize webhook payload: {e}");
                self.log_attempt(webhook, event, collection, record_id, 1, 0, Some(e.to_string()));
                return WebhookDeliveryStatus::Failed;
            }
        };

        let mut headers = vec![
            ("X-Webhook-Event".to_string(), event.to_string()),
            ("X-Webhook-Id".to_string(), webhook.id.clone()),
        ];

        // Compute HMAC signature if secret is set.
        if let Some(ref secret) = webhook.secret {
            match compute_hmac_signature(secret, &body) {
                Ok(sig) => headers.push(("X-Webhook-Signature".to_string(), sig)),
                Err(e) => {
                    tracing::error!(webhook_id = %webhook.id, "failed to compute HMAC: {e}");
                    self.log_attempt(webhook, event, collection, record_id, 1, 0, Some(e));
                    return WebhookDeliveryStatus::Failed;
                }
            }
        }

        let mut _last_error = None;
        for attempt in 1..=MAX_ATTEMPTS {
            // Apply backoff delay before retries (not before first attempt).
            if attempt > 1 {
                let delay = BASE_RETRY_DELAY * 2u32.pow((attempt - 2) as u32);
                tokio::time::sleep(delay).await;
            }

            match self.sender.post(&webhook.url, &body, headers.clone()).await {
                Ok(status) if (200..300).contains(&status) => {
                    tracing::info!(
                        webhook_id = %webhook.id,
                        attempt,
                        status,
                        "webhook delivered successfully"
                    );
                    self.log_attempt(
                        webhook,
                        event,
                        collection,
                        record_id,
                        attempt,
                        status,
                        None,
                    );
                    return WebhookDeliveryStatus::Success;
                }
                Ok(status) => {
                    let err_msg = format!("HTTP {status}");
                    tracing::warn!(
                        webhook_id = %webhook.id,
                        attempt,
                        status,
                        "webhook delivery got non-2xx response"
                    );
                    self.log_attempt(
                        webhook,
                        event,
                        collection,
                        record_id,
                        attempt,
                        status,
                        Some(err_msg.clone()),
                    );
                    _last_error = Some(err_msg);
                }
                Err(e) => {
                    tracing::warn!(
                        webhook_id = %webhook.id,
                        attempt,
                        error = %e,
                        "webhook delivery failed"
                    );
                    self.log_attempt(
                        webhook,
                        event,
                        collection,
                        record_id,
                        attempt,
                        0,
                        Some(e.clone()),
                    );
                    _last_error = Some(e);
                }
            }
        }

        tracing::error!(
            webhook_id = %webhook.id,
            max_attempts = MAX_ATTEMPTS,
            "webhook delivery failed after all retries"
        );
        WebhookDeliveryStatus::Failed
    }

    fn log_attempt(
        &self,
        webhook: &Webhook,
        event: WebhookEvent,
        collection: &str,
        record_id: &str,
        attempt: u8,
        response_status: u16,
        error: Option<String>,
    ) {
        let status = if error.is_none() && (200..300).contains(&(response_status as u32)) {
            WebhookDeliveryStatus::Success
        } else {
            WebhookDeliveryStatus::Failed
        };

        let log = WebhookDeliveryLog {
            id: generate_id(),
            webhook_id: webhook.id.clone(),
            event,
            collection: collection.to_string(),
            record_id: record_id.to_string(),
            url: webhook.url.clone(),
            response_status,
            attempt,
            status,
            error,
            created: chrono::Utc::now().to_rfc3339(),
        };

        if let Err(e) = self.repo.insert_delivery_log(&log) {
            tracing::error!(
                webhook_id = %webhook.id,
                "failed to log webhook delivery: {e}"
            );
        }
    }
}

/// Compute HMAC-SHA256 signature for a webhook payload.
///
/// Returns the hex-encoded signature prefixed with `sha256=`.
pub fn compute_hmac_signature(secret: &str, body: &str) -> std::result::Result<String, String> {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).map_err(|e| format!("HMAC error: {e}"))?;
    mac.update(body.as_bytes());
    let result = mac.finalize();
    let hex = hex::encode(result.into_bytes());
    Ok(format!("sha256={hex}"))
}

/// Verify an HMAC-SHA256 signature against a payload.
pub fn verify_hmac_signature(
    secret: &str,
    body: &str,
    signature: &str,
) -> std::result::Result<bool, String> {
    let expected = compute_hmac_signature(secret, body)?;
    Ok(expected == signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webhooks::service::InMemoryWebhookRepo;
    use std::sync::Mutex;

    // ── Mock HTTP sender ────────────────────────────────────────────────

    struct MockSender {
        responses: Mutex<Vec<std::result::Result<u16, String>>>,
        requests: Mutex<Vec<(String, String, Vec<(String, String)>)>>,
    }

    impl MockSender {
        fn new(responses: Vec<std::result::Result<u16, String>>) -> Self {
            Self {
                responses: Mutex::new(responses),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn request_count(&self) -> usize {
            self.requests.lock().unwrap().len()
        }

        fn last_request(&self) -> Option<(String, String, Vec<(String, String)>)> {
            self.requests.lock().unwrap().last().cloned()
        }
    }

    #[async_trait::async_trait]
    impl HttpSender for MockSender {
        async fn post(
            &self,
            url: &str,
            body: &str,
            headers: Vec<(String, String)>,
        ) -> std::result::Result<u16, String> {
            self.requests
                .lock()
                .unwrap()
                .push((url.to_string(), body.to_string(), headers));

            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Err("no more mock responses".to_string())
            } else {
                responses.remove(0)
            }
        }
    }

    fn make_webhook(secret: Option<&str>) -> Webhook {
        Webhook {
            id: "wh_test".to_string(),
            collection: "posts".to_string(),
            url: "https://example.com/hook".to_string(),
            events: vec![WebhookEvent::Create],
            secret: secret.map(|s| s.to_string()),
            enabled: true,
            created: "2026-01-01T00:00:00Z".to_string(),
            updated: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    // ── HMAC tests ──────────────────────────────────────────────────────

    #[test]
    fn hmac_signature_is_deterministic() {
        let sig1 = compute_hmac_signature("secret", "body").unwrap();
        let sig2 = compute_hmac_signature("secret", "body").unwrap();
        assert_eq!(sig1, sig2);
        assert!(sig1.starts_with("sha256="));
    }

    #[test]
    fn hmac_signature_differs_with_different_secrets() {
        let sig1 = compute_hmac_signature("secret1", "body").unwrap();
        let sig2 = compute_hmac_signature("secret2", "body").unwrap();
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn hmac_signature_differs_with_different_bodies() {
        let sig1 = compute_hmac_signature("secret", "body1").unwrap();
        let sig2 = compute_hmac_signature("secret", "body2").unwrap();
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn verify_hmac_valid() {
        let sig = compute_hmac_signature("my-secret", "payload").unwrap();
        assert!(verify_hmac_signature("my-secret", "payload", &sig).unwrap());
    }

    #[test]
    fn verify_hmac_invalid() {
        assert!(
            !verify_hmac_signature("my-secret", "payload", "sha256=deadbeef").unwrap()
        );
    }

    // ── Dispatch tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn dispatch_success_on_first_attempt() {
        let sender = Arc::new(MockSender::new(vec![Ok(200)]));
        let repo = Arc::new(InMemoryWebhookRepo::new());
        let dispatcher = WebhookDispatcher::new(sender.clone(), repo.clone());

        let webhook = make_webhook(None);
        let status = dispatcher
            .dispatch(
                &webhook,
                WebhookEvent::Create,
                "posts",
                "rec_1",
                serde_json::json!({"id": "rec_1", "title": "Test"}),
            )
            .await;

        assert_eq!(status, WebhookDeliveryStatus::Success);
        assert_eq!(sender.request_count(), 1);

        // Verify delivery was logged.
        let logs = repo.list_delivery_logs("wh_test", 10).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status, WebhookDeliveryStatus::Success);
        assert_eq!(logs[0].response_status, 200);
        assert_eq!(logs[0].attempt, 1);
    }

    #[tokio::test]
    async fn dispatch_sends_correct_headers() {
        let sender = Arc::new(MockSender::new(vec![Ok(200)]));
        let repo = Arc::new(InMemoryWebhookRepo::new());
        let dispatcher = WebhookDispatcher::new(sender.clone(), repo);

        let webhook = make_webhook(Some("test-secret"));
        dispatcher
            .dispatch(
                &webhook,
                WebhookEvent::Update,
                "posts",
                "rec_1",
                serde_json::json!({"id": "rec_1"}),
            )
            .await;

        let (url, body, headers) = sender.last_request().unwrap();
        assert_eq!(url, "https://example.com/hook");

        // Parse the body.
        let payload: WebhookPayload = serde_json::from_str(&body).unwrap();
        assert_eq!(payload.event, WebhookEvent::Update);
        assert_eq!(payload.collection, "posts");

        // Check headers.
        let header_map: std::collections::HashMap<_, _> =
            headers.into_iter().collect();
        assert_eq!(header_map.get("X-Webhook-Event").unwrap(), "update");
        assert_eq!(header_map.get("X-Webhook-Id").unwrap(), "wh_test");

        // Verify HMAC signature.
        let sig = header_map.get("X-Webhook-Signature").unwrap();
        assert!(sig.starts_with("sha256="));
        assert!(verify_hmac_signature("test-secret", &body, sig).unwrap());
    }

    #[tokio::test]
    async fn dispatch_no_signature_without_secret() {
        let sender = Arc::new(MockSender::new(vec![Ok(200)]));
        let repo = Arc::new(InMemoryWebhookRepo::new());
        let dispatcher = WebhookDispatcher::new(sender.clone(), repo);

        let webhook = make_webhook(None);
        dispatcher
            .dispatch(
                &webhook,
                WebhookEvent::Create,
                "posts",
                "rec_1",
                serde_json::json!({}),
            )
            .await;

        let (_, _, headers) = sender.last_request().unwrap();
        let header_map: std::collections::HashMap<_, _> =
            headers.into_iter().collect();
        assert!(!header_map.contains_key("X-Webhook-Signature"));
    }

    #[tokio::test]
    async fn dispatch_retries_on_failure() {
        // First two attempts fail, third succeeds.
        let sender = Arc::new(MockSender::new(vec![
            Err("connection refused".to_string()),
            Ok(500),
            Ok(200),
        ]));
        let repo = Arc::new(InMemoryWebhookRepo::new());
        let dispatcher = WebhookDispatcher::new(sender.clone(), repo.clone());

        let webhook = make_webhook(None);
        let status = dispatcher
            .dispatch(
                &webhook,
                WebhookEvent::Create,
                "posts",
                "rec_1",
                serde_json::json!({}),
            )
            .await;

        assert_eq!(status, WebhookDeliveryStatus::Success);
        assert_eq!(sender.request_count(), 3);

        // All 3 attempts should be logged.
        let logs = repo.list_delivery_logs("wh_test", 10).unwrap();
        assert_eq!(logs.len(), 3);
    }

    #[tokio::test]
    async fn dispatch_fails_after_max_attempts() {
        let sender = Arc::new(MockSender::new(vec![
            Ok(500),
            Ok(502),
            Ok(503),
        ]));
        let repo = Arc::new(InMemoryWebhookRepo::new());
        let dispatcher = WebhookDispatcher::new(sender.clone(), repo.clone());

        let webhook = make_webhook(None);
        let status = dispatcher
            .dispatch(
                &webhook,
                WebhookEvent::Delete,
                "posts",
                "rec_1",
                serde_json::json!({}),
            )
            .await;

        assert_eq!(status, WebhookDeliveryStatus::Failed);
        assert_eq!(sender.request_count(), 3);

        let logs = repo.list_delivery_logs("wh_test", 10).unwrap();
        assert_eq!(logs.len(), 3);
        // All attempts should be logged as failed.
        for log in &logs {
            assert_eq!(log.status, WebhookDeliveryStatus::Failed);
        }
    }

    #[tokio::test]
    async fn dispatch_network_error_all_retries() {
        let sender = Arc::new(MockSender::new(vec![
            Err("dns failure".to_string()),
            Err("connection timeout".to_string()),
            Err("connection reset".to_string()),
        ]));
        let repo = Arc::new(InMemoryWebhookRepo::new());
        let dispatcher = WebhookDispatcher::new(sender.clone(), repo.clone());

        let webhook = make_webhook(None);
        let status = dispatcher
            .dispatch(
                &webhook,
                WebhookEvent::Create,
                "posts",
                "rec_1",
                serde_json::json!({}),
            )
            .await;

        assert_eq!(status, WebhookDeliveryStatus::Failed);
        assert_eq!(sender.request_count(), 3);

        let logs = repo.list_delivery_logs("wh_test", 10).unwrap();
        assert_eq!(logs.len(), 3);
        assert!(logs[0].error.is_some());
    }
}
