//! Backup management handlers.
//!
//! Provides HTTP handlers for creating, listing, downloading, deleting,
//! and restoring database backups. All endpoints require superuser authentication.
//!
//! # Endpoints
//!
//! - `POST   /_/api/backups`                — create a new backup
//! - `GET    /_/api/backups`                — list all backups
//! - `GET    /_/api/backups/:name`          — download a backup file
//! - `DELETE /_/api/backups/:name`          — delete a backup
//! - `POST   /_/api/backups/:name/restore` — restore from a backup

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;

use zerobase_core::services::backup_service::{BackupRepository, CreateBackupRequest};
use zerobase_core::{BackupService, ZerobaseError};

// ── Handlers ────────────────────────────────────────────────────────────────

/// `POST /_/api/backups`
///
/// Create a new database backup.
///
/// Request body (optional):
/// ```json
/// { "name": "my_backup.db" }
/// ```
///
/// If `name` is omitted or null, an auto-generated timestamped name is used.
///
/// Response (200 OK): backup metadata.
pub async fn create_backup<R: BackupRepository + 'static>(
    State(service): State<Arc<BackupService<R>>>,
    Json(body): Json<CreateBackupRequest>,
) -> impl IntoResponse {
    match service.create_backup(body.name.as_deref()) {
        Ok(info) => (StatusCode::OK, Json(info)).into_response(),
        Err(e) => error_response(e),
    }
}

/// `GET /_/api/backups`
///
/// List all available backups ordered by creation time (newest first).
///
/// Response (200 OK): JSON array of backup metadata objects.
pub async fn list_backups<R: BackupRepository + 'static>(
    State(service): State<Arc<BackupService<R>>>,
) -> impl IntoResponse {
    match service.list_backups() {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => error_response(e),
    }
}

/// `GET /_/api/backups/:name`
///
/// Download a backup file.
///
/// Response: the backup file as an `application/octet-stream` download.
pub async fn download_backup<R: BackupRepository + 'static>(
    State(service): State<Arc<BackupService<R>>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let path = match service.backup_path(&name) {
        Ok(p) => p,
        Err(e) => return error_response(e),
    };

    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(e) => {
            return error_response(ZerobaseError::internal(format!(
                "failed to read backup file: {e}"
            )));
        }
    };

    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                "application/octet-stream".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// `DELETE /_/api/backups/:name`
///
/// Delete a backup file.
///
/// Response: 204 No Content on success.
pub async fn delete_backup<R: BackupRepository + 'static>(
    State(service): State<Arc<BackupService<R>>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match service.delete_backup(&name) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(e),
    }
}

/// `POST /_/api/backups/:name/restore`
///
/// Restore the database from a backup.
///
/// **Warning**: This replaces the current database contents.
///
/// Response: 204 No Content on success.
pub async fn restore_backup<R: BackupRepository + 'static>(
    State(service): State<Arc<BackupService<R>>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match service.restore_backup(&name) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(e),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Convert a [`ZerobaseError`] into an axum HTTP response.
fn error_response(err: ZerobaseError) -> axum::response::Response {
    let status =
        StatusCode::from_u16(err.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = err.error_response_body();
    (status, Json(body)).into_response()
}
