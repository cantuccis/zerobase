//! OTP (One-Time Password) authentication handlers.
//!
//! Provides HTTP handlers for:
//! - `POST /api/collections/:collection/request-otp` — send OTP code via email
//! - `POST /api/collections/:collection/auth-with-otp` — verify OTP and get auth token

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use zerobase_auth::OtpService;
use zerobase_core::services::record_service::{RecordRepository, SchemaLookup};
use zerobase_core::ZerobaseError;

use crate::response::RecordResponse;

/// Shared state for OTP endpoints.
pub struct OtpState<R: RecordRepository, S: SchemaLookup> {
    pub otp_service: Arc<OtpService<R, S>>,
}

impl<R: RecordRepository, S: SchemaLookup> Clone for OtpState<R, S> {
    fn clone(&self) -> Self {
        Self {
            otp_service: Arc::clone(&self.otp_service),
        }
    }
}

/// Request body for the request-otp endpoint.
#[derive(Debug, Deserialize)]
pub struct RequestOtpBody {
    pub email: String,
}

/// Request body for the auth-with-otp endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthWithOtpBody {
    pub otp_id: String,
    pub code: String,
}

/// `POST /api/collections/:collection_name/request-otp`
///
/// Send a one-time password to the given email address.
/// Returns the OTP ID needed to verify the code.
///
/// Always returns an `otpId` regardless of whether the email exists
/// (prevents enumeration).
pub async fn request_otp<R: RecordRepository, S: SchemaLookup>(
    State(state): State<OtpState<R, S>>,
    Path(collection_name): Path<String>,
    Json(body): Json<RequestOtpBody>,
) -> impl IntoResponse {
    match state.otp_service.request_otp(&collection_name, &body.email) {
        Ok(otp_id) => {
            let body = json!({ "otpId": otp_id });
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => error_response(e),
    }
}

/// `POST /api/collections/:collection_name/auth-with-otp`
///
/// Verify an OTP code and return a JWT auth token plus the user record.
///
/// # Success response (200)
///
/// ```json
/// {
///   "token": "<jwt>",
///   "record": { "id": "...", "collectionId": "...", "collectionName": "...", ... }
/// }
/// ```
pub async fn auth_with_otp<R: RecordRepository, S: SchemaLookup>(
    State(state): State<OtpState<R, S>>,
    Path(collection_name): Path<String>,
    Json(body): Json<AuthWithOtpBody>,
) -> impl IntoResponse {
    // Get the collection metadata for the response.
    let collection = match state
        .otp_service
        .record_service()
        .get_collection(&collection_name)
    {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    match state.otp_service.auth_with_otp(&body.otp_id, &body.code) {
        Ok((token, mut record)) => {
            // Strip sensitive fields from the response.
            record.remove("tokenKey");
            record.remove("password");

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
