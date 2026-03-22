//! Email change handlers.
//!
//! Provides HTTP handlers for:
//! - `POST /api/collections/:collection/request-email-change` — send confirmation to new email
//! - `POST /api/collections/:collection/confirm-email-change` — confirm email change with token

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use zerobase_auth::EmailChangeService;
use zerobase_core::services::record_service::{RecordRepository, SchemaLookup};
use zerobase_core::ZerobaseError;

use crate::middleware::auth_context::RequireAuth;

/// Shared state for email change endpoints.
pub struct EmailChangeState<R: RecordRepository, S: SchemaLookup> {
    pub email_change_service: Arc<EmailChangeService<R, S>>,
}

impl<R: RecordRepository, S: SchemaLookup> Clone for EmailChangeState<R, S> {
    fn clone(&self) -> Self {
        Self {
            email_change_service: Arc::clone(&self.email_change_service),
        }
    }
}

/// Request body for the request-email-change endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestEmailChangeBody {
    pub new_email: String,
}

/// Request body for the confirm-email-change endpoint.
#[derive(Debug, Deserialize)]
pub struct ConfirmEmailChangeBody {
    pub token: String,
}

/// `POST /api/collections/:collection_name/request-email-change`
///
/// Send a confirmation email to the new address.
/// Requires authentication. Returns 204 on success.
pub async fn request_email_change<R: RecordRepository, S: SchemaLookup>(
    State(state): State<EmailChangeState<R, S>>,
    Path(collection_name): Path<String>,
    auth: RequireAuth,
    Json(body): Json<RequestEmailChangeBody>,
) -> impl IntoResponse {
    let user_id = match &auth.token {
        Some(t) => &t.claims.id,
        None => {
            return error_response(ZerobaseError::auth(
                "The request requires valid authorization token to be set.",
            ))
        }
    };

    match state.email_change_service.request_email_change(
        &collection_name,
        user_id,
        &body.new_email,
    ) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(e),
    }
}

/// `POST /api/collections/:collection_name/confirm-email-change`
///
/// Confirm email change using a token from the confirmation email.
/// Returns 204 on success.
pub async fn confirm_email_change<R: RecordRepository, S: SchemaLookup>(
    State(state): State<EmailChangeState<R, S>>,
    Path(collection_name): Path<String>,
    Json(body): Json<ConfirmEmailChangeBody>,
) -> impl IntoResponse {
    match state
        .email_change_service
        .confirm_email_change(&collection_name, &body.token)
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
