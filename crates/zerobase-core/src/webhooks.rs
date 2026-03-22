//! Webhook configuration, dispatching, and delivery logging.
//!
//! Webhooks allow external systems to be notified when records change
//! in a collection. Each webhook is configured with:
//!
//! - A target URL
//! - Which events to fire on (create, update, delete)
//! - An optional HMAC secret for signing payloads
//!
//! Delivery is asynchronous with exponential backoff retry (3 attempts).
//! Every delivery attempt is logged for observability.

pub mod dispatcher;
pub mod hook;
pub mod model;
pub mod service;

pub use dispatcher::WebhookDispatcher;
pub use hook::WebhookHook;
pub use model::{
    Webhook, WebhookDeliveryLog, WebhookDeliveryStatus, WebhookEvent, WebhookId,
};
pub use service::{WebhookRepository, WebhookService};
