# Extensibility & Hooks System Design

> Design document for Zerobase's extensibility layer — hook points, Rust library mode, and embedded JS runtime.

---

## 1. Overview

Zerobase supports two extensibility modes, mirroring PocketBase:

1. **Rust Library Mode** — Use Zerobase as a framework/crate. Register strongly-typed hook handlers at compile time. Full access to all Rust types and async runtime.
2. **JS Hooks Mode** — Drop `.pb.js` files into a `pb_hooks/` directory. An embedded JS runtime (Boa or Rquickjs) evaluates them at startup, giving non-Rust developers a scripting surface without recompilation.

Both modes feed into the same underlying `HookRegistry`, so they compose naturally.

---

## 2. Hook Points

Every hook point fires in a **before/after** pair around a core operation. "Before" hooks can inspect and mutate the event, or abort it by returning an error. "After" hooks observe the completed operation (read-only on the committed state).

### 2.1 Record Lifecycle Hooks

| Hook | Fires When | Mutable Fields |
|------|-----------|----------------|
| `OnBeforeRecordCreate` | After validation, before `INSERT` | record data, collection ref |
| `OnAfterRecordCreate` | After successful `INSERT` + commit | record data (read-only) |
| `OnBeforeRecordUpdate` | After validation, before `UPDATE` | record data (new), old record snapshot |
| `OnAfterRecordUpdate` | After successful `UPDATE` + commit | record data (read-only), old snapshot |
| `OnBeforeRecordDelete` | Before `DELETE` | record data (read-only, can abort) |
| `OnAfterRecordDelete` | After successful `DELETE` + commit | deleted record snapshot (read-only) |

### 2.2 Auth Hooks

| Hook | Fires When | Mutable Fields |
|------|-----------|----------------|
| `OnBeforeAuth` | After credential validation, before token issuance | auth record, auth method, provider info |
| `OnAfterAuth` | After token issued | auth record, token (read-only) |
| `OnBeforeOAuthConnect` | Before linking OAuth identity | OAuth user info, target auth record |
| `OnAfterOAuthConnect` | After OAuth identity linked | linked external auth (read-only) |
| `OnBeforeOtpAuth` | Before OTP verification | OTP record, auth record |
| `OnAfterOtpAuth` | After OTP auth succeeds | auth record, token |
| `OnBeforePasskeyAuth` | Before passkey verification | webauthn assertion, auth record |
| `OnAfterPasskeyAuth` | After passkey auth succeeds | auth record, token |

### 2.3 Collection Schema Hooks

| Hook | Fires When |
|------|-----------|
| `OnBeforeCollectionCreate` | Before DDL for new collection |
| `OnAfterCollectionCreate` | After collection + table created |
| `OnBeforeCollectionUpdate` | Before schema migration (ALTER) |
| `OnAfterCollectionUpdate` | After schema migration applied |
| `OnBeforeCollectionDelete` | Before DROP TABLE |
| `OnAfterCollectionDelete` | After table dropped |

### 2.4 Request Lifecycle Hooks

| Hook | Fires When |
|------|-----------|
| `OnBeforeApiRequest` | Before any API route handler runs (global middleware) |
| `OnAfterApiRequest` | After response is produced, before sending |
| `OnBeforeServe` | During server startup, before listening |
| `OnAfterBootstrap` | After DB migrations and initial setup complete |
| `OnTerminate` | On graceful shutdown signal |

### 2.5 Realtime Hooks

| Hook | Fires When |
|------|-----------|
| `OnBeforeRealtimeConnect` | Before SSE connection accepted |
| `OnAfterRealtimeConnect` | After SSE client registered |
| `OnBeforeRealtimeSubscribe` | Before subscription set updated |
| `OnAfterRealtimeSubscribe` | After subscription set updated |
| `OnBeforeRealtimeMessage` | Before event delivered to client |

### 2.6 File Hooks

| Hook | Fires When |
|------|-----------|
| `OnBeforeFileUpload` | Before file persisted to storage |
| `OnAfterFileUpload` | After file stored and record updated |
| `OnBeforeFileDownload` | Before file served (can redirect, add headers) |

### 2.7 Mail Hooks

| Hook | Fires When |
|------|-----------|
| `OnBeforeMailSend` | Before email dispatched (verification, password reset, OTP) |
| `OnAfterMailSend` | After email sent |

### 2.8 Custom Routes

Not a hook per se, but part of the extensibility surface. Users register custom `axum::Router` subtrees that get merged into the main router.

---

## 3. Architecture

### 3.1 Core Types (`zerobase-core`)

```rust
/// Identifies a hook point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookPoint {
    BeforeRecordCreate,
    AfterRecordCreate,
    BeforeRecordUpdate,
    AfterRecordUpdate,
    BeforeRecordDelete,
    AfterRecordDelete,
    BeforeAuth,
    AfterAuth,
    BeforeOAuthConnect,
    AfterOAuthConnect,
    BeforeOtpAuth,
    AfterOtpAuth,
    BeforePasskeyAuth,
    AfterPasskeyAuth,
    BeforeCollectionCreate,
    AfterCollectionCreate,
    BeforeCollectionUpdate,
    AfterCollectionUpdate,
    BeforeCollectionDelete,
    AfterCollectionDelete,
    BeforeApiRequest,
    AfterApiRequest,
    BeforeServe,
    AfterBootstrap,
    Terminate,
    BeforeRealtimeConnect,
    AfterRealtimeConnect,
    BeforeRealtimeSubscribe,
    AfterRealtimeSubscribe,
    BeforeRealtimeMessage,
    BeforeFileUpload,
    AfterFileUpload,
    BeforeFileDownload,
    BeforeMailSend,
    AfterMailSend,
}
```

### 3.2 Hook Event

Each hook point has an associated event struct carrying the relevant context. All events implement a common trait:

```rust
/// Marker trait for all hook events.
pub trait HookEvent: Send + Sync + 'static {
    /// Which hook point this event corresponds to.
    fn hook_point(&self) -> HookPoint;
}
```

Example event structs:

```rust
/// Event fired before/after record creation.
pub struct RecordCreateEvent {
    /// The collection being written to.
    pub collection: Collection,
    /// The record data (mutable in "before" hooks).
    pub record: HashMap<String, Value>,
    /// The authenticated user (if any).
    pub auth_info: AuthInfo,
    /// HTTP request metadata.
    pub request_info: RequestInfo,
}

/// Event fired before/after authentication.
pub struct AuthEvent {
    pub collection: Collection,
    pub auth_record: HashMap<String, Value>,
    pub auth_method: String,         // "password", "otp", "oauth2", "passkey"
    pub provider: Option<String>,    // OAuth provider name
    pub token: Option<String>,       // set in AfterAuth
    pub auth_info: AuthInfo,
    pub request_info: RequestInfo,
}

/// Event fired before/after record update.
pub struct RecordUpdateEvent {
    pub collection: Collection,
    pub record: HashMap<String, Value>,      // new data
    pub old_record: HashMap<String, Value>,  // snapshot before mutation
    pub auth_info: AuthInfo,
    pub request_info: RequestInfo,
}

/// Event fired before/after record deletion.
pub struct RecordDeleteEvent {
    pub collection: Collection,
    pub record: HashMap<String, Value>,
    pub auth_info: AuthInfo,
    pub request_info: RequestInfo,
}

/// Minimal HTTP request metadata available to hooks.
pub struct RequestInfo {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub body: Option<Value>,
}
```

### 3.3 Hook Handler Trait

```rust
/// A hook handler that can be registered for one or more hook points.
///
/// The handler receives a mutable reference to the event so "before" hooks
/// can modify data. It returns `Result<()>` — returning `Err` from a
/// "before" hook aborts the operation.
#[async_trait]
pub trait HookHandler<E: HookEvent>: Send + Sync {
    /// Execute the hook logic.
    async fn handle(&self, event: &mut E) -> Result<()>;

    /// Optional: restrict this handler to specific collection(s).
    /// Returns `None` to match all collections.
    fn collection_filter(&self) -> Option<&[&str]> {
        None
    }

    /// Priority for ordering. Lower = runs first. Default = 0.
    fn priority(&self) -> i32 {
        0
    }
}
```

### 3.4 Hook Registry

The `HookRegistry` is a type-erased container that stores handlers per `HookPoint`. It lives in `zerobase-core` and is threadsafe (`Arc<HookRegistry>`).

```rust
pub struct HookRegistry {
    // Internal storage: HookPoint → Vec<(priority, Box<dyn ErasedHandler>)>
    handlers: RwLock<HashMap<HookPoint, Vec<PrioritizedHandler>>>,
}

impl HookRegistry {
    pub fn new() -> Self { /* ... */ }

    /// Register a typed hook handler.
    pub fn on<E: HookEvent>(&self, point: HookPoint, handler: impl HookHandler<E> + 'static);

    /// Fire all handlers for a hook point, in priority order.
    /// For "before" hooks, short-circuits on first error.
    pub async fn fire<E: HookEvent>(&self, point: HookPoint, event: &mut E) -> Result<()>;

    /// Remove all handlers for a hook point (used for testing/reset).
    pub fn clear(&self, point: HookPoint);

    /// Remove all handlers.
    pub fn clear_all(&self);
}
```

**Type erasure**: Internally, handlers are stored as `Box<dyn ErasedHandler>` where `ErasedHandler` wraps the generic `HookHandler<E>` behind a trait object that operates on `&mut dyn Any`. The `fire` method downcasts the event to the correct type before calling each handler.

### 3.5 Integration with Services

The `HookRegistry` is injected into services (like `RecordService`) via `Arc`:

```rust
pub struct RecordService<R, S> {
    record_repo: Arc<R>,
    schema_lookup: Arc<S>,
    password_hasher: Arc<dyn PasswordHasher>,
    hooks: Arc<HookRegistry>,  // ← NEW
}
```

Service methods fire hooks at the appropriate points:

```rust
impl<R: RecordRepository, S: SchemaLookup> RecordService<R, S> {
    pub async fn create_record(
        &self,
        collection: &Collection,
        mut data: HashMap<String, Value>,
        auth_info: &AuthInfo,
        request_info: &RequestInfo,
    ) -> Result<HashMap<String, Value>> {
        // 1. Validate
        self.validate(&collection, &data)?;

        // 2. Fire before hook
        let mut event = RecordCreateEvent {
            collection: collection.clone(),
            record: data.clone(),
            auth_info: auth_info.clone(),
            request_info: request_info.clone(),
        };
        self.hooks.fire(HookPoint::BeforeRecordCreate, &mut event).await?;
        data = event.record; // hooks may have mutated data

        // 3. Persist
        self.record_repo.insert(&collection.name, &data)?;

        // 4. Fire after hook (errors logged, not propagated)
        let mut after_event = RecordCreateEvent {
            collection: collection.clone(),
            record: data.clone(),
            auth_info: auth_info.clone(),
            request_info: request_info.clone(),
        };
        if let Err(e) = self.hooks.fire(HookPoint::AfterRecordCreate, &mut after_event).await {
            tracing::warn!(hook = "AfterRecordCreate", error = %e, "after-hook failed");
        }

        Ok(data)
    }
}
```

---

## 4. Extension Mode 1: Rust Library Mode

### 4.1 Usage Pattern

When Zerobase is used as a library (framework mode), users add `zerobase` as a dependency and compose their own `main()`:

```rust
use zerobase::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = Zerobase::new()
        .data_dir("./pb_data")
        .build()
        .await?;

    // Register hooks
    app.on_before_record_create(|event| async move {
        if event.collection.name == "posts" {
            // Auto-set author to authenticated user
            if let Some(auth_id) = event.auth_info.record_id() {
                event.record.insert("author".into(), Value::String(auth_id.to_string()));
            }
        }
        Ok(())
    });

    app.on_after_record_create(|event| async move {
        if event.collection.name == "orders" {
            // Send notification to external service
            notify_warehouse(&event.record).await?;
        }
        Ok(())
    });

    // Register custom routes
    app.add_routes(
        axum::Router::new()
            .route("/api/custom/stats", axum::routing::get(custom_stats_handler))
    );

    // Start server
    app.serve("0.0.0.0:8090").await?;

    Ok(())
}
```

### 4.2 Convenience Methods on `Zerobase`

The `Zerobase` app struct provides ergonomic methods that delegate to `HookRegistry`:

```rust
impl Zerobase {
    // Record hooks
    pub fn on_before_record_create(&self, f: impl IntoHookHandler<RecordCreateEvent>);
    pub fn on_after_record_create(&self, f: impl IntoHookHandler<RecordCreateEvent>);
    pub fn on_before_record_update(&self, f: impl IntoHookHandler<RecordUpdateEvent>);
    pub fn on_after_record_update(&self, f: impl IntoHookHandler<RecordUpdateEvent>);
    pub fn on_before_record_delete(&self, f: impl IntoHookHandler<RecordDeleteEvent>);
    pub fn on_after_record_delete(&self, f: impl IntoHookHandler<RecordDeleteEvent>);

    // Auth hooks
    pub fn on_before_auth(&self, f: impl IntoHookHandler<AuthEvent>);
    pub fn on_after_auth(&self, f: impl IntoHookHandler<AuthEvent>);

    // Collection hooks
    pub fn on_before_collection_create(&self, f: impl IntoHookHandler<CollectionEvent>);
    pub fn on_after_collection_create(&self, f: impl IntoHookHandler<CollectionEvent>);
    // ... etc for update, delete

    // Lifecycle hooks
    pub fn on_before_serve(&self, f: impl IntoHookHandler<ServeEvent>);
    pub fn on_terminate(&self, f: impl IntoHookHandler<TerminateEvent>);

    // Custom routes
    pub fn add_routes(&self, router: axum::Router);
    pub fn add_middleware(&self, layer: tower::Layer);
}
```

### 4.3 `IntoHookHandler` Trait

Allows both closures and structs to be used as handlers:

```rust
/// Converts closures and async fns into HookHandler implementations.
pub trait IntoHookHandler<E: HookEvent> {
    type Handler: HookHandler<E>;
    fn into_handler(self) -> Self::Handler;
}

// Blanket impl for async closures:
// impl<E, F, Fut> IntoHookHandler<E> for F
// where
//     F: Fn(&mut E) -> Fut + Send + Sync + 'static,
//     Fut: Future<Output = Result<()>> + Send,
```

### 4.4 Accessing App State from Hooks

Hooks can access the Zerobase `App` context (DB, services, etc.) through the event:

```rust
app.on_before_record_create(|event| async move {
    // Access DB directly via app reference on event
    let app = event.app();
    let count = app.record_service()
        .count(&event.collection.name, None)
        .await?;

    if count > 1000 {
        return Err(ZerobaseError::forbidden("collection record limit reached"));
    }
    Ok(())
});
```

---

## 5. Extension Mode 2: JS Hooks (Embedded Runtime)

### 5.1 Overview

For users who don't want to compile Rust, Zerobase supports JavaScript hook files placed in `pb_hooks/`. These are loaded and executed by an embedded JS runtime at startup.

**Runtime choice**: [Rquickjs](https://crates.io/crates/rquickjs) (QuickJS bindings for Rust). Reasons:
- Lightweight, embeddable, single-threaded per context
- Good Rust interop (Serde-based value conversion)
- Async support via Rust-side polling
- Well-maintained crate (~1M downloads)
- PocketBase uses a similar approach (JSVM)

### 5.2 JS API Surface

The JS runtime exposes a global `$app` object mirroring the Rust `Zerobase` struct:

```javascript
// pb_hooks/posts.pb.js

// Record hooks
$app.onBeforeRecordCreate((e) => {
    if (e.collection.name === "posts") {
        e.record.set("slug", slugify(e.record.get("title")));
    }
});

$app.onAfterRecordCreate((e) => {
    if (e.collection.name === "notifications") {
        // Send via built-in mailer
        $app.sendMail({
            to: e.record.get("email"),
            subject: "New notification",
            body: `You have a new notification: ${e.record.get("message")}`,
        });
    }
});

// Auth hooks
$app.onBeforeAuth((e) => {
    if (e.collection.name === "users" && !e.record.get("verified")) {
        throw new ForbiddenError("email not verified");
    }
});

// Custom routes
$app.addRoute({
    method: "GET",
    path: "/api/custom/hello",
    handler: (req, res) => {
        res.json({ message: "Hello from JS!" });
    },
});

// Access DB
$app.onAfterRecordCreate((e) => {
    const count = $app.db.count(e.collection.name);
    if (count > 500) {
        $app.sendMail({
            to: "admin@example.com",
            subject: "Collection growing large",
            body: `${e.collection.name} now has ${count} records.`,
        });
    }
});
```

### 5.3 JS Runtime Architecture

```
┌───────────────────────────────────────────────┐
│  zerobase-server startup                      │
│                                               │
│  1. Initialize HookRegistry                   │
│  2. Load pb_hooks/*.pb.js files               │
│  3. For each file:                            │
│     a. Create JsRuntime context               │
│     b. Inject $app global bindings            │
│     c. Evaluate script (registers hooks)      │
│  4. JS hooks → JsHookAdapter → HookRegistry   │
│  5. Start axum server                         │
└───────────────────────────────────────────────┘
```

### 5.4 `JsHookAdapter`

Bridges JS callback functions to the Rust `HookHandler` trait:

```rust
/// Wraps a JS function reference as a Rust HookHandler.
pub struct JsHookAdapter {
    runtime: Arc<JsRuntime>,
    callback: JsFunction,  // persistent reference to the JS callback
    collection_filter: Option<Vec<String>>,
}

#[async_trait]
impl<E: HookEvent + IntoJsValue + FromJsValue> HookHandler<E> for JsHookAdapter {
    async fn handle(&self, event: &mut E) -> Result<()> {
        // 1. Convert event to JS value
        let js_event = event.to_js_value(&self.runtime)?;

        // 2. Call JS function
        let result = self.runtime.call(&self.callback, &[js_event]).await?;

        // 3. If "before" hook, read back mutations from JS event object
        event.apply_js_mutations(&self.runtime, &js_event)?;

        // 4. Check for thrown exceptions
        if let Err(js_err) = result {
            return Err(ZerobaseError::hook_error(js_err.to_string()));
        }

        Ok(())
    }
}
```

### 5.5 JS ↔ Rust Value Conversion

Events are converted to plain JS objects via Serde. Mutations flow back by diffing the JS object state after the callback returns.

```rust
/// Trait for converting hook events to/from JS values.
pub trait JsEventBridge: Sized {
    fn to_js_value(&self, rt: &JsRuntime) -> Result<JsValue>;
    fn apply_js_mutations(&mut self, rt: &JsRuntime, js_val: &JsValue) -> Result<()>;
}
```

### 5.6 JS Built-in Globals

| Global | Purpose |
|--------|---------|
| `$app` | Main app object — hook registration, DB access, mail, settings |
| `$app.db` | Direct DB queries: `findOne`, `findMany`, `count`, `exec` |
| `$app.settings` | Read/write server settings |
| `$app.sendMail(opts)` | Send email |
| `$app.logger` | Structured logging: `info`, `warn`, `error` |
| `ForbiddenError` | Throw to return 403 |
| `NotFoundError` | Throw to return 404 |
| `ValidationError` | Throw to return 400 with field errors |
| `require(path)` | Load other JS modules (relative to `pb_hooks/`) |
| `console.log` | Maps to `tracing::info` |
| `setTimeout/setInterval` | Not supported (hooks are synchronous from JS perspective) |

### 5.7 File Loading & Hot Reload

- On startup, scan `pb_hooks/` for `*.pb.js` files, sorted alphabetically.
- Each file is evaluated in a shared JS context.
- **Optional hot reload** (dev mode only): watch `pb_hooks/` for changes, clear all JS-registered hooks, re-evaluate all files.
- Hot reload does NOT affect Rust-registered hooks.

### 5.8 Security Considerations

- JS runtime runs in the same process — no sandboxing beyond QuickJS's own isolation.
- No filesystem access from JS (no `fs` module). Only `$app`-provided APIs.
- No network access from JS (no `fetch`). Use `$app.sendMail` or `$app.httpClient` for outbound calls.
- Resource limits: max execution time per hook (default 10s), max memory per context (default 64MB).

---

## 6. Hook Execution Semantics

### 6.1 Ordering

1. Rust-registered handlers run first (by priority, then registration order).
2. JS-registered handlers run second (by file order, then registration order within file).
3. Within the same priority, registration order is preserved (FIFO).

### 6.2 Error Handling

| Hook Type | Error Behavior |
|-----------|---------------|
| **Before** hooks | First error aborts the operation. Caller receives the error. |
| **After** hooks | Errors are logged as warnings. Operation is NOT rolled back. |
| **Lifecycle** hooks (`OnBeforeServe`, etc.) | Errors are logged. Server continues startup. |

### 6.3 Async Behavior

- Rust hooks are fully async (`async fn`).
- JS hooks block the JS runtime thread but are called from an async context via `spawn_blocking` or the rquickjs async bridge.
- Each "before" hook chain runs sequentially (order matters for mutations).
- "After" hooks MAY run concurrently in the future (opt-in), but default is sequential.

### 6.4 Collection Filtering

Hooks can optionally filter by collection name. The registry skips handlers that don't match the current collection:

```rust
// Rust: only fires for "posts" collection
app.on_before_record_create(
    HookBuilder::new()
        .collections(&["posts"])
        .handler(|event| async move {
            // ...
            Ok(())
        })
);
```

```javascript
// JS: only fires for "posts" collection
$app.onBeforeRecordCreate((e) => {
    e.record.set("slug", slugify(e.record.get("title")));
}, { collections: ["posts"] });
```

---

## 7. Custom Routes

### 7.1 Rust Mode

```rust
use axum::{routing::get, Json};
use zerobase::prelude::*;

async fn custom_stats(
    State(app): State<Arc<Zerobase>>,
    AuthInfo(auth): AuthInfo,
) -> Result<Json<Value>> {
    let count = app.record_service().count("posts", None).await?;
    Ok(Json(json!({ "total_posts": count })))
}

app.add_routes(
    axum::Router::new()
        .route("/api/custom/stats", get(custom_stats))
);
```

Custom routes get the same middleware stack (auth, request ID, logging) as built-in routes.

### 7.2 JS Mode

```javascript
$app.addRoute({
    method: "GET",
    path: "/api/custom/stats",
    middleware: ["auth"],  // optional: require authentication
    handler: (req, res) => {
        const count = $app.db.count("posts");
        res.json({ total_posts: count });
    },
});
```

JS routes are compiled into axum handlers via a `JsRouteHandler` adapter at startup.

---

## 8. Crate Placement

| Component | Crate | Rationale |
|-----------|-------|-----------|
| `HookPoint`, `HookEvent`, `HookHandler` trait | `zerobase-core` | Domain-level abstractions, no I/O |
| `HookRegistry` | `zerobase-core` | Services need it, must be framework-agnostic |
| Event structs (`RecordCreateEvent`, etc.) | `zerobase-core` | Tied to domain types |
| `JsRuntime`, `JsHookAdapter`, `JsRouteHandler` | New: `zerobase-hooks` | Isolates JS runtime dependency |
| Hook integration in services | `zerobase-core` services | Natural placement |
| Hook wiring + `Zerobase` app struct | `zerobase-server` | Composition root |
| `$app` JS bindings | `zerobase-hooks` | Bridges Rust services to JS |

### New crate: `zerobase-hooks`

```toml
[package]
name = "zerobase-hooks"

[dependencies]
zerobase-core = { path = "../zerobase-core" }
rquickjs = { version = "0.6", features = ["bindgen", "classes", "properties"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["rt"] }
tracing = "0.1"
```

This keeps the JS runtime dependency optional — users of Rust library mode don't need it.

---

## 9. Feature Flags

```toml
# zerobase-server/Cargo.toml
[features]
default = ["js-hooks"]
js-hooks = ["zerobase-hooks"]
```

When `js-hooks` is disabled, the binary is smaller and has no JS runtime overhead. The `HookRegistry` still works for Rust-registered hooks.

---

## 10. Example Use Cases

### 10.1 Auto-populate fields on create (Rust)

```rust
app.on_before_record_create(|event| async move {
    if event.collection.name == "articles" {
        let title = event.record.get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let slug = title.to_lowercase().replace(' ', "-");
        event.record.insert("slug".into(), Value::String(slug));
    }
    Ok(())
});
```

### 10.2 Audit logging (JS)

```javascript
$app.onAfterRecordUpdate((e) => {
    $app.logger.info("record_updated", {
        collection: e.collection.name,
        recordId: e.record.get("id"),
        updatedBy: e.authRecord?.get("id") || "anonymous",
    });
});
```

### 10.3 Prevent deletion of protected records (Rust)

```rust
app.on_before_record_delete(
    HookBuilder::new()
        .collections(&["settings"])
        .handler(|event| async move {
            if event.record.get("protected").and_then(|v| v.as_bool()) == Some(true) {
                return Err(ZerobaseError::forbidden("cannot delete protected record"));
            }
            Ok(())
        })
);
```

### 10.4 Send welcome email after user creation (JS)

```javascript
$app.onAfterRecordCreate((e) => {
    if (e.collection.name === "users") {
        $app.sendMail({
            to: e.record.get("email"),
            subject: "Welcome!",
            body: `Welcome to the platform, ${e.record.get("name")}!`,
        });
    }
});
```

### 10.5 Custom API endpoint with DB access (Rust)

```rust
async fn leaderboard(
    State(app): State<Arc<Zerobase>>,
) -> Result<Json<Value>> {
    let top = app.record_service()
        .find_many("scores", &RecordQuery {
            sort: vec![("points".into(), SortDirection::Desc)],
            per_page: 10,
            ..Default::default()
        })
        .await?;
    Ok(Json(json!({ "leaderboard": top.items })))
}

app.add_routes(
    axum::Router::new().route("/api/custom/leaderboard", get(leaderboard))
);
```

### 10.6 Rate limiting via before-request hook (Rust)

```rust
app.on_before_api_request(|event| async move {
    let ip = event.request_info.headers.get("x-forwarded-for")
        .or(event.request_info.headers.get("x-real-ip"))
        .map(|s| s.as_str())
        .unwrap_or("unknown");

    if rate_limiter.check(ip).is_err() {
        return Err(ZerobaseError::too_many_requests("rate limit exceeded"));
    }
    Ok(())
});
```

### 10.7 OAuth post-processing — sync profile picture (JS)

```javascript
$app.onAfterOAuthConnect((e) => {
    if (e.provider === "google" && e.oauthUser.avatarUrl) {
        $app.db.update(e.collection.name, e.record.get("id"), {
            avatar_url: e.oauthUser.avatarUrl,
        });
    }
});
```

---

## 11. Testing Strategy

### 11.1 Unit Tests (`zerobase-core`)

- Test `HookRegistry` registration, ordering, priority, and firing.
- Test that "before" hook errors abort the chain.
- Test that "after" hook errors are contained (don't propagate).
- Test collection filtering.
- Mock `HookHandler` implementations.

### 11.2 Integration Tests

- Test that `RecordService` fires hooks in the correct order during CRUD.
- Test that hook mutations in "before" hooks are reflected in the persisted record.
- Test that aborting a "before" hook prevents the DB write.
- Test custom route registration and execution.

### 11.3 JS Hook Tests (`zerobase-hooks`)

- Test JS → Rust value conversion roundtrip.
- Test that JS hooks can mutate event data.
- Test that JS thrown errors map to `ZerobaseError`.
- Test `$app` API surface (db, sendMail, logger).
- Test file loading from `pb_hooks/`.
- Test hot reload (dev mode).
- Test resource limits (execution timeout, memory cap).

---

## 12. Migration Path

1. **Phase 1**: Implement `HookPoint`, `HookEvent`, `HookHandler`, `HookRegistry` in `zerobase-core`. Add `hooks: Arc<HookRegistry>` to all services. Wire hook firing into `RecordService` CRUD methods. Unit tests.
2. **Phase 2**: Build the `Zerobase` app struct with convenience methods (`on_before_record_create`, etc.). Integration tests with Rust hooks.
3. **Phase 3**: Create `zerobase-hooks` crate. Embed Rquickjs. Implement `$app` bindings, `JsHookAdapter`, file loading. Feature-flag it.
4. **Phase 4**: Add custom route support (both Rust and JS).
5. **Phase 5**: Add remaining hook points (realtime, file, mail).
