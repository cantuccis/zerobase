//! Email verification handlers.
//!
//! Provides HTTP handlers for:
//! - `POST /api/collections/:collection/request-verification` — send verification email
//! - `POST /api/collections/:collection/confirm-verification` — confirm email with token

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use zerobase_auth::VerificationService;
use zerobase_core::services::record_service::{RecordRepository, SchemaLookup};
use zerobase_core::ZerobaseError;

/// Shared state for verification endpoints.
pub struct VerificationState<R: RecordRepository, S: SchemaLookup> {
    pub verification_service: Arc<VerificationService<R, S>>,
}

impl<R: RecordRepository, S: SchemaLookup> Clone for VerificationState<R, S> {
    fn clone(&self) -> Self {
        Self {
            verification_service: Arc::clone(&self.verification_service),
        }
    }
}

/// Request body for the request-verification endpoint.
#[derive(Debug, Deserialize)]
pub struct RequestVerificationBody {
    pub email: String,
}

/// Request body for the confirm-verification endpoint.
#[derive(Debug, Deserialize)]
pub struct ConfirmVerificationBody {
    pub token: String,
}

/// `POST /api/collections/:collection_name/request-verification`
///
/// Send a verification email to the user with the given email address.
/// Always returns 204 regardless of whether the email exists (prevents enumeration).
pub async fn request_verification<R: RecordRepository, S: SchemaLookup>(
    State(state): State<VerificationState<R, S>>,
    Path(collection_name): Path<String>,
    Json(body): Json<RequestVerificationBody>,
) -> impl IntoResponse {
    match state
        .verification_service
        .request_verification(&collection_name, &body.email)
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(e),
    }
}

/// `POST /api/collections/:collection_name/confirm-verification`
///
/// Confirm email verification using a token from the verification email.
/// Returns 204 on success.
pub async fn confirm_verification<R: RecordRepository, S: SchemaLookup>(
    State(state): State<VerificationState<R, S>>,
    Path(collection_name): Path<String>,
    Json(body): Json<ConfirmVerificationBody>,
) -> impl IntoResponse {
    match state
        .verification_service
        .confirm_verification(&collection_name, &body.token)
    {
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
