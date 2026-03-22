//! Zerobase Auth — authentication strategies and token management.
//!
//! Provides extensible auth via traits: `AuthMethod` for direct auth
//! (password, OTP, passkeys) and `AuthProvider` for external OAuth2 providers.
//!
//! ## Token management
//!
//! The [`token`] module provides [`JwtTokenService`], an HMAC-SHA256 JWT
//! implementation of the core [`TokenService`] trait. Tokens carry user ID,
//! collection ID, type (auth/refresh/file), and a `tokenKey` claim for
//! per-user invalidation.

pub mod email;
pub mod email_change;
pub mod mfa;
pub mod oauth2;
pub mod otp;
pub mod passkey;
pub mod password;
pub mod password_reset;
pub mod providers;
#[cfg(test)]
mod security_tests;
pub mod token;
pub mod verification;

pub use email::SmtpEmailService;
pub use email_change::EmailChangeService;
pub use mfa::MfaService;
pub use oauth2::OAuth2Service;
pub use otp::OtpService;
pub use passkey::PasskeyService;
pub use password::{hash_password, verify_password, PasswordHashError};
pub use password_reset::PasswordResetService;
pub use providers::{register_default_providers, GoogleProvider};
pub use token::JwtTokenService;
pub use verification::VerificationService;

use zerobase_core::auth::PasswordHasher;
use zerobase_core::error::ZerobaseError;

/// Argon2id password hasher implementing the core [`PasswordHasher`] trait.
///
/// This is the production hasher used by the record service to hash
/// passwords before storing them in auth collections.
pub struct Argon2Hasher;

impl PasswordHasher for Argon2Hasher {
    fn hash(&self, plain: &str) -> Result<String, ZerobaseError> {
        hash_password(plain).map_err(|e| ZerobaseError::internal(e.to_string()))
    }

    fn verify(&self, plain: &str, hash: &str) -> Result<bool, ZerobaseError> {
        verify_password(plain, hash).map_err(|e| ZerobaseError::internal(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2_hasher_trait_impl_works() {
        let hasher = Argon2Hasher;
        let hash = hasher.hash("test_password").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(hasher.verify("test_password", &hash).unwrap());
        assert!(!hasher.verify("wrong", &hash).unwrap());
    }
}
