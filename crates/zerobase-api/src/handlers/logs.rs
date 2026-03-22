//! Request log handlers.
//!
//! Provides HTTP handlers for viewing and querying request logs.
//! All endpoints require superuser authentication.
//!
//! # Endpoints
//!
//! - `GET /_/api/logs`       — list logs with filtering and pagination
//! - `GET /_/api/logs/stats` — aggregate log statistics
//! - `GET /_/api/logs/:id`   — view a single log entry

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use zerobase_core::services::log_service::{LogQuery, LogRepository, LogStatsQuery};
use zerobase_core::{LogService, ZerobaseError};

// ── Query parameter types ────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct ListLogsParams {
    pub method: Option<String>,
    pub url: Option<String>,
    #[serde(rename = "statusMin")]
    pub status_min: Option<u16>,
    #[serde(rename = "statusMax")]
    pub status_max: Option<u16>,
    #[serde(rename = "authId")]
    pub auth_id: Option<String>,
    pub ip: Option<String>,
    #[serde(rename = "createdAfter")]
    pub created_after: Option<String>,
    #[serde(rename = "createdBefore")]
    pub created_before: Option<String>,
    pub filter: Option<String>,
    pub page: Option<u32>,
    #[serde(rename = "perPage")]
    pub per_page: Option<u32>,
    pub sort: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct LogStatsParams {
    #[serde(rename = "createdAfter")]
    pub created_after: Option<String>,
    #[serde(rename = "createdBefore")]
    pub created_before: Option<String>,
    #[serde(rename = "groupBy")]
    pub group_by: Option<String>,
}

// ── Handlers ────────────────────────────────────────────────────────────

/// `GET /_/api/logs`
pub async fn list_logs<R: LogRepository + 'static>(
    State(service): State<Arc<LogService<R>>>,
    Query(params): Query<ListLogsParams>,
) -> impl IntoResponse {
    let query = LogQuery {
        method: params.method,
        url: params.url,
        status_min: params.status_min,
        status_max: params.status_max,
        auth_id: params.auth_id,
        ip: params.ip,
        created_after: params.created_after,
        created_before: params.created_before,
        filter: params.filter,
        page: params.page.unwrap_or(1),
        per_page: params.per_page.unwrap_or(20),
        sort: params.sort,
    };

    match service.list(&query) {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => error_response(e),
    }
}

/// `GET /_/api/logs/stats`
pub async fn log_stats<R: LogRepository + 'static>(
    State(service): State<Arc<LogService<R>>>,
    Query(params): Query<LogStatsParams>,
) -> impl IntoResponse {
    let query = LogStatsQuery {
        created_after: params.created_after,
        created_before: params.created_before,
        group_by: params.group_by,
    };

    match service.stats(&query) {
        Ok(stats) => (StatusCode::OK, Json(stats)).into_response(),
        Err(e) => error_response(e),
    }
}

/// `GET /_/api/logs/:id`
pub async fn get_log<R: LogRepository + 'static>(
    State(service): State<Arc<LogService<R>>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match service.get(&id) {
        Ok(entry) => (StatusCode::OK, Json(entry)).into_response(),
        Err(e) => error_response(e),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn error_response(err: ZerobaseError) -> axum::response::Response {
    let status =
        StatusCode::from_u16(err.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = err.error_response_body();
    (status, Json(body)).into_response()
}
