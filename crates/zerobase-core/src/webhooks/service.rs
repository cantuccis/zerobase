//! Webhook CRUD service and repository trait.

use crate::error::{Result, ZerobaseError};
use crate::id::generate_id;

use super::model::{Webhook, WebhookDeliveryLog, WebhookEvent};

// ── Repository trait ────────────────────────────────────────────────────────

/// Persistence contract for webhook configurations and delivery logs.
///
/// Defined in core so the service doesn't depend on `zerobase-db` directly.
pub trait WebhookRepository: Send + Sync {
    /// List all webhooks, optionally filtered by collection name.
    fn list_webhooks(
        &self,
        collection: Option<&str>,
    ) -> std::result::Result<Vec<Webhook>, WebhookRepoError>;

    /// Get a webhook by its ID.
    fn get_webhook(&self, id: &str) -> std::result::Result<Webhook, WebhookRepoError>;

    /// Insert a new webhook.
    fn insert_webhook(&self, webhook: &Webhook) -> std::result::Result<(), WebhookRepoError>;

    /// Update an existing webhook. Returns true if a row was modified.
    fn update_webhook(&self, webhook: &Webhook) -> std::result::Result<bool, WebhookRepoError>;

    /// Delete a webhook by its ID. Returns true if a row was deleted.
    fn delete_webhook(&self, id: &str) -> std::result::Result<bool, WebhookRepoError>;

    /// Get all enabled webhooks for a given collection and event.
    fn get_active_webhooks(
        &self,
        collection: &str,
        event: WebhookEvent,
    ) -> std::result::Result<Vec<Webhook>, WebhookRepoError>;

    /// Insert a delivery log entry.
    fn insert_delivery_log(
        &self,
        log: &WebhookDeliveryLog,
    ) -> std::result::Result<(), WebhookRepoError>;

    /// List delivery logs for a webhook, ordered by creation time descending.
    fn list_delivery_logs(
        &self,
        webhook_id: &str,
        limit: u32,
    ) -> std::result::Result<Vec<WebhookDeliveryLog>, WebhookRepoError>;
}

/// Errors from the webhook repository layer.
#[derive(Debug, thiserror::Error)]
pub enum WebhookRepoError {
    #[error("webhook not found: {id}")]
    NotFound { id: String },

    #[error("database error: {0}")]
    Database(String),
}

impl From<WebhookRepoError> for ZerobaseError {
    fn from(e: WebhookRepoError) -> Self {
        match e {
            WebhookRepoError::NotFound { id } => ZerobaseError::NotFound {
                resource_type: "Webhook".to_string(),
                resource_id: Some(id),
            },
            WebhookRepoError::Database(msg) => ZerobaseError::Database {
                message: msg,
                source: None,
            },
        }
    }
}

// ── WebhookService ──────────────────────────────────────────────────────────

/// Service for managing webhook configurations.
pub struct WebhookService<R: WebhookRepository> {
    repo: R,
}

impl<R: WebhookRepository> WebhookService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    /// Access the underlying repository.
    pub fn repo(&self) -> &R {
        &self.repo
    }

    /// List all webhooks, optionally filtered by collection.
    pub fn list(&self, collection: Option<&str>) -> Result<Vec<Webhook>> {
        Ok(self.repo.list_webhooks(collection)?)
    }

    /// Get a single webhook by ID.
    pub fn get(&self, id: &str) -> Result<Webhook> {
        Ok(self.repo.get_webhook(id)?)
    }

    /// Create a new webhook.
    ///
    /// Validates the URL and events before persisting.
    pub fn create(&self, input: CreateWebhookInput) -> Result<Webhook> {
        validate_webhook_input(&input.url, &input.events)?;

        let now = chrono::Utc::now().to_rfc3339();
        let webhook = Webhook {
            id: generate_id(),
            collection: input.collection,
            url: input.url,
            events: input.events,
            secret: input.secret,
            enabled: input.enabled.unwrap_or(true),
            created: now.clone(),
            updated: now,
        };

        self.repo.insert_webhook(&webhook)?;
        Ok(webhook)
    }

    /// Update an existing webhook.
    pub fn update(&self, id: &str, input: UpdateWebhookInput) -> Result<Webhook> {
        let mut webhook = self.repo.get_webhook(id)?;

        if let Some(url) = input.url {
            validate_url(&url)?;
            webhook.url = url;
        }
        if let Some(events) = input.events {
            validate_events(&events)?;
            webhook.events = events;
        }
        if let Some(secret) = input.secret {
            webhook.secret = secret;
        }
        if let Some(enabled) = input.enabled {
            webhook.enabled = enabled;
        }
        if let Some(collection) = input.collection {
            webhook.collection = collection;
        }

        webhook.updated = chrono::Utc::now().to_rfc3339();
        self.repo.update_webhook(&webhook)?;
        Ok(webhook)
    }

    /// Delete a webhook by ID.
    pub fn delete(&self, id: &str) -> Result<()> {
        let deleted = self.repo.delete_webhook(id)?;
        if !deleted {
            return Err(ZerobaseError::not_found_with_id("Webhook", id));
        }
        Ok(())
    }

    /// Get all active (enabled) webhooks for a given collection and event.
    pub fn get_active_for_event(
        &self,
        collection: &str,
        event: WebhookEvent,
    ) -> Result<Vec<Webhook>> {
        Ok(self.repo.get_active_webhooks(collection, event)?)
    }

    /// List delivery logs for a webhook.
    pub fn delivery_logs(&self, webhook_id: &str, limit: u32) -> Result<Vec<WebhookDeliveryLog>> {
        Ok(self.repo.list_delivery_logs(webhook_id, limit)?)
    }

    /// Log a delivery attempt.
    pub fn log_delivery(&self, log: &WebhookDeliveryLog) -> Result<()> {
        Ok(self.repo.insert_delivery_log(log)?)
    }
}

// ── Input types ─────────────────────────────────────────────────────────────

/// Input for creating a new webhook.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateWebhookInput {
    pub collection: String,
    pub url: String,
    pub events: Vec<WebhookEvent>,
    #[serde(default)]
    pub secret: Option<String>,
    pub enabled: Option<bool>,
}

/// Input for updating an existing webhook.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateWebhookInput {
    pub collection: Option<String>,
    pub url: Option<String>,
    pub events: Option<Vec<WebhookEvent>>,
    pub secret: Option<Option<String>>,
    pub enabled: Option<bool>,
}

use serde::Deserialize;

// ── Validation helpers ──────────────────────────────────────────────────────

fn validate_webhook_input(url: &str, events: &[WebhookEvent]) -> Result<()> {
    validate_url(url)?;
    validate_events(events)?;
    Ok(())
}

fn validate_url(url: &str) -> Result<()> {
    if url.is_empty() {
        return Err(ZerobaseError::validation("webhook URL must not be empty"));
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ZerobaseError::validation(
            "webhook URL must start with http:// or https://",
        ));
    }
    Ok(())
}

fn validate_events(events: &[WebhookEvent]) -> Result<()> {
    if events.is_empty() {
        return Err(ZerobaseError::validation(
            "webhook must have at least one event",
        ));
    }

    // Check for duplicates.
    let mut seen = std::collections::HashSet::new();
    for event in events {
        if !seen.insert(event) {
            return Err(ZerobaseError::validation(format!(
                "duplicate webhook event: {event}"
            )));
        }
    }

    Ok(())
}

// ── In-memory mock for testing ──────────────────────────────────────────────

/// In-memory webhook repository for unit tests.
#[cfg(test)]
pub struct InMemoryWebhookRepo {
    webhooks: std::sync::Mutex<Vec<Webhook>>,
    logs: std::sync::Mutex<Vec<WebhookDeliveryLog>>,
}

#[cfg(test)]
impl InMemoryWebhookRepo {
    pub fn new() -> Self {
        Self {
            webhooks: std::sync::Mutex::new(Vec::new()),
            logs: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn with_webhooks(webhooks: Vec<Webhook>) -> Self {
        Self {
            webhooks: std::sync::Mutex::new(webhooks),
            logs: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl Default for InMemoryWebhookRepo {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl WebhookRepository for InMemoryWebhookRepo {
    fn list_webhooks(
        &self,
        collection: Option<&str>,
    ) -> std::result::Result<Vec<Webhook>, WebhookRepoError> {
        let webhooks = self.webhooks.lock().unwrap();
        Ok(match collection {
            Some(c) => webhooks.iter().filter(|w| w.collection == c).cloned().collect(),
            None => webhooks.clone(),
        })
    }

    fn get_webhook(&self, id: &str) -> std::result::Result<Webhook, WebhookRepoError> {
        let webhooks = self.webhooks.lock().unwrap();
        webhooks
            .iter()
            .find(|w| w.id == id)
            .cloned()
            .ok_or_else(|| WebhookRepoError::NotFound { id: id.to_string() })
    }

    fn insert_webhook(&self, webhook: &Webhook) -> std::result::Result<(), WebhookRepoError> {
        let mut webhooks = self.webhooks.lock().unwrap();
        webhooks.push(webhook.clone());
        Ok(())
    }

    fn update_webhook(&self, webhook: &Webhook) -> std::result::Result<bool, WebhookRepoError> {
        let mut webhooks = self.webhooks.lock().unwrap();
        if let Some(existing) = webhooks.iter_mut().find(|w| w.id == webhook.id) {
            *existing = webhook.clone();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn delete_webhook(&self, id: &str) -> std::result::Result<bool, WebhookRepoError> {
        let mut webhooks = self.webhooks.lock().unwrap();
        let before = webhooks.len();
        webhooks.retain(|w| w.id != id);
        Ok(webhooks.len() < before)
    }

    fn get_active_webhooks(
        &self,
        collection: &str,
        event: WebhookEvent,
    ) -> std::result::Result<Vec<Webhook>, WebhookRepoError> {
        let webhooks = self.webhooks.lock().unwrap();
        Ok(webhooks
            .iter()
            .filter(|w| w.enabled && w.collection == collection && w.events.contains(&event))
            .cloned()
            .collect())
    }

    fn insert_delivery_log(
        &self,
        log: &WebhookDeliveryLog,
    ) -> std::result::Result<(), WebhookRepoError> {
        let mut logs = self.logs.lock().unwrap();
        logs.push(log.clone());
        Ok(())
    }

    fn list_delivery_logs(
        &self,
        webhook_id: &str,
        limit: u32,
    ) -> std::result::Result<Vec<WebhookDeliveryLog>, WebhookRepoError> {
        let logs = self.logs.lock().unwrap();
        let mut matching: Vec<_> = logs
            .iter()
            .filter(|l| l.webhook_id == webhook_id)
            .cloned()
            .collect();
        matching.sort_by(|a, b| b.created.cmp(&a.created));
        matching.truncate(limit as usize);
        Ok(matching)
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_input() -> CreateWebhookInput {
        CreateWebhookInput {
            collection: "posts".to_string(),
            url: "https://example.com/webhook".to_string(),
            events: vec![WebhookEvent::Create, WebhookEvent::Update],
            secret: Some("my-secret".to_string()),
            enabled: None,
        }
    }

    #[test]
    fn create_webhook_generates_id_and_timestamps() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        let webhook = service.create(make_input()).unwrap();

        assert!(!webhook.id.is_empty());
        assert_eq!(webhook.collection, "posts");
        assert_eq!(webhook.url, "https://example.com/webhook");
        assert_eq!(webhook.events.len(), 2);
        assert_eq!(webhook.secret, Some("my-secret".to_string()));
        assert!(webhook.enabled);
        assert!(!webhook.created.is_empty());
        assert!(!webhook.updated.is_empty());
    }

    #[test]
    fn create_webhook_defaults_enabled_to_true() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        let mut input = make_input();
        input.enabled = None;
        let webhook = service.create(input).unwrap();
        assert!(webhook.enabled);
    }

    #[test]
    fn create_webhook_rejects_empty_url() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        let mut input = make_input();
        input.url = String::new();
        let err = service.create(input).unwrap_err();
        assert!(err.to_string().contains("URL"));
    }

    #[test]
    fn create_webhook_rejects_non_http_url() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        let mut input = make_input();
        input.url = "ftp://example.com".to_string();
        let err = service.create(input).unwrap_err();
        assert!(err.to_string().contains("http"));
    }

    #[test]
    fn create_webhook_rejects_empty_events() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        let mut input = make_input();
        input.events = vec![];
        let err = service.create(input).unwrap_err();
        assert!(err.to_string().contains("event"));
    }

    #[test]
    fn create_webhook_rejects_duplicate_events() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        let mut input = make_input();
        input.events = vec![WebhookEvent::Create, WebhookEvent::Create];
        let err = service.create(input).unwrap_err();
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn list_webhooks_returns_all() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        service.create(make_input()).unwrap();

        let mut input2 = make_input();
        input2.collection = "users".to_string();
        service.create(input2).unwrap();

        let all = service.list(None).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn list_webhooks_filtered_by_collection() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        service.create(make_input()).unwrap();

        let mut input2 = make_input();
        input2.collection = "users".to_string();
        service.create(input2).unwrap();

        let posts_only = service.list(Some("posts")).unwrap();
        assert_eq!(posts_only.len(), 1);
        assert_eq!(posts_only[0].collection, "posts");
    }

    #[test]
    fn get_webhook_by_id() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        let created = service.create(make_input()).unwrap();
        let fetched = service.get(&created.id).unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.url, created.url);
    }

    #[test]
    fn get_nonexistent_webhook_returns_not_found() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        let err = service.get("nonexistent").unwrap_err();
        assert!(matches!(err, ZerobaseError::NotFound { .. }));
    }

    #[test]
    fn update_webhook_fields() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        let created = service.create(make_input()).unwrap();

        let updated = service
            .update(
                &created.id,
                UpdateWebhookInput {
                    collection: None,
                    url: Some("https://new-url.com/hook".to_string()),
                    events: Some(vec![WebhookEvent::Delete]),
                    secret: Some(None), // Remove secret
                    enabled: Some(false),
                },
            )
            .unwrap();

        assert_eq!(updated.url, "https://new-url.com/hook");
        assert_eq!(updated.events, vec![WebhookEvent::Delete]);
        assert_eq!(updated.secret, None);
        assert!(!updated.enabled);
        assert!(updated.updated > created.updated);
    }

    #[test]
    fn update_webhook_partial() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        let created = service.create(make_input()).unwrap();

        let updated = service
            .update(
                &created.id,
                UpdateWebhookInput {
                    collection: None,
                    url: None,
                    events: None,
                    secret: None,
                    enabled: Some(false),
                },
            )
            .unwrap();

        // URL and events unchanged.
        assert_eq!(updated.url, created.url);
        assert_eq!(updated.events, created.events);
        assert!(!updated.enabled);
    }

    #[test]
    fn delete_webhook() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        let created = service.create(make_input()).unwrap();
        service.delete(&created.id).unwrap();
        assert!(service.get(&created.id).is_err());
    }

    #[test]
    fn delete_nonexistent_webhook() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        let err = service.delete("nonexistent").unwrap_err();
        assert!(matches!(err, ZerobaseError::NotFound { .. }));
    }

    #[test]
    fn get_active_webhooks_filters_correctly() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());

        // Create an enabled webhook for posts: create + update
        service.create(make_input()).unwrap();

        // Create a disabled webhook for posts
        let mut input2 = make_input();
        input2.enabled = Some(false);
        service.create(input2).unwrap();

        // Create a webhook for different collection
        let mut input3 = make_input();
        input3.collection = "users".to_string();
        service.create(input3).unwrap();

        // Only the first webhook should match.
        let active = service
            .get_active_for_event("posts", WebhookEvent::Create)
            .unwrap();
        assert_eq!(active.len(), 1);
        assert!(active[0].enabled);

        // Delete event not configured on the first webhook.
        let active_delete = service
            .get_active_for_event("posts", WebhookEvent::Delete)
            .unwrap();
        assert_eq!(active_delete.len(), 0);
    }

    #[test]
    fn delivery_log_crud() {
        let service = WebhookService::new(InMemoryWebhookRepo::new());
        let webhook = service.create(make_input()).unwrap();

        let log = WebhookDeliveryLog {
            id: "log_1".to_string(),
            webhook_id: webhook.id.clone(),
            event: WebhookEvent::Create,
            collection: "posts".to_string(),
            record_id: "rec_1".to_string(),
            url: "https://example.com/webhook".to_string(),
            response_status: 200,
            attempt: 1,
            status: super::super::model::WebhookDeliveryStatus::Success,
            error: None,
            created: "2026-01-01T00:00:00Z".to_string(),
        };
        service.log_delivery(&log).unwrap();

        let logs = service.delivery_logs(&webhook.id, 10).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].id, "log_1");
    }
}
