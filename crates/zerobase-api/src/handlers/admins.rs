//! Admin (superuser) authentication handlers.
//!
//! Provides the HTTP handler for superuser email/password login via
//! `POST /_/api/admins/auth-with-password`.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use zerobase_core::auth::{TokenService, TokenType};
use zerobase_core::services::superuser_service::{
    SuperuserRepository, SuperuserService, SUPERUSERS_COLLECTION_ID, SUPERUSERS_COLLECTION_NAME,
};
use zerobase_core::ZerobaseError;

/// Shared state for admin auth endpoints.
pub struct AdminAuthState<R: SuperuserRepository> {
    pub superuser_service: Arc<SuperuserService<R>>,
    pub token_service: Arc<dyn TokenService>,
}

impl<R: SuperuserRepository> Clone for AdminAuthState<R> {
    fn clone(&self) -> Self {
        Self {
            superuser_service: Arc::clone(&self.superuser_service),
            token_service: Arc::clone(&self.token_service),
        }
    }
}

/// Request body for the admin auth-with-password endpoint.
#[derive(Debug, Deserialize)]
pub struct AdminAuthRequest {
    /// The admin's email address.
    pub identity: String,
    /// The admin's plaintext password.
    pub password: String,
}

/// `POST /_/api/admins/auth-with-password`
///
/// Authenticate a superuser with email/password and return a JWT token plus the admin record.
///
/// # Request body
///
/// ```json
/// { "identity": "admin@example.com", "password": "secret123" }
/// ```
///
/// # Success response (200)
///
/// ```json
/// {
///   "token": "<jwt>",
///   "admin": { "id": "...", "email": "...", "created": "...", "updated": "..." }
/// }
/// ```
///
/// # Error responses
///
/// - 400 — missing identity or password, or invalid credentials
pub async fn admin_auth_with_password<R: SuperuserRepository>(
    State(state): State<AdminAuthState<R>>,
    Json(body): Json<AdminAuthRequest>,
) -> impl IntoResponse {
    // Validate input.
    if body.identity.trim().is_empty() || body.password.is_empty() {
        return error_response(ZerobaseError::validation(
            "identity and password are required",
        ));
    }

    // Authenticate: find superuser by email, verify password.
    let record = match state
        .superuser_service
        .authenticate(&body.identity, &body.password)
    {
        Ok(r) => r,
        Err(e) => {
            // Map auth errors to 400 to match PocketBase behavior.
            if matches!(e, ZerobaseError::Auth { .. }) {
                return error_response(ZerobaseError::validation("Failed to authenticate."));
            }
            return error_response(e);
        }
    };

    // Extract tokenKey for JWT generation (before stripping from response).
    let token_key = record
        .get("tokenKey")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let admin_id = record
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Generate JWT auth token with the well-known _superusers collection ID.
    let token = match state.token_service.generate(
        &admin_id,
        SUPERUSERS_COLLECTION_ID,
        TokenType::Auth,
        &token_key,
        None,
    ) {
        Ok(t) => t,
        Err(e) => return error_response(e),
    };

    // Build admin response (strip tokenKey).
    let mut admin_record = record;
    admin_record.remove("tokenKey");

    // Add collection context for consistency.
    admin_record
        .entry("collectionId".to_string())
        .or_insert_with(|| json!(SUPERUSERS_COLLECTION_ID));
    admin_record
        .entry("collectionName".to_string())
        .or_insert_with(|| json!(SUPERUSERS_COLLECTION_NAME));

    let body = json!({
        "token": token,
        "admin": admin_record,
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
