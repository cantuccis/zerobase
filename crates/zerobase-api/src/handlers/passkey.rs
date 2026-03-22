//! Passkey/WebAuthn authentication handlers.
//!
//! Provides HTTP handlers for:
//! - `POST /api/collections/:collection/request-passkey-register` — begin registration
//! - `POST /api/collections/:collection/confirm-passkey-register` — complete registration
//! - `POST /api/collections/:collection/auth-with-passkey-begin` — begin authentication
//! - `POST /api/collections/:collection/auth-with-passkey-finish` — complete authentication

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use webauthn_rs::prelude::{PublicKeyCredential, RegisterPublicKeyCredential};

use zerobase_auth::PasskeyService;
use zerobase_core::services::record_service::{RecordRepository, SchemaLookup};
use zerobase_core::services::webauthn_credential::WebauthnCredentialRepository;
use zerobase_core::ZerobaseError;

use crate::response::RecordResponse;

/// Shared state for passkey endpoints.
pub struct PasskeyState<R: RecordRepository, S: SchemaLookup, W: WebauthnCredentialRepository> {
    pub passkey_service: Arc<PasskeyService<R, S, W>>,
}

impl<R: RecordRepository, S: SchemaLookup, W: WebauthnCredentialRepository> Clone
    for PasskeyState<R, S, W>
{
    fn clone(&self) -> Self {
        Self {
            passkey_service: Arc::clone(&self.passkey_service),
        }
    }
}

/// Request body for `request-passkey-register`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPasskeyRegisterBody {
    pub user_id: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// Request body for `confirm-passkey-register`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmPasskeyRegisterBody {
    pub registration_id: String,
    pub credential: RegisterPublicKeyCredential,
}

/// Request body for `auth-with-passkey-finish`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthWithPasskeyFinishBody {
    pub authentication_id: String,
    pub credential: PublicKeyCredential,
}

/// `POST /api/collections/:collection_name/request-passkey-register`
///
/// Begin passkey registration for a user. Returns WebAuthn creation options.
pub async fn request_passkey_register<
    R: RecordRepository,
    S: SchemaLookup,
    W: WebauthnCredentialRepository,
>(
    State(state): State<PasskeyState<R, S, W>>,
    Path(collection_name): Path<String>,
    Json(body): Json<RequestPasskeyRegisterBody>,
) -> impl IntoResponse {
    match state.passkey_service.request_passkey_register(
        &collection_name,
        &body.user_id,
        body.name.as_deref(),
    ) {
        Ok(response) => {
            let body = json!({
                "registrationId": response.registration_id,
                "options": response.options,
            });
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => error_response(e),
    }
}

/// `POST /api/collections/:collection_name/confirm-passkey-register`
///
/// Complete passkey registration by verifying the browser's credential response.
pub async fn confirm_passkey_register<
    R: RecordRepository,
    S: SchemaLookup,
    W: WebauthnCredentialRepository,
>(
    State(state): State<PasskeyState<R, S, W>>,
    Path(_collection_name): Path<String>,
    Json(body): Json<ConfirmPasskeyRegisterBody>,
) -> impl IntoResponse {
    match state
        .passkey_service
        .confirm_passkey_register(&body.registration_id, &body.credential)
    {
        Ok(()) => {
            let body = json!({ "success": true });
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => error_response(e),
    }
}

/// `POST /api/collections/:collection_name/auth-with-passkey-begin`
///
/// Begin passkey authentication. Returns WebAuthn assertion options.
pub async fn auth_with_passkey_begin<
    R: RecordRepository,
    S: SchemaLookup,
    W: WebauthnCredentialRepository,
>(
    State(state): State<PasskeyState<R, S, W>>,
    Path(collection_name): Path<String>,
) -> impl IntoResponse {
    match state
        .passkey_service
        .auth_with_passkey_begin(&collection_name)
    {
        Ok(response) => {
            let body = json!({
                "authenticationId": response.authentication_id,
                "options": response.options,
            });
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => error_response(e),
    }
}

/// `POST /api/collections/:collection_name/auth-with-passkey-finish`
///
/// Complete passkey authentication. Returns a JWT token and the authenticated record.
pub async fn auth_with_passkey_finish<
    R: RecordRepository,
    S: SchemaLookup,
    W: WebauthnCredentialRepository,
>(
    State(state): State<PasskeyState<R, S, W>>,
    Path(collection_name): Path<String>,
    Json(body): Json<AuthWithPasskeyFinishBody>,
) -> impl IntoResponse {
    let collection = match state
        .passkey_service
        .record_service()
        .get_collection(&collection_name)
    {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    match state
        .passkey_service
        .auth_with_passkey_finish(&body.authentication_id, &body.credential)
    {
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
