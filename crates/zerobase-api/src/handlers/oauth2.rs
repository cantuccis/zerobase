//! OAuth2 authentication handlers.
//!
//! Provides HTTP handlers for:
//! - `GET  /api/collections/:collection/auth-methods` — list enabled auth methods
//! - `POST /api/collections/:collection/auth-with-oauth2` — complete OAuth2 flow

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use zerobase_auth::oauth2::OAuth2Service;
use zerobase_core::services::external_auth::ExternalAuthRepository;
use zerobase_core::services::record_service::{RecordRepository, SchemaLookup};
use zerobase_core::ZerobaseError;

use crate::response::RecordResponse;

/// Shared state for OAuth2 endpoints.
pub struct OAuth2State<R: RecordRepository, S: SchemaLookup, E: ExternalAuthRepository> {
    pub oauth2_service: Arc<OAuth2Service<R, S, E>>,
}

impl<R: RecordRepository, S: SchemaLookup, E: ExternalAuthRepository> Clone
    for OAuth2State<R, S, E>
{
    fn clone(&self) -> Self {
        Self {
            oauth2_service: Arc::clone(&self.oauth2_service),
        }
    }
}

/// `GET /api/collections/:collection_name/auth-methods`
///
/// List available authentication methods for a collection. No auth required.
///
/// # Success response (200)
///
/// ```json
/// {
///   "password": {
///     "enabled": true,
///     "identityFields": ["email"]
///   },
///   "oauth2": {
///     "enabled": true,
///     "providers": [
///       {
///         "name": "google",
///         "displayName": "Google",
///         "state": "...",
///         "authUrl": "https://accounts.google.com/...",
///         "codeVerifier": "..."
///       }
///     ]
///   },
///   "otp": { "enabled": false },
///   "mfa": { "enabled": false, "duration": 0 }
/// }
/// ```
pub async fn list_auth_methods<R: RecordRepository, S: SchemaLookup, E: ExternalAuthRepository>(
    State(state): State<OAuth2State<R, S, E>>,
    Path(collection_name): Path<String>,
) -> impl IntoResponse {
    match state.oauth2_service.list_auth_methods(&collection_name) {
        Ok(response) => (StatusCode::OK, Json(json!(response))).into_response(),
        Err(e) => error_response(e),
    }
}

/// Request body for the auth-with-oauth2 endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthWithOAuth2Request {
    /// The OAuth2 provider name (e.g., "google", "github").
    pub provider: String,
    /// The authorization code from the provider callback.
    pub code: String,
    /// The redirect URL used in the authorization request.
    pub redirect_url: String,
    /// The PKCE code verifier, if PKCE was used.
    #[serde(default)]
    pub code_verifier: Option<String>,
}

/// `POST /api/collections/:collection_name/auth-with-oauth2`
///
/// Complete the OAuth2 authorization code flow.
///
/// # Request body
///
/// ```json
/// {
///   "provider": "google",
///   "code": "auth-code-from-callback",
///   "redirectUrl": "http://localhost:8090/redirect",
///   "codeVerifier": "optional-pkce-verifier"
/// }
/// ```
///
/// # Success response (200)
///
/// ```json
/// {
///   "token": "<jwt>",
///   "record": { "id": "...", "collectionId": "...", ... },
///   "meta": { "isNew": false }
/// }
/// ```
pub async fn auth_with_oauth2<R: RecordRepository, S: SchemaLookup, E: ExternalAuthRepository>(
    State(state): State<OAuth2State<R, S, E>>,
    Path(collection_name): Path<String>,
    Json(body): Json<AuthWithOAuth2Request>,
) -> impl IntoResponse {
    // Validate required fields.
    if body.provider.trim().is_empty() {
        return error_response(ZerobaseError::validation("provider is required"));
    }
    if body.code.trim().is_empty() {
        return error_response(ZerobaseError::validation("code is required"));
    }
    if body.redirect_url.trim().is_empty() {
        return error_response(ZerobaseError::validation("redirectUrl is required"));
    }

    match state
        .oauth2_service
        .authenticate_with_oauth2(
            &collection_name,
            body.provider.trim(),
            body.code.trim(),
            body.redirect_url.trim(),
            body.code_verifier.as_deref(),
        )
        .await
    {
        Ok(result) => {
            let record_response =
                RecordResponse::new(result.collection_id, result.collection_name, result.record);

            let body = json!({
                "token": result.token,
                "record": serde_json::to_value(&record_response).unwrap(),
                "meta": {
                    "isNew": result.is_new_user,
                },
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
