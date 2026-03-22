//! Health check handler with optional database diagnostics.
//!
//! When a [`Database`] is provided via [`HealthState`], the health endpoint
//! includes detailed database diagnostics: pool utilization, connection
//! latency, and exhaustion detection. Without a database, it returns a
//! simple `{"status": "ok"}` response.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use zerobase_db::{Database, HealthDiagnostics, HealthStatus, PoolStats};

/// Shared state for the health endpoint.
#[derive(Clone)]
pub struct HealthState {
    pub db: Arc<Database>,
}

/// Response body for the health check endpoint.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Top-level status: `"healthy"`, `"degraded"`, or `"unhealthy"`.
    pub status: HealthStatus,
    /// Database diagnostics (present when a DB is configured).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<DatabaseHealth>,
}

/// Database health details included in the health response.
#[derive(Debug, Serialize)]
pub struct DatabaseHealth {
    /// Whether the read pool passed its connectivity check.
    pub read_pool_ok: bool,
    /// Whether the write connection passed its connectivity check.
    pub write_conn_ok: bool,
    /// Connection pool statistics.
    pub pool: PoolStats,
    /// Pool utilization as a percentage (0.0–100.0).
    pub pool_utilization_pct: f64,
    /// Whether the pool is fully exhausted (zero idle connections at max size).
    pub pool_exhausted: bool,
    /// Read-path latency in microseconds.
    pub read_latency_us: u64,
    /// Write-path latency in microseconds.
    pub write_latency_us: u64,
}

impl From<HealthDiagnostics> for DatabaseHealth {
    fn from(d: HealthDiagnostics) -> Self {
        Self {
            read_pool_ok: d.read_pool_ok,
            write_conn_ok: d.write_conn_ok,
            pool: d.pool,
            pool_utilization_pct: d.pool_utilization_pct,
            pool_exhausted: d.pool_exhausted,
            read_latency_us: d.read_latency_us,
            write_latency_us: d.write_latency_us,
        }
    }
}

/// Health check handler **with** database diagnostics.
///
/// Returns 200 when healthy or degraded, 503 when unhealthy.
pub async fn health_check_with_db(
    State(state): State<HealthState>,
) -> impl IntoResponse {
    let diagnostics = state.db.health_diagnostics();
    let status = diagnostics.status;

    let http_status = match status {
        HealthStatus::Healthy | HealthStatus::Degraded => StatusCode::OK,
        HealthStatus::Unhealthy => StatusCode::SERVICE_UNAVAILABLE,
    };

    let body = HealthResponse {
        status,
        database: Some(DatabaseHealth::from(diagnostics)),
    };

    (http_status, Json(body))
}

/// Minimal health check handler **without** database diagnostics.
///
/// Always returns 200 with `{"status": "healthy"}`.
pub async fn health_check_simple() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: HealthStatus::Healthy,
        database: None,
    })
}
