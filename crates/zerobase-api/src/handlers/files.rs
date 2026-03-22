//! File download and serving endpoints.
//!
//! Implements PocketBase-compatible file serving:
//!
//! - `GET /api/files/token` — generate a short-lived file access token
//! - `GET /api/files/:collectionId/:recordId/:filename` — serve a file
//!
//! # Protected files
//!
//! File fields can be marked `protected: true` in the collection schema.
//! Protected files require a valid file token passed as a `?token=` query
//! parameter. Public files are served directly without authentication.
//!
//! # MIME types
//!
//! Files are served with the `Content-Type` header set to the MIME type
//! stored at upload time. If the stored type is missing or empty, we fall
//! back to `application/octet-stream`.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use zerobase_core::auth::{TokenService, TokenType};
use zerobase_core::schema::FieldType;
use zerobase_core::services::record_service::SchemaLookup;
use zerobase_core::storage::file_key;
use zerobase_core::ZerobaseError;
use zerobase_files::thumb::parse_thumb_spec;
use zerobase_files::FileService;

use crate::middleware::auth_context::AuthInfo;

// ── State ──────────────────────────────────────────────────────────────────

/// Shared state for file-serving routes.
pub struct FileState<S: SchemaLookup + 'static> {
    pub file_service: Arc<FileService>,
    pub token_service: Arc<dyn TokenService>,
    pub schema_lookup: Arc<S>,
}

impl<S: SchemaLookup> Clone for FileState<S> {
    fn clone(&self) -> Self {
        Self {
            file_service: self.file_service.clone(),
            token_service: self.token_service.clone(),
            schema_lookup: self.schema_lookup.clone(),
        }
    }
}

// ── GET /api/files/token ───────────────────────────────────────────────────

/// Response body for the file token endpoint.
#[derive(Debug, Serialize)]
pub struct FileTokenResponse {
    pub token: String,
}

/// Generate a short-lived file access token.
///
/// Requires an authenticated user (any auth collection). Returns a JWT
/// of type `File` valid for 2 minutes (120 seconds).
pub async fn request_file_token<S: SchemaLookup>(
    auth: AuthInfo,
    State(state): State<FileState<S>>,
) -> Response {
    // Must be authenticated to request a file token.
    if !auth.is_authenticated() {
        return error_response(ZerobaseError::auth("authentication required"));
    }

    // Extract user id and collection id from the auth context.
    let (user_id, collection_id, token_key) = if auth.is_superuser {
        // Superusers get a token with their admin context.
        let id = auth
            .auth_record
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("_superuser");
        let token_key = auth
            .auth_record
            .get("tokenKey")
            .and_then(|v| v.as_str())
            .unwrap_or("_superuser_key");
        (
            id.to_string(),
            "_superusers".to_string(),
            token_key.to_string(),
        )
    } else {
        let id = auth
            .auth_record
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let collection_id = auth
            .token
            .as_ref()
            .map(|t| t.claims.collection_id.clone())
            .unwrap_or_default();
        let token_key = auth
            .auth_record
            .get("tokenKey")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        (id, collection_id, token_key)
    };

    if user_id.is_empty() {
        return error_response(ZerobaseError::auth("authentication required"));
    }

    // Generate a short-lived file token (120 seconds = 2 minutes).
    match state.token_service.generate(
        &user_id,
        &collection_id,
        TokenType::File,
        &token_key,
        Some(120),
    ) {
        Ok(token) => (StatusCode::OK, Json(FileTokenResponse { token })).into_response(),
        Err(e) => error_response(e),
    }
}

// ── GET /api/files/:collectionId/:recordId/:filename ───────────────────────

/// Query parameters for the file download endpoint.
#[derive(Debug, Deserialize)]
pub struct FileDownloadQuery {
    /// File access token for protected files.
    pub token: Option<String>,
    /// Thumbnail specification (e.g. "100x100", "200x0", "100x100f").
    ///
    /// When provided, generates (or returns cached) a thumbnail at the
    /// requested size. Only supported for image files (JPEG, PNG, GIF, WebP).
    /// Returns 400 for non-image files.
    pub thumb: Option<String>,
    /// Whether to force download (Content-Disposition: attachment).
    pub download: Option<bool>,
}

/// Path parameters for the file download endpoint.
#[derive(Debug, Deserialize)]
pub struct FileDownloadPath {
    pub collection_id: String,
    pub record_id: String,
    pub filename: String,
}

/// Serve a file attached to a record.
///
/// Public files are served directly. Protected files require a valid
/// `?token=` query parameter containing a File-type JWT.
pub async fn serve_file<S: SchemaLookup>(
    Path(path): Path<FileDownloadPath>,
    Query(query): Query<FileDownloadQuery>,
    State(state): State<FileState<S>>,
) -> Response {
    // Look up the collection to check if the file field is protected.
    let is_protected = match check_file_protected(&state, &path) {
        Ok(protected) => protected,
        Err(e) => return error_response(e),
    };

    // If the file is protected, validate the token.
    if is_protected {
        match &query.token {
            Some(token) if !token.is_empty() => {
                match state.token_service.validate(token, TokenType::File) {
                    Ok(_) => { /* Token is valid, proceed */ }
                    Err(_) => {
                        return error_response(ZerobaseError::auth(
                            "invalid or expired file token",
                        ));
                    }
                }
            }
            _ => {
                return error_response(ZerobaseError::auth(
                    "file token is required for protected files",
                ));
            }
        }
    }

    // If a thumbnail is requested, parse the spec and serve it.
    let thumb_spec = match &query.thumb {
        Some(spec) if !spec.is_empty() => match parse_thumb_spec(spec) {
            Some(parsed) => Some(parsed),
            None => {
                return error_response(ZerobaseError::validation(format!(
                    "invalid thumbnail specification: '{spec}'"
                )));
            }
        },
        _ => None,
    };

    // Download the file (or thumbnail) from storage.
    let download = if let Some(ref spec) = thumb_spec {
        match state
            .file_service
            .get_or_generate_thumbnail(&path.collection_id, &path.record_id, &path.filename, spec)
            .await
        {
            Ok(d) => d,
            Err(zerobase_core::storage::StorageError::NotFound { .. }) => {
                return error_response(ZerobaseError::not_found_with_id("File", &path.filename));
            }
            Err(zerobase_core::storage::StorageError::Io { ref message, .. })
                if message.contains("cannot generate thumbnail") =>
            {
                return error_response(ZerobaseError::validation(message.clone()));
            }
            Err(e) => {
                return error_response(ZerobaseError::from(e));
            }
        }
    } else {
        let key = file_key(&path.collection_id, &path.record_id, &path.filename);
        match state.file_service.storage().download(&key).await {
            Ok(d) => d,
            Err(zerobase_core::storage::StorageError::NotFound { .. }) => {
                return error_response(ZerobaseError::not_found_with_id("File", &path.filename));
            }
            Err(e) => {
                return error_response(ZerobaseError::from(e));
            }
        }
    };

    // Determine Content-Type.
    let content_type = if download.metadata.content_type.is_empty() {
        "application/octet-stream".to_string()
    } else {
        download.metadata.content_type.clone()
    };

    // Build response headers.
    let mut headers = vec![
        (header::CONTENT_TYPE, content_type),
        (header::CONTENT_LENGTH, download.data.len().to_string()),
        (header::CACHE_CONTROL, "max-age=604800".to_string()),
    ];

    // Content-Disposition: inline by default, attachment if ?download=true.
    // Sanitize filename to prevent header injection: strip quotes and control chars.
    let safe_filename = sanitize_content_disposition_filename(&path.filename);
    let disposition = if query.download.unwrap_or(false) {
        format!("attachment; filename=\"{safe_filename}\"")
    } else {
        format!("inline; filename=\"{safe_filename}\"")
    };
    headers.push((header::CONTENT_DISPOSITION, disposition));

    // Build the response.
    let mut response = (StatusCode::OK, download.data).into_response();
    for (name, value) in headers {
        if let Ok(v) = value.parse() {
            response.headers_mut().insert(name, v);
        }
    }
    response
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Check if the file field containing the given filename is marked as protected.
///
/// We look up the collection by ID, find the record to determine which field
/// the file belongs to, and check the field's `FileOptions.protected` flag.
///
/// If the collection or field cannot be found (e.g., deleted schema), we
/// default to treating the file as **not protected** — the file still exists
/// in storage and should be servable. This matches PocketBase's behavior
/// where files remain accessible even if the schema changes.
fn check_file_protected<S: SchemaLookup>(
    state: &FileState<S>,
    path: &FileDownloadPath,
) -> Result<bool, ZerobaseError> {
    // Try to find the collection. We search by ID first since the URL
    // uses collection_id, but SchemaLookup.get_collection works with
    // both name and ID depending on implementation.
    let collection = match state.schema_lookup.get_collection(&path.collection_id) {
        Ok(c) => c,
        Err(_) => return Ok(false), // Collection not found — treat as public
    };

    // Check each file field to see if any has the `protected` flag.
    // Since we don't know which field this file belongs to, we check all
    // file fields. If any file field is protected, the file requires a token.
    for field in &collection.fields {
        if let FieldType::File(opts) = &field.field_type {
            if opts.protected {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

/// Sanitize a filename for use inside a `Content-Disposition` header value.
///
/// Removes characters that could break out of the double-quoted `filename="…"`
/// parameter: double quotes, backslashes, newlines, carriage returns, and null
/// bytes. This prevents HTTP response header injection.
fn sanitize_content_disposition_filename(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '"' && *c != '\\' && *c != '\n' && *c != '\r' && *c != '\0')
        .collect()
}

fn error_response(err: ZerobaseError) -> Response {
    let status =
        StatusCode::from_u16(err.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = err.error_response_body();
    (status, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_token_response_serializes() {
        let resp = FileTokenResponse {
            token: "abc.def.ghi".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["token"], "abc.def.ghi");
    }

    #[test]
    fn sanitize_filename_strips_quotes() {
        assert_eq!(
            sanitize_content_disposition_filename("file\"name.txt"),
            "filename.txt"
        );
    }

    #[test]
    fn sanitize_filename_strips_backslashes() {
        assert_eq!(
            sanitize_content_disposition_filename("file\\name.txt"),
            "filename.txt"
        );
    }

    #[test]
    fn sanitize_filename_strips_newlines_and_null() {
        assert_eq!(
            sanitize_content_disposition_filename("file\r\nname\0.txt"),
            "filename.txt"
        );
    }

    #[test]
    fn sanitize_filename_preserves_normal_names() {
        assert_eq!(
            sanitize_content_disposition_filename("photo_2024.jpg"),
            "photo_2024.jpg"
        );
    }

    #[test]
    fn sanitize_filename_header_injection_attempt() {
        // Attempt to inject a second header via CRLF in filename
        let malicious = "file.txt\r\nX-Injected: true";
        let safe = sanitize_content_disposition_filename(malicious);
        assert!(!safe.contains('\r'));
        assert!(!safe.contains('\n'));
    }
}
