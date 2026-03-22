# Realtime Subscription System Design

> Design document for Zerobase's SSE-based realtime subscription system.
> Mirrors PocketBase's realtime model: SSE connections with per-collection/per-record subscriptions, access rule enforcement, and event broadcasting on record changes.

---

## 1. Overview

Zerobase provides realtime notifications via **Server-Sent Events (SSE)**, not WebSockets. This matches PocketBase's approach and is simpler to implement, proxy-friendly, and sufficient for server-to-client push (the primary use case for record change notifications).

### Why SSE over WebSockets

| Concern | SSE | WebSocket |
|---------|-----|-----------|
| Direction | Server → client (sufficient for notifications) | Bidirectional |
| Protocol | Standard HTTP | Upgrade handshake required |
| Proxy support | Works through standard HTTP proxies/CDNs | Requires proxy configuration |
| Reconnection | Built-in (`EventSource` auto-reconnects) | Manual reconnect logic needed |
| Complexity | Simple text stream | Frame-based binary protocol |
| Auth | Standard headers + query params | Must authenticate during upgrade |

### Core Concepts

1. **SSE Connection**: A long-lived HTTP response that streams events to the client.
2. **Subscription**: A client declares interest in changes to specific collections or individual records.
3. **Event Broadcasting**: When a record is created, updated, or deleted, all subscribers with matching subscriptions AND passing access rules receive the event.

---

## 2. SSE Protocol

### 2.1 Connection Endpoint

```
GET /api/realtime
```

**Response headers:**

```http
HTTP/1.1 200 OK
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: keep-alive
X-Request-Id: <request-id>
```

**Authentication:** The client may include a Bearer token in the `Authorization` header. The token is validated once at connection time. If the token expires during the connection, the client must reconnect with a fresh token.

**Query parameters:**

| Param | Type | Description |
|-------|------|-------------|
| `token` | string | Alternative to `Authorization` header (for `EventSource` which doesn't support custom headers) |

### 2.2 Initial Handshake

Upon connecting, the server immediately sends a **PB_CONNECT** event containing the client's connection ID. This ID is used in subsequent subscription requests.

```
event: PB_CONNECT
data: {"clientId":"abc123xyz"}
```

The `clientId` is a server-generated unique identifier (e.g., a nanoid or UUID) for this connection. The client uses it to manage subscriptions via a separate HTTP endpoint.

### 2.3 Keepalive

The server sends a keepalive comment every **30 seconds** (configurable) to prevent proxies and load balancers from closing idle connections:

```
: keepalive
```

SSE comments (lines starting with `:`) are ignored by the `EventSource` API but keep the TCP connection alive.

### 2.4 Event Format

All record change events use the collection name as the SSE `event` field:

```
event: posts
id: <event-id>
data: {"action":"create","record":{"id":"rec_123","title":"Hello","created":"2026-03-21T10:00:00Z","updated":"2026-03-21T10:00:00Z"}}

event: posts
id: <event-id>
data: {"action":"update","record":{"id":"rec_123","title":"Updated","created":"2026-03-21T10:00:00Z","updated":"2026-03-21T10:01:00Z"}}

event: posts
id: <event-id>
data: {"action":"delete","record":{"id":"rec_123"}}
```

**Event data fields:**

| Field | Type | Description |
|-------|------|-------------|
| `action` | `"create"` \| `"update"` \| `"delete"` | The type of change |
| `record` | object | The full record data (after access rule field filtering). For `delete`, only `id` is included. |

**SSE `id` field:** A monotonically increasing event ID (per connection) enabling `Last-Event-ID` reconnection. On reconnect, missed events are NOT replayed (same as PocketBase) — the client should re-fetch data after reconnection.

### 2.5 Subscription Topics

Clients can subscribe to:

| Topic format | Example | Matches |
|-------------|---------|---------|
| `<collection>` | `posts` | All record changes in the `posts` collection |
| `<collection>/<recordId>` | `posts/rec_123` | Changes to a specific record |

Wildcard `*` subscriptions are NOT supported (mirrors PocketBase — you must name specific collections).

---

## 3. Subscription Management

### 3.1 Subscribe/Unsubscribe Endpoint

Subscriptions are managed via a **POST** endpoint, NOT through the SSE stream itself (since SSE is server→client only):

```
POST /api/realtime
Content-Type: application/json
```

**Request body:**

```json
{
    "clientId": "abc123xyz",
    "subscriptions": ["posts", "comments", "posts/rec_123"]
}
```

**Semantics:** Each call **replaces** the client's entire subscription set. To add a subscription, send the full desired set. To unsubscribe from everything, send an empty array.

This "set-based" approach (matching PocketBase) is simpler than incremental add/remove and avoids race conditions.

**Response:**

```json
{
    "code": 200,
    "message": "Subscriptions updated.",
    "data": {
        "clientId": "abc123xyz",
        "subscriptions": ["posts", "comments", "posts/rec_123"]
    }
}
```

**Error cases:**

| Condition | HTTP Status | Message |
|-----------|-------------|---------|
| Missing `clientId` | 400 | `"clientId is required"` |
| Unknown `clientId` (no active SSE connection) | 404 | `"client connection not found"` |
| Invalid subscription format | 400 | `"invalid subscription topic: <topic>"` |
| Referenced collection does not exist | 400 | `"collection not found: <name>"` |

### 3.2 Subscription Validation

When a client subscribes to a collection:

1. **Collection existence**: Verify the collection exists.
2. **View collection exclusion**: View collections are excluded from realtime subscriptions (they have no underlying records to change).
3. **Topic format**: Must match `^[a-zA-Z0-9_]+(/[a-zA-Z0-9_]+)?$`.

Access rules are NOT checked at subscription time — they are checked at event delivery time. This matches PocketBase: you can subscribe to anything, but you only receive events you're authorized to see.

### 3.3 Client Lifecycle

```
Client                                     Server
  │                                          │
  │──── GET /api/realtime ──────────────────>│
  │                                          │ Generate clientId
  │<──── event: PB_CONNECT ────────────────│ Store connection
  │      data: {"clientId":"abc123"}        │
  │                                          │
  │──── POST /api/realtime ────────────────>│
  │      {"clientId":"abc123",              │ Validate & store
  │       "subscriptions":["posts"]}        │ subscriptions
  │<──── 200 OK ────────────────────────────│
  │                                          │
  │                                          │ (another client creates a post)
  │<──── event: posts ─────────────────────│ Rule check passes
  │      data: {"action":"create",...}      │
  │                                          │
  │                   ...                    │
  │                                          │
  │──── (connection drops) ────────────────>│
  │                                          │ Cleanup: remove client,
  │                                          │ drop subscriptions
```

### 3.4 Connection Cleanup

When an SSE connection is closed (client disconnect, network failure, or server shutdown):

1. Remove the client from the connection registry.
2. Drop all subscriptions for that `clientId`.
3. Release any associated resources (broadcast receiver, etc.).

A **connection timeout** of **5 minutes** of inactivity (no keepalive acknowledgment or subscription changes) may optionally be enforced, though SSE connections are expected to be long-lived.

---

## 4. Server Architecture

### 4.1 Component Overview

```
┌─────────────────────────────────────────────────────────────┐
│                      zerobase-api                           │
│                                                             │
│  ┌─────────────────┐    ┌──────────────────────────────┐    │
│  │ SSE Handler     │    │ Subscription POST Handler    │    │
│  │ GET /api/       │    │ POST /api/realtime           │    │
│  │   realtime      │    │                              │    │
│  │                 │    │ Updates subscription set     │    │
│  │ Sends PB_CONNECT│    │ for a clientId               │    │
│  │ Streams events  │    └──────────┬───────────────────┘    │
│  └────────┬────────┘               │                        │
│           │                        │                        │
│           ▼                        ▼                        │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              ConnectionManager                       │    │
│  │                                                     │    │
│  │  clients: HashMap<ClientId, ClientConnection>        │    │
│  │                                                     │    │
│  │  ClientConnection {                                  │    │
│  │    sender: tokio::sync::mpsc::Sender<SseEvent>,     │    │
│  │    subscriptions: HashSet<String>,                   │    │
│  │    auth_info: Option<AuthInfo>,                     │    │
│  │    connected_at: Instant,                           │    │
│  │  }                                                  │    │
│  └────────────────────────┬────────────────────────────┘    │
│                           │                                  │
│                           │ listens on                       │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              EventBroadcaster                        │    │
│  │                                                     │    │
│  │  channel: tokio::sync::broadcast::Sender<RecordEvent>│   │
│  │                                                     │    │
│  │  Receives events from RecordService hooks           │    │
│  │  Fans out to all connected clients                  │    │
│  └─────────────────────────────────────────────────────┘    │
│                           ▲                                  │
└───────────────────────────┼──────────────────────────────────┘
                            │ emits events
┌───────────────────────────┼──────────────────────────────────┐
│                    zerobase-core                              │
│                                                              │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              RecordService                           │    │
│  │                                                     │    │
│  │  create_record() ──> emit RecordEvent::Created      │    │
│  │  update_record() ──> emit RecordEvent::Updated      │    │
│  │  delete_record() ──> emit RecordEvent::Deleted      │    │
│  └─────────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────┘
```

### 4.2 Core Types

```rust
/// Unique identifier for an SSE client connection.
pub type ClientId = String;

/// A record change event emitted by RecordService.
#[derive(Debug, Clone)]
pub struct RecordEvent {
    /// The collection name.
    pub collection: String,
    /// The collection ID.
    pub collection_id: String,
    /// The type of change.
    pub action: RecordAction,
    /// The full record data (all fields, before access rule filtering).
    /// For delete events, contains only the record ID.
    pub record: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordAction {
    Create,
    Update,
    Delete,
}

/// An SSE event ready to be sent to a client.
#[derive(Debug, Clone)]
pub struct SseEvent {
    /// The SSE event name (collection name for record events).
    pub event: String,
    /// The JSON-serialized event data.
    pub data: String,
    /// Monotonic event ID.
    pub id: Option<String>,
}
```

### 4.3 ConnectionManager

The `ConnectionManager` is the central component that tracks all active SSE connections and their subscriptions.

```rust
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

pub struct ConnectionManager {
    /// Map of client ID → client connection state.
    clients: Arc<RwLock<HashMap<ClientId, ClientConnection>>>,
}

struct ClientConnection {
    /// Channel to send SSE events to this client's HTTP response stream.
    sender: mpsc::Sender<SseEvent>,
    /// Active subscription topics (e.g., "posts", "posts/rec_123").
    subscriptions: HashSet<String>,
    /// Auth info captured at connection time (for rule evaluation).
    auth_info: Option<AuthInfo>,
    /// When the connection was established.
    connected_at: std::time::Instant,
}

impl ConnectionManager {
    /// Register a new client connection. Returns the assigned ClientId.
    pub async fn connect(
        &self,
        sender: mpsc::Sender<SseEvent>,
        auth_info: Option<AuthInfo>,
    ) -> ClientId { ... }

    /// Remove a client connection and all its subscriptions.
    pub async fn disconnect(&self, client_id: &str) { ... }

    /// Replace a client's subscription set.
    pub async fn set_subscriptions(
        &self,
        client_id: &str,
        subscriptions: HashSet<String>,
    ) -> Result<(), RealtimeError> { ... }

    /// Get all clients subscribed to a given topic.
    /// Returns (ClientId, sender, auth_info) tuples.
    pub async fn subscribers_for(
        &self,
        collection: &str,
        record_id: &str,
    ) -> Vec<(ClientId, mpsc::Sender<SseEvent>, Option<AuthInfo>)> { ... }

    /// Number of active connections (for metrics/health).
    pub async fn connection_count(&self) -> usize { ... }
}
```

**Thread safety:** The `ConnectionManager` uses `Arc<RwLock<...>>` for the client map. Reads (subscription lookups during event fan-out) are far more frequent than writes (connect/disconnect/subscription changes), making `RwLock` the right choice.

### 4.4 EventBroadcaster

The `EventBroadcaster` bridges `RecordService` mutations to SSE clients.

```rust
use tokio::sync::broadcast;

pub struct EventBroadcaster {
    sender: broadcast::Sender<RecordEvent>,
}

impl EventBroadcaster {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Emit a record change event. Called by RecordService after successful mutations.
    pub fn emit(&self, event: RecordEvent) {
        // Ignore send errors (no receivers = no subscribers = no-op)
        let _ = self.sender.send(event);
    }

    /// Subscribe to the event stream. Used by the SSE connection handler.
    pub fn subscribe(&self) -> broadcast::Receiver<RecordEvent> {
        self.sender.subscribe()
    }
}
```

**Channel capacity:** Default to **256** events. If a client falls behind (slow consumer), older events are dropped. This is acceptable because:
- SSE already has no delivery guarantees (clients must re-fetch on reconnect).
- Slow consumers should not block the event pipeline.
- The `broadcast::Receiver::recv()` returns `RecvError::Lagged(n)` which we can log.

### 4.5 Event Fan-Out Flow

When a record is mutated:

```
RecordService::create_record()
    │
    ▼
EventBroadcaster::emit(RecordEvent { collection: "posts", action: Create, record: {...} })
    │
    ▼
broadcast::channel delivers to all receivers
    │
    ▼ (each SSE connection handler task receives the event)
    │
    ▼
For each client connection:
    1. Check if client is subscribed to "posts" or "posts/<record_id>"
       ├── No  → skip
       └── Yes → continue
    2. Evaluate collection's view_rule against client's auth_info
       (with @request.context = "realtime")
       ├── Rule is None (locked) → skip (unless superuser)
       ├── Rule is Some("") → allow
       └── Rule is Some(expr) → evaluate
           ├── Denied → skip
           └── Allowed → continue
    3. Apply field-level visibility (strip fields the client shouldn't see)
    4. Serialize event and send via client's mpsc::Sender
    5. If send fails (channel closed) → client disconnected, cleanup
```

---

## 5. Access Rule Enforcement

### 5.1 Which Rule Applies

For realtime events, the **view_rule** of the collection is used for access control. This is consistent with PocketBase: receiving a realtime event is equivalent to "viewing" the record.

| Action | Rule checked | Rationale |
|--------|-------------|-----------|
| `create` | `view_rule` | Client needs permission to **see** the new record |
| `update` | `view_rule` | Client needs permission to **see** the updated record |
| `delete` | `view_rule` | Client needs permission to **see** that this record existed |

### 5.2 Request Context for Realtime

When evaluating rules for realtime events, the `RequestContext` is constructed with:

```rust
RequestContext {
    auth: client_auth_fields.clone(),  // Captured at connection time
    data: HashMap::new(),              // No request body for SSE events
    query: HashMap::new(),             // No query params
    headers: HashMap::new(),           // No per-event headers
    method: "GET".to_string(),         // Treat as read operation
    context: "realtime".to_string(),   // Distinguishes from REST API requests
}
```

The `context: "realtime"` value enables rules that differentiate between REST and realtime access:

```
// Allow REST listing but not realtime:
@request.context = "default"

// Allow only realtime access (unlikely but possible):
@request.context = "realtime"

// Allow both (no context check needed, just use normal rules)
```

### 5.3 Rule Evaluation Strategy

Rules are evaluated **at event delivery time**, not at subscription time. This is important because:

1. **Rules may change** between subscription and event delivery.
2. **Record data matters** — rules like `status = "published"` depend on the record's content, which varies per event.
3. **Consistent with PocketBase** — subscribe to anything, receive only what you're authorized to see.

**Performance consideration:** Rule evaluation happens for every (event, client) pair. For `N` clients subscribed to a collection and `M` events/second, that's `N * M` rule evaluations per second. For most deployments this is fine (SQLite single-node, limited concurrent users). Optimizations for high-throughput scenarios:

- **Short-circuit open rules:** `Some("")` (open) skips evaluation entirely.
- **Cache superuser status:** Don't evaluate rules for superusers.
- **In-memory evaluation only:** Realtime rule evaluation uses the in-memory evaluator (no SQL queries), since we already have the full record data.

### 5.4 Field Visibility

After the rule check passes, the event record data is filtered to remove fields the client shouldn't see:

1. **Password fields**: Always stripped from auth collection records (already handled by `RecordService`).
2. **Email visibility**: If the auth collection's `emailVisibility` is not enabled and the viewing user is not the record owner, strip the `email` field.
3. **Hidden fields**: Any fields marked as hidden in the collection schema are stripped.

This uses the same field-stripping logic already in `RecordService::get_record()`.

---

## 6. RecordService Integration

### 6.1 Event Emission Points

The `RecordService` needs to emit events after successful mutations. This is done via an `EventBroadcaster` injected into the service:

```rust
pub struct RecordService<R: RecordRepository, S: SchemaLookup> {
    repo: Arc<R>,
    schema: Arc<S>,
    password_hasher: Arc<dyn PasswordHasher>,
    // NEW: Optional event broadcaster for realtime notifications
    event_broadcaster: Option<Arc<EventBroadcaster>>,
}

impl<R: RecordRepository, S: SchemaLookup> RecordService<R, S> {
    pub fn create_record(&self, ...) -> Result<Record, ZerobaseError> {
        // ... existing create logic ...
        let record = self.repo.insert(collection, &data)?;

        // Emit event AFTER successful persistence
        if let Some(broadcaster) = &self.event_broadcaster {
            broadcaster.emit(RecordEvent {
                collection: collection_name.to_string(),
                collection_id: collection_id.to_string(),
                action: RecordAction::Create,
                record: record.clone(),
            });
        }

        Ok(record)
    }

    pub fn update_record(&self, ...) -> Result<Record, ZerobaseError> {
        // ... existing update logic ...
        let updated = self.repo.update(collection, id, &data)?;

        if let Some(broadcaster) = &self.event_broadcaster {
            broadcaster.emit(RecordEvent {
                collection: collection_name.to_string(),
                collection_id: collection_id.to_string(),
                action: RecordAction::Update,
                record: updated.clone(),
            });
        }

        Ok(updated)
    }

    pub fn delete_record(&self, ...) -> Result<(), ZerobaseError> {
        // Capture record ID before deletion
        let record_id = id.to_string();

        // ... existing delete logic ...
        self.repo.delete(collection, id)?;

        if let Some(broadcaster) = &self.event_broadcaster {
            let mut minimal = serde_json::Map::new();
            minimal.insert("id".to_string(), serde_json::Value::String(record_id));
            broadcaster.emit(RecordEvent {
                collection: collection_name.to_string(),
                collection_id: collection_id.to_string(),
                action: RecordAction::Delete,
                record: minimal,
            });
        }

        Ok(())
    }
}
```

### 6.2 Event Ordering

Events are emitted **after** the database write succeeds. This means:

- No event is emitted for failed mutations.
- Events reflect committed state.
- There is no strict ordering guarantee across concurrent mutations (SQLite's single-writer model provides natural serialization for writes, so events from the write connection are inherently ordered).

### 6.3 Batch Operations

For batch operations (`POST /api/batch`), events are emitted **after the entire batch commits successfully**. All events from the batch are emitted in order.

---

## 7. Axum SSE Handler Implementation

### 7.1 SSE Connection Handler

```rust
use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use futures::stream::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

async fn sse_handler(
    State(connection_manager): State<Arc<ConnectionManager>>,
    State(event_broadcaster): State<Arc<EventBroadcaster>>,
    auth_info: AuthInfo,  // Extracted by auth middleware
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let (tx, rx) = mpsc::channel::<SseEvent>(64);

    // Register this client
    let client_id = connection_manager.connect(tx.clone(), Some(auth_info)).await;

    // Send PB_CONNECT event
    let connect_event = SseEvent {
        event: "PB_CONNECT".to_string(),
        data: serde_json::json!({"clientId": client_id}).to_string(),
        id: None,
    };
    let _ = tx.send(connect_event).await;

    // Spawn background task: listen to broadcast channel and fan out
    let cm = connection_manager.clone();
    let client_id_clone = client_id.clone();
    tokio::spawn(async move {
        let mut receiver = event_broadcaster.subscribe();
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    // Fan-out logic: check subscription, evaluate rules, send
                    handle_event(&cm, &client_id_clone, &event).await;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(client_id = %client_id_clone, skipped = n, "client lagged");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Convert mpsc receiver to SSE stream
    let stream = ReceiverStream::new(rx).map(|sse_event| {
        let mut event = Event::default().event(sse_event.event).data(sse_event.data);
        if let Some(id) = sse_event.id {
            event = event.id(id);
        }
        Ok(event)
    });

    // Cleanup on stream close
    let cm = connection_manager.clone();
    let client_id_cleanup = client_id.clone();
    tokio::spawn(async move {
        // This task waits for the SSE stream to end, then cleans up.
        // The stream ends when the client disconnects.
        // We detect this via the mpsc sender being dropped or closed.
        // (Implementation detail: axum's Sse drops the stream on disconnect)
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(30))
            .text("keepalive"),
    )
}
```

### 7.2 Subscription Handler

```rust
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubscriptionRequest {
    client_id: String,
    subscriptions: Vec<String>,
}

async fn subscribe_handler(
    State(connection_manager): State<Arc<ConnectionManager>>,
    State(schema_lookup): State<Arc<dyn SchemaLookup>>,
    Json(body): Json<SubscriptionRequest>,
) -> Result<Json<serde_json::Value>, ZerobaseError> {
    // Validate each subscription topic
    for topic in &body.subscriptions {
        validate_subscription_topic(topic, &schema_lookup)?;
    }

    // Replace subscription set
    let subscriptions: HashSet<String> = body.subscriptions.iter().cloned().collect();
    connection_manager
        .set_subscriptions(&body.client_id, subscriptions)
        .await?;

    Ok(Json(serde_json::json!({
        "code": 200,
        "message": "Subscriptions updated.",
        "data": {
            "clientId": body.client_id,
            "subscriptions": body.subscriptions,
        }
    })))
}
```

### 7.3 Route Registration

```rust
pub fn realtime_routes(state: AppState) -> Router {
    Router::new()
        .route("/api/realtime", get(sse_handler).post(subscribe_handler))
        .with_state(state)
}
```

---

## 8. Client Usage

### 8.1 JavaScript Client Example

```javascript
// 1. Open SSE connection
const eventSource = new EventSource('/api/realtime', {
    // Note: EventSource doesn't support custom headers.
    // Pass token via query param if needed:
    // new EventSource('/api/realtime?token=<jwt>')
});

let clientId = null;

// 2. Handle connection event
eventSource.addEventListener('PB_CONNECT', (e) => {
    const data = JSON.parse(e.data);
    clientId = data.clientId;

    // 3. Subscribe to collections
    fetch('/api/realtime', {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            'Authorization': 'Bearer <token>',
        },
        body: JSON.stringify({
            clientId: clientId,
            subscriptions: ['posts', 'comments'],
        }),
    });
});

// 4. Listen for record changes
eventSource.addEventListener('posts', (e) => {
    const data = JSON.parse(e.data);
    console.log(`Post ${data.action}:`, data.record);
    // data.action is "create", "update", or "delete"
    // data.record contains the record data
});

eventSource.addEventListener('comments', (e) => {
    const data = JSON.parse(e.data);
    console.log(`Comment ${data.action}:`, data.record);
});

// 5. Handle errors / reconnection
eventSource.onerror = (e) => {
    console.error('SSE error, will auto-reconnect');
    // EventSource automatically reconnects
    // After reconnect, re-subscribe (new clientId will be issued)
};
```

### 8.2 Rust Client Example (for testing)

```rust
use eventsource_stream::Eventsource;
use reqwest::Client;

let client = Client::new();
let response = client
    .get("http://localhost:8090/api/realtime")
    .header("Authorization", format!("Bearer {}", token))
    .send()
    .await?;

let mut stream = response.bytes_stream().eventsource();

while let Some(event) = stream.next().await {
    match event {
        Ok(ev) if ev.event == "PB_CONNECT" => {
            let data: serde_json::Value = serde_json::from_str(&ev.data)?;
            let client_id = data["clientId"].as_str().unwrap();
            // Subscribe via POST...
        }
        Ok(ev) => {
            println!("Event: {} - {}", ev.event, ev.data);
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

---

## 9. Configuration

### 9.1 Server Settings

Add to the existing `[server]` configuration section:

```toml
[realtime]
# Whether realtime subscriptions are enabled
enabled = true

# Maximum number of concurrent SSE connections
max_connections = 1000

# Keepalive interval in seconds
keepalive_interval_secs = 30

# Maximum subscriptions per client
max_subscriptions_per_client = 100

# Event broadcast channel capacity
broadcast_channel_capacity = 256
```

### 9.2 Defaults

| Setting | Default | Rationale |
|---------|---------|-----------|
| `enabled` | `true` | Realtime is a core feature |
| `max_connections` | `1000` | Reasonable for single-node SQLite |
| `keepalive_interval_secs` | `30` | Standard for SSE, prevents proxy timeouts |
| `max_subscriptions_per_client` | `100` | Prevent abuse |
| `broadcast_channel_capacity` | `256` | Handles bursts without excessive memory |

---

## 10. Error Handling & Edge Cases

### 10.1 Client Disconnection

When the SSE connection drops:
- The `mpsc::Sender` for the client will return an error on next send.
- The fan-out task detects this and calls `ConnectionManager::disconnect()`.
- All subscriptions and resources are cleaned up.

### 10.2 Slow Consumers

If a client cannot consume events fast enough:
- The `mpsc::channel(64)` per client provides a buffer.
- If the per-client buffer fills up, the send fails and the client is disconnected (they can reconnect).
- The `broadcast::channel(256)` provides a shared buffer; if a receiver lags, `RecvError::Lagged(n)` is returned. The fan-out task logs the lag and continues.

### 10.3 Server Shutdown

On graceful shutdown:
- The `EventBroadcaster` channel is dropped, causing all fan-out tasks to exit.
- Each SSE stream is closed, causing clients to receive an error and (if using `EventSource`) auto-reconnect.

### 10.4 Auth Token Expiration

Auth info is captured at connection time. If the token expires mid-connection:
- The connection remains open (SSE has no per-message auth).
- Rule evaluation uses the stale auth info, which may deny events.
- The client should proactively reconnect before token expiry to maintain access.

This matches PocketBase's behavior. A future enhancement could add periodic token re-validation.

### 10.5 Collection Schema Changes

If a collection's schema or rules change while clients are subscribed:
- Future events use the **new** rules (rules are evaluated per event, not cached at subscription time).
- If a collection is deleted, existing subscriptions to it simply receive no more events (no error event is sent).
- Optionally, a system event (`PB_SCHEMA_CHANGE`) could be sent to all clients, but this is not required for v1.

---

## 11. Crate Placement

| Component | Crate | Rationale |
|-----------|-------|-----------|
| `RecordEvent`, `RecordAction`, `SseEvent` | `zerobase-core` | Domain types, no I/O |
| `EventBroadcaster` trait | `zerobase-core` | Interface definition |
| `EventBroadcaster` impl (tokio broadcast) | `zerobase-api` | Runtime concern (tokio channels) |
| `ConnectionManager` | `zerobase-api` | HTTP layer state management |
| SSE handler, subscription handler | `zerobase-api` | HTTP handlers |
| `RecordService` event emission | `zerobase-core` | Hooks into existing mutation methods |
| Realtime configuration | `zerobase-core` (configuration) | Part of server settings |

---

## 12. Testing Strategy

### 12.1 Unit Tests (zerobase-core)

- `RecordEvent` serialization/deserialization.
- `RecordAction` variant coverage.
- Event emission trait — mock broadcaster captures emitted events.

### 12.2 Unit Tests (zerobase-api)

- **ConnectionManager**:
  - `connect()` returns unique client IDs.
  - `disconnect()` removes client and subscriptions.
  - `set_subscriptions()` replaces (not appends) the subscription set.
  - `set_subscriptions()` for unknown `clientId` returns error.
  - `subscribers_for("posts", "")` returns clients subscribed to `"posts"`.
  - `subscribers_for("posts", "rec_123")` returns clients subscribed to `"posts"` OR `"posts/rec_123"`.
  - Concurrent connect/disconnect is safe (no deadlocks, no panics).

- **EventBroadcaster**:
  - Single event reaches all subscribers.
  - No receivers = no error (fire-and-forget).
  - Lagged receiver gets `RecvError::Lagged`.

- **Subscription validation**:
  - Valid topics accepted: `"posts"`, `"posts/rec_123"`.
  - Invalid topics rejected: `""`, `"posts/"`, `"posts/rec/extra"`, `"../etc"`.
  - Non-existent collection rejected.

### 12.3 Integration Tests

- **Full SSE flow**: Connect → receive `PB_CONNECT` → subscribe → create record → receive event.
- **Access rules**: Client without view permission does NOT receive events.
- **Superuser bypass**: Superuser receives all events regardless of rules.
- **Record-specific subscription**: Subscribe to `"posts/rec_123"`, only receive events for that record.
- **Collection-wide subscription**: Subscribe to `"posts"`, receive events for all records in the collection.
- **Delete events**: Receive delete event with record ID only.
- **Subscription replacement**: Change subscriptions, verify old topics no longer deliver events.
- **Disconnect cleanup**: After disconnect, no more events are delivered, no resource leaks.
- **Concurrent clients**: Multiple clients with different subscriptions receive correct events.
- **Auth integration**: Authenticated client receives events that pass view_rule; anonymous client does not.
- **Realtime context**: Rule `@request.context = "realtime"` evaluates correctly for SSE events.

### 12.4 Load/Stress Tests (optional, future)

- 100 concurrent SSE connections with rapid record mutations.
- Slow consumer behavior (verify graceful degradation, not memory leak).
- Connection churn (rapid connect/disconnect cycles).

---

## 13. Dependencies

### New Dependencies Required

| Crate | Version | Purpose |
|-------|---------|---------|
| `tokio-stream` | `0.1` | Convert `mpsc::Receiver` to `Stream` for axum's `Sse` |
| `futures` | `0.3` | `Stream` trait and combinators |

**Note:** `axum` already provides `axum::response::sse::{Sse, Event, KeepAlive}` — no additional SSE crate needed. `tokio::sync::broadcast` and `tokio::sync::mpsc` are already available through the `tokio` dependency.

### No New Dependencies for Core

The `zerobase-core` crate only needs the event types and trait definitions — no new dependencies required.

---

## 14. Security Considerations

1. **No subscription-time auth check**: Subscribing to a collection does not require view access. Access is checked at event delivery time. This prevents information leakage about rule configuration.

2. **Token via query parameter**: Since `EventSource` doesn't support custom headers, tokens can be passed via `?token=<jwt>`. This token appears in server logs — ensure log sanitization strips tokens from URLs.

3. **Connection limits**: The `max_connections` setting prevents resource exhaustion from too many SSE connections. Connections beyond the limit receive `503 Service Unavailable`.

4. **Subscription limits**: `max_subscriptions_per_client` prevents a single client from subscribing to an excessive number of topics.

5. **No event replay**: Events are not persisted or replayed on reconnection. This avoids the complexity of an event store and the risk of replaying events to clients whose access has since been revoked.

6. **Stale auth info**: Auth is captured at connection time. A client whose access is revoked will continue to be evaluated with old auth info until they reconnect. For sensitive applications, consider adding periodic re-validation (future enhancement, not v1).

7. **Field stripping**: Event payloads go through the same field visibility logic as REST API responses — password fields, hidden fields, and email visibility are all respected.

8. **CORS**: SSE connections respect the same CORS policy as other API endpoints. The `EventSource` API sends cookies and respects CORS automatically.

---

## 15. PocketBase Compatibility

This design closely mirrors PocketBase's realtime system:

| Feature | PocketBase | Zerobase | Notes |
|---------|-----------|----------|-------|
| Transport | SSE | SSE | Same |
| Connection event | `PB_CONNECT` | `PB_CONNECT` | Same event name for client compatibility |
| Subscription endpoint | `POST /api/realtime` | `POST /api/realtime` | Same |
| Subscription model | Set replacement | Set replacement | Same |
| Topic format | `collection` or `collection/recordId` | `collection` or `collection/recordId` | Same |
| Rule enforcement | `view_rule` at delivery time | `view_rule` at delivery time | Same |
| Request context | `@request.context = "realtime"` | `@request.context = "realtime"` | Same |
| Event replay on reconnect | No | No | Same |
| Auth | Token at connect time | Token at connect time | Same |

---

## 16. Future Enhancements (Out of Scope for v1)

1. **Schema change events**: Notify clients when collection schemas change (`PB_SCHEMA_CHANGE`).
2. **Periodic auth re-validation**: Re-check tokens periodically to revoke access mid-connection.
3. **Event filtering**: Allow clients to subscribe with a filter expression (e.g., `posts?filter=status="published"`), reducing server-side fan-out.
4. **Metrics**: Track connection count, events/second, rule evaluation latency.
5. **Clustered deployments**: For multi-node setups, use Redis pub/sub or similar for cross-node event distribution.
