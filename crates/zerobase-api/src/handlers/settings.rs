//! Settings management handlers.
//!
//! Provides HTTP handlers for reading and updating server-wide settings.
//! All endpoints require superuser authentication.
//!
//! # Endpoints
//!
//! - `GET    /api/settings`            — get all settings
//! - `PATCH  /api/settings`            — update settings (partial)
//! - `GET    /api/settings/:key`       — get a single setting by key
//! - `DELETE /api/settings/:key`       — reset a single setting to default
//! - `POST   /api/settings/test-email` — send a test email using current SMTP settings

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use serde::Deserialize;
use serde_json::Value as JsonValue;

use zerobase_core::email::{EmailMessage, EmailService};
use zerobase_core::services::settings_service::SettingsRepository;
use zerobase_core::{SettingsService, ZerobaseError};

// ── Handlers ────────────────────────────────────────────────────────────────

/// `GET /api/settings`
///
/// Return all settings as a JSON object with one key per setting category.
///
/// Response (200 OK):
/// ```json
/// {
///   "meta": { "appName": "", "appUrl": "", ... },
///   "smtp": { "enabled": false, "host": "", ... },
///   "s3": { "enabled": false, "bucket": "", ... },
///   "backups": { ... },
///   "auth": { ... }
/// }
/// ```
pub async fn get_all_settings<R: SettingsRepository + 'static>(
    State(service): State<Arc<SettingsService<R>>>,
) -> impl IntoResponse {
    match service.get_all() {
        Ok(settings) => (StatusCode::OK, Json(settings)).into_response(),
        Err(e) => error_response(e),
    }
}

/// `PATCH /api/settings`
///
/// Update one or more setting categories. Accepts a partial JSON object —
/// only the provided keys are updated. Fields within a category are
/// deep-merged with the existing values.
///
/// Request body:
/// ```json
/// {
///   "meta": { "appName": "My App" },
///   "smtp": { "enabled": true, "host": "smtp.example.com" }
/// }
/// ```
///
/// Response (200 OK): the full settings object after the update.
pub async fn update_settings<R: SettingsRepository + 'static>(
    State(service): State<Arc<SettingsService<R>>>,
    Json(body): Json<HashMap<String, JsonValue>>,
) -> impl IntoResponse {
    match service.update(&body) {
        Ok(settings) => (StatusCode::OK, Json(settings)).into_response(),
        Err(e) => error_response(e),
    }
}

/// `GET /api/settings/:key`
///
/// Return a single setting category by key.
///
/// Response (200 OK): the setting value as a JSON object.
pub async fn get_setting<R: SettingsRepository + 'static>(
    State(service): State<Arc<SettingsService<R>>>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    match service.get(&key) {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(e) => error_response(e),
    }
}

/// `DELETE /api/settings/:key`
///
/// Reset a single setting category to its default value.
///
/// Response: 204 No Content on success.
pub async fn delete_setting<R: SettingsRepository + 'static>(
    State(service): State<Arc<SettingsService<R>>>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    match service.delete(&key) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(e),
    }
}

// ── Test Email ────────────────────────────────────────────────────────────

/// Request body for the test-email endpoint.
#[derive(Debug, Deserialize)]
pub struct TestEmailRequest {
    /// Email address to send the test message to.
    pub to: String,
}

/// Shared state for the test-email handler.
pub struct TestEmailState<R: SettingsRepository> {
    pub settings_service: Arc<SettingsService<R>>,
    pub email_service: Arc<dyn EmailService>,
}

impl<R: SettingsRepository> Clone for TestEmailState<R> {
    fn clone(&self) -> Self {
        Self {
            settings_service: Arc::clone(&self.settings_service),
            email_service: Arc::clone(&self.email_service),
        }
    }
}

/// `POST /api/settings/test-email`
///
/// Send a test email using the currently configured SMTP settings.
/// The request body must contain a `to` field with the recipient address.
///
/// Response (200 OK): `{ "success": true }`
/// Error (400/500): standard error response if SMTP is misconfigured or sending fails.
pub async fn test_email<R: SettingsRepository + 'static>(
    State(state): State<TestEmailState<R>>,
    Json(body): Json<TestEmailRequest>,
) -> impl IntoResponse {
    // Validate recipient
    if body.to.trim().is_empty() {
        return error_response(ZerobaseError::validation(
            "recipient email address is required",
        ));
    }

    // Check SMTP is enabled
    let smtp_settings = match state.settings_service.get("smtp") {
        Ok(value) => value,
        Err(e) => return error_response(e),
    };

    let enabled = smtp_settings
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if !enabled {
        return error_response(ZerobaseError::validation(
            "SMTP is not enabled. Enable SMTP in settings first.",
        ));
    }

    // Send test email
    let message = EmailMessage {
        to: body.to.trim().to_string(),
        subject: "Zerobase Test Email".to_string(),
        body_text: "This is a test email from your Zerobase server.\n\nIf you received this, your SMTP configuration is working correctly.".to_string(),
        body_html: Some(
            "<div style=\"font-family: sans-serif; max-width: 480px; margin: 0 auto; padding: 24px;\">\
             <h2 style=\"color: #1e293b;\">Zerobase Test Email</h2>\
             <p style=\"color: #475569;\">This is a test email from your Zerobase server.</p>\
             <p style=\"color: #475569;\">If you received this, your SMTP configuration is working correctly.</p>\
             <hr style=\"border: none; border-top: 1px solid #e2e8f0; margin: 24px 0;\" />\
             <p style=\"color: #94a3b8; font-size: 12px;\">Sent by Zerobase</p>\
             </div>"
                .to_string(),
        ),
    };

    match state.email_service.send(&message) {
        Ok(()) => {
            let body = serde_json::json!({ "success": true });
            (StatusCode::OK, Json(body)).into_response()
        }
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
