//! Webhook domain types.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Unique identifier for a webhook configuration.
pub type WebhookId = String;

/// Events that can trigger a webhook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebhookEvent {
    /// A new record was created.
    Create,
    /// An existing record was updated.
    Update,
    /// A record was deleted.
    Delete,
}

impl fmt::Display for WebhookEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create => write!(f, "create"),
            Self::Update => write!(f, "update"),
            Self::Delete => write!(f, "delete"),
        }
    }
}

impl WebhookEvent {
    /// Parse an event name string into a `WebhookEvent`.
    pub fn from_str_checked(s: &str) -> Option<Self> {
        match s {
            "create" => Some(Self::Create),
            "update" => Some(Self::Update),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
}

/// A webhook configuration attached to a collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Webhook {
    /// Unique identifier.
    pub id: WebhookId,
    /// The collection this webhook is attached to.
    pub collection: String,
    /// The URL to send the webhook payload to.
    pub url: String,
    /// Which events trigger this webhook.
    pub events: Vec<WebhookEvent>,
    /// Optional secret used for HMAC-SHA256 signing of payloads.
    /// When set, a `X-Webhook-Signature` header is included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    /// Whether this webhook is active.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// ISO 8601 creation timestamp.
    pub created: String,
    /// ISO 8601 last-update timestamp.
    pub updated: String,
}

fn default_true() -> bool {
    true
}

/// Delivery status of a webhook invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebhookDeliveryStatus {
    /// Delivery succeeded (2xx response).
    Success,
    /// Delivery failed after all retry attempts.
    Failed,
    /// Delivery is pending (in-flight or queued for retry).
    Pending,
}

impl fmt::Display for WebhookDeliveryStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Success => write!(f, "success"),
            Self::Failed => write!(f, "failed"),
            Self::Pending => write!(f, "pending"),
        }
    }
}

/// A log entry for a single webhook delivery attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookDeliveryLog {
    /// Unique log entry ID.
    pub id: String,
    /// The webhook that was triggered.
    pub webhook_id: WebhookId,
    /// The event that triggered the delivery.
    pub event: WebhookEvent,
    /// The collection the event occurred on.
    pub collection: String,
    /// The record ID that triggered the event.
    pub record_id: String,
    /// The URL the payload was sent to.
    pub url: String,
    /// HTTP status code of the response (0 if network error).
    pub response_status: u16,
    /// Number of attempts made (1-based).
    pub attempt: u8,
    /// Final delivery status.
    pub status: WebhookDeliveryStatus,
    /// Error message if delivery failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// ISO 8601 timestamp of this log entry.
    pub created: String,
}

/// The payload sent in webhook HTTP requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookPayload {
    /// The event that occurred.
    pub event: WebhookEvent,
    /// The collection name.
    pub collection: String,
    /// The record data.
    pub record: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_event_display() {
        assert_eq!(WebhookEvent::Create.to_string(), "create");
        assert_eq!(WebhookEvent::Update.to_string(), "update");
        assert_eq!(WebhookEvent::Delete.to_string(), "delete");
    }

    #[test]
    fn webhook_event_from_str() {
        assert_eq!(
            WebhookEvent::from_str_checked("create"),
            Some(WebhookEvent::Create)
        );
        assert_eq!(
            WebhookEvent::from_str_checked("update"),
            Some(WebhookEvent::Update)
        );
        assert_eq!(
            WebhookEvent::from_str_checked("delete"),
            Some(WebhookEvent::Delete)
        );
        assert_eq!(WebhookEvent::from_str_checked("invalid"), None);
    }

    #[test]
    fn webhook_event_serde_roundtrip() {
        let events = vec![WebhookEvent::Create, WebhookEvent::Update, WebhookEvent::Delete];
        let json = serde_json::to_string(&events).unwrap();
        assert_eq!(json, r#"["create","update","delete"]"#);

        let parsed: Vec<WebhookEvent> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, events);
    }

    #[test]
    fn delivery_status_display() {
        assert_eq!(WebhookDeliveryStatus::Success.to_string(), "success");
        assert_eq!(WebhookDeliveryStatus::Failed.to_string(), "failed");
        assert_eq!(WebhookDeliveryStatus::Pending.to_string(), "pending");
    }

    #[test]
    fn webhook_serialization() {
        let webhook = Webhook {
            id: "wh_123".to_string(),
            collection: "posts".to_string(),
            url: "https://example.com/hook".to_string(),
            events: vec![WebhookEvent::Create, WebhookEvent::Delete],
            secret: Some("my-secret".to_string()),
            enabled: true,
            created: "2026-01-01T00:00:00Z".to_string(),
            updated: "2026-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_value(&webhook).unwrap();
        assert_eq!(json["collection"], "posts");
        assert_eq!(json["url"], "https://example.com/hook");
        assert_eq!(json["events"].as_array().unwrap().len(), 2);
        assert_eq!(json["secret"], "my-secret");
        assert_eq!(json["enabled"], true);
    }

    #[test]
    fn webhook_without_secret_omits_field() {
        let webhook = Webhook {
            id: "wh_123".to_string(),
            collection: "posts".to_string(),
            url: "https://example.com/hook".to_string(),
            events: vec![WebhookEvent::Create],
            secret: None,
            enabled: true,
            created: "2026-01-01T00:00:00Z".to_string(),
            updated: "2026-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_value(&webhook).unwrap();
        assert!(json.get("secret").is_none());
    }

    #[test]
    fn webhook_payload_serialization() {
        let payload = WebhookPayload {
            event: WebhookEvent::Create,
            collection: "posts".to_string(),
            record: serde_json::json!({"id": "rec_1", "title": "Hello"}),
        };

        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["event"], "create");
        assert_eq!(json["collection"], "posts");
        assert_eq!(json["record"]["id"], "rec_1");
    }
}
