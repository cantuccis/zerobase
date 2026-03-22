//! OAuth2 provider abstractions.
//!
//! Defines the [`OAuthProvider`] trait for implementing OAuth2 authorization
//! code flow providers, and the [`OAuthProviderRegistry`] for dynamic provider
//! lookup. Concrete implementations live in `zerobase-auth`.
//!
//! # Architecture
//!
//! Adding a new OAuth2 provider requires:
//! 1. Implement [`OAuthProvider`] for your provider type.
//! 2. Register it with [`OAuthProviderRegistry::register`].
//!
//! The registry uses a factory pattern: each provider is registered by name
//! and can be looked up at runtime. This supports PocketBase's model where
//! OAuth2 providers are configured per-collection.
//!
//! # Authorization Code Flow
//!
//! ```text
//! ┌──────────┐     ┌──────────┐     ┌──────────────┐
//! │  Client   │     │ Zerobase │     │  OAuth2      │
//! │ (browser) │     │  Server  │     │  Provider    │
//! └────┬─────┘     └────┬─────┘     └──────┬───────┘
//!      │  1. Start       │                   │
//!      │─────────────────>                   │
//!      │  2. auth_url()  │                   │
//!      │<─────────────────                   │
//!      │  3. Redirect    │                   │
//!      │─────────────────────────────────────>
//!      │  4. User consents                   │
//!      │<─────────────────────────────────────
//!      │  5. Callback with code              │
//!      │─────────────────>                   │
//!      │  6. exchange_code()                 │
//!      │                 │───────────────────>
//!      │                 │<───────────────────
//!      │  7. get_user_info()                 │
//!      │                 │───────────────────>
//!      │                 │<───────────────────
//!      │  8. Auth token  │                   │
//!      │<─────────────────                   │
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::ZerobaseError;

// ── OAuth2 types ────────────────────────────────────────────────────────────

/// Configuration for an OAuth2 provider instance.
///
/// Stores the client credentials and endpoint URLs needed to perform
/// the authorization code flow. Each provider type knows its default
/// endpoints, but they can be overridden here for self-hosted identity
/// servers (e.g., on-premise GitLab or Keycloak).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthProviderConfig {
    /// The OAuth2 client ID issued by the provider.
    pub client_id: String,
    /// The OAuth2 client secret issued by the provider.
    pub client_secret: String,
    /// Optional override for the authorization endpoint URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_url: Option<String>,
    /// Optional override for the token endpoint URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_url: Option<String>,
    /// Optional override for the user info endpoint URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_info_url: Option<String>,
    /// Additional scopes to request beyond the provider defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_scopes: Vec<String>,
}

/// The response from [`OAuthProvider::auth_url`].
///
/// Contains the URL to redirect the user to and an optional PKCE code
/// verifier that must be stored (e.g., in session state) and passed back
/// during [`OAuthProvider::exchange_code`].
#[derive(Debug, Clone)]
pub struct AuthUrlResponse {
    /// The full authorization URL including query parameters.
    pub url: String,
    /// The OAuth2 state parameter for CSRF protection.
    pub state: String,
    /// PKCE code verifier — must be stored and sent with the token exchange.
    /// Providers that support PKCE will populate this; others leave it `None`.
    pub code_verifier: Option<String>,
}

/// Tokens received from the OAuth2 provider after code exchange.
#[derive(Debug, Clone)]
pub struct OAuthToken {
    /// The access token for API calls to the provider.
    pub access_token: String,
    /// The refresh token, if the provider issued one.
    pub refresh_token: Option<String>,
    /// Token expiry in seconds from issuance, if provided.
    pub expires_in: Option<u64>,
}

/// Normalized user information retrieved from an OAuth2 provider.
///
/// Different providers return user data in different shapes. This struct
/// provides a common representation that the auth layer can use to
/// create or match local user records, while preserving the full raw
/// response for extensibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthUserInfo {
    /// The user's unique identifier at the provider (provider-specific format).
    pub id: String,
    /// The user's email address, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Whether the provider has verified this email address.
    #[serde(default)]
    pub email_verified: bool,
    /// The user's display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// URL to the user's avatar image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    /// The full raw JSON response from the provider's user info endpoint.
    /// Preserved for access to provider-specific fields not captured above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
}

// ── OAuthProvider trait ─────────────────────────────────────────────────────

/// Trait for OAuth2 providers implementing the authorization code flow.
///
/// Each provider (Google, Microsoft, GitHub, etc.) implements this trait.
/// The trait is object-safe and uses `async_trait` to support dynamic
/// dispatch through [`OAuthProviderRegistry`].
///
/// # Implementing a new provider
///
/// ```ignore
/// use async_trait::async_trait;
/// use zerobase_core::oauth::*;
/// use zerobase_core::error::ZerobaseError;
///
/// pub struct GitHubProvider { config: OAuthProviderConfig }
///
/// #[async_trait]
/// impl OAuthProvider for GitHubProvider {
///     fn name(&self) -> &str { "github" }
///     fn display_name(&self) -> &str { "GitHub" }
///
///     fn auth_url(&self, state: &str, redirect_url: &str)
///         -> Result<AuthUrlResponse, ZerobaseError> { /* ... */ }
///
///     async fn exchange_code(&self, code: &str, redirect_url: &str, code_verifier: Option<&str>)
///         -> Result<OAuthToken, ZerobaseError> { /* ... */ }
///
///     async fn get_user_info(&self, token: &OAuthToken)
///         -> Result<OAuthUserInfo, ZerobaseError> { /* ... */ }
/// }
/// ```
#[async_trait]
pub trait OAuthProvider: Send + Sync {
    /// Machine-readable identifier for this provider (e.g., `"google"`, `"microsoft"`).
    ///
    /// Must be unique within a registry. Used as the lookup key and in
    /// serialized OAuth2 records (e.g., the `provider` field on external auths).
    fn name(&self) -> &str;

    /// Human-readable display name (e.g., `"Google"`, `"Microsoft"`).
    fn display_name(&self) -> &str;

    /// Generate the authorization URL the user should be redirected to.
    ///
    /// # Arguments
    /// - `state` — CSRF protection token; will be validated on callback.
    /// - `redirect_url` — The callback URL registered with the provider.
    ///
    /// # Returns
    /// An [`AuthUrlResponse`] containing the full URL, the state parameter,
    /// and an optional PKCE code verifier to store for the token exchange.
    fn auth_url(&self, state: &str, redirect_url: &str) -> Result<AuthUrlResponse, ZerobaseError>;

    /// Exchange an authorization code for access and refresh tokens.
    ///
    /// # Arguments
    /// - `code` — The authorization code from the provider callback.
    /// - `redirect_url` — Must match the redirect URL used in [`auth_url`](Self::auth_url).
    /// - `code_verifier` — The PKCE code verifier, if one was generated.
    async fn exchange_code(
        &self,
        code: &str,
        redirect_url: &str,
        code_verifier: Option<&str>,
    ) -> Result<OAuthToken, ZerobaseError>;

    /// Fetch the authenticated user's profile from the provider.
    ///
    /// Uses the access token obtained from [`exchange_code`](Self::exchange_code)
    /// to call the provider's user info endpoint and normalize the response
    /// into an [`OAuthUserInfo`].
    async fn get_user_info(&self, token: &OAuthToken) -> Result<OAuthUserInfo, ZerobaseError>;
}

// ── Provider registry ───────────────────────────────────────────────────────

/// A factory function that creates an [`OAuthProvider`] from configuration.
///
/// Used by [`OAuthProviderRegistry`] to lazily construct providers when
/// their configuration is supplied at runtime (e.g., from the admin UI
/// or settings API).
pub type OAuthProviderFactory =
    Arc<dyn Fn(OAuthProviderConfig) -> Arc<dyn OAuthProvider> + Send + Sync>;

/// Registry for OAuth2 providers.
///
/// Supports two usage patterns:
///
/// 1. **Pre-built providers**: Register fully configured provider instances
///    with [`register`](Self::register). Useful when provider config is
///    known at startup.
///
/// 2. **Factory pattern**: Register provider factories with
///    [`register_factory`](Self::register_factory). The factory is called
///    with an [`OAuthProviderConfig`] when a provider is needed, supporting
///    dynamic configuration (e.g., per-collection OAuth2 settings).
///
/// # Example
///
/// ```ignore
/// let mut registry = OAuthProviderRegistry::new();
///
/// // Register a factory for Google
/// registry.register_factory("google", Arc::new(|config| {
///     Arc::new(GoogleProvider::new(config))
/// }));
///
/// // Later, create a provider instance from config
/// let provider = registry.create("google", config)?;
/// ```
#[derive(Default, Clone)]
pub struct OAuthProviderRegistry {
    /// Pre-built, fully configured provider instances.
    providers: HashMap<String, Arc<dyn OAuthProvider>>,
    /// Factory functions keyed by provider name.
    factories: HashMap<String, OAuthProviderFactory>,
}

impl OAuthProviderRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a fully configured provider instance.
    ///
    /// Overwrites any previously registered provider with the same name.
    pub fn register(&mut self, provider: Arc<dyn OAuthProvider>) {
        let name = provider.name().to_string();
        self.providers.insert(name, provider);
    }

    /// Register a factory function for a provider type.
    ///
    /// The factory will be called by [`create`](Self::create) to produce
    /// provider instances on demand from configuration.
    pub fn register_factory(&mut self, name: impl Into<String>, factory: OAuthProviderFactory) {
        self.factories.insert(name.into(), factory);
    }

    /// Look up a pre-registered provider by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn OAuthProvider>> {
        self.providers.get(name).cloned()
    }

    /// Create a provider instance from a registered factory.
    ///
    /// Returns an error if no factory is registered for the given name.
    pub fn create(
        &self,
        name: &str,
        config: OAuthProviderConfig,
    ) -> Result<Arc<dyn OAuthProvider>, ZerobaseError> {
        let factory = self
            .factories
            .get(name)
            .ok_or_else(|| ZerobaseError::validation(format!("unknown OAuth2 provider: {name}")))?;
        Ok(factory(config))
    }

    /// List all registered provider names (both pre-built and factory-backed).
    pub fn available_providers(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .providers
            .keys()
            .map(|s| s.as_str())
            .chain(self.factories.keys().map(|s| s.as_str()))
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Returns `true` if a provider or factory is registered for the given name.
    pub fn has_provider(&self, name: &str) -> bool {
        self.providers.contains_key(name) || self.factories.contains_key(name)
    }
}

impl std::fmt::Debug for OAuthProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthProviderRegistry")
            .field("providers", &self.providers.keys().collect::<Vec<_>>())
            .field("factories", &self.factories.keys().collect::<Vec<_>>())
            .finish()
    }
}

// ── Test helpers ────────────────────────────────────────────────────────────

/// A mock OAuth2 provider for testing. **Only for testing.**
///
/// Returns deterministic values for all methods, making it easy to assert
/// against in integration tests without hitting real OAuth2 endpoints.
#[cfg(test)]
pub struct MockOAuthProvider {
    /// Provider name returned by [`name()`](OAuthProvider::name).
    pub provider_name: String,
    /// User info returned by [`get_user_info()`](OAuthProvider::get_user_info).
    pub user_info: OAuthUserInfo,
}

#[cfg(test)]
impl MockOAuthProvider {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            provider_name: name.into(),
            user_info: OAuthUserInfo {
                id: "mock-oauth-id-123".to_string(),
                email: Some("oauth-user@example.com".to_string()),
                email_verified: true,
                name: Some("Mock User".to_string()),
                avatar_url: None,
                raw: None,
            },
        }
    }

    pub fn with_user_info(mut self, info: OAuthUserInfo) -> Self {
        self.user_info = info;
        self
    }
}

#[cfg(test)]
#[async_trait]
impl OAuthProvider for MockOAuthProvider {
    fn name(&self) -> &str {
        &self.provider_name
    }

    fn display_name(&self) -> &str {
        "Mock Provider"
    }

    fn auth_url(&self, state: &str, redirect_url: &str) -> Result<AuthUrlResponse, ZerobaseError> {
        Ok(AuthUrlResponse {
            url: format!(
                "https://mock-provider.example.com/authorize?state={}&redirect_uri={}",
                state, redirect_url
            ),
            state: state.to_string(),
            code_verifier: Some("mock-pkce-verifier".to_string()),
        })
    }

    async fn exchange_code(
        &self,
        _code: &str,
        _redirect_url: &str,
        _code_verifier: Option<&str>,
    ) -> Result<OAuthToken, ZerobaseError> {
        Ok(OAuthToken {
            access_token: "mock-access-token".to_string(),
            refresh_token: Some("mock-refresh-token".to_string()),
            expires_in: Some(3600),
        })
    }

    async fn get_user_info(&self, _token: &OAuthToken) -> Result<OAuthUserInfo, ZerobaseError> {
        Ok(self.user_info.clone())
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── OAuthUserInfo ───────────────────────────────────────────────────

    #[test]
    fn oauth_user_info_serializes_to_camel_case() {
        let info = OAuthUserInfo {
            id: "123".to_string(),
            email: Some("user@example.com".to_string()),
            email_verified: true,
            name: Some("Test User".to_string()),
            avatar_url: Some("https://example.com/avatar.png".to_string()),
            raw: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["id"], "123");
        assert_eq!(json["email"], "user@example.com");
        assert_eq!(json["emailVerified"], true);
        assert_eq!(json["name"], "Test User");
        assert_eq!(json["avatarUrl"], "https://example.com/avatar.png");
        // raw is None so should be absent
        assert!(json.get("raw").is_none());
    }

    #[test]
    fn oauth_user_info_deserializes_from_camel_case() {
        let json = r#"{
            "id": "456",
            "email": "other@example.com",
            "emailVerified": false,
            "name": "Other User"
        }"#;
        let info: OAuthUserInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.id, "456");
        assert_eq!(info.email.as_deref(), Some("other@example.com"));
        assert!(!info.email_verified);
        assert_eq!(info.name.as_deref(), Some("Other User"));
        assert!(info.avatar_url.is_none());
    }

    #[test]
    fn oauth_user_info_minimal_deserialization() {
        let json = r#"{"id": "789"}"#;
        let info: OAuthUserInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.id, "789");
        assert!(info.email.is_none());
        assert!(!info.email_verified);
        assert!(info.name.is_none());
        assert!(info.avatar_url.is_none());
    }

    // ── OAuthProviderConfig ─────────────────────────────────────────────

    #[test]
    fn provider_config_serializes_with_camel_case() {
        let config = OAuthProviderConfig {
            client_id: "id123".to_string(),
            client_secret: "secret456".to_string(),
            auth_url: Some("https://auth.example.com".to_string()),
            token_url: None,
            user_info_url: None,
            extra_scopes: vec!["profile".to_string()],
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["clientId"], "id123");
        assert_eq!(json["clientSecret"], "secret456");
        assert_eq!(json["authUrl"], "https://auth.example.com");
        // None fields should be absent
        assert!(json.get("tokenUrl").is_none());
        assert!(json.get("userInfoUrl").is_none());
        assert_eq!(json["extraScopes"], serde_json::json!(["profile"]));
    }

    #[test]
    fn provider_config_deserializes_minimal() {
        let json = r#"{"clientId": "abc", "clientSecret": "def"}"#;
        let config: OAuthProviderConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.client_id, "abc");
        assert_eq!(config.client_secret, "def");
        assert!(config.auth_url.is_none());
        assert!(config.token_url.is_none());
        assert!(config.user_info_url.is_none());
        assert!(config.extra_scopes.is_empty());
    }

    // ── AuthUrlResponse ─────────────────────────────────────────────────

    #[test]
    fn auth_url_response_with_pkce() {
        let resp = AuthUrlResponse {
            url: "https://provider.com/auth?code_challenge=abc".to_string(),
            state: "csrf-token".to_string(),
            code_verifier: Some("verifier-123".to_string()),
        };
        assert!(resp.url.contains("code_challenge"));
        assert_eq!(resp.state, "csrf-token");
        assert_eq!(resp.code_verifier.as_deref(), Some("verifier-123"));
    }

    #[test]
    fn auth_url_response_without_pkce() {
        let resp = AuthUrlResponse {
            url: "https://provider.com/auth".to_string(),
            state: "state-abc".to_string(),
            code_verifier: None,
        };
        assert!(resp.code_verifier.is_none());
    }

    // ── OAuthToken ──────────────────────────────────────────────────────

    #[test]
    fn oauth_token_with_refresh() {
        let token = OAuthToken {
            access_token: "at-123".to_string(),
            refresh_token: Some("rt-456".to_string()),
            expires_in: Some(3600),
        };
        assert_eq!(token.access_token, "at-123");
        assert_eq!(token.refresh_token.as_deref(), Some("rt-456"));
        assert_eq!(token.expires_in, Some(3600));
    }

    #[test]
    fn oauth_token_without_refresh() {
        let token = OAuthToken {
            access_token: "at-789".to_string(),
            refresh_token: None,
            expires_in: None,
        };
        assert!(token.refresh_token.is_none());
        assert!(token.expires_in.is_none());
    }

    // ── MockOAuthProvider ───────────────────────────────────────────────

    #[tokio::test]
    async fn mock_provider_returns_auth_url() {
        let provider = MockOAuthProvider::new("test-provider");
        assert_eq!(provider.name(), "test-provider");
        assert_eq!(provider.display_name(), "Mock Provider");

        let resp = provider
            .auth_url("my-state", "http://localhost/callback")
            .unwrap();
        assert!(resp.url.contains("my-state"));
        assert!(resp.url.contains("http://localhost/callback"));
        assert_eq!(resp.state, "my-state");
        assert!(resp.code_verifier.is_some());
    }

    #[tokio::test]
    async fn mock_provider_exchanges_code() {
        let provider = MockOAuthProvider::new("test");
        let token = provider
            .exchange_code("auth-code", "http://localhost/callback", None)
            .await
            .unwrap();
        assert_eq!(token.access_token, "mock-access-token");
        assert!(token.refresh_token.is_some());
        assert_eq!(token.expires_in, Some(3600));
    }

    #[tokio::test]
    async fn mock_provider_returns_user_info() {
        let provider = MockOAuthProvider::new("test");
        let token = OAuthToken {
            access_token: "token".to_string(),
            refresh_token: None,
            expires_in: None,
        };
        let info = provider.get_user_info(&token).await.unwrap();
        assert_eq!(info.id, "mock-oauth-id-123");
        assert_eq!(info.email.as_deref(), Some("oauth-user@example.com"));
        assert!(info.email_verified);
    }

    #[tokio::test]
    async fn mock_provider_with_custom_user_info() {
        let custom_info = OAuthUserInfo {
            id: "custom-id".to_string(),
            email: Some("custom@example.com".to_string()),
            email_verified: false,
            name: Some("Custom User".to_string()),
            avatar_url: Some("https://example.com/custom.png".to_string()),
            raw: Some(serde_json::json!({"custom_field": "value"})),
        };
        let provider = MockOAuthProvider::new("custom").with_user_info(custom_info);
        let token = OAuthToken {
            access_token: "t".to_string(),
            refresh_token: None,
            expires_in: None,
        };
        let info = provider.get_user_info(&token).await.unwrap();
        assert_eq!(info.id, "custom-id");
        assert_eq!(info.email.as_deref(), Some("custom@example.com"));
        assert!(!info.email_verified);
        assert!(info.raw.is_some());
    }

    // ── OAuthProviderRegistry ───────────────────────────────────────────

    #[test]
    fn registry_starts_empty() {
        let registry = OAuthProviderRegistry::new();
        assert!(registry.available_providers().is_empty());
        assert!(!registry.has_provider("google"));
    }

    #[test]
    fn registry_register_and_get() {
        let mut registry = OAuthProviderRegistry::new();
        let provider: Arc<dyn OAuthProvider> = Arc::new(MockOAuthProvider::new("mock-google"));
        registry.register(provider);

        assert!(registry.has_provider("mock-google"));
        let retrieved = registry.get("mock-google").unwrap();
        assert_eq!(retrieved.name(), "mock-google");
    }

    #[test]
    fn registry_get_returns_none_for_unknown() {
        let registry = OAuthProviderRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn registry_register_factory_and_create() {
        let mut registry = OAuthProviderRegistry::new();
        registry.register_factory(
            "mock-factory",
            Arc::new(|_config| Arc::new(MockOAuthProvider::new("mock-factory"))),
        );

        assert!(registry.has_provider("mock-factory"));

        let config = OAuthProviderConfig {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
            auth_url: None,
            token_url: None,
            user_info_url: None,
            extra_scopes: vec![],
        };
        let provider = registry.create("mock-factory", config).unwrap();
        assert_eq!(provider.name(), "mock-factory");
    }

    #[test]
    fn registry_create_unknown_factory_returns_error() {
        let registry = OAuthProviderRegistry::new();
        let config = OAuthProviderConfig {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
            auth_url: None,
            token_url: None,
            user_info_url: None,
            extra_scopes: vec![],
        };
        let result = registry.create("nonexistent", config);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("unknown OAuth2 provider"));
    }

    #[test]
    fn registry_available_providers_lists_all() {
        let mut registry = OAuthProviderRegistry::new();

        let provider: Arc<dyn OAuthProvider> = Arc::new(MockOAuthProvider::new("alpha"));
        registry.register(provider);

        registry.register_factory(
            "beta",
            Arc::new(|_| Arc::new(MockOAuthProvider::new("beta"))),
        );

        let available = registry.available_providers();
        assert_eq!(available, vec!["alpha", "beta"]);
    }

    #[test]
    fn registry_available_providers_deduplicates() {
        let mut registry = OAuthProviderRegistry::new();

        let provider: Arc<dyn OAuthProvider> = Arc::new(MockOAuthProvider::new("same"));
        registry.register(provider);

        registry.register_factory(
            "same",
            Arc::new(|_| Arc::new(MockOAuthProvider::new("same"))),
        );

        let available = registry.available_providers();
        assert_eq!(available, vec!["same"]);
    }

    #[test]
    fn registry_register_overwrites_existing() {
        let mut registry = OAuthProviderRegistry::new();

        let p1: Arc<dyn OAuthProvider> = Arc::new(MockOAuthProvider::new("google"));
        registry.register(p1);

        // Overwrite with a provider that has a different display behavior
        let p2: Arc<dyn OAuthProvider> = Arc::new(MockOAuthProvider::new("google").with_user_info(
            OAuthUserInfo {
                id: "new-id".to_string(),
                email: None,
                email_verified: false,
                name: None,
                avatar_url: None,
                raw: None,
            },
        ));
        registry.register(p2);

        // Should still have only one "google"
        assert_eq!(
            registry
                .available_providers()
                .iter()
                .filter(|&&n| n == "google")
                .count(),
            1
        );
    }

    #[test]
    fn registry_debug_shows_provider_names() {
        let mut registry = OAuthProviderRegistry::new();
        let provider: Arc<dyn OAuthProvider> = Arc::new(MockOAuthProvider::new("debug-test"));
        registry.register(provider);
        let debug_str = format!("{:?}", registry);
        assert!(debug_str.contains("debug-test"));
    }
}
