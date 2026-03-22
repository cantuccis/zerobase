//! Authentication handlers.
//!
//! Provides HTTP handlers for:
//! - `POST /api/collections/:collection/auth-with-password` — email/password login
//! - `POST /api/collections/:collection/auth-refresh` — refresh an existing auth token

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use zerobase_auth::MfaService;
use zerobase_core::auth::{TokenService, TokenType};
use zerobase_core::services::record_service::{RecordRepository, RecordService, SchemaLookup};
use zerobase_core::ZerobaseError;

use crate::middleware::auth_context::RequireAuth;
use crate::response::RecordResponse;

/// Shared state for auth endpoints, bundling the record service with a token service.
pub struct AuthState<R: RecordRepository, S: SchemaLookup> {
    pub record_service: Arc<RecordService<R, S>>,
    pub token_service: Arc<dyn TokenService>,
}

impl<R: RecordRepository, S: SchemaLookup> Clone for AuthState<R, S> {
    fn clone(&self) -> Self {
        Self {
            record_service: Arc::clone(&self.record_service),
            token_service: Arc::clone(&self.token_service),
        }
    }
}

/// Request body for the auth-with-password endpoint.
#[derive(Debug, Deserialize)]
pub struct AuthWithPasswordRequest {
    /// The user's identity value (typically an email address).
    pub identity: String,
    /// The user's plaintext password.
    pub password: String,
}

/// `POST /api/collections/:collection_name/auth-with-password`
///
/// Authenticate a user with email/password and return a JWT token plus the user record.
///
/// # Request body
///
/// ```json
/// { "identity": "user@example.com", "password": "secret123" }
/// ```
///
/// # Success response (200)
///
/// ```json
/// {
///   "token": "<jwt>",
///   "record": { "id": "...", "collectionId": "...", "collectionName": "...", "email": "...", ... }
/// }
/// ```
///
/// # Error responses
///
/// - 400 — collection is not an auth collection, or email auth is disabled
/// - 401 — invalid credentials (returned as 400 to match PocketBase)
/// - 404 — collection not found
pub async fn auth_with_password<R: RecordRepository, S: SchemaLookup>(
    State(state): State<AuthState<R, S>>,
    Path(collection_name): Path<String>,
    Json(body): Json<AuthWithPasswordRequest>,
) -> impl IntoResponse {
    // Validate input.
    if body.identity.trim().is_empty() || body.password.is_empty() {
        return error_response(ZerobaseError::validation(
            "identity and password are required",
        ));
    }

    // Look up the collection for metadata (id, name) for the response.
    let collection = match state.record_service.get_collection(&collection_name) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    // Authenticate: find user by identity, verify password.
    let record = match state.record_service.authenticate_with_password(
        &collection_name,
        body.identity.trim(),
        &body.password,
    ) {
        Ok(r) => r,
        Err(e) => {
            // Map auth errors to 400 to match PocketBase behavior.
            if matches!(e, ZerobaseError::Auth { .. }) {
                return error_response(ZerobaseError::validation("Failed to authenticate."));
            }
            return error_response(e);
        }
    };

    // Extract tokenKey for JWT generation (before stripping it from the response).
    let token_key = record
        .get("tokenKey")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let user_id = record
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Check if MFA is enabled — if so, return a partial token instead of a full auth token.
    if MfaService::<R, S>::is_mfa_enabled(&record) {
        let mfa_token = match state.token_service.generate(
            &user_id,
            &collection.id,
            TokenType::MfaPartial,
            &token_key,
            None,
        ) {
            Ok(t) => t,
            Err(e) => return error_response(e),
        };

        let body = json!({
            "mfaToken": mfa_token,
            "mfaRequired": true,
        });

        return (StatusCode::OK, Json(body)).into_response();
    }

    // Generate JWT auth token.
    let token = match state.token_service.generate(
        &user_id,
        &collection.id,
        TokenType::Auth,
        &token_key,
        None,
    ) {
        Ok(t) => t,
        Err(e) => return error_response(e),
    };

    // Build the record response (strip tokenKey from the response to the client).
    let mut response_record = record;
    response_record.remove("tokenKey");

    let record_response = RecordResponse::new(collection.id, collection.name, response_record);

    let body = json!({
        "token": token,
        "record": serde_json::to_value(&record_response).unwrap(),
    });

    (StatusCode::OK, Json(body)).into_response()
}

/// `POST /api/collections/:collection_name/auth-refresh`
///
/// Refresh an existing auth token, returning a new JWT with extended expiry
/// and the latest user record.
///
/// The caller must present a valid (non-expired) auth token in the
/// `Authorization` header. The token must belong to a user in the specified
/// collection and the user's `tokenKey` must not have changed since the token
/// was issued.
///
/// # Success response (200)
///
/// ```json
/// {
///   "token": "<new_jwt>",
///   "record": { "id": "...", "collectionId": "...", "collectionName": "...", ... }
/// }
/// ```
///
/// # Error responses
///
/// - 401 — missing, expired, or invalid auth token
/// - 401 — tokenKey has changed (token revoked)
/// - 400 — collection mismatch (token was issued for a different collection)
/// - 404 — collection not found
pub async fn auth_refresh<R: RecordRepository, S: SchemaLookup>(
    State(state): State<AuthState<R, S>>,
    Path(collection_name): Path<String>,
    auth: RequireAuth,
) -> impl IntoResponse {
    // Look up the target collection.
    let collection = match state.record_service.get_collection(&collection_name) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    // Ensure the token was issued for *this* collection.
    let validated_token = match &auth.token {
        Some(t) => t,
        None => {
            return error_response(ZerobaseError::auth(
                "The request requires valid authorization token to be set.",
            ))
        }
    };

    if validated_token.claims.collection_id != collection.id {
        return error_response(ZerobaseError::validation(
            "Token collection does not match the requested collection.",
        ));
    }

    let user_id = &validated_token.claims.id;

    // Load the fresh user record from the database to get the current tokenKey
    // and return up-to-date data.
    let record = match state.record_service.get_record(&collection_name, user_id) {
        Ok(r) => r,
        Err(e) => return error_response(e),
    };

    // Verify tokenKey hasn't changed since the original token was issued.
    let current_token_key = record
        .get("tokenKey")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if current_token_key != validated_token.claims.token_key {
        return error_response(ZerobaseError::auth("token has been invalidated"));
    }

    // Generate a fresh auth token with extended expiry.
    let new_token = match state.token_service.generate(
        user_id,
        &collection.id,
        TokenType::Auth,
        current_token_key,
        None,
    ) {
        Ok(t) => t,
        Err(e) => return error_response(e),
    };

    // Build the response record (strip sensitive fields).
    let mut response_record = record;
    response_record.remove("tokenKey");
    response_record.remove("password");

    let record_response = RecordResponse::new(collection.id, collection.name, response_record);

    let body = json!({
        "token": new_token,
        "record": serde_json::to_value(&record_response).unwrap(),
    });

    (StatusCode::OK, Json(body)).into_response()
}

/// Convert a [`ZerobaseError`] into an axum HTTP response.
fn error_response(err: ZerobaseError) -> axum::response::Response {
    let status =
        StatusCode::from_u16(err.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = err.error_response_body();
    (status, Json(body)).into_response()
}
