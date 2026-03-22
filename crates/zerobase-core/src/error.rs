//! Unified error types for the Zerobase platform.
//!
//! [`ZerobaseError`] is the canonical error enum used across all crates.
//! It is framework-agnostic — HTTP status code mapping is provided via
//! [`ZerobaseError::status_code`] so that the API layer can convert errors
//! into responses without pulling framework types into the core.

use std::collections::HashMap;

/// Canonical error type for the Zerobase platform.
///
/// Each variant carries enough context for both logging (via `source` chains)
/// and user-facing responses (via `status_code` + `Display`).
#[derive(Debug, thiserror::Error)]
pub enum ZerobaseError {
    // ── Database ────────────────────────────────────────────────────────
    /// A database operation failed (query, migration, connection, etc.).
    #[error("database error: {message}")]
    Database {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    // ── Validation ──────────────────────────────────────────────────────
    /// One or more input fields failed validation.
    #[error("validation error: {message}")]
    Validation {
        message: String,
        /// Per-field error messages (field name → description).
        field_errors: HashMap<String, String>,
    },

    // ── Auth ────────────────────────────────────────────────────────────
    /// Authentication failed — bad credentials, expired token, etc.
    #[error("authentication error: {message}")]
    Auth {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    // ── NotFound ────────────────────────────────────────────────────────
    /// The requested resource does not exist.
    #[error("{resource_type} not found")]
    NotFound {
        resource_type: String,
        /// Optional identifier that was looked up.
        resource_id: Option<String>,
    },

    // ── Forbidden ───────────────────────────────────────────────────────
    /// The caller is authenticated but lacks permission for this action.
    #[error("forbidden: {message}")]
    Forbidden { message: String },

    // ── Conflict ────────────────────────────────────────────────────────
    /// A uniqueness or state-transition constraint was violated.
    #[error("conflict: {message}")]
    Conflict { message: String },

    // ── HookAbort ──────────────────────────────────────────────────────
    /// A JS hook explicitly aborted the request via `e.httpError()`.
    #[error("{message}")]
    HookAbort {
        /// HTTP status code chosen by the hook (e.g. 400, 403, 500).
        status: u16,
        message: String,
    },

    // ── PayloadTooLarge ─────────────────────────────────────────────────
    /// The request body exceeds the configured maximum size.
    #[error("request body too large: {message}")]
    PayloadTooLarge { message: String },

    // ── Internal ────────────────────────────────────────────────────────
    /// An unexpected internal error — always log, never expose details.
    #[error("internal error")]
    Internal {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

impl ZerobaseError {
    /// Map this error to an HTTP status code (as a `u16`).
    ///
    /// This keeps the core crate free of any HTTP framework dependency
    /// while still allowing the API layer to derive a response status.
    pub fn status_code(&self) -> u16 {
        match self {
            Self::Validation { .. } => 400,
            Self::Auth { .. } => 401,
            Self::Forbidden { .. } => 403,
            Self::NotFound { .. } => 404,
            Self::Conflict { .. } => 409,
            Self::PayloadTooLarge { .. } => 413,
            Self::HookAbort { status, .. } => *status,
            Self::Database { .. } | Self::Internal { .. } => 500,
        }
    }

    /// Whether the error details are safe to expose to end users.
    ///
    /// Internal/database errors should be logged but never returned
    /// verbatim in API responses.
    pub fn is_user_facing(&self) -> bool {
        match self {
            Self::Validation { .. }
            | Self::Auth { .. }
            | Self::Forbidden { .. }
            | Self::NotFound { .. }
            | Self::Conflict { .. }
            | Self::PayloadTooLarge { .. }
            | Self::HookAbort { .. } => true,
            Self::Database { .. } | Self::Internal { .. } => false,
        }
    }

    /// Produce a JSON-friendly error body matching PocketBase's format.
    ///
    /// For user-facing errors the real message is returned; for internal
    /// errors a generic message is substituted. Field-level validation
    /// errors are returned in the `data` map as `{ code, message }` objects.
    pub fn error_response_body(&self) -> ErrorResponseBody {
        let (message, data) = if self.is_user_facing() {
            let data = match self {
                Self::Validation { field_errors, .. } if !field_errors.is_empty() => field_errors
                    .iter()
                    .map(|(field, msg)| {
                        (
                            field.clone(),
                            FieldError {
                                code: format!("validation_{field}"),
                                message: msg.clone(),
                            },
                        )
                    })
                    .collect(),
                Self::NotFound {
                    resource_id: Some(id),
                    ..
                } => {
                    let mut map = HashMap::new();
                    map.insert(
                        "id".to_string(),
                        FieldError {
                            code: "not_found".to_string(),
                            message: format!(
                                "The requested resource with ID \"{id}\" was not found."
                            ),
                        },
                    );
                    map
                }
                _ => HashMap::new(),
            };
            (self.to_string(), data)
        } else {
            ("An internal error occurred.".to_string(), HashMap::new())
        };

        ErrorResponseBody {
            code: self.status_code(),
            message,
            data,
        }
    }
}

// ── Convenience constructors ────────────────────────────────────────────────

impl ZerobaseError {
    pub fn database(message: impl Into<String>) -> Self {
        Self::Database {
            message: message.into(),
            source: None,
        }
    }

    pub fn database_with_source(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Database {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation {
            message: message.into(),
            field_errors: HashMap::new(),
        }
    }

    pub fn validation_with_fields(
        message: impl Into<String>,
        field_errors: HashMap<String, String>,
    ) -> Self {
        Self::Validation {
            message: message.into(),
            field_errors,
        }
    }

    pub fn auth(message: impl Into<String>) -> Self {
        Self::Auth {
            message: message.into(),
            source: None,
        }
    }

    pub fn auth_with_source(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Auth {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    pub fn not_found(resource_type: impl Into<String>) -> Self {
        Self::NotFound {
            resource_type: resource_type.into(),
            resource_id: None,
        }
    }

    pub fn not_found_with_id(
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
    ) -> Self {
        Self::NotFound {
            resource_type: resource_type.into(),
            resource_id: Some(resource_id.into()),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden {
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict {
            message: message.into(),
        }
    }

    pub fn hook_abort(status: u16, message: impl Into<String>) -> Self {
        Self::HookAbort {
            status,
            message: message.into(),
        }
    }

    pub fn payload_too_large(message: impl Into<String>) -> Self {
        Self::PayloadTooLarge {
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
            source: None,
        }
    }

    pub fn internal_with_source(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Internal {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }
}

// ── From conversions ────────────────────────────────────────────────────────

impl From<rusqlite::Error> for ZerobaseError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Database {
            message: err.to_string(),
            source: Some(Box::new(err)),
        }
    }
}

impl From<serde_json::Error> for ZerobaseError {
    fn from(err: serde_json::Error) -> Self {
        Self::Validation {
            message: format!("invalid JSON: {err}"),
            field_errors: HashMap::new(),
        }
    }
}

impl From<r2d2::Error> for ZerobaseError {
    fn from(err: r2d2::Error) -> Self {
        Self::Database {
            message: format!("connection pool error: {err}"),
            source: Some(Box::new(err)),
        }
    }
}

// ── Serializable error body ─────────────────────────────────────────────────

/// A single field-level error matching PocketBase's format.
///
/// PocketBase returns field errors as `{ "code": "validation_…", "message": "…" }`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct FieldError {
    pub code: String,
    pub message: String,
}

/// JSON-serializable error body returned to API clients.
///
/// Matches PocketBase's error response format:
/// ```json
/// {
///   "code": 400,
///   "message": "Failed to create record.",
///   "data": {
///     "email": { "code": "validation_required", "message": "Missing required value." }
///   }
/// }
/// ```
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ErrorResponseBody {
    pub code: u16,
    pub message: String,
    pub data: HashMap<String, FieldError>,
}

/// Alias used throughout the crate for fallible operations.
pub type Result<T> = std::result::Result<T, ZerobaseError>;

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Status code mapping ─────────────────────────────────────────────

    #[test]
    fn database_error_maps_to_500() {
        let err = ZerobaseError::database("connection lost");
        assert_eq!(err.status_code(), 500);
    }

    #[test]
    fn validation_error_maps_to_400() {
        let err = ZerobaseError::validation("bad input");
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn auth_error_maps_to_401() {
        let err = ZerobaseError::auth("invalid token");
        assert_eq!(err.status_code(), 401);
    }

    #[test]
    fn not_found_error_maps_to_404() {
        let err = ZerobaseError::not_found("Record");
        assert_eq!(err.status_code(), 404);
    }

    #[test]
    fn forbidden_error_maps_to_403() {
        let err = ZerobaseError::forbidden("insufficient privileges");
        assert_eq!(err.status_code(), 403);
    }

    #[test]
    fn conflict_error_maps_to_409() {
        let err = ZerobaseError::conflict("slug already exists");
        assert_eq!(err.status_code(), 409);
    }

    #[test]
    fn payload_too_large_maps_to_413() {
        let err = ZerobaseError::payload_too_large("body exceeds 10MB limit");
        assert_eq!(err.status_code(), 413);
    }

    #[test]
    fn internal_error_maps_to_500() {
        let err = ZerobaseError::internal("segfault in FFI");
        assert_eq!(err.status_code(), 500);
    }

    // ── Display / user-facing ───────────────────────────────────────────

    #[test]
    fn not_found_display_includes_resource_type() {
        let err = ZerobaseError::not_found("Collection");
        assert_eq!(err.to_string(), "Collection not found");
    }

    #[test]
    fn validation_display_includes_message() {
        let err = ZerobaseError::validation("email is required");
        assert_eq!(err.to_string(), "validation error: email is required");
    }

    // ── is_user_facing ──────────────────────────────────────────────────

    #[test]
    fn internal_errors_are_not_user_facing() {
        assert!(!ZerobaseError::internal("oops").is_user_facing());
        assert!(!ZerobaseError::database("oops").is_user_facing());
    }

    #[test]
    fn client_errors_are_user_facing() {
        assert!(ZerobaseError::validation("x").is_user_facing());
        assert!(ZerobaseError::auth("x").is_user_facing());
        assert!(ZerobaseError::forbidden("x").is_user_facing());
        assert!(ZerobaseError::not_found("x").is_user_facing());
        assert!(ZerobaseError::conflict("x").is_user_facing());
    }

    // ── error_response_body ─────────────────────────────────────────────

    #[test]
    fn internal_error_body_hides_details() {
        let err = ZerobaseError::internal("secret DB password in stack trace");
        let body = err.error_response_body();
        assert_eq!(body.code, 500);
        assert_eq!(body.message, "An internal error occurred.");
        assert!(body.data.is_empty());
    }

    #[test]
    fn validation_error_body_includes_field_errors() {
        let mut fields = HashMap::new();
        fields.insert("email".to_string(), "must be valid".to_string());
        let err = ZerobaseError::validation_with_fields("invalid input", fields);
        let body = err.error_response_body();
        assert_eq!(body.code, 400);
        let email_err = body
            .data
            .get("email")
            .expect("should have email field error");
        assert_eq!(email_err.code, "validation_email");
        assert_eq!(email_err.message, "must be valid");
    }

    #[test]
    fn not_found_body_includes_id_when_present() {
        let err = ZerobaseError::not_found_with_id("Record", "abc123");
        let body = err.error_response_body();
        assert_eq!(body.code, 404);
        let id_err = body.data.get("id").expect("should have id field error");
        assert_eq!(id_err.code, "not_found");
        assert!(id_err.message.contains("abc123"));
    }

    #[test]
    fn error_response_body_serializes_to_json() {
        let err = ZerobaseError::forbidden("not your record");
        let body = err.error_response_body();
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains("\"code\":403"));
        assert!(json.contains("forbidden: not your record"));
    }

    #[test]
    fn error_body_matches_pocketbase_format() {
        let mut fields = HashMap::new();
        fields.insert("title".to_string(), "Missing required value.".to_string());
        let err = ZerobaseError::validation_with_fields("Failed to create record.", fields);
        let body = err.error_response_body();
        let json: serde_json::Value = serde_json::to_value(&body).unwrap();

        // PocketBase format: { code, message, data: { field: { code, message } } }
        assert!(json["code"].is_number());
        assert!(json["message"].is_string());
        assert!(json["data"].is_object());
        assert!(json["data"]["title"].is_object());
        assert!(json["data"]["title"]["code"].is_string());
        assert!(json["data"]["title"]["message"].is_string());
    }

    #[test]
    fn error_body_data_is_empty_object_not_null_when_no_field_errors() {
        let err = ZerobaseError::forbidden("access denied");
        let body = err.error_response_body();
        let json: serde_json::Value = serde_json::to_value(&body).unwrap();
        // PocketBase always returns data as {} not null
        assert!(json["data"].is_object());
    }

    // ── From conversions ────────────────────────────────────────────────

    #[test]
    fn from_serde_json_error() {
        let raw = "{ broken json";
        let json_err = serde_json::from_str::<serde_json::Value>(raw).unwrap_err();
        let err: ZerobaseError = json_err.into();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("invalid JSON"));
    }

    #[test]
    fn from_rusqlite_error() {
        let sqlite_err = rusqlite::Error::QueryReturnedNoRows;
        let err: ZerobaseError = sqlite_err.into();
        assert_eq!(err.status_code(), 500);
        assert!(err.to_string().contains("database error"));
    }

    // ── Source chaining ─────────────────────────────────────────────────

    #[test]
    fn database_error_chains_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
        let err = ZerobaseError::database_with_source("write failed", io_err);
        let source = std::error::Error::source(&err).expect("should have source");
        assert!(source.to_string().contains("pipe broke"));
    }

    #[test]
    fn auth_error_chains_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::TimedOut, "token fetch timeout");
        let err = ZerobaseError::auth_with_source("provider unreachable", io_err);
        let source = std::error::Error::source(&err).expect("should have source");
        assert!(source.to_string().contains("token fetch timeout"));
    }

    // ── Result alias ────────────────────────────────────────────────────

    #[test]
    fn result_alias_works() {
        fn try_op() -> Result<u32> {
            Ok(42)
        }
        assert_eq!(try_op().unwrap(), 42);

        fn try_fail() -> Result<u32> {
            Err(ZerobaseError::internal("boom"))
        }
        assert!(try_fail().is_err());
    }
}
