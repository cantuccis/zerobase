//! Realtime SSE endpoint handler.
//!
//! Provides `GET /api/realtime` which establishes a Server-Sent Events (SSE)
//! connection, and `POST /api/realtime` for managing per-client subscriptions.
//!
//! Each client receives a unique ID on connect and periodic keep-alive comments
//! to prevent connection timeouts.
//!
//! The [`RealtimeHub`] manages all active connections, their subscriptions,
//! and distributes events via tokio broadcast channels.
//!
//! # Protocol
//!
//! On connection the server sends a `PB_CONNECT` event containing the client ID:
//!
//! ```text
//! event: PB_CONNECT
//! data: {"clientId":"abc123"}
//! ```
//!
//! The client then sets its subscriptions via `POST /api/realtime`:
//!
//! ```json
//! {"clientId":"abc123","subscriptions":["posts","comments/rec_456"]}
//! ```
//!
//! Each POST **replaces** the client's entire subscription set (set-based
//! semantics, matching PocketBase).
//!
//! Keep-alive comments (`: ping`) are sent at a configurable interval (default
//! 30 seconds) to prevent proxies/load balancers from closing idle connections.
//!
//! # PocketBase Compatibility
//!
//! This endpoint mirrors PocketBase's realtime SSE behaviour:
//! - Client IDs are short, URL-safe strings (nanoid).
//! - The connect event uses `PB_CONNECT` as the event name.
//! - Keep-alive uses SSE comment syntax (`: ping`).
//! - Subscription management uses set-based replacement via POST.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tracing::{debug, info, warn};

use crate::middleware::auth_context::AuthInfo;
use zerobase_core::schema::rule_engine::{check_rule, evaluate_rule_str, RuleDecision};
use zerobase_core::schema::ApiRules;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// An event distributed to connected SSE clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealtimeEvent {
    /// SSE event name (e.g. `PB_CONNECT`, collection name).
    pub event: String,
    /// JSON-serializable payload.
    pub data: serde_json::Value,
    /// Topic for subscription matching (e.g. `"posts/rec_123"`).
    /// Empty for system events like `PB_CONNECT`.
    #[serde(default)]
    pub topic: String,
    /// Collection API rules for per-client access filtering.
    /// `None` for system events (always delivered).
    #[serde(skip)]
    pub rules: Option<ApiRules>,
}

/// Metadata about a connected SSE client.
#[derive(Debug, Clone)]
pub struct ClientInfo {
    /// Unique client identifier (nanoid).
    pub client_id: String,
    /// Timestamp when the client connected.
    pub connected_at: std::time::Instant,
    /// Active subscription topics (e.g. `"posts"`, `"posts/rec_123"`).
    pub subscriptions: HashSet<String>,
    /// Authentication context of the client at connection time.
    /// Used to evaluate access rules when filtering events.
    pub auth: AuthInfo,
}

/// Regex pattern for valid subscription topics: `collection` or `collection/recordId`.
/// Allows alphanumeric characters and underscores in each segment.
fn is_valid_topic(topic: &str) -> bool {
    if topic.is_empty() {
        return false;
    }
    let parts: Vec<&str> = topic.split('/').collect();
    match parts.len() {
        1 => parts[0]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_'),
        2 => {
            parts[0]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !parts[0].is_empty()
                && parts[1]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !parts[1].is_empty()
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// RealtimeHub
// ---------------------------------------------------------------------------

/// Central hub that manages SSE client connections and event broadcasting.
///
/// Clone-friendly — internally uses `Arc`-wrapped state so all clones share
/// the same connection pool and broadcast channel.
#[derive(Clone)]
pub struct RealtimeHub {
    inner: Arc<RealtimeHubInner>,
}

struct RealtimeHubInner {
    /// Broadcast sender — every SSE client holds a receiver.
    sender: broadcast::Sender<RealtimeEvent>,
    /// Connected clients keyed by client ID.
    clients: RwLock<HashMap<String, ClientInfo>>,
    /// Keep-alive interval for SSE connections.
    keep_alive_interval: Duration,
}

/// Configuration for creating a [`RealtimeHub`].
pub struct RealtimeHubConfig {
    /// Maximum number of buffered events per client before messages are dropped.
    /// Defaults to 256.
    pub channel_capacity: usize,
    /// Interval between keep-alive SSE comments.
    /// Defaults to 30 seconds.
    pub keep_alive_interval: Duration,
}

impl Default for RealtimeHubConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 256,
            keep_alive_interval: Duration::from_secs(30),
        }
    }
}

impl RealtimeHub {
    /// Create a new hub with default configuration.
    pub fn new() -> Self {
        Self::with_config(RealtimeHubConfig::default())
    }

    /// Create a new hub with custom configuration.
    pub fn with_config(config: RealtimeHubConfig) -> Self {
        let (sender, _) = broadcast::channel(config.channel_capacity);
        Self {
            inner: Arc::new(RealtimeHubInner {
                sender,
                clients: RwLock::new(HashMap::new()),
                keep_alive_interval: config.keep_alive_interval,
            }),
        }
    }

    /// Register a new client and return its info along with a broadcast receiver.
    async fn connect(&self, auth: AuthInfo) -> (ClientInfo, broadcast::Receiver<RealtimeEvent>) {
        let client_id = nanoid::nanoid!(15);
        let info = ClientInfo {
            client_id: client_id.clone(),
            connected_at: std::time::Instant::now(),
            subscriptions: HashSet::new(),
            auth,
        };

        self.inner
            .clients
            .write()
            .await
            .insert(client_id.clone(), info.clone());

        let receiver = self.inner.sender.subscribe();

        info!(client_id = %client_id, "SSE client connected");
        (info, receiver)
    }

    /// Remove a client from the connected set and drop all its subscriptions.
    async fn disconnect(&self, client_id: &str) {
        self.inner.clients.write().await.remove(client_id);
        info!(client_id = %client_id, "SSE client disconnected");
    }

    /// Replace the subscription set for a connected client.
    ///
    /// Returns `Ok(())` if the client exists and subscriptions were updated.
    /// Returns `Err(SetSubscriptionsError::ClientNotFound)` if the client ID
    /// is not connected. Returns `Err(SetSubscriptionsError::InvalidTopic(topic))`
    /// if any topic fails validation.
    pub async fn set_subscriptions(
        &self,
        client_id: &str,
        subscriptions: HashSet<String>,
    ) -> Result<(), SetSubscriptionsError> {
        // Validate all topics before acquiring the write lock.
        for topic in &subscriptions {
            if !is_valid_topic(topic) {
                return Err(SetSubscriptionsError::InvalidTopic(topic.clone()));
            }
        }

        let mut clients = self.inner.clients.write().await;
        match clients.get_mut(client_id) {
            Some(client) => {
                info!(
                    client_id = %client_id,
                    count = subscriptions.len(),
                    "subscriptions updated"
                );
                client.subscriptions = subscriptions;
                Ok(())
            }
            None => Err(SetSubscriptionsError::ClientNotFound),
        }
    }

    /// Return the current subscription set for a client.
    ///
    /// Returns `None` if the client is not connected.
    pub async fn get_subscriptions(&self, client_id: &str) -> Option<HashSet<String>> {
        self.inner
            .clients
            .read()
            .await
            .get(client_id)
            .map(|c| c.subscriptions.clone())
    }

    /// Check whether a client is subscribed to a given topic.
    ///
    /// A subscription to `"posts"` matches both the collection-level topic
    /// `"posts"` and record-level topics like `"posts/rec_123"`.
    /// A subscription to `"posts/rec_123"` only matches that exact record.
    pub async fn is_subscribed(&self, client_id: &str, topic: &str) -> bool {
        let clients = self.inner.clients.read().await;
        match clients.get(client_id) {
            Some(client) => {
                if client.subscriptions.contains(topic) {
                    return true;
                }
                // Check if subscribed to the collection for a record-level topic.
                if let Some(collection) = topic.split('/').next() {
                    if collection != topic {
                        return client.subscriptions.contains(collection);
                    }
                }
                false
            }
            None => false,
        }
    }

    /// Broadcast an event to all connected clients.
    ///
    /// Returns the number of receivers that will see the event.
    /// Returns 0 if there are no active receivers.
    pub fn broadcast(&self, event: RealtimeEvent) -> usize {
        match self.inner.sender.send(event) {
            Ok(n) => n,
            Err(_) => {
                // No active receivers — not an error.
                0
            }
        }
    }

    /// Broadcast a record change event to all subscribed and authorized clients.
    ///
    /// The event is sent as a broadcast to all connected clients, but each
    /// client's SSE stream filters based on subscriptions and access rules.
    ///
    /// # Arguments
    ///
    /// * `collection_name` - The name of the collection (used as the SSE event name)
    /// * `record_id` - The ID of the affected record
    /// * `action` - The action that occurred: `"create"`, `"update"`, or `"delete"`
    /// * `record` - The record data (after the operation; for delete, the record before deletion)
    /// * `rules` - The collection's API rules (used for per-client access filtering)
    pub fn broadcast_record_event(
        &self,
        collection_name: &str,
        record_id: &str,
        action: &str,
        record: &HashMap<String, serde_json::Value>,
        rules: &ApiRules,
    ) -> usize {
        let event = RealtimeEvent {
            event: collection_name.to_string(),
            data: serde_json::json!({
                "action": action,
                "record": record,
            }),
            // Store the topic for subscription matching.
            topic: format!("{collection_name}/{record_id}"),
            rules: Some(rules.clone()),
        };
        self.broadcast(event)
    }

    /// Check whether a specific client should receive a record event based on
    /// their subscriptions and access rules.
    ///
    /// This is called per-client from the SSE stream filter.
    pub async fn should_client_receive(
        &self,
        client_id: &str,
        event: &RealtimeEvent,
    ) -> bool {
        let clients = self.inner.clients.read().await;
        let client = match clients.get(client_id) {
            Some(c) => c,
            None => return false,
        };

        // 1. Check subscription match.
        if !Self::topic_matches_subscriptions(&event.topic, &client.subscriptions) {
            return false;
        }

        // 2. Check access rules (view_rule) if rules are attached.
        if let Some(rules) = &event.rules {
            if !Self::client_passes_view_rule(&client.auth, rules, &event.data) {
                return false;
            }
        }

        true
    }

    /// Check whether a topic matches any of the client's subscriptions.
    fn topic_matches_subscriptions(topic: &str, subscriptions: &HashSet<String>) -> bool {
        if topic.is_empty() {
            return false;
        }

        // Exact match.
        if subscriptions.contains(topic) {
            return true;
        }

        // Collection-level subscription matches record-level topics.
        // e.g. subscription "posts" matches topic "posts/rec_123".
        if let Some(collection) = topic.split('/').next() {
            if collection != topic {
                return subscriptions.contains(collection);
            }
        }

        false
    }

    /// Evaluate the collection's view_rule against a client's auth context.
    ///
    /// Returns `true` if the client is allowed to see the record based on
    /// the collection's view_rule (and manage_rule). Superusers always pass.
    pub fn client_passes_view_rule(
        auth: &AuthInfo,
        rules: &ApiRules,
        event_data: &serde_json::Value,
    ) -> bool {
        // Superusers bypass all rules.
        if auth.is_superuser {
            return true;
        }

        // Extract the record from the event data for rule evaluation.
        let record: HashMap<String, serde_json::Value> = event_data
            .get("record")
            .and_then(|v| v.as_object())
            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        // Check manage_rule first.
        match check_rule(&rules.manage_rule) {
            RuleDecision::Allow => {
                if auth.is_authenticated() {
                    return true;
                }
            }
            RuleDecision::Evaluate(expr) => {
                let ctx = auth.to_simple_context("GET");
                if evaluate_rule_str(&expr, &ctx, &record).unwrap_or(false) {
                    return true;
                }
            }
            RuleDecision::Deny => {}
        }

        // Check view_rule.
        match check_rule(&rules.view_rule) {
            RuleDecision::Allow => true,
            RuleDecision::Deny => false,
            RuleDecision::Evaluate(expr) => {
                let ctx = auth.to_simple_context("GET");
                evaluate_rule_str(&expr, &ctx, &record).unwrap_or(false)
            }
        }
    }

    /// Return the number of currently connected clients.
    pub async fn client_count(&self) -> usize {
        self.inner.clients.read().await.len()
    }

    /// Return a snapshot of all connected client IDs.
    pub async fn connected_client_ids(&self) -> Vec<String> {
        self.inner.clients.read().await.keys().cloned().collect()
    }

    /// Get the configured keep-alive interval.
    pub fn keep_alive_interval(&self) -> Duration {
        self.inner.keep_alive_interval
    }
}

impl Default for RealtimeHub {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Subscription errors
// ---------------------------------------------------------------------------

/// Error returned by [`RealtimeHub::set_subscriptions`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetSubscriptionsError {
    /// The supplied `clientId` does not match any active SSE connection.
    ClientNotFound,
    /// A subscription topic has an invalid format.
    InvalidTopic(String),
}

impl std::fmt::Display for SetSubscriptionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClientNotFound => write!(f, "client connection not found"),
            Self::InvalidTopic(t) => write!(f, "invalid subscription topic: {t}"),
        }
    }
}

impl std::error::Error for SetSubscriptionsError {}

// ---------------------------------------------------------------------------
// Axum state
// ---------------------------------------------------------------------------

/// Axum state for the realtime SSE endpoint.
#[derive(Clone)]
pub struct RealtimeState {
    pub hub: RealtimeHub,
}

// ---------------------------------------------------------------------------
// Subscription request/response types
// ---------------------------------------------------------------------------

/// Request body for `POST /api/realtime`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSubscriptionsRequest {
    /// The client ID obtained from the `PB_CONNECT` SSE event.
    pub client_id: String,
    /// The full set of subscription topics. Replaces any previous subscriptions.
    pub subscriptions: Vec<String>,
}

/// Successful response body for `POST /api/realtime`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSubscriptionsResponse {
    pub code: u16,
    pub message: String,
    pub data: SubscriptionData,
}

/// Inner data payload for [`SetSubscriptionsResponse`].
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionData {
    pub client_id: String,
    pub subscriptions: Vec<String>,
}

/// Error response body for subscription failures.
#[derive(Debug, Serialize)]
pub struct SubscriptionErrorResponse {
    pub code: u16,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/realtime` — establish an SSE connection.
///
/// The response uses `text/event-stream` content type. The first event is
/// always `PB_CONNECT` with the assigned `clientId`. Subsequent events are
/// broadcast from the [`RealtimeHub`].
///
/// Each client only receives events that match their subscriptions and
/// pass the collection's access rules (view_rule) for the client's auth context.
///
/// If the client disconnects (stream closes), the client is automatically
/// removed from the hub.
pub async fn sse_connect(
    State(state): State<RealtimeState>,
    auth: AuthInfo,
) -> impl IntoResponse {
    let hub = state.hub.clone();
    let (client_info, receiver) = hub.connect(auth).await;
    let client_id = client_info.client_id.clone();

    // Build the initial PB_CONNECT event.
    let connect_event = Event::default()
        .event("PB_CONNECT")
        .json_data(serde_json::json!({ "clientId": &client_id }))
        .expect("failed to serialize connect event");

    // Convert the broadcast receiver into a stream of SSE events,
    // filtering by subscription match and access rules.
    // We use .then() (async) followed by .filter_map() (sync) because
    // tokio_stream's filter_map does not support async closures.
    let filter_hub = hub.clone();
    let filter_client_id = client_id.clone();
    let event_stream = BroadcastStream::new(receiver)
        .then(move |result| {
            let hub = filter_hub.clone();
            let cid = filter_client_id.clone();
            async move {
                match result {
                    Ok(rt_event) => {
                        // Filter: check subscription match and access rules.
                        if !rt_event.topic.is_empty()
                            && !hub.should_client_receive(&cid, &rt_event).await
                        {
                            return None;
                        }

                        let sse_event = Event::default()
                            .event(&rt_event.event)
                            .json_data(&rt_event.data);
                        match sse_event {
                            Ok(e) => Some(Ok::<_, std::convert::Infallible>(e)),
                            Err(err) => {
                                warn!(error = %err, "failed to serialize SSE event");
                                None
                            }
                        }
                    }
                    Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                        debug!(missed = n, "SSE client lagged, skipping missed events");
                        None
                    }
                }
            }
        })
        .filter_map(|x| x);

    // Prepend the connect event to the stream.
    let initial = tokio_stream::once(Ok(connect_event));
    let combined = initial.chain(event_stream);

    // Register a drop guard so we clean up when the stream ends.
    let guarded_stream = DropGuardStream {
        inner: Box::pin(combined),
        hub: hub.clone(),
        client_id: client_id.clone(),
        _dropped: false,
    };

    Sse::new(guarded_stream).keep_alive(
        KeepAlive::new()
            .interval(hub.keep_alive_interval())
            .text("ping"),
    )
}

/// `POST /api/realtime` — set subscriptions for a connected SSE client.
///
/// The request body must contain `clientId` (from `PB_CONNECT`) and a
/// `subscriptions` array of topic strings. Each call **replaces** the
/// client's entire subscription set.
///
/// # Errors
///
/// - **400** if `clientId` is missing/empty or a topic is invalid.
/// - **404** if the `clientId` does not match any active SSE connection.
pub async fn set_subscriptions(
    State(state): State<RealtimeState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Parse clientId — must be present and non-empty.
    let client_id = match body.get("clientId").and_then(|v| v.as_str()) {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(SubscriptionErrorResponse {
                    code: 400,
                    message: "clientId is required".to_string(),
                }),
            )
                .into_response();
        }
    };

    // Parse subscriptions — must be an array of strings.
    let subscriptions: Vec<String> = match body.get("subscriptions") {
        Some(serde_json::Value::Array(arr)) => {
            let mut subs = Vec::with_capacity(arr.len());
            for val in arr {
                match val.as_str() {
                    Some(s) => subs.push(s.to_string()),
                    None => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(SubscriptionErrorResponse {
                                code: 400,
                                message: "subscriptions must be an array of strings".to_string(),
                            }),
                        )
                            .into_response();
                    }
                }
            }
            subs
        }
        Some(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(SubscriptionErrorResponse {
                    code: 400,
                    message: "subscriptions must be an array".to_string(),
                }),
            )
                .into_response();
        }
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(SubscriptionErrorResponse {
                    code: 400,
                    message: "subscriptions is required".to_string(),
                }),
            )
                .into_response();
        }
    };

    let sub_set: HashSet<String> = subscriptions.iter().cloned().collect();

    match state.hub.set_subscriptions(&client_id, sub_set).await {
        Ok(()) => {
            let response = SetSubscriptionsResponse {
                code: 200,
                message: "Subscriptions updated.".to_string(),
                data: SubscriptionData {
                    client_id,
                    subscriptions,
                },
            };
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(SetSubscriptionsError::ClientNotFound) => (
            StatusCode::NOT_FOUND,
            Json(SubscriptionErrorResponse {
                code: 404,
                message: "client connection not found".to_string(),
            }),
        )
            .into_response(),
        Err(SetSubscriptionsError::InvalidTopic(topic)) => (
            StatusCode::BAD_REQUEST,
            Json(SubscriptionErrorResponse {
                code: 400,
                message: format!("invalid subscription topic: {topic}"),
            }),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Drop-guard wrapper stream
// ---------------------------------------------------------------------------

use std::pin::Pin;
use std::task::{Context, Poll};
use tokio_stream::Stream;

/// A stream wrapper that runs cleanup (disconnect) when dropped.
struct DropGuardStream<S> {
    inner: Pin<Box<S>>,
    hub: RealtimeHub,
    client_id: String,
    _dropped: bool,
}

impl<S: Stream> Stream for DropGuardStream<S> {
    type Item = S::Item;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

impl<S> Drop for DropGuardStream<S> {
    fn drop(&mut self) {
        if !self._dropped {
            self._dropped = true;
            let hub = self.hub.clone();
            let client_id = self.client_id.clone();
            tokio::spawn(async move {
                hub.disconnect(&client_id).await;
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn anon() -> AuthInfo {
        AuthInfo::anonymous()
    }

    fn open_rules() -> ApiRules {
        ApiRules {
            list_rule: Some(String::new()),
            view_rule: Some(String::new()),
            create_rule: Some(String::new()),
            update_rule: Some(String::new()),
            delete_rule: Some(String::new()),
            manage_rule: None,
        }
    }

    fn locked_rules() -> ApiRules {
        ApiRules {
            list_rule: None,
            view_rule: None,
            create_rule: None,
            update_rule: None,
            delete_rule: None,
            manage_rule: None,
        }
    }

    fn system_event(name: &str) -> RealtimeEvent {
        RealtimeEvent {
            event: name.to_string(),
            data: serde_json::json!({}),
            topic: String::new(),
            rules: None,
        }
    }

    #[tokio::test]
    async fn hub_connect_assigns_unique_ids() {
        let hub = RealtimeHub::new();

        let (info1, _rx1) = hub.connect(anon()).await;
        let (info2, _rx2) = hub.connect(anon()).await;

        assert_ne!(info1.client_id, info2.client_id);
        assert_eq!(hub.client_count().await, 2);
    }

    #[tokio::test]
    async fn hub_disconnect_removes_client() {
        let hub = RealtimeHub::new();

        let (info, _rx) = hub.connect(anon()).await;
        assert_eq!(hub.client_count().await, 1);

        hub.disconnect(&info.client_id).await;
        assert_eq!(hub.client_count().await, 0);
    }

    #[tokio::test]
    async fn hub_broadcast_reaches_receivers() {
        let hub = RealtimeHub::new();
        let (_info, mut rx) = hub.connect(anon()).await;

        let event = RealtimeEvent {
            event: "test".to_string(),
            data: serde_json::json!({"msg": "hello"}),
            topic: String::new(),
            rules: None,
        };

        let count = hub.broadcast(event.clone());
        assert_eq!(count, 1);

        let received = rx.recv().await.unwrap();
        assert_eq!(received.event, "test");
        assert_eq!(received.data, serde_json::json!({"msg": "hello"}));
    }

    #[tokio::test]
    async fn hub_broadcast_returns_zero_with_no_receivers() {
        let hub = RealtimeHub::new();
        assert_eq!(hub.broadcast(system_event("test")), 0);
    }

    #[tokio::test]
    async fn hub_connected_client_ids_lists_all() {
        let hub = RealtimeHub::new();

        let (info1, _rx1) = hub.connect(anon()).await;
        let (info2, _rx2) = hub.connect(anon()).await;

        let ids = hub.connected_client_ids().await;
        assert!(ids.contains(&info1.client_id));
        assert!(ids.contains(&info2.client_id));
    }

    #[tokio::test]
    async fn hub_custom_config() {
        let hub = RealtimeHub::with_config(RealtimeHubConfig {
            channel_capacity: 16,
            keep_alive_interval: Duration::from_secs(10),
        });
        assert_eq!(hub.keep_alive_interval(), Duration::from_secs(10));
    }

    #[tokio::test]
    async fn hub_default_keep_alive_is_30s() {
        let hub = RealtimeHub::new();
        assert_eq!(hub.keep_alive_interval(), Duration::from_secs(30));
    }

    // -----------------------------------------------------------------------
    // Subscription management tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn hub_set_subscriptions_replaces_previous() {
        let hub = RealtimeHub::new();
        let (info, _rx) = hub.connect(anon()).await;

        let subs1: HashSet<String> = ["posts", "comments"].iter().map(|s| s.to_string()).collect();
        hub.set_subscriptions(&info.client_id, subs1).await.unwrap();

        let current = hub.get_subscriptions(&info.client_id).await.unwrap();
        assert_eq!(current.len(), 2);
        assert!(current.contains("posts"));
        assert!(current.contains("comments"));

        // Replace with a different set.
        let subs2: HashSet<String> = ["tags"].iter().map(|s| s.to_string()).collect();
        hub.set_subscriptions(&info.client_id, subs2).await.unwrap();

        let current = hub.get_subscriptions(&info.client_id).await.unwrap();
        assert_eq!(current.len(), 1);
        assert!(current.contains("tags"));
        assert!(!current.contains("posts"));
    }

    #[tokio::test]
    async fn hub_set_subscriptions_empty_clears_all() {
        let hub = RealtimeHub::new();
        let (info, _rx) = hub.connect(anon()).await;

        let subs: HashSet<String> = ["posts"].iter().map(|s| s.to_string()).collect();
        hub.set_subscriptions(&info.client_id, subs).await.unwrap();
        assert_eq!(hub.get_subscriptions(&info.client_id).await.unwrap().len(), 1);

        hub.set_subscriptions(&info.client_id, HashSet::new()).await.unwrap();
        assert!(hub.get_subscriptions(&info.client_id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn hub_set_subscriptions_unknown_client_returns_error() {
        let hub = RealtimeHub::new();
        let result = hub
            .set_subscriptions("nonexistent", HashSet::new())
            .await;
        assert_eq!(result, Err(SetSubscriptionsError::ClientNotFound));
    }

    #[tokio::test]
    async fn hub_set_subscriptions_invalid_topic_returns_error() {
        let hub = RealtimeHub::new();
        let (info, _rx) = hub.connect(anon()).await;

        let subs: HashSet<String> = ["valid", "not valid!"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let result = hub.set_subscriptions(&info.client_id, subs).await;
        assert!(matches!(result, Err(SetSubscriptionsError::InvalidTopic(_))));
    }

    #[tokio::test]
    async fn hub_get_subscriptions_returns_none_for_unknown_client() {
        let hub = RealtimeHub::new();
        assert!(hub.get_subscriptions("unknown").await.is_none());
    }

    #[tokio::test]
    async fn hub_disconnect_clears_subscriptions() {
        let hub = RealtimeHub::new();
        let (info, _rx) = hub.connect(anon()).await;

        let subs: HashSet<String> = ["posts"].iter().map(|s| s.to_string()).collect();
        hub.set_subscriptions(&info.client_id, subs).await.unwrap();

        hub.disconnect(&info.client_id).await;
        assert!(hub.get_subscriptions(&info.client_id).await.is_none());
    }

    #[tokio::test]
    async fn hub_is_subscribed_exact_match() {
        let hub = RealtimeHub::new();
        let (info, _rx) = hub.connect(anon()).await;

        let subs: HashSet<String> = ["posts/rec_123"].iter().map(|s| s.to_string()).collect();
        hub.set_subscriptions(&info.client_id, subs).await.unwrap();

        assert!(hub.is_subscribed(&info.client_id, "posts/rec_123").await);
        assert!(!hub.is_subscribed(&info.client_id, "posts/rec_456").await);
        assert!(!hub.is_subscribed(&info.client_id, "posts").await);
    }

    #[tokio::test]
    async fn hub_is_subscribed_collection_level_matches_records() {
        let hub = RealtimeHub::new();
        let (info, _rx) = hub.connect(anon()).await;

        let subs: HashSet<String> = ["posts"].iter().map(|s| s.to_string()).collect();
        hub.set_subscriptions(&info.client_id, subs).await.unwrap();

        assert!(hub.is_subscribed(&info.client_id, "posts").await);
        assert!(hub.is_subscribed(&info.client_id, "posts/rec_123").await);
        assert!(!hub.is_subscribed(&info.client_id, "comments").await);
    }

    #[tokio::test]
    async fn hub_is_subscribed_unknown_client_returns_false() {
        let hub = RealtimeHub::new();
        assert!(!hub.is_subscribed("unknown", "posts").await);
    }

    // -----------------------------------------------------------------------
    // Topic validation tests
    // -----------------------------------------------------------------------

    #[test]
    fn valid_topics() {
        assert!(is_valid_topic("posts"));
        assert!(is_valid_topic("my_collection"));
        assert!(is_valid_topic("posts/rec_123"));
        assert!(is_valid_topic("Users/abc123"));
        assert!(is_valid_topic("a/b"));
    }

    #[test]
    fn invalid_topics() {
        assert!(!is_valid_topic(""));
        assert!(!is_valid_topic("has spaces"));
        assert!(!is_valid_topic("special!chars"));
        assert!(!is_valid_topic("too/many/segments"));
        assert!(!is_valid_topic("/leading_slash"));
        assert!(!is_valid_topic("trailing_slash/"));
        assert!(!is_valid_topic("a//b"));
        assert!(!is_valid_topic("hello.world"));
    }

    // -----------------------------------------------------------------------
    // Topic matching tests
    // -----------------------------------------------------------------------

    #[test]
    fn topic_matches_exact_subscription() {
        let subs: HashSet<String> = ["posts/rec_1"].iter().map(|s| s.to_string()).collect();
        assert!(RealtimeHub::topic_matches_subscriptions("posts/rec_1", &subs));
        assert!(!RealtimeHub::topic_matches_subscriptions("posts/rec_2", &subs));
    }

    #[test]
    fn topic_matches_collection_level_subscription() {
        let subs: HashSet<String> = ["posts"].iter().map(|s| s.to_string()).collect();
        assert!(RealtimeHub::topic_matches_subscriptions("posts/rec_1", &subs));
        assert!(!RealtimeHub::topic_matches_subscriptions("comments/rec_1", &subs));
    }

    #[test]
    fn topic_matches_empty_topic_returns_false() {
        let subs: HashSet<String> = ["posts"].iter().map(|s| s.to_string()).collect();
        assert!(!RealtimeHub::topic_matches_subscriptions("", &subs));
    }

    #[test]
    fn topic_matches_empty_subscriptions_returns_false() {
        let subs: HashSet<String> = HashSet::new();
        assert!(!RealtimeHub::topic_matches_subscriptions("posts/rec_1", &subs));
    }

    // -----------------------------------------------------------------------
    // Access rule filtering tests
    // -----------------------------------------------------------------------

    #[test]
    fn view_rule_open_allows_anonymous() {
        let auth = AuthInfo::anonymous();
        let rules = open_rules();
        let data = serde_json::json!({"action": "create", "record": {"id": "r1"}});
        assert!(RealtimeHub::client_passes_view_rule(&auth, &rules, &data));
    }

    #[test]
    fn view_rule_locked_denies_anonymous() {
        let auth = AuthInfo::anonymous();
        let rules = locked_rules();
        let data = serde_json::json!({"action": "create", "record": {"id": "r1"}});
        assert!(!RealtimeHub::client_passes_view_rule(&auth, &rules, &data));
    }

    #[test]
    fn view_rule_locked_allows_superuser() {
        let auth = AuthInfo::superuser();
        let rules = locked_rules();
        let data = serde_json::json!({"action": "create", "record": {"id": "r1"}});
        assert!(RealtimeHub::client_passes_view_rule(&auth, &rules, &data));
    }

    #[test]
    fn manage_rule_open_allows_authenticated() {
        let mut record = HashMap::new();
        record.insert("id".to_string(), serde_json::json!("user1"));
        let auth = AuthInfo::authenticated(record);
        let rules = ApiRules {
            view_rule: None, // locked
            manage_rule: Some(String::new()), // open to authenticated
            ..locked_rules()
        };
        let data = serde_json::json!({"action": "update", "record": {"id": "r1"}});
        assert!(RealtimeHub::client_passes_view_rule(&auth, &rules, &data));
    }

    // -----------------------------------------------------------------------
    // broadcast_record_event tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn broadcast_record_event_sends_correct_format() {
        let hub = RealtimeHub::new();
        let (_info, mut rx) = hub.connect(anon()).await;

        let mut record = HashMap::new();
        record.insert("id".to_string(), serde_json::json!("rec_1"));
        record.insert("title".to_string(), serde_json::json!("Hello"));

        let count = hub.broadcast_record_event("posts", "rec_1", "create", &record, &open_rules());
        assert_eq!(count, 1);

        let received = rx.recv().await.unwrap();
        assert_eq!(received.event, "posts");
        assert_eq!(received.topic, "posts/rec_1");
        assert_eq!(received.data["action"], "create");
        assert_eq!(received.data["record"]["id"], "rec_1");
        assert_eq!(received.data["record"]["title"], "Hello");
        assert!(received.rules.is_some());
    }

    // -----------------------------------------------------------------------
    // should_client_receive tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn should_receive_subscribed_open_rules() {
        let hub = RealtimeHub::new();
        let (info, _rx) = hub.connect(anon()).await;

        let subs: HashSet<String> = ["posts"].iter().map(|s| s.to_string()).collect();
        hub.set_subscriptions(&info.client_id, subs).await.unwrap();

        let event = RealtimeEvent {
            event: "posts".to_string(),
            data: serde_json::json!({"action": "create", "record": {"id": "r1"}}),
            topic: "posts/r1".to_string(),
            rules: Some(open_rules()),
        };
        assert!(hub.should_client_receive(&info.client_id, &event).await);
    }

    #[tokio::test]
    async fn should_not_receive_unsubscribed_topic() {
        let hub = RealtimeHub::new();
        let (info, _rx) = hub.connect(anon()).await;

        let subs: HashSet<String> = ["comments"].iter().map(|s| s.to_string()).collect();
        hub.set_subscriptions(&info.client_id, subs).await.unwrap();

        let event = RealtimeEvent {
            event: "posts".to_string(),
            data: serde_json::json!({"action": "create", "record": {"id": "r1"}}),
            topic: "posts/r1".to_string(),
            rules: Some(open_rules()),
        };
        assert!(!hub.should_client_receive(&info.client_id, &event).await);
    }

    #[tokio::test]
    async fn should_not_receive_locked_rules_anonymous() {
        let hub = RealtimeHub::new();
        let (info, _rx) = hub.connect(anon()).await;

        let subs: HashSet<String> = ["posts"].iter().map(|s| s.to_string()).collect();
        hub.set_subscriptions(&info.client_id, subs).await.unwrap();

        let event = RealtimeEvent {
            event: "posts".to_string(),
            data: serde_json::json!({"action": "create", "record": {"id": "r1"}}),
            topic: "posts/r1".to_string(),
            rules: Some(locked_rules()),
        };
        assert!(!hub.should_client_receive(&info.client_id, &event).await);
    }

    #[tokio::test]
    async fn should_receive_locked_rules_superuser() {
        let hub = RealtimeHub::new();
        let (info, _rx) = hub.connect(AuthInfo::superuser()).await;

        let subs: HashSet<String> = ["posts"].iter().map(|s| s.to_string()).collect();
        hub.set_subscriptions(&info.client_id, subs).await.unwrap();

        let event = RealtimeEvent {
            event: "posts".to_string(),
            data: serde_json::json!({"action": "delete", "record": {"id": "r1"}}),
            topic: "posts/r1".to_string(),
            rules: Some(locked_rules()),
        };
        assert!(hub.should_client_receive(&info.client_id, &event).await);
    }

    #[tokio::test]
    async fn should_not_receive_unknown_client() {
        let hub = RealtimeHub::new();
        let event = RealtimeEvent {
            event: "posts".to_string(),
            data: serde_json::json!({"action": "create", "record": {"id": "r1"}}),
            topic: "posts/r1".to_string(),
            rules: Some(open_rules()),
        };
        assert!(!hub.should_client_receive("nonexistent", &event).await);
    }
}
