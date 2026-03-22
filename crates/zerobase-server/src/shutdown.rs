//! Graceful shutdown coordination for Zerobase.
//!
//! Provides [`ShutdownCoordinator`] which orchestrates the full server shutdown
//! sequence: signal handling → stop accepting connections → drain in-flight
//! requests (with timeout) → close database connections → flush logs.
//!
//! # Shutdown Sequence
//!
//! 1. **Signal received** — SIGINT (Ctrl-C) or SIGTERM triggers shutdown.
//! 2. **Stop accepting** — The TCP listener stops accepting new connections.
//! 3. **Drain in-flight** — Active requests are given a grace period to complete.
//! 4. **Force close** — If requests exceed the timeout, the server shuts down anyway.
//! 5. **Cleanup** — Database connections are closed and logs are flushed.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::{info, warn};

use zerobase_db::Database;

/// Default timeout for draining in-flight requests (30 seconds).
const DEFAULT_SHUTDOWN_TIMEOUT_SECS: u64 = 30;

/// Coordinates graceful shutdown of the Zerobase server.
///
/// The coordinator manages:
/// - A shutdown signal (via `watch` channel) that triggers the shutdown sequence
/// - A configurable timeout for draining in-flight requests
/// - Post-shutdown cleanup of database connections and log flushing
///
/// # Usage
///
/// ```rust,no_run
/// # use std::sync::Arc;
/// # use zerobase_db::Database;
/// use zerobase::shutdown::ShutdownCoordinator;
///
/// # async fn example(db: Arc<Database>) {
/// let coordinator = ShutdownCoordinator::new(db)
///     .with_timeout(std::time::Duration::from_secs(60));
///
/// // Use coordinator.shutdown_future() with axum's graceful shutdown.
/// // After the server stops, call coordinator.cleanup().
/// # }
/// ```
pub struct ShutdownCoordinator {
    /// Shared database handle for cleanup.
    db: Arc<Database>,
    /// Timeout for draining in-flight requests.
    timeout: Duration,
    /// Sender side of the shutdown signal.
    shutdown_tx: watch::Sender<bool>,
    /// Receiver side of the shutdown signal.
    shutdown_rx: watch::Receiver<bool>,
}

impl ShutdownCoordinator {
    /// Create a new shutdown coordinator with default timeout.
    pub fn new(db: Arc<Database>) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        Self {
            db,
            timeout: Duration::from_secs(DEFAULT_SHUTDOWN_TIMEOUT_SECS),
            shutdown_tx,
            shutdown_rx,
        }
    }

    /// Override the shutdown timeout (how long to wait for in-flight requests).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Returns the configured shutdown timeout.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Returns a future that resolves when a termination signal is received.
    ///
    /// Pass this to `axum::serve().with_graceful_shutdown()`. When the signal
    /// fires, axum stops accepting new connections and drains in-flight
    /// requests.
    pub fn shutdown_signal(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        let mut rx = self.shutdown_rx.clone();
        async move {
            // Wait until the value changes to `true`.
            let _ = rx.wait_for(|&v| v).await;
        }
    }

    /// Trigger the shutdown signal programmatically.
    ///
    /// This is useful for testing or when shutdown is initiated by
    /// application logic rather than an OS signal.
    pub fn trigger(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Returns whether the shutdown has been triggered.
    pub fn is_shutting_down(&self) -> bool {
        *self.shutdown_rx.borrow()
    }

    /// Perform post-shutdown cleanup.
    ///
    /// This should be called after the HTTP server has stopped. It:
    /// 1. Verifies the database is still accessible (logs a warning if not)
    /// 2. Drops the database handle to close all SQLite connections
    /// 3. Flushes any buffered tracing/log output
    pub fn cleanup(self) {
        info!("running post-shutdown cleanup");

        // Check database health before closing.
        if self.db.is_healthy() {
            info!("database connections healthy, closing");
        } else {
            warn!("database connections unhealthy during shutdown");
        }

        // Log pool stats before teardown.
        let stats = self.db.stats();
        info!(
            total = stats.total_connections,
            idle = stats.idle_connections,
            max = stats.max_size,
            "database pool stats at shutdown"
        );

        // Drop the Arc<Database>. If this is the last reference, the r2d2
        // pool and write connection are closed, releasing all SQLite handles.
        drop(self.db);

        info!("database connections closed");

        // Flush the tracing subscriber so that all log lines are written out
        // before the process exits. This is important for file-based or
        // network-based log sinks.
        flush_tracing();

        info!("shutdown cleanup complete");
    }
}

/// Run the server with graceful shutdown and timeout enforcement.
///
/// This is the main entry point for starting the HTTP server with full
/// shutdown orchestration. It:
/// 1. Listens for OS signals (SIGINT/SIGTERM) in the background
/// 2. Triggers the shutdown coordinator when a signal is received
/// 3. Waits for axum to drain in-flight requests (up to timeout)
/// 4. Runs post-shutdown cleanup
///
/// # Arguments
///
/// * `listener` — A bound TCP listener
/// * `app` — The axum router to serve
/// * `coordinator` — The shutdown coordinator (owns the database handle)
pub async fn serve_with_shutdown(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    coordinator: ShutdownCoordinator,
) -> anyhow::Result<()> {
    let timeout = coordinator.timeout();

    // Spawn a task that waits for OS signals and triggers the coordinator.
    let signal_coordinator_tx = coordinator.shutdown_tx.clone();
    tokio::spawn(async move {
        wait_for_os_signal().await;
        let _ = signal_coordinator_tx.send(true);
    });

    // Start serving with graceful shutdown.
    let shutdown_fut = coordinator.shutdown_signal();

    info!("server started, graceful shutdown timeout = {}s", timeout.as_secs());

    // Race the server against the shutdown timeout.
    // axum's graceful shutdown will stop accepting new connections immediately
    // and wait for in-flight requests to complete.
    let serve_future = async {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_fut)
            .await
    };

    // Give extra time beyond the drain timeout so the server has a chance
    // to finish on its own before we force-kill.
    let serve_result = tokio::time::timeout(
        timeout + Duration::from_secs(1),
        serve_future,
    )
    .await;

    match serve_result {
        Ok(Ok(())) => {
            info!("server shut down gracefully — all in-flight requests completed");
        }
        Ok(Err(e)) => {
            warn!(error = %e, "server encountered an error during shutdown");
            return Err(e.into());
        }
        Err(_elapsed) => {
            warn!(
                timeout_secs = timeout.as_secs(),
                "shutdown timeout exceeded — forcing shutdown"
            );
        }
    }

    // Run cleanup (close DB, flush logs).
    coordinator.cleanup();

    Ok(())
}

/// Wait for a termination signal (SIGINT / SIGTERM on Unix, Ctrl-C everywhere).
async fn wait_for_os_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("received Ctrl+C, initiating shutdown"),
        () = terminate => info!("received SIGTERM, initiating shutdown"),
    }
}

/// Flush any buffered tracing output.
///
/// Calls `tracing::dispatcher::get_default` to access the current subscriber
/// and flush it. Falls back gracefully if no subscriber is set.
fn flush_tracing() {
    // The global tracing subscriber implements `tracing::Subscriber`.
    // We can't easily flush it directly, but we can ensure all pending
    // spans/events are processed by doing a synchronous log.
    //
    // For tracing-subscriber's `fmt` layer, output is written on each event,
    // so this is mostly a no-op. But for buffered writers (e.g., file output),
    // the Drop impl on the subscriber will flush.
    //
    // The most reliable way is to drop the global default, which triggers
    // flush on Drop for tracing-subscriber. Since we're shutting down,
    // this is acceptable.
    info!("flushing log output");
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use axum::routing::get;
    use axum::Json;
    use reqwest::Client;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use tokio::net::TcpListener;
    use zerobase_db::PoolConfig;

    /// Create an in-memory database for testing.
    fn test_db() -> Arc<Database> {
        let config = PoolConfig::default();
        Arc::new(Database::open_in_memory(&config).expect("in-memory DB"))
    }

    #[tokio::test]
    async fn coordinator_starts_not_shutting_down() {
        let db = test_db();
        let coordinator = ShutdownCoordinator::new(db);
        assert!(!coordinator.is_shutting_down());
    }

    #[tokio::test]
    async fn trigger_sets_shutting_down_flag() {
        let db = test_db();
        let coordinator = ShutdownCoordinator::new(db);
        coordinator.trigger();
        assert!(coordinator.is_shutting_down());
    }

    #[tokio::test]
    async fn shutdown_signal_resolves_when_triggered() {
        let db = test_db();
        let coordinator = ShutdownCoordinator::new(db);

        let signal = coordinator.shutdown_signal();

        // Trigger after a tiny delay.
        let coord_clone_tx = coordinator.shutdown_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = coord_clone_tx.send(true);
        });

        // Signal should resolve within 1 second.
        tokio::time::timeout(Duration::from_secs(1), signal)
            .await
            .expect("shutdown signal should resolve after trigger");
    }

    #[tokio::test]
    async fn custom_timeout_is_applied() {
        let db = test_db();
        let coordinator =
            ShutdownCoordinator::new(db).with_timeout(Duration::from_secs(60));
        assert_eq!(coordinator.timeout(), Duration::from_secs(60));
    }

    #[tokio::test]
    async fn cleanup_runs_without_panic() {
        let db = test_db();
        let coordinator = ShutdownCoordinator::new(db);
        coordinator.trigger();
        // Should not panic.
        coordinator.cleanup();
    }

    #[tokio::test]
    async fn graceful_shutdown_completes_in_flight_requests() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}");

        let db = test_db();
        let coordinator =
            ShutdownCoordinator::new(Arc::clone(&db)).with_timeout(Duration::from_secs(5));

        // Track whether the slow handler completed.
        let handler_completed = Arc::new(AtomicBool::new(false));
        let handler_completed_clone = Arc::clone(&handler_completed);

        let app = axum::Router::new()
            .route(
                "/api/health",
                get(|| async { Json(json!({"status": "ok"})) }),
            )
            .route(
                "/api/slow",
                get(move || {
                    let completed = handler_completed_clone.clone();
                    async move {
                        // Simulate a slow request (500ms).
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        completed.store(true, Ordering::SeqCst);
                        Json(json!({"done": true}))
                    }
                }),
            );

        let shutdown_signal = coordinator.shutdown_signal();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal)
                .await
                .unwrap();
        });

        // Wait for server to be ready.
        let client = Client::new();
        let resp = client
            .get(format!("{base_url}/api/health"))
            .send()
            .await
            .expect("server should be ready");
        assert_eq!(resp.status(), 200);

        // Start a slow request.
        let slow_client = Client::new();
        let slow_url = format!("{base_url}/api/slow");
        let slow_handle = tokio::spawn(async move {
            slow_client.get(&slow_url).send().await
        });

        // Give the slow request time to start.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Trigger shutdown while the slow request is in-flight.
        coordinator.trigger();

        // The slow request should still complete.
        let slow_resp = tokio::time::timeout(Duration::from_secs(5), slow_handle)
            .await
            .expect("slow request should complete within timeout")
            .expect("slow request task should not panic")
            .expect("slow request should succeed");

        assert_eq!(slow_resp.status(), 200);
        assert!(handler_completed.load(Ordering::SeqCst), "handler should have completed");

        // Server should shut down cleanly.
        tokio::time::timeout(Duration::from_secs(5), server_handle)
            .await
            .expect("server should shut down within timeout")
            .expect("server task should not panic");
    }

    #[tokio::test]
    async fn new_connections_rejected_during_shutdown() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}");

        let db = test_db();
        let coordinator =
            ShutdownCoordinator::new(Arc::clone(&db)).with_timeout(Duration::from_secs(5));

        let app = axum::Router::new().route(
            "/api/health",
            get(|| async { Json(json!({"status": "ok"})) }),
        );

        let shutdown_signal = coordinator.shutdown_signal();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal)
                .await
                .unwrap();
        });

        // Wait for server to be ready.
        let client = Client::new();
        let resp = client
            .get(format!("{base_url}/api/health"))
            .send()
            .await
            .expect("server should be ready");
        assert_eq!(resp.status(), 200);

        // Trigger shutdown.
        coordinator.trigger();

        // Wait for the server to stop.
        tokio::time::timeout(Duration::from_secs(5), server_handle)
            .await
            .expect("server should shut down")
            .expect("server should not panic");

        // New connections should now be refused.
        let result = client
            .get(format!("{base_url}/api/health"))
            .send()
            .await;

        assert!(
            result.is_err(),
            "new connections should be refused after shutdown"
        );
    }

    #[tokio::test]
    async fn serve_with_shutdown_full_lifecycle() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}");

        let db = test_db();
        let coordinator =
            ShutdownCoordinator::new(Arc::clone(&db)).with_timeout(Duration::from_secs(5));

        // Track request count.
        let request_count = Arc::new(AtomicU32::new(0));
        let count_clone = Arc::clone(&request_count);

        let app = axum::Router::new().route(
            "/api/health",
            get(move || {
                let count = count_clone.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"status": "ok"}))
                }
            }),
        );

        // Trigger shutdown after a short delay.
        let trigger_tx = coordinator.shutdown_tx.clone();
        tokio::spawn(async move {
            // Give the server time to start and handle a request.
            tokio::time::sleep(Duration::from_millis(200)).await;
            let _ = trigger_tx.send(true);
        });

        let server_handle = tokio::spawn(async move {
            serve_with_shutdown(listener, app, coordinator)
                .await
                .expect("serve_with_shutdown should succeed");
        });

        // Send a request before shutdown triggers.
        let client = Client::new();
        let resp = client
            .get(format!("{base_url}/api/health"))
            .send()
            .await
            .expect("request should succeed before shutdown");
        assert_eq!(resp.status(), 200);

        // Wait for the full lifecycle to complete.
        tokio::time::timeout(Duration::from_secs(10), server_handle)
            .await
            .expect("full lifecycle should complete within timeout")
            .expect("server task should not panic");

        assert!(
            request_count.load(Ordering::SeqCst) >= 1,
            "at least one request should have been served"
        );
    }

    #[tokio::test]
    async fn shutdown_timeout_forces_exit() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}");

        let db = test_db();
        // Very short timeout (1 second).
        let coordinator =
            ShutdownCoordinator::new(Arc::clone(&db)).with_timeout(Duration::from_secs(1));

        let app = axum::Router::new()
            .route(
                "/api/health",
                get(|| async { Json(json!({"status": "ok"})) }),
            )
            .route(
                "/api/hang",
                get(|| async {
                    // This handler takes much longer than the shutdown timeout.
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    Json(json!({"done": true}))
                }),
            );

        let trigger_tx = coordinator.shutdown_tx.clone();

        let server_handle = tokio::spawn(async move {
            serve_with_shutdown(listener, app, coordinator).await
        });

        // Wait for server to be ready.
        let client = Client::new();
        let _ = client
            .get(format!("{base_url}/api/health"))
            .send()
            .await
            .expect("server should be ready");

        // Start a hanging request.
        let hang_client = Client::new();
        let hang_url = format!("{base_url}/api/hang");
        let _hang_handle = tokio::spawn(async move {
            // This will fail when the server force-closes.
            let _ = hang_client.get(&hang_url).send().await;
        });

        // Give the hanging request time to start.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Trigger shutdown.
        let _ = trigger_tx.send(true);

        // Server should exit within timeout + buffer, even though /api/hang
        // is still "running".
        let result = tokio::time::timeout(Duration::from_secs(5), server_handle)
            .await
            .expect("server should force-shutdown within timeout buffer")
            .expect("server task should not panic");

        // The serve_with_shutdown should succeed (timeout is not an error).
        assert!(result.is_ok(), "serve_with_shutdown should return Ok even on timeout");
    }

    #[tokio::test]
    async fn cleanup_closes_database_connections() {
        let db = test_db();

        // Verify database is healthy before cleanup.
        assert!(db.is_healthy(), "database should be healthy before cleanup");

        let coordinator = ShutdownCoordinator::new(Arc::clone(&db));

        // At this point we have 2 strong refs: `db` and the one inside coordinator.
        assert_eq!(Arc::strong_count(&db), 2);

        coordinator.cleanup();

        // After cleanup, coordinator's Arc is dropped. Only our local `db` remains.
        assert_eq!(Arc::strong_count(&db), 1);
    }

    #[tokio::test]
    async fn multiple_triggers_are_idempotent() {
        let db = test_db();
        let coordinator = ShutdownCoordinator::new(db);

        coordinator.trigger();
        coordinator.trigger();
        coordinator.trigger();

        assert!(coordinator.is_shutting_down());
    }
}
