//! External auth identity handlers.
//!
//! Provides HTTP handlers for:
//! - `GET    /api/collections/:collection/records/:id/external-auths` — list linked identities
//! - `DELETE /api/collections/:collection/records/:id/external-auths/:provider` — unlink a provider

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

use zerobase_core::schema::CollectionType;
use zerobase_core::services::external_auth::ExternalAuthRepository;
use zerobase_core::services::record_service::{RecordRepository, SchemaLookup};
use zerobase_core::ZerobaseError;

use crate::middleware::auth_context::RequireAuth;

/// Shared state for external auth endpoints.
pub struct ExternalAuthState<R: RecordRepository, S: SchemaLookup, E: ExternalAuthRepository> {
    pub record_repo: Arc<R>,
    pub schema_lookup: Arc<S>,
    pub external_auth_repo: Arc<E>,
}

impl<R: RecordRepository, S: SchemaLookup, E: ExternalAuthRepository> Clone
    for ExternalAuthState<R, S, E>
{
    fn clone(&self) -> Self {
        Self {
            record_repo: Arc::clone(&self.record_repo),
            schema_lookup: Arc::clone(&self.schema_lookup),
            external_auth_repo: Arc::clone(&self.external_auth_repo),
        }
    }
}

/// `GET /api/collections/:collection_name/records/:id/external-auths`
///
/// List all external OAuth2 identities linked to a record.
///
/// Only the record owner or a superuser may access this endpoint.
///
/// # Success response (200)
///
/// ```json
/// [
///   {
///     "id": "...",
///     "collectionId": "...",
///     "recordId": "...",
///     "provider": "google",
///     "providerId": "...",
///     "created": "...",
///     "updated": "..."
///   }
/// ]
/// ```
pub async fn list_external_auths<
    R: RecordRepository,
    S: SchemaLookup,
    E: ExternalAuthRepository,
>(
    State(state): State<ExternalAuthState<R, S, E>>,
    Path((collection_name, record_id)): Path<(String, String)>,
    auth: RequireAuth,
) -> impl IntoResponse {
    // Resolve the collection to get its ID and verify it exists.
    let collection = match state.schema_lookup.get_collection(&collection_name) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    // Verify the collection is an auth collection.
    if collection.collection_type != CollectionType::Auth {
        return error_response(ZerobaseError::validation(
            "external auths are only available for auth collections",
        ));
    }

    // Verify the record exists.
    if let Err(e) = state.record_repo.find_one(&collection_name, &record_id) {
        return error_response(ZerobaseError::from(e));
    }

    // Authorization: only the record owner or a superuser can list external auths.
    if !auth.is_superuser {
        let caller_id = auth
            .auth_record
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if caller_id != record_id {
            return error_response(ZerobaseError::forbidden(
                "only the record owner or a superuser can access external auths",
            ));
        }
    }

    match state
        .external_auth_repo
        .find_by_record(&collection.id, &record_id)
    {
        Ok(auths) => (StatusCode::OK, Json(json!(auths))).into_response(),
        Err(e) => error_response(e),
    }
}

/// `DELETE /api/collections/:collection_name/records/:id/external-auths/:provider`
///
/// Unlink an external OAuth2 identity from a record.
///
/// Only the record owner or a superuser may perform this action.
///
/// # Success response (204)
///
/// Empty body.
pub async fn unlink_external_auth<
    R: RecordRepository,
    S: SchemaLookup,
    E: ExternalAuthRepository,
>(
    State(state): State<ExternalAuthState<R, S, E>>,
    Path((collection_name, record_id, provider)): Path<(String, String, String)>,
    auth: RequireAuth,
) -> impl IntoResponse {
    // Resolve the collection.
    let collection = match state.schema_lookup.get_collection(&collection_name) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    // Verify the collection is an auth collection.
    if collection.collection_type != CollectionType::Auth {
        return error_response(ZerobaseError::validation(
            "external auths are only available for auth collections",
        ));
    }

    // Verify the record exists.
    if let Err(e) = state.record_repo.find_one(&collection_name, &record_id) {
        return error_response(ZerobaseError::from(e));
    }

    // Authorization: only the record owner or a superuser can unlink.
    if !auth.is_superuser {
        let caller_id = auth
            .auth_record
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if caller_id != record_id {
            return error_response(ZerobaseError::forbidden(
                "only the record owner or a superuser can manage external auths",
            ));
        }
    }

    // Find the specific external auth link for this provider.
    let auths = match state
        .external_auth_repo
        .find_by_record(&collection.id, &record_id)
    {
        Ok(a) => a,
        Err(e) => return error_response(e),
    };

    let ext_auth = match auths.iter().find(|a| a.provider == provider) {
        Some(a) => a,
        None => return error_response(ZerobaseError::not_found_with_id("ExternalAuth", &provider)),
    };

    match state.external_auth_repo.delete(&ext_auth.id) {
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
