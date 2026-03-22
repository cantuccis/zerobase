//! Example: Using Zerobase as a Rust library (framework mode).
//!
//! This demonstrates how to embed Zerobase in your own application,
//! adding custom routes and hooks alongside the built-in BaaS features.
//!
//! Run with:
//!   cargo run --example framework_mode

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use zerobase::axum::{extract::State, routing::get, Json, Router};
use zerobase::zerobase_core::hooks::{Hook, HookContext, HookResult};
use zerobase::ZerobaseApp;

use serde_json::{json, Value};

// ── Custom application state ─────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    request_count: Arc<AtomicU64>,
}

// ── Custom route handlers ────────────────────────────────────────────────────

async fn hello_handler() -> Json<Value> {
    Json(json!({
        "message": "Hello from a custom route!",
        "powered_by": "zerobase"
    }))
}

async fn stats_handler(State(state): State<AppState>) -> Json<Value> {
    let count = state.request_count.fetch_add(1, Ordering::Relaxed);
    Json(json!({
        "requests_served": count,
    }))
}

// ── Custom hook ──────────────────────────────────────────────────────────────

/// A hook that logs every record operation to stdout.
struct AuditLogger;

impl Hook for AuditLogger {
    fn name(&self) -> &str {
        "audit_logger"
    }

    fn after_operation(&self, ctx: &HookContext) -> HookResult<()> {
        println!(
            "[audit] {} on collection '{}' (record: {})",
            ctx.operation, ctx.collection, ctx.record_id
        );
        Ok(())
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state = AppState {
        request_count: Arc::new(AtomicU64::new(0)),
    };

    // Build custom routes with shared state.
    let custom_routes = Router::new()
        .route("/api/custom/hello", get(hello_handler))
        .route("/api/custom/stats", get(stats_handler))
        .with_state(state);

    // Create and configure the Zerobase application.
    let app = ZerobaseApp::new()?
        .with_host("127.0.0.1")
        .with_port(8090)
        .with_tracing()
        .with_custom_routes(custom_routes)
        .with_hook(AuditLogger, 100);

    println!("Zerobase running at http://127.0.0.1:8090");
    println!("Custom endpoints:");
    println!("  GET /api/custom/hello  — greeting");
    println!("  GET /api/custom/stats  — request counter");
    println!("  GET /api/health        — built-in health check");

    app.serve().await
}
