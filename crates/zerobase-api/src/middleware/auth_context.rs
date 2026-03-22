//! Auth context extraction from HTTP requests.
//!
//! Provides [`AuthInfo`], an axum extractor that determines the caller's
//! authentication state from the `Authorization` header by validating the
//! JWT token, loading the user record, and checking `tokenKey` invalidation.
//!
//! - **Superuser**: Token issued from the `_superusers` collection.
//! - **Authenticated user**: Valid JWT with matching `tokenKey` in the user record.
//! - **Anonymous**: No `Authorization` header, empty value, or invalid token.
//!
//! Also provides [`RequireAuth`], a stricter extractor that returns 401
//! for unauthenticated requests.
//!
//! # Middleware setup
//!
//! The [`auth_middleware`] function is a tower middleware that performs JWT
//! validation and injects `AuthInfo` into request extensions. Install it
//! on the router so that handlers can extract `AuthInfo` or `RequireAuth`.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::Value as JsonValue;

use zerobase_core::auth::{TokenService, TokenType, ValidatedToken};
use zerobase_core::schema::rule_engine::RequestContext;
use zerobase_core::services::record_service::{RecordRepository, SchemaLookup};
use zerobase_core::services::superuser_service::SUPERUSERS_COLLECTION_NAME;
use zerobase_core::ErrorResponseBody;

// ── AuthInfo ────────────────────────────────────────────────────────────────

/// Authentication information extracted from an HTTP request.
///
/// This is an axum extractor — add it as a handler parameter and the framework
/// will populate it automatically. It is *infallible*: if the auth middleware
/// has not run or the request is unauthenticated, it returns
/// `AuthInfo::anonymous()`.
///
/// For endpoints that *require* authentication, use [`RequireAuth`] instead.
#[derive(Debug, Clone)]
pub struct AuthInfo {
    /// Whether the caller is a superuser (admin).
    pub is_superuser: bool,
    /// Fields from the authenticated user's record.
    /// Empty if anonymous.
    pub auth_record: HashMap<String, JsonValue>,
    /// The validated token claims, if authentication succeeded.
    pub token: Option<ValidatedToken>,
}

impl AuthInfo {
    /// An anonymous (unauthenticated) caller.
    pub fn anonymous() -> Self {
        Self {
            is_superuser: false,
            auth_record: HashMap::new(),
            token: None,
        }
    }

    /// A superuser caller.
    pub fn superuser() -> Self {
        Self {
            is_superuser: true,
            auth_record: HashMap::new(),
            token: None,
        }
    }

    /// A superuser caller with token and record.
    pub fn superuser_with(token: ValidatedToken, record: HashMap<String, JsonValue>) -> Self {
        Self {
            is_superuser: true,
            auth_record: record,
            token: Some(token),
        }
    }

    /// An authenticated (non-superuser) caller with the given record fields.
    pub fn authenticated(record: HashMap<String, JsonValue>) -> Self {
        Self {
            is_superuser: false,
            auth_record: record,
            token: None,
        }
    }

    /// An authenticated caller with token and record.
    pub fn authenticated_with(token: ValidatedToken, record: HashMap<String, JsonValue>) -> Self {
        Self {
            is_superuser: false,
            auth_record: record,
            token: Some(token),
        }
    }

    /// Returns `true` if the caller has a non-empty auth identity.
    pub fn is_authenticated(&self) -> bool {
        self.is_superuser
            || self
                .auth_record
                .get("id")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty())
    }

    /// Build a [`RequestContext`] from this auth info and HTTP request data.
    pub fn to_request_context(
        &self,
        method: &str,
        data: HashMap<String, JsonValue>,
        query: HashMap<String, JsonValue>,
        headers: HashMap<String, JsonValue>,
    ) -> RequestContext {
        RequestContext {
            auth: self.auth_record.clone(),
            data,
            query,
            headers,
            method: method.to_string(),
            context: "default".to_string(),
        }
    }

    /// Build a minimal [`RequestContext`] (used for rule evaluation with
    /// only auth context and HTTP method).
    pub fn to_simple_context(&self, method: &str) -> RequestContext {
        RequestContext {
            auth: self.auth_record.clone(),
            method: method.to_string(),
            context: "default".to_string(),
            ..Default::default()
        }
    }
}

/// Axum extractor implementation.
///
/// **Infallible** — always succeeds. Reads the `AuthInfo` placed in request
/// extensions by [`auth_middleware`]. If the middleware has not run, returns
/// `AuthInfo::anonymous()`.
impl<S: Send + Sync> FromRequestParts<S> for AuthInfo {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(parts
            .extensions
            .get::<AuthInfo>()
            .cloned()
            .unwrap_or_else(AuthInfo::anonymous))
    }
}

// ── RequireAuth ─────────────────────────────────────────────────────────────

/// An axum extractor that *requires* valid authentication.
///
/// Returns 401 Unauthorized if the caller is not authenticated.
/// For endpoints that support optional auth, use [`AuthInfo`] directly.
#[derive(Debug, Clone)]
pub struct RequireAuth(pub AuthInfo);

impl RequireAuth {
    /// Get the inner `AuthInfo`.
    pub fn into_inner(self) -> AuthInfo {
        self.0
    }
}

impl std::ops::Deref for RequireAuth {
    type Target = AuthInfo;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for RequireAuth {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let info = AuthInfo::from_request_parts(parts, state).await.unwrap();

        if info.is_authenticated() {
            Ok(RequireAuth(info))
        } else {
            let body = ErrorResponseBody {
                code: 401,
                message: "The request requires valid authorization token to be set.".to_string(),
                data: HashMap::new(),
            };
            Err((StatusCode::UNAUTHORIZED, Json(body)).into_response())
        }
    }
}

// ── Auth middleware ──────────────────────────────────────────────────────────

/// Extract the raw Bearer token from the `Authorization` header.
fn extract_bearer_token(request: &Request<Body>) -> Option<String> {
    let header_value = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if header_value.is_empty() {
        return None;
    }

    let token = header_value
        .strip_prefix("Bearer ")
        .or_else(|| header_value.strip_prefix("bearer "))
        .unwrap_or(header_value);

    if token.is_empty() {
        return None;
    }

    Some(token.to_string())
}

/// Validate a JWT token and load the corresponding user record.
///
/// Returns `Some(AuthInfo)` on success or `None` if validation fails.
fn validate_and_load<R: RecordRepository, S: SchemaLookup>(
    token_str: &str,
    token_service: &dyn TokenService,
    record_repo: &R,
    schema_lookup: &S,
) -> Option<AuthInfo> {
    // Step 1: Validate JWT signature and expiry.
    let validated = token_service.validate(token_str, TokenType::Auth).ok()?;

    let claims = &validated.claims;

    // Step 2: Resolve collection from the token's collectionId.
    let collection = schema_lookup
        .get_collection_by_id(&claims.collection_id)
        .ok()?;

    let is_superuser = collection.name == SUPERUSERS_COLLECTION_NAME;

    // Step 3: Load the user record.
    let record = record_repo.find_one(&collection.name, &claims.id).ok()?;

    // Step 4: Validate tokenKey (if the record has one).
    // The _superusers table may not have tokenKey; skip the check for those.
    if let Some(stored_key) = record.get("tokenKey").and_then(|v| v.as_str()) {
        if stored_key != claims.token_key {
            return None; // Token has been revoked.
        }
    }

    // Step 5: Strip sensitive fields from the record before storing in AuthInfo.
    let mut auth_record = record;
    auth_record.remove("password");
    auth_record.remove("tokenKey");

    // Add collection context to the auth record for rule evaluation.
    auth_record
        .entry("collectionId".to_string())
        .or_insert_with(|| JsonValue::String(collection.id.clone()));
    auth_record
        .entry("collectionName".to_string())
        .or_insert_with(|| JsonValue::String(collection.name.clone()));

    if is_superuser {
        Some(AuthInfo::superuser_with(validated, auth_record))
    } else {
        Some(AuthInfo::authenticated_with(validated, auth_record))
    }
}

/// Tower middleware that validates JWTs and injects [`AuthInfo`] into request
/// extensions.
///
/// Install this on the router using [`axum::middleware::from_fn_with_state`]
/// with an [`AuthMiddlewareState`]. Handlers can then extract [`AuthInfo`]
/// or [`RequireAuth`] from the request.
pub async fn auth_middleware<R: RecordRepository + 'static, S: SchemaLookup + 'static>(
    axum::extract::State(state): axum::extract::State<Arc<AuthMiddlewareState<R, S>>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let auth_info = match extract_bearer_token(&request) {
        Some(token_str) => validate_and_load(
            &token_str,
            state.token_service.as_ref(),
            state.record_repo.as_ref(),
            state.schema_lookup.as_ref(),
        )
        .unwrap_or_else(AuthInfo::anonymous),
        None => AuthInfo::anonymous(),
    };

    request.extensions_mut().insert(auth_info);
    next.run(request).await
}

/// State required by [`auth_middleware`].
pub struct AuthMiddlewareState<R: RecordRepository, S: SchemaLookup> {
    pub token_service: Arc<dyn TokenService>,
    pub record_repo: Arc<R>,
    pub schema_lookup: Arc<S>,
}

impl<R: RecordRepository, S: SchemaLookup> Clone for AuthMiddlewareState<R, S> {
    fn clone(&self) -> Self {
        Self {
            token_service: Arc::clone(&self.token_service),
            record_repo: Arc::clone(&self.record_repo),
            schema_lookup: Arc::clone(&self.schema_lookup),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anonymous_is_not_authenticated() {
        let info = AuthInfo::anonymous();
        assert!(!info.is_authenticated());
        assert!(!info.is_superuser);
        assert!(info.token.is_none());
    }

    #[test]
    fn superuser_is_authenticated() {
        let info = AuthInfo::superuser();
        assert!(info.is_authenticated());
        assert!(info.is_superuser);
    }

    #[test]
    fn authenticated_user_with_id() {
        let mut record = HashMap::new();
        record.insert("id".to_string(), JsonValue::String("user123".into()));
        let info = AuthInfo::authenticated(record);
        assert!(info.is_authenticated());
        assert!(!info.is_superuser);
    }

    #[test]
    fn simple_context_carries_auth() {
        let mut record = HashMap::new();
        record.insert("id".to_string(), JsonValue::String("u1".into()));
        let info = AuthInfo::authenticated(record);
        let ctx = info.to_simple_context("GET");
        assert!(ctx.is_authenticated());
        assert_eq!(ctx.method, "GET");
    }

    #[test]
    fn require_auth_deref_gives_auth_info() {
        let mut record = HashMap::new();
        record.insert("id".to_string(), JsonValue::String("u1".into()));
        let info = AuthInfo::authenticated(record);
        let required = RequireAuth(info.clone());
        assert!(required.is_authenticated());
        assert_eq!(required.into_inner().is_superuser, info.is_superuser);
    }

    #[test]
    fn anonymous_without_id_is_not_authenticated() {
        let info = AuthInfo::authenticated(HashMap::new());
        assert!(!info.is_authenticated());
    }

    #[test]
    fn empty_id_is_not_authenticated() {
        let mut record = HashMap::new();
        record.insert("id".to_string(), JsonValue::String("".into()));
        let info = AuthInfo::authenticated(record);
        assert!(!info.is_authenticated());
    }
}
