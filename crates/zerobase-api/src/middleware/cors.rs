//! CORS middleware builder.
//!
//! Builds a [`CorsLayer`] from [`CorsSettingsDto`] configuration stored in the
//! settings service. When CORS settings are disabled (or absent), a permissive
//! default layer is returned.

use std::time::Duration;

use axum::http::{HeaderName, Method};
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, Any, CorsLayer};

use zerobase_core::services::settings_service::CorsSettingsDto;

/// Build a [`CorsLayer`] from the given CORS settings.
///
/// When `settings.enabled` is `false`, returns a fully permissive layer
/// (allow all origins, methods, and headers) suitable for development.
pub fn build_cors_layer(settings: &CorsSettingsDto) -> CorsLayer {
    if !settings.enabled {
        return CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);
    }

    let mut layer = CorsLayer::new();

    // ── Origins ──────────────────────────────────────────────────────────
    layer = if settings.allowed_origins.iter().any(|o| o == "*") {
        layer.allow_origin(Any)
    } else {
        let origins: Vec<_> = settings
            .allowed_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        layer.allow_origin(AllowOrigin::list(origins))
    };

    // When credentials are enabled, wildcards are not allowed per the CORS
    // spec (and tower-http enforces this at runtime). Expand wildcards to
    // explicit lists in that case.
    let credentials = settings.allow_credentials;

    // ── Methods ──────────────────────────────────────────────────────────
    let methods_is_wildcard = settings.allowed_methods.iter().any(|m| m == "*");
    layer = if methods_is_wildcard && !credentials {
        layer.allow_methods(Any)
    } else {
        let methods: Vec<Method> = if methods_is_wildcard {
            // Expand wildcard to explicit list for credentials mode
            vec![
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
                Method::HEAD,
            ]
        } else {
            settings
                .allowed_methods
                .iter()
                .filter_map(|m| m.parse().ok())
                .collect()
        };
        layer.allow_methods(AllowMethods::list(methods))
    };

    // ── Headers ──────────────────────────────────────────────────────────
    let headers_is_wildcard = settings.allowed_headers.iter().any(|h| h == "*");
    layer = if headers_is_wildcard && !credentials {
        layer.allow_headers(Any)
    } else {
        let headers: Vec<HeaderName> = if headers_is_wildcard {
            // Common headers to allow when credentials mode prevents wildcard
            vec![
                HeaderName::from_static("content-type"),
                HeaderName::from_static("authorization"),
                HeaderName::from_static("accept"),
                HeaderName::from_static("origin"),
                HeaderName::from_static("x-requested-with"),
            ]
        } else {
            settings
                .allowed_headers
                .iter()
                .filter_map(|h| h.parse().ok())
                .collect()
        };
        layer.allow_headers(AllowHeaders::list(headers))
    };

    // ── Exposed headers ──────────────────────────────────────────────────
    if !settings.exposed_headers.is_empty() {
        let headers: Vec<HeaderName> = settings
            .exposed_headers
            .iter()
            .filter_map(|h| h.parse().ok())
            .collect();
        layer = layer.expose_headers(headers);
    }

    // ── Credentials ──────────────────────────────────────────────────────
    if settings.allow_credentials {
        layer = layer.allow_credentials(true);
    }

    // ── Max age ──────────────────────────────────────────────────────────
    layer = layer.max_age(Duration::from_secs(settings.max_age));

    layer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_returns_permissive_layer() {
        let settings = CorsSettingsDto::default();
        assert!(!settings.enabled);
        // Should not panic
        let _layer = build_cors_layer(&settings);
    }

    #[test]
    fn enabled_with_specific_origins() {
        let settings = CorsSettingsDto {
            enabled: true,
            allowed_origins: vec![
                "https://example.com".to_string(),
                "https://app.example.com".to_string(),
            ],
            ..Default::default()
        };
        let _layer = build_cors_layer(&settings);
    }

    #[test]
    fn enabled_with_wildcard() {
        let settings = CorsSettingsDto {
            enabled: true,
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec!["*".to_string()],
            allowed_headers: vec!["*".to_string()],
            ..Default::default()
        };
        let _layer = build_cors_layer(&settings);
    }

    #[test]
    fn enabled_with_credentials() {
        let settings = CorsSettingsDto {
            enabled: true,
            allowed_origins: vec!["https://example.com".to_string()],
            allow_credentials: true,
            ..Default::default()
        };
        let _layer = build_cors_layer(&settings);
    }

    #[test]
    fn enabled_with_exposed_headers() {
        let settings = CorsSettingsDto {
            enabled: true,
            exposed_headers: vec![
                "X-Custom-Header".to_string(),
                "X-Request-Id".to_string(),
            ],
            ..Default::default()
        };
        let _layer = build_cors_layer(&settings);
    }

    #[test]
    fn custom_max_age() {
        let settings = CorsSettingsDto {
            enabled: true,
            max_age: 3600,
            ..Default::default()
        };
        let _layer = build_cors_layer(&settings);
    }
}
