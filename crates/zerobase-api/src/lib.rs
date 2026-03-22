//! Zerobase API — HTTP layer built on axum.
//!
//! Auto-generates CRUD routes per collection, provides realtime SSE,
//! file serving, filtering, sorting, and pagination.

pub mod handlers;
pub mod middleware;
pub mod response;

use std::sync::Arc;

use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::Level;

use zerobase_db::Database;

use zerobase_auth::{
    EmailChangeService, MfaService, OAuth2Service, OtpService, PasskeyService,
    PasswordResetService, VerificationService,
};
use zerobase_core::auth::TokenService;
use zerobase_core::email::templates::EmailTemplateEngine;
use zerobase_core::email::EmailService;
use zerobase_core::oauth::OAuthProviderRegistry;
use zerobase_core::services::backup_service::BackupRepository;
use zerobase_core::services::collection_service::SchemaRepository;
use zerobase_core::services::external_auth::ExternalAuthRepository;
use zerobase_core::services::log_service::LogRepository;
use zerobase_core::services::record_service::{RecordRepository, SchemaLookup};
use zerobase_core::services::settings_service::SettingsRepository;
use zerobase_core::services::superuser_service::SuperuserRepository;
use zerobase_core::services::webauthn_credential::WebauthnCredentialRepository;
use zerobase_core::{BackupService, CollectionService, LogService, RecordService, SettingsService, SuperuserService};

pub use handlers::admins::AdminAuthState;
pub use handlers::health::{HealthResponse, HealthState};
pub use handlers::auth::AuthState;
pub use handlers::email_change::EmailChangeState;
pub use handlers::external_auths::ExternalAuthState;
pub use handlers::files::FileState;
pub use handlers::mfa::MfaState;
pub use handlers::oauth2::OAuth2State;
pub use handlers::otp::OtpState;
pub use handlers::passkey::PasskeyState;
pub use handlers::password_reset::PasswordResetState;
pub use handlers::batch::BatchState;
pub use handlers::realtime::{
    RealtimeEvent, RealtimeHub, RealtimeHubConfig, RealtimeState, SetSubscriptionsError,
};
pub use handlers::export::ExportState;
pub use handlers::records::RecordState;
pub use handlers::verification::VerificationState;
pub use middleware::auth_context::{AuthInfo, AuthMiddlewareState, RequireAuth};
pub use middleware::rate_limit::{
    CategoryLimit, RateLimitConfig, RateLimiter, RouteCategory, rate_limit_middleware,
};
pub use middleware::body_limit::{BodyLimitConfig, body_limit_middleware};
pub use middleware::cors::build_cors_layer;
pub use middleware::request_logging::request_logging_middleware;
pub use middleware::security_headers::security_headers_middleware;

/// Build the top-level API [`Router`] with all middleware pre-configured.
///
/// This is the main entry point for composing the HTTP layer.  The caller
/// (typically `zerobase-server`) adds application state and a listener.
pub fn api_router() -> Router {
    api_router_with_rate_limit(RateLimitConfig::default())
}

/// Build the top-level API [`Router`] with custom rate-limit configuration.
///
/// Uses a fully permissive CORS policy (allow all origins, methods, headers)
/// and default body limit configuration.
pub fn api_router_with_rate_limit(rate_limit_config: RateLimitConfig) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    api_router_with_rate_limit_and_cors(rate_limit_config, cors)
}

/// Build the top-level API [`Router`] with custom rate-limit and CORS configuration.
///
/// Uses default body limit configuration.
pub fn api_router_with_rate_limit_and_cors(
    rate_limit_config: RateLimitConfig,
    cors_layer: CorsLayer,
) -> Router {
    api_router_full(rate_limit_config, cors_layer, BodyLimitConfig::default())
}

/// Build the top-level API [`Router`] with custom rate-limit, CORS, and body limit.
pub fn api_router_full(
    rate_limit_config: RateLimitConfig,
    cors_layer: CorsLayer,
    body_limit_config: BodyLimitConfig,
) -> Router {
    let limiter = Arc::new(RateLimiter::new(rate_limit_config));
    let body_limit = Arc::new(body_limit_config);

    Router::new()
        .route("/api/health", get(health_check))
        .layer(axum::middleware::from_fn_with_state(
            body_limit,
            middleware::body_limit::body_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            limiter,
            middleware::rate_limit::rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn(
            middleware::request_id::request_id_middleware,
        ))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<_>| {
                let request_id = request
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("-");

                tracing::span!(
                    Level::INFO,
                    "http_request",
                    method = %request.method(),
                    uri = %request.uri(),
                    request_id = %request_id,
                )
            }),
        )
        .layer(cors_layer)
        .layer(axum::middleware::from_fn(
            middleware::security_headers::security_headers_middleware,
        ))
}

/// Build the top-level API [`Router`] with JWT auth middleware pre-configured.
///
/// Like [`api_router`], but includes the auth middleware that validates JWTs
/// and injects [`AuthInfo`] into request extensions. Uses the default
/// [`RateLimitConfig`], default body limits, and a permissive CORS policy.
/// This should be the preferred entry point for production use.
pub fn api_router_with_auth<R: RecordRepository + 'static, S: SchemaLookup + 'static>(
    auth_state: Arc<AuthMiddlewareState<R, S>>,
) -> Router {
    api_router_with_auth_and_rate_limit(auth_state, RateLimitConfig::default())
}

/// Build the top-level API [`Router`] with JWT auth and custom rate limiting.
///
/// Uses a fully permissive CORS policy (allow all origins, methods, headers)
/// and default body limit configuration.
pub fn api_router_with_auth_and_rate_limit<
    R: RecordRepository + 'static,
    S: SchemaLookup + 'static,
>(
    auth_state: Arc<AuthMiddlewareState<R, S>>,
    rate_limit_config: RateLimitConfig,
) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    api_router_with_auth_rate_limit_and_cors(auth_state, rate_limit_config, cors)
}

/// Build the top-level API [`Router`] with JWT auth, custom rate limiting, and CORS.
///
/// Uses default body limit configuration.
pub fn api_router_with_auth_rate_limit_and_cors<
    R: RecordRepository + 'static,
    S: SchemaLookup + 'static,
>(
    auth_state: Arc<AuthMiddlewareState<R, S>>,
    rate_limit_config: RateLimitConfig,
    cors_layer: CorsLayer,
) -> Router {
    api_router_with_auth_full(
        auth_state,
        rate_limit_config,
        cors_layer,
        BodyLimitConfig::default(),
    )
}

/// Build the top-level API [`Router`] with JWT auth, rate limiting, CORS, and body limits.
pub fn api_router_with_auth_full<
    R: RecordRepository + 'static,
    S: SchemaLookup + 'static,
>(
    auth_state: Arc<AuthMiddlewareState<R, S>>,
    rate_limit_config: RateLimitConfig,
    cors_layer: CorsLayer,
    body_limit_config: BodyLimitConfig,
) -> Router {
    let limiter = Arc::new(RateLimiter::new(rate_limit_config));
    let body_limit = Arc::new(body_limit_config);

    Router::new()
        .route("/api/health", get(health_check))
        .layer(axum::middleware::from_fn_with_state(
            body_limit,
            middleware::body_limit::body_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            limiter,
            middleware::rate_limit::rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            middleware::auth_context::auth_middleware::<R, S>,
        ))
        .layer(axum::middleware::from_fn(
            middleware::request_id::request_id_middleware,
        ))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<_>| {
                let request_id = request
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("-");

                tracing::span!(
                    Level::INFO,
                    "http_request",
                    method = %request.method(),
                    uri = %request.uri(),
                    request_id = %request_id,
                )
            }),
        )
        .layer(cors_layer)
        .layer(axum::middleware::from_fn(
            middleware::security_headers::security_headers_middleware,
        ))
}

/// Build collection management routes with superuser auth middleware.
///
/// Returns a [`Router`] scoped to `/api/collections` with CRUD endpoints.
/// The router requires a [`CollectionService`] as state, wrapped in an `Arc`.
///
/// All routes are protected by the [`require_superuser`](middleware::require_superuser)
/// middleware which checks the [`AuthInfo`] in request extensions.
///
/// # Endpoints
///
/// - `GET    /api/collections`             — list all collections
/// - `POST   /api/collections`             — create a new collection
/// - `GET    /api/collections/export`      — export all collection schemas
/// - `PUT    /api/collections/import`      — import collection schemas
/// - `GET    /api/collections/:id_or_name` — view a collection
/// - `PATCH  /api/collections/:id_or_name` — update a collection
/// - `DELETE /api/collections/:id_or_name` — delete a collection
pub fn collection_routes<R: SchemaRepository + 'static>(
    service: Arc<CollectionService<R>>,
) -> Router {
    use handlers::collections::*;

    Router::new()
        .route(
            "/api/collections",
            get(list_collections::<R>).post(create_collection::<R>),
        )
        // Import/export routes must be registered before the `{id_or_name}` wildcard
        // to avoid "export" and "import" being captured as collection names.
        .route("/api/collections/export", get(export_collections::<R>))
        .route("/api/collections/import", put(import_collections::<R>))
        .route(
            "/api/collections/{id_or_name}",
            get(view_collection::<R>)
                .patch(update_collection::<R>)
                .delete(delete_collection::<R>),
        )
        // Index management routes
        .route(
            "/api/collections/{id_or_name}/indexes",
            get(list_indexes::<R>).post(add_index::<R>),
        )
        .route(
            "/api/collections/{id_or_name}/indexes/{index_pos}",
            delete(remove_index::<R>),
        )
        .layer(axum::middleware::from_fn(
            middleware::require_superuser::require_superuser,
        ))
        .with_state(service)
}

/// Build settings management routes with superuser auth middleware.
///
/// Returns a [`Router`] scoped to `/api/settings` with read/write endpoints.
/// The router requires a [`SettingsService`] as state, wrapped in an `Arc`.
///
/// All routes are protected by the [`require_superuser`](middleware::require_superuser)
/// middleware.
///
/// # Endpoints
///
/// - `GET    /api/settings`            — get all settings
/// - `PATCH  /api/settings`            — update settings (partial)
/// - `POST   /api/settings/test-email` — send a test email
/// - `GET    /api/settings/:key`       — get a single setting by key
/// - `DELETE /api/settings/:key`       — reset a single setting to default
pub fn settings_routes<R: SettingsRepository + 'static>(
    service: Arc<SettingsService<R>>,
    email_service: Arc<dyn EmailService>,
) -> Router {
    use handlers::settings::*;

    let test_email_state = TestEmailState {
        settings_service: Arc::clone(&service),
        email_service,
    };

    // Settings CRUD routes (state: SettingsService)
    let crud_routes = Router::new()
        .route(
            "/api/settings",
            get(get_all_settings::<R>).patch(update_settings::<R>),
        )
        .route(
            "/api/settings/{key}",
            get(get_setting::<R>).delete(delete_setting::<R>),
        )
        .layer(axum::middleware::from_fn(
            middleware::require_superuser::require_superuser,
        ))
        .with_state(service);

    // Test-email route (state: TestEmailState)
    let test_email_route = Router::new()
        .route(
            "/api/settings/test-email",
            post(test_email::<R>),
        )
        .layer(axum::middleware::from_fn(
            middleware::require_superuser::require_superuser,
        ))
        .with_state(test_email_state);

    // Merge both — test-email route must come first to match before {key}
    test_email_route.merge(crud_routes)
}

/// Build batch operation routes.
///
/// Returns a [`Router`] with the batch endpoint for executing multiple
/// record operations atomically.
///
/// # Endpoints
///
/// - `POST /api/batch` — execute a batch of record operations
pub fn batch_routes<R: RecordRepository + 'static, S: SchemaLookup + 'static>(
    service: Arc<RecordService<R, S>>,
) -> Router {
    use handlers::batch::*;

    let state = BatchState {
        record_service: service,
    };

    Router::new()
        .route("/api/batch", post(execute_batch::<R, S>))
        .with_state(state)
}

/// Build data export routes (superuser-only).
///
/// Returns a [`Router`] with an endpoint for exporting collection records
/// as CSV or JSON. All routes are protected by superuser middleware.
///
/// # Endpoints
///
/// - `GET /_/api/collections/:collection/export` — export records (CSV or JSON)
pub fn export_routes<R: RecordRepository + 'static, S: SchemaLookup + 'static>(
    service: Arc<RecordService<R, S>>,
) -> Router {
    use handlers::export::*;

    let state = ExportState {
        record_service: service,
    };

    Router::new()
        .route(
            "/_/api/collections/{collection_name}/export",
            get(export_records::<R, S>),
        )
        .layer(axum::middleware::from_fn(
            middleware::require_superuser::require_superuser,
        ))
        .with_state(state)
}

/// Build record CRUD routes for all collections.
///
/// Returns a [`Router`] with endpoints for listing, viewing, creating, updating,
/// counting, and deleting records in any collection. The collection name is a path
/// parameter.
///
/// # Endpoints
///
/// - `GET    /api/collections/:collection/records`       — list records
/// - `POST   /api/collections/:collection/records`       — create a record
/// - `GET    /api/collections/:collection/records/count` — count records
/// - `GET    /api/collections/:collection/records/:id`   — view a record
/// - `PATCH  /api/collections/:collection/records/:id`   — update a record
/// - `DELETE /api/collections/:collection/records/:id`   — delete a record
pub fn record_routes<R: RecordRepository + 'static, S: SchemaLookup + 'static>(
    service: Arc<RecordService<R, S>>,
) -> Router {
    record_routes_full(service, None, None)
}

/// Build record CRUD routes with optional file upload support.
///
/// When a [`FileService`] is provided, create/update endpoints accept
/// `multipart/form-data` with file uploads, and delete cleans up files.
pub fn record_routes_with_files<R: RecordRepository + 'static, S: SchemaLookup + 'static>(
    service: Arc<RecordService<R, S>>,
    file_service: Option<Arc<zerobase_files::FileService>>,
) -> Router {
    record_routes_full(service, file_service, None)
}

/// Build record CRUD routes with optional file upload and realtime support.
///
/// When a [`RealtimeHub`] is provided, record create/update/delete operations
/// broadcast events to subscribed SSE clients with per-client access rule filtering.
pub fn record_routes_full<R: RecordRepository + 'static, S: SchemaLookup + 'static>(
    service: Arc<RecordService<R, S>>,
    file_service: Option<Arc<zerobase_files::FileService>>,
    realtime_hub: Option<RealtimeHub>,
) -> Router {
    use handlers::records::*;

    let state = RecordState {
        record_service: service,
        file_service,
        realtime_hub,
    };

    Router::new()
        .route(
            "/api/collections/{collection_name}/records",
            get(list_records::<R, S>).post(create_record::<R, S>),
        )
        // Count route must be registered before the `{id}` wildcard
        // to avoid "count" being captured as a record ID.
        .route(
            "/api/collections/{collection_name}/records/count",
            get(count_records::<R, S>),
        )
        .route(
            "/api/collections/{collection_name}/records/{id}",
            get(view_record::<R, S>)
                .patch(update_record::<R, S>)
                .delete(delete_record::<R, S>),
        )
        .with_state(state)
}

/// Build authentication routes for auth collections.
///
/// Returns a [`Router`] with authentication endpoints.
///
/// # Endpoints
///
/// - `POST /api/collections/:collection/auth-with-password` — email/password login
/// - `POST /api/collections/:collection/auth-refresh` — refresh auth token
pub fn auth_routes<R: RecordRepository + 'static, S: SchemaLookup + 'static>(
    record_service: Arc<RecordService<R, S>>,
    token_service: Arc<dyn TokenService>,
) -> Router {
    use handlers::auth::*;

    let state = AuthState {
        record_service,
        token_service,
    };

    Router::new()
        .route(
            "/api/collections/{collection_name}/auth-with-password",
            post(auth_with_password::<R, S>),
        )
        .route(
            "/api/collections/{collection_name}/auth-refresh",
            post(auth_refresh::<R, S>),
        )
        .with_state(state)
}

/// Build email verification routes for auth collections.
///
/// Returns a [`Router`] with verification endpoints.
///
/// # Endpoints
///
/// - `POST /api/collections/:collection/request-verification` — send verification email
/// - `POST /api/collections/:collection/confirm-verification` — confirm email with token
pub fn verification_routes<R: RecordRepository + 'static, S: SchemaLookup + 'static>(
    record_service: Arc<RecordService<R, S>>,
    token_service: Arc<dyn TokenService>,
    email_service: Arc<dyn EmailService>,
    template_engine: EmailTemplateEngine,
    app_url: String,
) -> Router {
    use handlers::verification::*;

    let verification_service = Arc::new(VerificationService::new(
        record_service,
        token_service,
        email_service,
        template_engine,
        app_url,
    ));

    let state = VerificationState {
        verification_service,
    };

    Router::new()
        .route(
            "/api/collections/{collection_name}/request-verification",
            post(request_verification::<R, S>),
        )
        .route(
            "/api/collections/{collection_name}/confirm-verification",
            post(confirm_verification::<R, S>),
        )
        .with_state(state)
}

/// Build password reset routes for auth collections.
///
/// Returns a [`Router`] with password reset endpoints.
///
/// # Endpoints
///
/// - `POST /api/collections/:collection/request-password-reset` — send reset email
/// - `POST /api/collections/:collection/confirm-password-reset` — set new password with token
pub fn password_reset_routes<R: RecordRepository + 'static, S: SchemaLookup + 'static>(
    record_service: Arc<RecordService<R, S>>,
    token_service: Arc<dyn TokenService>,
    email_service: Arc<dyn EmailService>,
    template_engine: EmailTemplateEngine,
    app_url: String,
) -> Router {
    use handlers::password_reset::*;

    let password_reset_service = Arc::new(PasswordResetService::new(
        record_service,
        token_service,
        email_service,
        template_engine,
        app_url,
    ));

    let state = PasswordResetState {
        password_reset_service,
    };

    Router::new()
        .route(
            "/api/collections/{collection_name}/request-password-reset",
            post(request_password_reset::<R, S>),
        )
        .route(
            "/api/collections/{collection_name}/confirm-password-reset",
            post(confirm_password_reset::<R, S>),
        )
        .with_state(state)
}

/// Build email change routes for auth collections.
///
/// Returns a [`Router`] with email change endpoints.
///
/// # Endpoints
///
/// - `POST /api/collections/:collection/request-email-change` — send confirmation to new email
/// - `POST /api/collections/:collection/confirm-email-change` — confirm email change with token
pub fn email_change_routes<R: RecordRepository + 'static, S: SchemaLookup + 'static>(
    record_service: Arc<RecordService<R, S>>,
    token_service: Arc<dyn TokenService>,
    email_service: Arc<dyn EmailService>,
    template_engine: EmailTemplateEngine,
    app_url: String,
) -> Router {
    use handlers::email_change::*;

    let email_change_service = Arc::new(EmailChangeService::new(
        record_service,
        token_service,
        email_service,
        template_engine,
        app_url,
    ));

    let state = EmailChangeState {
        email_change_service,
    };

    Router::new()
        .route(
            "/api/collections/{collection_name}/request-email-change",
            post(request_email_change::<R, S>),
        )
        .route(
            "/api/collections/{collection_name}/confirm-email-change",
            post(confirm_email_change::<R, S>),
        )
        .with_state(state)
}

/// Build OTP (One-Time Password) authentication routes for auth collections.
///
/// Returns a [`Router`] with OTP endpoints.
///
/// # Endpoints
///
/// - `POST /api/collections/:collection/request-otp` — send OTP code via email
/// - `POST /api/collections/:collection/auth-with-otp` — verify OTP and get auth token
pub fn otp_routes<R: RecordRepository + 'static, S: SchemaLookup + 'static>(
    record_service: Arc<RecordService<R, S>>,
    token_service: Arc<dyn TokenService>,
    email_service: Arc<dyn EmailService>,
    template_engine: EmailTemplateEngine,
) -> Router {
    use handlers::otp::*;

    let otp_service = Arc::new(OtpService::new(
        record_service,
        token_service,
        email_service,
        template_engine,
    ));

    let state = OtpState { otp_service };

    Router::new()
        .route(
            "/api/collections/{collection_name}/request-otp",
            post(request_otp::<R, S>),
        )
        .route(
            "/api/collections/{collection_name}/auth-with-otp",
            post(auth_with_otp::<R, S>),
        )
        .with_state(state)
}

/// Build MFA (Multi-Factor Authentication) routes for auth collections.
///
/// Returns a [`Router`] with MFA endpoints.
///
/// # Endpoints
///
/// - `POST /api/collections/:collection/records/:id/request-mfa-setup` — begin MFA setup
/// - `POST /api/collections/:collection/records/:id/confirm-mfa` — confirm MFA with TOTP code
/// - `POST /api/collections/:collection/auth-with-mfa` — verify TOTP/recovery code for auth
pub fn mfa_routes<R: RecordRepository + 'static, S: SchemaLookup + 'static>(
    record_service: Arc<RecordService<R, S>>,
    token_service: Arc<dyn TokenService>,
) -> Router {
    use handlers::mfa::*;

    let mfa_service = Arc::new(MfaService::new(
        record_service,
        token_service,
        "Zerobase".to_string(),
    ));

    let state = MfaState { mfa_service };

    Router::new()
        .route(
            "/api/collections/{collection_name}/records/{id}/request-mfa-setup",
            post(request_mfa_setup::<R, S>),
        )
        .route(
            "/api/collections/{collection_name}/records/{id}/confirm-mfa",
            post(confirm_mfa::<R, S>),
        )
        .route(
            "/api/collections/{collection_name}/auth-with-mfa",
            post(auth_with_mfa::<R, S>),
        )
        .with_state(state)
}

/// Build Passkey/WebAuthn authentication routes for auth collections.
///
/// Returns a [`Router`] with passkey endpoints.
///
/// # Endpoints
///
/// - `POST /api/collections/:collection/request-passkey-register` — begin registration
/// - `POST /api/collections/:collection/confirm-passkey-register` — complete registration
/// - `POST /api/collections/:collection/auth-with-passkey-begin` — begin authentication
/// - `POST /api/collections/:collection/auth-with-passkey-finish` — complete authentication
pub fn passkey_routes<
    R: RecordRepository + 'static,
    S: SchemaLookup + 'static,
    W: WebauthnCredentialRepository + 'static,
>(
    passkey_service: Arc<PasskeyService<R, S, W>>,
) -> Router {
    use handlers::passkey::*;

    let state = PasskeyState { passkey_service };

    Router::new()
        .route(
            "/api/collections/{collection_name}/request-passkey-register",
            post(request_passkey_register::<R, S, W>),
        )
        .route(
            "/api/collections/{collection_name}/confirm-passkey-register",
            post(confirm_passkey_register::<R, S, W>),
        )
        .route(
            "/api/collections/{collection_name}/auth-with-passkey-begin",
            post(auth_with_passkey_begin::<R, S, W>),
        )
        .route(
            "/api/collections/{collection_name}/auth-with-passkey-finish",
            post(auth_with_passkey_finish::<R, S, W>),
        )
        .with_state(state)
}

/// Build OAuth2 authentication routes for auth collections.
///
/// Returns a [`Router`] with OAuth2 endpoints.
///
/// # Endpoints
///
/// - `GET  /api/collections/:collection/auth-methods` — list enabled auth methods
/// - `POST /api/collections/:collection/auth-with-oauth2` — complete OAuth2 flow
pub fn oauth2_routes<
    R: RecordRepository + 'static,
    S: SchemaLookup + 'static,
    E: ExternalAuthRepository + 'static,
>(
    record_service: Arc<RecordService<R, S>>,
    token_service: Arc<dyn TokenService>,
    external_auth_repo: Arc<E>,
    provider_registry: Arc<OAuthProviderRegistry>,
) -> Router {
    use handlers::oauth2::*;

    let oauth2_service = Arc::new(OAuth2Service::new(
        record_service,
        token_service,
        external_auth_repo,
        provider_registry,
    ));

    let state = OAuth2State { oauth2_service };

    Router::new()
        .route(
            "/api/collections/{collection_name}/auth-methods",
            get(list_auth_methods::<R, S, E>),
        )
        .route(
            "/api/collections/{collection_name}/auth-with-oauth2",
            post(auth_with_oauth2::<R, S, E>),
        )
        .with_state(state)
}

/// Build external auth identity routes for auth collections.
///
/// Returns a [`Router`] with endpoints for listing and unlinking external
/// OAuth2 identities linked to a record.
///
/// # Endpoints
///
/// - `GET    /api/collections/:collection/records/:id/external-auths`           — list linked identities
/// - `DELETE /api/collections/:collection/records/:id/external-auths/:provider` — unlink a provider
pub fn external_auth_routes<
    R: RecordRepository + 'static,
    S: SchemaLookup + 'static,
    E: ExternalAuthRepository + 'static,
>(
    record_repo: Arc<R>,
    schema_lookup: Arc<S>,
    external_auth_repo: Arc<E>,
) -> Router {
    use handlers::external_auths::*;

    let state = ExternalAuthState {
        record_repo,
        schema_lookup,
        external_auth_repo,
    };

    Router::new()
        .route(
            "/api/collections/{collection_name}/records/{id}/external-auths",
            get(list_external_auths::<R, S, E>),
        )
        .route(
            "/api/collections/{collection_name}/records/{id}/external-auths/{provider}",
            delete(unlink_external_auth::<R, S, E>),
        )
        .with_state(state)
}

/// Build file download and serving routes.
///
/// Returns a [`Router`] with endpoints for generating file access tokens
/// and serving file downloads.
///
/// # Endpoints
///
/// - `GET /api/files/token`                                  — generate short-lived file token
/// - `GET /api/files/:collectionId/:recordId/:filename`      — serve a file
pub fn file_routes<S: SchemaLookup + 'static>(
    file_service: Arc<zerobase_files::FileService>,
    token_service: Arc<dyn TokenService>,
    schema_lookup: Arc<S>,
) -> Router {
    use handlers::files::*;

    let state = FileState {
        file_service,
        token_service,
        schema_lookup,
    };

    Router::new()
        // Token endpoint must be registered before the wildcard path
        // to avoid "token" being captured as a collection ID.
        .route("/api/files/token", get(request_file_token::<S>))
        .route(
            "/api/files/{collection_id}/{record_id}/{filename}",
            get(serve_file::<S>),
        )
        .with_state(state)
}

/// Build admin (superuser) authentication routes.
///
/// Returns a [`Router`] with the superuser auth-with-password endpoint.
///
/// # Endpoints
///
/// - `POST /_/api/admins/auth-with-password` — superuser email/password login
pub fn admin_routes<R: SuperuserRepository + 'static>(
    superuser_service: Arc<SuperuserService<R>>,
    token_service: Arc<dyn TokenService>,
) -> Router {
    use handlers::admins::*;

    let state = AdminAuthState {
        superuser_service,
        token_service,
    };

    Router::new()
        .route(
            "/_/api/admins/auth-with-password",
            post(admin_auth_with_password::<R>),
        )
        .with_state(state)
}

/// Build backup management routes with superuser auth middleware.
///
/// Returns a [`Router`] scoped to `/_/api/backups` with backup CRUD endpoints.
/// The router requires a [`BackupService`] as state, wrapped in an `Arc`.
///
/// All routes are protected by the [`require_superuser`](middleware::require_superuser)
/// middleware.
///
/// # Endpoints
///
/// - `POST   /_/api/backups`                — create a new backup
/// - `GET    /_/api/backups`                — list all backups
/// - `GET    /_/api/backups/:name`          — download a backup file
/// - `DELETE /_/api/backups/:name`          — delete a backup
/// - `POST   /_/api/backups/:name/restore` — restore from a backup
pub fn backup_routes<R: BackupRepository + 'static>(
    service: Arc<BackupService<R>>,
) -> Router {
    use handlers::backups::*;

    Router::new()
        .route(
            "/_/api/backups",
            get(list_backups::<R>).post(create_backup::<R>),
        )
        // Restore route must be registered before the `{name}` wildcard
        // to avoid "restore" being captured as a backup name... but actually
        // restore is under /{name}/restore, so the wildcard works naturally.
        .route(
            "/_/api/backups/{name}/restore",
            post(restore_backup::<R>),
        )
        .route(
            "/_/api/backups/{name}",
            get(download_backup::<R>).delete(delete_backup::<R>),
        )
        .layer(axum::middleware::from_fn(
            middleware::require_superuser::require_superuser,
        ))
        .with_state(service)
}

/// Build log management routes with superuser auth middleware.
///
/// Returns a [`Router`] scoped to `/_/api/logs` with query endpoints.
///
/// All routes are protected by the [`require_superuser`](middleware::require_superuser)
/// middleware.
///
/// # Endpoints
///
/// - `GET /_/api/logs`       — list logs with filtering and pagination
/// - `GET /_/api/logs/stats` — aggregate log statistics
/// - `GET /_/api/logs/:id`   — view a single log entry
pub fn log_routes<R: LogRepository + 'static>(
    service: Arc<LogService<R>>,
) -> Router {
    use handlers::logs::*;

    Router::new()
        .route("/_/api/logs", get(list_logs::<R>))
        // Stats route must be registered before the `{id}` wildcard.
        .route("/_/api/logs/stats", get(log_stats::<R>))
        .route("/_/api/logs/{id}", get(get_log::<R>))
        .layer(axum::middleware::from_fn(
            middleware::require_superuser::require_superuser,
        ))
        .with_state(service)
}

/// Build realtime SSE routes.
///
/// Returns a [`Router`] with the SSE connection and subscription management
/// endpoints.
///
/// # Endpoints
///
/// - `GET  /api/realtime` — establish an SSE connection
/// - `POST /api/realtime` — set subscriptions for a connected client
pub fn realtime_routes(hub: RealtimeHub) -> Router {
    use handlers::realtime::*;

    let state = RealtimeState { hub };

    Router::new()
        .route(
            "/api/realtime",
            get(sse_connect).post(set_subscriptions),
        )
        .with_state(state)
}

/// Build OpenAPI documentation routes.
///
/// Returns a [`Router`] with endpoints for the OpenAPI spec and Swagger UI.
///
/// # Endpoints
///
/// - `GET /_/api/docs`              — Swagger UI
/// - `GET /_/api/docs/openapi.json` — OpenAPI 3.1.0 JSON specification
pub fn openapi_routes<R: SchemaRepository + 'static>(
    collection_service: Arc<CollectionService<R>>,
) -> Router {
    use handlers::openapi::*;

    Router::new()
        .route("/_/api/docs", get(swagger_ui))
        .route("/_/api/docs/openapi.json", get(openapi_spec::<R>))
        .with_state(collection_service)
}

/// Minimal health check endpoint (no DB state).
async fn health_check() -> Json<handlers::health::HealthResponse> {
    handlers::health::health_check_simple().await
}

/// Build the top-level API [`Router`] with a [`Database`] for health checks.
///
/// The `/api/health` endpoint includes detailed database diagnostics
/// (pool utilization, latency, exhaustion detection). For routers without
/// a database reference, use [`api_router`] instead.
pub fn api_router_with_db(db: Arc<Database>) -> Router {
    api_router_with_db_full(db, RateLimitConfig::default(), default_cors(), BodyLimitConfig::default())
}

/// Build the top-level API [`Router`] with a [`Database`] and full configuration.
pub fn api_router_with_db_full(
    db: Arc<Database>,
    rate_limit_config: RateLimitConfig,
    cors_layer: CorsLayer,
    body_limit_config: BodyLimitConfig,
) -> Router {
    let limiter = Arc::new(RateLimiter::new(rate_limit_config));
    let body_limit = Arc::new(body_limit_config);
    let health_state = HealthState { db };

    Router::new()
        .route(
            "/api/health",
            get(handlers::health::health_check_with_db).with_state(health_state),
        )
        .layer(axum::middleware::from_fn_with_state(
            body_limit,
            middleware::body_limit::body_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            limiter,
            middleware::rate_limit::rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn(
            middleware::request_id::request_id_middleware,
        ))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<_>| {
                let request_id = request
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("-");

                tracing::span!(
                    Level::INFO,
                    "http_request",
                    method = %request.method(),
                    uri = %request.uri(),
                    request_id = %request_id,
                )
            }),
        )
        .layer(cors_layer)
}

/// Build the top-level API [`Router`] with DB health, JWT auth, and full configuration.
pub fn api_router_with_db_and_auth_full<
    R: RecordRepository + 'static,
    S: SchemaLookup + 'static,
>(
    db: Arc<Database>,
    auth_state: Arc<AuthMiddlewareState<R, S>>,
    rate_limit_config: RateLimitConfig,
    cors_layer: CorsLayer,
    body_limit_config: BodyLimitConfig,
) -> Router {
    let limiter = Arc::new(RateLimiter::new(rate_limit_config));
    let body_limit = Arc::new(body_limit_config);
    let health_state = HealthState { db };

    Router::new()
        .route(
            "/api/health",
            get(handlers::health::health_check_with_db).with_state(health_state),
        )
        .layer(axum::middleware::from_fn_with_state(
            body_limit,
            middleware::body_limit::body_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            limiter,
            middleware::rate_limit::rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            middleware::auth_context::auth_middleware::<R, S>,
        ))
        .layer(axum::middleware::from_fn(
            middleware::request_id::request_id_middleware,
        ))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<_>| {
                let request_id = request
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("-");

                tracing::span!(
                    Level::INFO,
                    "http_request",
                    method = %request.method(),
                    uri = %request.uri(),
                    request_id = %request_id,
                )
            }),
        )
        .layer(cors_layer)
}

fn default_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}
