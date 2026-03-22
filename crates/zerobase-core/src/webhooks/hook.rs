//! WebhookHook — a [`Hook`] implementation that dispatches webhooks on record changes.
//!
//! This hook fires **after** create, update, and delete operations. It looks up
//! all active webhooks for the affected collection and spawns a background task
//! to deliver each one via [`WebhookDispatcher`].

use std::sync::Arc;

use crate::hooks::{Hook, HookContext, HookResult, RecordOperation};

use super::dispatcher::{HttpSender, WebhookDispatcher};
use super::model::WebhookEvent;
use super::service::WebhookRepository;

/// A hook that dispatches configured webhooks after record mutations.
///
/// The hook runs in the `after_operation` phase only and only for
/// create/update/delete operations. Webhook HTTP calls are spawned
/// as background tokio tasks so they never block the request.
pub struct WebhookHook<S: HttpSender + 'static, R: WebhookRepository + 'static> {
    dispatcher: Arc<WebhookDispatcher<S, R>>,
    repo: Arc<R>,
}

impl<S: HttpSender + 'static, R: WebhookRepository + 'static> WebhookHook<S, R> {
    /// Create a new webhook hook.
    ///
    /// - `dispatcher` — shared dispatcher used to send HTTP payloads.
    /// - `repo` — webhook repository for looking up active webhooks.
    pub fn new(dispatcher: Arc<WebhookDispatcher<S, R>>, repo: Arc<R>) -> Self {
        Self { dispatcher, repo }
    }
}

/// Map a [`RecordOperation`] to the corresponding [`WebhookEvent`].
fn operation_to_event(op: RecordOperation) -> Option<WebhookEvent> {
    match op {
        RecordOperation::Create => Some(WebhookEvent::Create),
        RecordOperation::Update => Some(WebhookEvent::Update),
        RecordOperation::Delete => Some(WebhookEvent::Delete),
        _ => None,
    }
}

impl<S: HttpSender + 'static, R: WebhookRepository + 'static> Hook for WebhookHook<S, R> {
    fn name(&self) -> &str {
        "webhook_dispatcher"
    }

    fn matches(&self, operation: RecordOperation, _collection: &str) -> bool {
        matches!(
            operation,
            RecordOperation::Create | RecordOperation::Update | RecordOperation::Delete
        )
    }

    fn after_operation(&self, ctx: &HookContext) -> HookResult<()> {
        let event = match operation_to_event(ctx.operation) {
            Some(e) => e,
            None => return Ok(()),
        };

        // Look up active webhooks for this collection + event.
        let webhooks = match self.repo.get_active_webhooks(&ctx.collection, event) {
            Ok(wh) => wh,
            Err(e) => {
                tracing::error!(
                    collection = %ctx.collection,
                    event = %event,
                    error = %e,
                    "failed to look up webhooks"
                );
                return Ok(()); // Don't fail the operation because of webhook lookup errors.
            }
        };

        if webhooks.is_empty() {
            return Ok(());
        }

        // Build the record as a serde_json::Value from the HookContext's HashMap.
        let record = serde_json::to_value(&ctx.record).unwrap_or_default();
        let collection = ctx.collection.clone();
        let record_id = ctx.record_id.clone();

        for webhook in webhooks {
            let dispatcher = Arc::clone(&self.dispatcher);
            let record = record.clone();
            let collection = collection.clone();
            let record_id = record_id.clone();

            tokio::spawn(async move {
                dispatcher
                    .dispatch(&webhook, event, &collection, &record_id, record)
                    .await;
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::{HookPhase, RecordOperation};
    use crate::webhooks::dispatcher::HttpSender;
    use crate::webhooks::model::{Webhook, WebhookEvent};
    use crate::webhooks::service::InMemoryWebhookRepo;

    use std::collections::HashMap;
    use std::sync::Mutex;

    // ── Mock sender that records calls ──────────────────────────────────

    struct MockSender {
        responses: Mutex<Vec<std::result::Result<u16, String>>>,
        call_count: Mutex<usize>,
    }

    impl MockSender {
        fn new(responses: Vec<std::result::Result<u16, String>>) -> Self {
            Self {
                responses: Mutex::new(responses),
                call_count: Mutex::new(0),
            }
        }

        fn calls(&self) -> usize {
            *self.call_count.lock().unwrap()
        }
    }

    #[async_trait::async_trait]
    impl HttpSender for MockSender {
        async fn post(
            &self,
            _url: &str,
            _body: &str,
            _headers: Vec<(String, String)>,
        ) -> std::result::Result<u16, String> {
            *self.call_count.lock().unwrap() += 1;
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Ok(200)
            } else {
                responses.remove(0)
            }
        }
    }

    fn make_webhook(collection: &str, events: Vec<WebhookEvent>) -> Webhook {
        Webhook {
            id: format!("wh_{collection}"),
            collection: collection.to_string(),
            url: "https://example.com/hook".to_string(),
            events,
            secret: None,
            enabled: true,
            created: "2026-01-01T00:00:00Z".to_string(),
            updated: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    fn make_context(
        operation: RecordOperation,
        collection: &str,
        record_id: &str,
    ) -> HookContext {
        let mut record = HashMap::new();
        record.insert(
            "id".to_string(),
            serde_json::Value::String(record_id.to_string()),
        );
        HookContext::new(operation, HookPhase::After, collection, record_id, record)
    }

    #[test]
    fn matches_only_cud_operations() {
        let repo = Arc::new(InMemoryWebhookRepo::new());
        let sender = Arc::new(MockSender::new(vec![]));
        let dispatcher = Arc::new(WebhookDispatcher::new(sender, repo.clone()));
        let hook = WebhookHook::new(dispatcher, repo);

        assert!(hook.matches(RecordOperation::Create, "any"));
        assert!(hook.matches(RecordOperation::Update, "any"));
        assert!(hook.matches(RecordOperation::Delete, "any"));
        assert!(!hook.matches(RecordOperation::View, "any"));
        assert!(!hook.matches(RecordOperation::List, "any"));
    }

    #[tokio::test]
    async fn fires_webhook_on_create() {
        let repo = Arc::new(InMemoryWebhookRepo::with_webhooks(vec![make_webhook(
            "posts",
            vec![WebhookEvent::Create],
        )]));
        let sender = Arc::new(MockSender::new(vec![Ok(200)]));
        let dispatcher = Arc::new(WebhookDispatcher::new(sender.clone(), repo.clone()));
        let hook = WebhookHook::new(dispatcher, repo);

        let ctx = make_context(RecordOperation::Create, "posts", "rec_1");
        hook.after_operation(&ctx).unwrap();

        // Give the spawned task time to run.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert_eq!(sender.calls(), 1);
    }

    #[tokio::test]
    async fn no_webhook_for_unmatched_collection() {
        let repo = Arc::new(InMemoryWebhookRepo::with_webhooks(vec![make_webhook(
            "posts",
            vec![WebhookEvent::Create],
        )]));
        let sender = Arc::new(MockSender::new(vec![]));
        let dispatcher = Arc::new(WebhookDispatcher::new(sender.clone(), repo.clone()));
        let hook = WebhookHook::new(dispatcher, repo);

        let ctx = make_context(RecordOperation::Create, "users", "rec_1");
        hook.after_operation(&ctx).unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert_eq!(sender.calls(), 0);
    }

    #[tokio::test]
    async fn no_webhook_for_unmatched_event() {
        let repo = Arc::new(InMemoryWebhookRepo::with_webhooks(vec![make_webhook(
            "posts",
            vec![WebhookEvent::Create],
        )]));
        let sender = Arc::new(MockSender::new(vec![]));
        let dispatcher = Arc::new(WebhookDispatcher::new(sender.clone(), repo.clone()));
        let hook = WebhookHook::new(dispatcher, repo);

        let ctx = make_context(RecordOperation::Delete, "posts", "rec_1");
        hook.after_operation(&ctx).unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert_eq!(sender.calls(), 0);
    }

    #[test]
    fn operation_to_event_mapping() {
        assert_eq!(
            operation_to_event(RecordOperation::Create),
            Some(WebhookEvent::Create)
        );
        assert_eq!(
            operation_to_event(RecordOperation::Update),
            Some(WebhookEvent::Update)
        );
        assert_eq!(
            operation_to_event(RecordOperation::Delete),
            Some(WebhookEvent::Delete)
        );
        assert_eq!(operation_to_event(RecordOperation::View), None);
        assert_eq!(operation_to_event(RecordOperation::List), None);
    }
}
