//! MFA (Multi-Factor Authentication) handlers.
//!
//! Provides HTTP handlers for:
//! - `POST /api/collections/:collection/records/:id/request-mfa-setup` — generate TOTP secret/QR
//! - `POST /api/collections/:collection/records/:id/confirm-mfa` — verify code and enable MFA
//! - `POST /api/collections/:collection/auth-with-mfa` — verify TOTP/recovery code and get auth token

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use zerobase_auth::MfaService;
use zerobase_core::services::record_service::{RecordRepository, SchemaLookup};
use zerobase_core::ZerobaseError;

use crate::response::RecordResponse;

/// Shared state for MFA endpoints.
pub struct MfaState<R: RecordRepository, S: SchemaLookup> {
    pub mfa_service: Arc<MfaService<R, S>>,
}

impl<R: RecordRepository, S: SchemaLookup> Clone for MfaState<R, S> {
    fn clone(&self) -> Self {
        Self {
            mfa_service: Arc::clone(&self.mfa_service),
        }
    }
}

/// Request body for confirm-mfa endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmMfaBody {
    pub mfa_id: String,
    pub code: String,
}

/// Request body for auth-with-mfa endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthWithMfaBody {
    pub mfa_token: String,
    pub code: String,
}

/// `POST /api/collections/:collection_name/records/:id/request-mfa-setup`
///
/// Begin MFA setup for a user. Returns the TOTP secret and QR URI.
///
/// # Success response (200)
///
/// ```json
/// {
///   "mfaId": "...",
///   "secret": "BASE32SECRET...",
///   "qrUri": "otpauth://totp/..."
/// }
/// ```
pub async fn request_mfa_setup<R: RecordRepository, S: SchemaLookup>(
    State(state): State<MfaState<R, S>>,
    Path((collection_name, id)): Path<(String, String)>,
) -> impl IntoResponse {
    match state.mfa_service.request_mfa_setup(&collection_name, &id) {
        Ok(response) => {
            let body = json!({
                "mfaId": response.mfa_id,
                "secret": response.secret,
                "qrUri": response.qr_uri,
            });
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => error_response(e),
    }
}

/// `POST /api/collections/:collection_name/records/:id/confirm-mfa`
///
/// Confirm MFA setup by verifying a TOTP code. On success, MFA is enabled
/// and recovery codes are returned (shown once).
///
/// # Success response (200)
///
/// ```json
/// {
///   "recoveryCodes": ["code1", "code2", ...]
/// }
/// ```
pub async fn confirm_mfa<R: RecordRepository, S: SchemaLookup>(
    State(state): State<MfaState<R, S>>,
    Path((_collection_name, _id)): Path<(String, String)>,
    Json(body): Json<ConfirmMfaBody>,
) -> impl IntoResponse {
    match state
        .mfa_service
        .confirm_mfa_setup(&body.mfa_id, &body.code)
    {
        Ok(recovery_codes) => {
            let body = json!({
                "recoveryCodes": recovery_codes,
            });
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => error_response(e),
    }
}

/// `POST /api/collections/:collection_name/auth-with-mfa`
///
/// Verify a TOTP or recovery code and exchange the MFA partial token
/// for a full auth token.
///
/// # Request body
///
/// ```json
/// { "mfaToken": "<partial_jwt>", "code": "123456" }
/// ```
///
/// # Success response (200)
///
/// ```json
/// {
///   "token": "<jwt>",
///   "record": { "id": "...", "collectionId": "...", ... }
/// }
/// ```
pub async fn auth_with_mfa<R: RecordRepository, S: SchemaLookup>(
    State(state): State<MfaState<R, S>>,
    Path(collection_name): Path<String>,
    Json(body): Json<AuthWithMfaBody>,
) -> impl IntoResponse {
    // Get collection metadata for the response.
    let collection = match state
        .mfa_service
        .record_service()
        .get_collection(&collection_name)
    {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    match state.mfa_service.auth_with_mfa(&body.mfa_token, &body.code) {
        Ok((token, record)) => {
            let record_response = RecordResponse::new(collection.id, collection.name, record);

            let body = json!({
                "token": token,
                "record": serde_json::to_value(&record_response).unwrap(),
            });

            (StatusCode::OK, Json(body)).into_response()
        }
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
