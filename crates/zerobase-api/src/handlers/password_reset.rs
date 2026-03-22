//! Password reset handlers.
//!
//! Provides HTTP handlers for:
//! - `POST /api/collections/:collection/request-password-reset` — send reset email
//! - `POST /api/collections/:collection/confirm-password-reset` — set new password with token

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use zerobase_auth::PasswordResetService;
use zerobase_core::services::record_service::{RecordRepository, SchemaLookup};
use zerobase_core::ZerobaseError;

/// Shared state for password reset endpoints.
pub struct PasswordResetState<R: RecordRepository, S: SchemaLookup> {
    pub password_reset_service: Arc<PasswordResetService<R, S>>,
}

impl<R: RecordRepository, S: SchemaLookup> Clone for PasswordResetState<R, S> {
    fn clone(&self) -> Self {
        Self {
            password_reset_service: Arc::clone(&self.password_reset_service),
        }
    }
}

/// Request body for the request-password-reset endpoint.
#[derive(Debug, Deserialize)]
pub struct RequestPasswordResetBody {
    pub email: String,
}

/// Request body for the confirm-password-reset endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmPasswordResetBody {
    pub token: String,
    pub password: String,
    pub password_confirm: String,
}

/// `POST /api/collections/:collection_name/request-password-reset`
///
/// Send a password reset email to the user with the given email address.
/// Always returns 204 regardless of whether the email exists (prevents enumeration).
pub async fn request_password_reset<R: RecordRepository, S: SchemaLookup>(
    State(state): State<PasswordResetState<R, S>>,
    Path(collection_name): Path<String>,
    Json(body): Json<RequestPasswordResetBody>,
) -> impl IntoResponse {
    match state
        .password_reset_service
        .request_password_reset(&collection_name, &body.email)
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(e),
    }
}

/// `POST /api/collections/:collection_name/confirm-password-reset`
///
/// Confirm password reset using a token from the reset email.
/// Returns 204 on success.
pub async fn confirm_password_reset<R: RecordRepository, S: SchemaLookup>(
    State(state): State<PasswordResetState<R, S>>,
    Path(collection_name): Path<String>,
    Json(body): Json<ConfirmPasswordResetBody>,
) -> impl IntoResponse {
    match state.password_reset_service.confirm_password_reset(
        &collection_name,
        &body.token,
        &body.password,
        &body.password_confirm,
    ) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(e),
    }
}

/// Convert a [`ZerobaseError`] into an axum HTTP response.
fn error_response(err: ZerobaseError) -> axum::response::Response {
    let status =
        StatusCode::from_u16(err.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = err.error_response_body();
    (status, Json(body)).into_response()
}
