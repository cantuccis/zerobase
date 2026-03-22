//! Concrete OAuth2 provider implementations.
//!
//! Each provider implements the [`OAuthProvider`](zerobase_core::oauth::OAuthProvider)
//! trait from `zerobase-core`, handling provider-specific endpoint URLs, scopes,
//! and user info response parsing.
//!
//! # Adding a new provider
//!
//! 1. Create a new module (e.g., `github.rs`).
//! 2. Implement [`OAuthProvider`](zerobase_core::oauth::OAuthProvider) for your type.
//! 3. Re-export from this module.
//! 4. Register the factory in [`register_default_providers`].

pub mod google;
pub mod microsoft;

pub use google::GoogleProvider;
pub use microsoft::MicrosoftProvider;

use std::sync::Arc;

use zerobase_core::oauth::OAuthProviderRegistry;

/// Register factories for all built-in OAuth2 providers.
///
/// Call this during application startup to make providers available
/// through the [`OAuthProviderRegistry`] factory pattern.
pub fn register_default_providers(registry: &mut OAuthProviderRegistry) {
    registry.register_factory(
        "google",
        Arc::new(|config| Arc::new(GoogleProvider::new(config))),
    );
    registry.register_factory(
        "microsoft",
        Arc::new(|config| Arc::new(MicrosoftProvider::new(config))),
    );
}
