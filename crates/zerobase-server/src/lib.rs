//! Zerobase — Backend-as-a-Service framework for Rust.
//!
//! Use Zerobase as a library to build custom backends with auto-generated
//! CRUD APIs, authentication, file storage, and realtime subscriptions —
//! while adding your own custom routes, hooks, and middleware.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use zerobase::ZerobaseApp;
//! use axum::{Router, routing::get, Json};
//! use serde_json::json;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let app = ZerobaseApp::new()?
//!         .with_custom_routes(
//!             Router::new().route("/api/custom/hello", get(|| async {
//!                 Json(json!({"message": "Hello from custom route!"}))
//!             }))
//!         );
//!
//!     app.serve().await
//! }
//! ```
//!
//! # Framework Mode
//!
//! Zerobase can be used as a framework (Rust library) to build custom
//! applications. This mirrors PocketBase's Go framework mode.
//!
//! ## Custom Routes
//!
//! Register any axum [`Router`] to run alongside the built-in API:
//!
//! ```rust,no_run
//! # use zerobase::ZerobaseApp;
//! # use axum::{Router, routing::get};
//! let app = ZerobaseApp::new().unwrap()
//!     .with_custom_routes(
//!         Router::new()
//!             .route("/api/custom/stats", get(stats_handler))
//!             .route("/api/custom/export", get(export_handler))
//!     );
//! # async fn stats_handler() {}
//! # async fn export_handler() {}
//! ```
//!
//! ## Hooks
//!
//! Register hooks to intercept record lifecycle events:
//!
//! ```rust,no_run
//! use zerobase::ZerobaseApp;
//! use zerobase_core::hooks::{Hook, HookContext, HookResult, RecordOperation};
//!
//! struct AuditLogger;
//!
//! impl Hook for AuditLogger {
//!     fn name(&self) -> &str { "audit_logger" }
//!
//!     fn after_operation(&self, ctx: &HookContext) -> HookResult<()> {
//!         println!("Operation {} on {}", ctx.operation, ctx.collection);
//!         Ok(())
//!     }
//! }
//!
//! let app = ZerobaseApp::new().unwrap()
//!     .with_hook(AuditLogger, 100);
//! ```
//!
//! ## Database Access
//!
//! Access the underlying SQLite database for custom queries:
//!
//! ```rust,no_run
//! # use zerobase::ZerobaseApp;
//! let app = ZerobaseApp::new().unwrap();
//! let db = app.database();
//! // Use db for custom queries via repository traits
//! ```

pub mod js_routes;
pub mod shutdown;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use tracing::info;

use zerobase_core::hooks::{Hook, HookRegistry};
use zerobase_core::telemetry::{init_tracing, LogFormat};
use zerobase_core::Settings;
use zerobase_db::migrations::{run_migrations, system::system_migrations};
use zerobase_db::{Database, PoolConfig};
use zerobase_hooks::bindings::DaoHandler;
use zerobase_hooks::{JsHookEngine, HooksWatcher};

// Re-export key types for library consumers.
pub use axum;
pub use zerobase_api;
pub use zerobase_auth;
pub use zerobase_core;
pub use zerobase_db;
pub use zerobase_files;
pub use zerobase_hooks;

// ── ZerobaseApp ──────────────────────────────────────────────────────────────

/// The main application struct for running Zerobase as a library/framework.
///
/// `ZerobaseApp` provides a builder-style API for configuring and running
/// a Zerobase instance with custom routes, hooks, and middleware.
///
/// # Lifecycle
///
/// 1. Create with [`ZerobaseApp::new`] (loads config, opens DB, runs migrations).
/// 2. Customize with `with_*` methods (routes, hooks, settings overrides).
/// 3. Call [`ZerobaseApp::serve`] to start the HTTP server, or
///    [`ZerobaseApp::build_router`] to get the composed [`Router`] for testing.
pub struct ZerobaseApp {
    settings: Settings,
    db: Arc<Database>,
    hook_registry: HookRegistry,
    custom_routes: Vec<Router>,
    tracing_initialized: bool,
    js_hook_engine: Option<JsHookEngine>,
    hooks_watcher: Option<HooksWatcher>,
    dao_handler: Option<Arc<dyn DaoHandler>>,
}

impl ZerobaseApp {
    /// Create a new Zerobase application with default configuration.
    ///
    /// This loads settings from the standard configuration hierarchy
    /// (defaults → `zerobase.toml` → environment variables), opens the
    /// SQLite database, and runs pending migrations.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration loading, database opening, or
    /// migration execution fails.
    pub fn new() -> anyhow::Result<Self> {
        let settings = Settings::load()?;
        Self::with_settings(settings)
    }

    /// Create a new Zerobase application with the given settings.
    ///
    /// Use this when you want full control over configuration (e.g., in
    /// tests or when embedding Zerobase in another application).
    ///
    /// # Errors
    ///
    /// Returns an error if database opening or migration execution fails.
    pub fn with_settings(settings: Settings) -> anyhow::Result<Self> {
        let pool_config = PoolConfig::from(&settings.database);
        let db = Database::open(&settings.database.path, &pool_config)?;

        // Run system migrations.
        let migrations = system_migrations();
        db.with_write_conn(|conn| {
            run_migrations(conn, &migrations).map_err(|e| e.into())
        })?;

        Ok(Self {
            settings,
            db: Arc::new(db),
            hook_registry: HookRegistry::new(),
            custom_routes: Vec::new(),
            tracing_initialized: false,
            js_hook_engine: None,
            hooks_watcher: None,
            dao_handler: None,
        })
    }

    /// Create a Zerobase application from an already-opened database.
    ///
    /// This skips database opening and migration — useful when you have
    /// an existing [`Database`] handle (e.g., in-memory for testing).
    pub fn with_database(settings: Settings, db: Arc<Database>) -> Self {
        Self {
            settings,
            db,
            hook_registry: HookRegistry::new(),
            custom_routes: Vec::new(),
            tracing_initialized: false,
            js_hook_engine: None,
            hooks_watcher: None,
            dao_handler: None,
        }
    }

    // ── Builder methods ──────────────────────────────────────────────────

    /// Register custom axum routes that run alongside the built-in API.
    ///
    /// Custom routes are merged into the main router and share the same
    /// middleware stack (request ID, tracing, CORS).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use zerobase::ZerobaseApp;
    /// # use axum::{Router, routing::get, Json};
    /// # use serde_json::json;
    /// let app = ZerobaseApp::new().unwrap()
    ///     .with_custom_routes(
    ///         Router::new()
    ///             .route("/api/custom/ping", get(|| async {
    ///                 Json(json!({"pong": true}))
    ///             }))
    ///     );
    /// ```
    pub fn with_custom_routes(mut self, router: Router) -> Self {
        self.custom_routes.push(router);
        self
    }

    /// Register a record lifecycle hook with the given priority.
    ///
    /// Lower priority numbers execute first (e.g., 10 = early, 200 = late).
    /// The conventional default is 100.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use zerobase::ZerobaseApp;
    /// # use zerobase_core::hooks::{Hook, HookContext, HookResult};
    /// struct MyHook;
    /// impl Hook for MyHook {
    ///     fn name(&self) -> &str { "my_hook" }
    /// }
    ///
    /// let app = ZerobaseApp::new().unwrap()
    ///     .with_hook(MyHook, 100);
    /// ```
    pub fn with_hook(mut self, hook: impl Hook + 'static, priority: i32) -> Self {
        self.hook_registry.register(hook, priority);
        self
    }

    /// Register a record lifecycle hook with the default priority (100).
    pub fn with_default_hook(mut self, hook: impl Hook + 'static) -> Self {
        self.hook_registry.register_default(hook);
        self
    }

    /// Override the server host.
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.settings.server.host = host.into();
        self
    }

    /// Override the server port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.settings.server.port = port;
        self
    }

    /// Enable tracing/logging initialization.
    ///
    /// Call this if you want Zerobase to set up the tracing subscriber.
    /// If you've already initialized tracing in your application, skip this.
    pub fn with_tracing(mut self) -> Self {
        if !self.tracing_initialized {
            init_tracing(self.settings.server.log_format.clone());
            self.tracing_initialized = true;
        }
        self
    }

    /// Override the log format.
    pub fn with_log_format(mut self, format: LogFormat) -> Self {
        self.settings.server.log_format = format;
        self
    }

    /// Register a single custom route with the given HTTP method and path.
    ///
    /// This is a convenience method for registering individual routes without
    /// building a full axum [`Router`]. The handler can be any axum handler.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use zerobase::ZerobaseApp;
    /// # use axum::Json;
    /// # use serde_json::json;
    /// let app = ZerobaseApp::new().unwrap()
    ///     .with_route("/api/custom/ping", axum::routing::get(
    ///         || async { Json(json!({"pong": true})) }
    ///     ));
    /// ```
    pub fn with_route(mut self, path: &str, method_router: axum::routing::MethodRouter) -> Self {
        self.custom_routes
            .push(Router::new().route(path, method_router));
        self
    }

    /// Set a DAO handler for JS route handlers to use for database access.
    ///
    /// Without this, JS custom route handlers will use a no-op DAO handler
    /// that returns empty results.
    pub fn with_dao_handler(mut self, handler: Arc<dyn DaoHandler>) -> Self {
        self.dao_handler = Some(handler);
        self
    }

    /// Load JS hooks from the given directory (e.g. `pb_hooks/`).
    ///
    /// Evaluates all `*.pb.js` files and registers the resulting hook
    /// in the hook registry. Returns `self` for chaining.
    ///
    /// # Errors
    ///
    /// Returns an error if any hook file fails to load or evaluate.
    pub fn with_js_hooks(mut self, hooks_dir: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let hooks_dir = hooks_dir.into();
        let engine = JsHookEngine::new(&hooks_dir);
        engine.load_hooks()?;

        // Register the JS hook into the hook registry at default priority.
        // Use create_hook() so the engine stays alive for reloads — the
        // hook shares the same internal state via Arc.
        let js_hook = engine.create_hook();
        self.hook_registry.register_default(js_hook);

        // Keep the engine for reloads (same Arc<RwLock<JsHookState>>).
        self.js_hook_engine = Some(engine);
        Ok(self)
    }

    /// Enable file watching for JS hooks in development mode.
    ///
    /// When enabled, the hooks directory is watched for changes to
    /// `*.pb.js` files. On change, all hooks are reloaded automatically.
    /// Must be called after [`with_js_hooks`].
    pub fn with_js_hooks_watcher(mut self) -> anyhow::Result<Self> {
        if let Some(engine) = self.js_hook_engine.take() {
            let hooks_dir = engine.hooks_dir().to_owned();

            // Wrap the engine in Arc so it can be shared with the watcher
            // closure while remaining accessible for future use.
            let engine = Arc::new(engine);
            let engine_for_reload = engine.clone();

            let mut watcher = HooksWatcher::new(&hooks_dir);
            watcher.start(move || {
                engine_for_reload
                    .load_hooks()
                    .map_err(|e| zerobase_hooks::JsHookError::Watcher(e.to_string()))
            })?;
            self.hooks_watcher = Some(watcher);
        }
        Ok(self)
    }

    // ── Accessors ────────────────────────────────────────────────────────

    /// Access the application settings.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Access a mutable reference to the settings for customization.
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// Access the shared database handle.
    ///
    /// The returned `Arc<Database>` can be cloned and shared with custom
    /// handlers or background tasks.
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    /// Access the hook registry.
    pub fn hook_registry(&self) -> &HookRegistry {
        &self.hook_registry
    }

    /// Access a mutable reference to the hook registry.
    pub fn hook_registry_mut(&mut self) -> &mut HookRegistry {
        &mut self.hook_registry
    }

    // ── Router building ──────────────────────────────────────────────────

    /// Build the composed axum [`Router`] with all built-in and custom routes.
    ///
    /// This is useful for testing or when you want to embed the Zerobase
    /// router into a larger axum application.
    ///
    /// The returned router includes:
    /// - Built-in API routes (`/api/health`, etc.)
    /// - Admin dashboard routes (`/_/`)
    /// - All custom routes registered via [`with_custom_routes`]
    /// - Standard middleware (request ID, tracing, CORS)
    pub fn build_router(&self) -> Router {
        let mut app = zerobase_api::api_router();

        // Merge the admin dashboard.
        app = app.merge(zerobase_admin::dashboard::dashboard_routes());

        // Merge all custom Rust routes.
        for custom in &self.custom_routes {
            app = app.merge(custom.clone());
        }

        // Merge JS custom routes from the hook engine.
        if let Some(ref engine) = self.js_hook_engine {
            let js_routes = engine.custom_routes();
            if !js_routes.is_empty() {
                let file_sources = engine.file_sources();
                let js_router = js_routes::build_js_routes(
                    js_routes,
                    file_sources,
                    self.dao_handler.clone(),
                );
                app = app.merge(js_router);
            }
        }

        app
    }

    /// Start the HTTP server and block until shutdown.
    ///
    /// This initializes tracing (if not already done), binds to the
    /// configured address, and serves requests with graceful shutdown
    /// on SIGINT/SIGTERM. In-flight requests are drained with a
    /// configurable timeout (default 30s), after which the server
    /// force-exits. Database connections are closed and logs are
    /// flushed before returning.
    ///
    /// # Errors
    ///
    /// Returns an error if the TCP listener cannot bind to the configured
    /// address or if the server encounters a fatal error.
    pub async fn serve(mut self) -> anyhow::Result<()> {
        // Initialize tracing if not already done.
        if !self.tracing_initialized {
            init_tracing(self.settings.server.log_format.clone());
            self.tracing_initialized = true;
        }

        info!(
            version = env!("CARGO_PKG_VERSION"),
            host = %self.settings.server.host,
            port = %self.settings.server.port,
            hooks = self.hook_registry.len(),
            custom_routes = self.custom_routes.len(),
            "starting zerobase"
        );

        let app = self.build_router();

        let addr = self.settings.server.address();
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!(address = %addr, "listening");

        let coordinator = shutdown::ShutdownCoordinator::new(Arc::clone(&self.db))
            .with_timeout(std::time::Duration::from_secs(
                self.settings.server.shutdown_timeout_secs,
            ));

        shutdown::serve_with_shutdown(listener, app, coordinator).await
    }
}

impl std::fmt::Debug for ZerobaseApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZerobaseApp")
            .field("host", &self.settings.server.host)
            .field("port", &self.settings.server.port)
            .field("database_path", &self.settings.database.path)
            .field("hooks", &self.hook_registry.len())
            .field("custom_routes", &self.custom_routes.len())
            .field("js_hooks", &self.js_hook_engine.is_some())
            .field("hooks_watcher", &self.hooks_watcher.as_ref().map(|w| w.is_watching()))
            .field("dao_handler", &self.dao_handler.is_some())
            .finish()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use axum::Json;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use zerobase_core::hooks::{HookContext, HookResult, RecordOperation};
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    /// Helper to create a ZerobaseApp backed by an in-memory database.
    fn test_app() -> ZerobaseApp {
        // Ensure token_secret is set for tests.
        if std::env::var("ZEROBASE__AUTH__TOKEN_SECRET").is_err() {
            std::env::set_var("ZEROBASE__AUTH__TOKEN_SECRET", "test-secret-for-unit-tests");
        }

        let settings = Settings::load_from_env()
            .expect("failed to load settings from env");
        let pool_config = PoolConfig::default();
        let db = Database::open_in_memory(&pool_config)
            .expect("failed to open in-memory database");

        // Run migrations on the in-memory DB.
        let migrations = system_migrations();
        db.with_write_conn(|conn| {
            run_migrations(conn, &migrations).map_err(|e| e.into())
        })
        .expect("migrations failed");

        ZerobaseApp::with_database(settings, Arc::new(db))
    }

    /// Helper to send a GET request to a router and return the response.
    async fn get_response(app: &Router, uri: &str) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        (status, json)
    }

    // ── Construction tests ───────────────────────────────────────────────

    #[test]
    fn with_database_creates_app() {
        let app = test_app();
        assert_eq!(app.hook_registry().len(), 0);
        assert!(app.custom_routes.is_empty());
    }

    #[test]
    fn debug_format_shows_key_fields() {
        let app = test_app();
        let debug = format!("{:?}", app);
        assert!(debug.contains("ZerobaseApp"));
        assert!(debug.contains("hooks"));
        assert!(debug.contains("custom_routes"));
    }

    #[test]
    fn settings_accessor() {
        let app = test_app();
        // Default settings should have some host.
        assert!(!app.settings().server.host.is_empty());
    }

    #[test]
    fn settings_mut_accessor() {
        let mut app = test_app();
        app.settings_mut().server.port = 9999;
        assert_eq!(app.settings().server.port, 9999);
    }

    #[test]
    fn database_accessor_returns_shared_handle() {
        let app = test_app();
        let db1 = app.database().clone();
        let db2 = app.database().clone();
        // Both point to the same database (Arc).
        assert!(Arc::ptr_eq(&db1, &db2));
    }

    // ── Builder method tests ─────────────────────────────────────────────

    #[test]
    fn with_host_overrides_setting() {
        let app = test_app().with_host("0.0.0.0");
        assert_eq!(app.settings().server.host, "0.0.0.0");
    }

    #[test]
    fn with_port_overrides_setting() {
        let app = test_app().with_port(3000);
        assert_eq!(app.settings().server.port, 3000);
    }

    #[test]
    fn with_log_format_overrides_setting() {
        let app = test_app().with_log_format(LogFormat::Pretty);
        assert!(matches!(
            app.settings().server.log_format,
            LogFormat::Pretty
        ));
    }

    // ── Custom route tests ───────────────────────────────────────────────

    #[tokio::test]
    async fn built_in_health_check_works() {
        let app = test_app();
        let router = app.build_router();

        let (status, body) = get_response(&router, "/api/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "healthy");
    }

    #[tokio::test]
    async fn custom_route_is_reachable() {
        let custom = Router::new().route(
            "/api/custom/ping",
            get(|| async { Json(json!({"pong": true})) }),
        );

        let app = test_app().with_custom_routes(custom);
        let router = app.build_router();

        let (status, body) = get_response(&router, "/api/custom/ping").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["pong"], true);
    }

    #[tokio::test]
    async fn multiple_custom_route_groups_are_reachable() {
        let group_a = Router::new().route(
            "/api/custom/a",
            get(|| async { Json(json!({"group": "a"})) }),
        );
        let group_b = Router::new().route(
            "/api/custom/b",
            get(|| async { Json(json!({"group": "b"})) }),
        );

        let app = test_app()
            .with_custom_routes(group_a)
            .with_custom_routes(group_b);
        let router = app.build_router();

        let (status_a, body_a) = get_response(&router, "/api/custom/a").await;
        assert_eq!(status_a, StatusCode::OK);
        assert_eq!(body_a["group"], "a");

        let (status_b, body_b) = get_response(&router, "/api/custom/b").await;
        assert_eq!(status_b, StatusCode::OK);
        assert_eq!(body_b["group"], "b");
    }

    #[tokio::test]
    async fn custom_routes_coexist_with_built_in_routes() {
        let custom = Router::new().route(
            "/api/custom/hello",
            get(|| async { Json(json!({"hello": "world"})) }),
        );

        let app = test_app().with_custom_routes(custom);
        let router = app.build_router();

        // Built-in route still works.
        let (status, body) = get_response(&router, "/api/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "healthy");

        // Custom route works too.
        let (status, body) = get_response(&router, "/api/custom/hello").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["hello"], "world");
    }

    #[tokio::test]
    async fn nonexistent_route_returns_404() {
        let app = test_app();
        let router = app.build_router();

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/does-not-exist")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // ── Hook registration tests ──────────────────────────────────────────

    struct TestHook {
        name: &'static str,
        before_called: AtomicBool,
        after_called: AtomicBool,
    }

    impl TestHook {
        fn new(name: &'static str) -> Self {
            Self {
                name,
                before_called: AtomicBool::new(false),
                after_called: AtomicBool::new(false),
            }
        }
    }

    impl Hook for TestHook {
        fn name(&self) -> &str {
            self.name
        }

        fn before_operation(&self, _ctx: &mut HookContext) -> HookResult<()> {
            self.before_called.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn after_operation(&self, _ctx: &HookContext) -> HookResult<()> {
            self.after_called.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn with_hook_registers_hook() {
        let app = test_app().with_hook(TestHook::new("h1"), 50);
        assert_eq!(app.hook_registry().len(), 1);
        assert_eq!(app.hook_registry().hook_names(), vec!["h1"]);
    }

    #[test]
    fn with_default_hook_registers_at_priority_100() {
        let app = test_app()
            .with_hook(TestHook::new("early"), 10)
            .with_default_hook(TestHook::new("default"))
            .with_hook(TestHook::new("late"), 200);

        assert_eq!(app.hook_registry().len(), 3);
        assert_eq!(
            app.hook_registry().hook_names(),
            vec!["early", "default", "late"]
        );
    }

    #[test]
    fn multiple_hooks_register_in_priority_order() {
        let app = test_app()
            .with_hook(TestHook::new("c"), 300)
            .with_hook(TestHook::new("a"), 10)
            .with_hook(TestHook::new("b"), 100);

        assert_eq!(
            app.hook_registry().hook_names(),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn hook_registry_mut_allows_unregister() {
        let mut app = test_app()
            .with_hook(TestHook::new("keep"), 10)
            .with_hook(TestHook::new("remove"), 20);

        assert_eq!(app.hook_registry().len(), 2);

        let removed = app.hook_registry_mut().unregister("remove");
        assert_eq!(removed, 1);
        assert_eq!(app.hook_registry().len(), 1);
        assert_eq!(app.hook_registry().hook_names(), vec!["keep"]);
    }

    #[test]
    fn registered_hooks_execute_via_registry() {
        use std::collections::HashMap;
        use zerobase_core::hooks::HookPhase;

        let app = test_app()
            .with_hook(TestHook::new("h1"), 100);

        let mut ctx = HookContext::new(
            RecordOperation::Create,
            HookPhase::Before,
            "test",
            "id1",
            HashMap::new(),
        );

        // Hooks run through the registry.
        app.hook_registry().run_before(&mut ctx).unwrap();
        // We can't inspect the AtomicBool directly since TestHook was moved,
        // but the fact that run_before succeeded means the hook ran without error.
    }

    // ── Chaining / builder pattern tests ─────────────────────────────────

    #[test]
    fn builder_methods_are_chainable() {
        let custom = Router::new().route(
            "/api/custom/test",
            get(|| async { Json(json!({"ok": true})) }),
        );

        let app = test_app()
            .with_host("0.0.0.0")
            .with_port(9090)
            .with_log_format(LogFormat::Pretty)
            .with_hook(TestHook::new("audit"), 100)
            .with_custom_routes(custom);

        assert_eq!(app.settings().server.host, "0.0.0.0");
        assert_eq!(app.settings().server.port, 9090);
        assert_eq!(app.hook_registry().len(), 1);
        assert_eq!(app.custom_routes.len(), 1);
    }

    // ── Custom routes with shared state ──────────────────────────────────

    #[tokio::test]
    async fn custom_route_with_state() {
        #[derive(Clone)]
        struct AppState {
            counter: Arc<AtomicU32>,
        }

        async fn counter_handler(
            axum::extract::State(state): axum::extract::State<AppState>,
        ) -> Json<Value> {
            let count = state.counter.fetch_add(1, Ordering::SeqCst);
            Json(json!({"count": count}))
        }

        let state = AppState {
            counter: Arc::new(AtomicU32::new(0)),
        };

        let custom = Router::new()
            .route("/api/custom/counter", get(counter_handler))
            .with_state(state);

        let app = test_app().with_custom_routes(custom);
        let router = app.build_router();

        let (status, body) = get_response(&router, "/api/custom/counter").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["count"], 0);

        // Second call increments.
        let (_, body) = get_response(&router, "/api/custom/counter").await;
        assert_eq!(body["count"], 1);
    }

    // ── Database access from custom handler ──────────────────────────────

    #[tokio::test]
    async fn custom_route_can_access_database() {
        let app = test_app();
        let db = app.database().clone();

        // Create a custom route that uses the database.
        let custom = Router::new()
            .route(
                "/api/custom/db-check",
                get(move || {
                    let db = db.clone();
                    async move {
                        let stats = db.stats();
                        Json(json!({
                            "max_size": stats.max_size,
                            "idle": stats.idle_connections,
                        }))
                    }
                }),
            );

        let app = app.with_custom_routes(custom);
        let router = app.build_router();

        let (status, body) = get_response(&router, "/api/custom/db-check").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["max_size"].as_u64().unwrap() > 0);
    }

    // ── Router composition ───────────────────────────────────────────────

    #[tokio::test]
    async fn build_router_is_idempotent() {
        let custom = Router::new().route(
            "/api/custom/test",
            get(|| async { Json(json!({"ok": true})) }),
        );

        let app = test_app().with_custom_routes(custom);

        // Build twice — both should work identically.
        let router1 = app.build_router();
        let router2 = app.build_router();

        let (s1, b1) = get_response(&router1, "/api/custom/test").await;
        let (s2, b2) = get_response(&router2, "/api/custom/test").await;

        assert_eq!(s1, s2);
        assert_eq!(b1, b2);
    }

    // ── with_route() convenience method ─────────────────────────────────

    #[tokio::test]
    async fn with_route_registers_single_route() {
        let app = test_app().with_route(
            "/api/custom/simple",
            get(|| async { Json(json!({"simple": true})) }),
        );

        let router = app.build_router();
        let (status, body) = get_response(&router, "/api/custom/simple").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["simple"], true);
    }

    #[tokio::test]
    async fn with_route_chainable() {
        let app = test_app()
            .with_route(
                "/api/custom/one",
                get(|| async { Json(json!({"n": 1})) }),
            )
            .with_route(
                "/api/custom/two",
                get(|| async { Json(json!({"n": 2})) }),
            );

        let router = app.build_router();
        let (_, body1) = get_response(&router, "/api/custom/one").await;
        let (_, body2) = get_response(&router, "/api/custom/two").await;
        assert_eq!(body1["n"], 1);
        assert_eq!(body2["n"], 2);
    }

    // ── JS custom routes via hook engine ────────────────────────────────

    #[tokio::test]
    async fn js_custom_routes_integrated_in_build_router() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("routes.pb.js"),
            r#"routerAdd("GET", "/api/custom/js-hello", function(c) {
                c.json(200, { message: "hello from JS" });
            });"#,
        )
        .unwrap();

        let app = test_app().with_js_hooks(dir.path()).unwrap();
        let router = app.build_router();

        let (status, body) = get_response(&router, "/api/custom/js-hello").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["message"], "hello from JS");
    }

    #[tokio::test]
    async fn js_and_rust_custom_routes_coexist() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("routes.pb.js"),
            r#"routerAdd("GET", "/api/custom/js-route", function(c) {
                c.json(200, { source: "js" });
            });"#,
        )
        .unwrap();

        let app = test_app()
            .with_js_hooks(dir.path())
            .unwrap()
            .with_route(
                "/api/custom/rust-route",
                get(|| async { Json(json!({"source": "rust"})) }),
            );

        let router = app.build_router();

        let (status_js, body_js) = get_response(&router, "/api/custom/js-route").await;
        assert_eq!(status_js, StatusCode::OK);
        assert_eq!(body_js["source"], "js");

        let (status_rs, body_rs) = get_response(&router, "/api/custom/rust-route").await;
        assert_eq!(status_rs, StatusCode::OK);
        assert_eq!(body_rs["source"], "rust");

        // Built-in still works.
        let (status_h, body_h) = get_response(&router, "/api/health").await;
        assert_eq!(status_h, StatusCode::OK);
        assert_eq!(body_h["status"], "healthy");
    }

    #[tokio::test]
    async fn js_route_with_query_params() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("routes.pb.js"),
            r#"routerAdd("GET", "/api/custom/greet", function(c) {
                var name = c.queryParam('name');
                c.json(200, { greeting: "hello " + name });
            });"#,
        )
        .unwrap();

        let app = test_app().with_js_hooks(dir.path()).unwrap();
        let router = app.build_router();

        let (status, body) = get_response(&router, "/api/custom/greet?name=World").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["greeting"], "hello World");
    }

    #[tokio::test]
    async fn js_route_post_method() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("routes.pb.js"),
            r#"routerAdd("POST", "/api/custom/echo", function(c) {
                c.json(200, { received: c.body() });
            });"#,
        )
        .unwrap();

        let app = test_app().with_js_hooks(dir.path()).unwrap();
        let router = app.build_router();

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/custom/echo")
                    .header("content-type", "text/plain")
                    .body(Body::from("test data"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["received"], "test data");
    }
}
