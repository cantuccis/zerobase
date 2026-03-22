//! Auth abstractions for the core layer.
//!
//! Defines the [`PasswordHasher`] trait used by [`RecordService`] to hash
//! passwords when creating or updating records in auth collections, and
//! the [`TokenService`] trait for JWT token generation and validation.
//! Concrete implementations live in `zerobase-auth`.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::ZerobaseError;

// ── Password hashing ──────────────────────────────────────────────────────

/// Abstraction over password hashing and verification.
///
/// The record service uses this trait to hash passwords before storage
/// and to verify passwords during authentication. This keeps the core
/// crate free from direct `argon2` dependency while allowing the auth
/// crate to provide the concrete implementation.
pub trait PasswordHasher: Send + Sync {
    /// Hash a plaintext password, returning a PHC-format string.
    fn hash(&self, plain: &str) -> Result<String, ZerobaseError>;

    /// Verify a plaintext password against a stored hash.
    fn verify(&self, plain: &str, hash: &str) -> Result<bool, ZerobaseError>;
}

// ── Token types ───────────────────────────────────────────────────────────

/// The purpose a token was issued for.
///
/// Each variant results in a different `type` claim in the JWT, preventing
/// tokens from being used outside their intended context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenType {
    /// Standard authentication token issued after login.
    Auth,
    /// Refresh token used to obtain a new auth token.
    Refresh,
    /// Short-lived token for accessing protected files.
    File,
    /// Email verification token.
    Verification,
    /// Password reset token.
    PasswordReset,
    /// Email change confirmation token.
    EmailChange,
    /// Partial auth token issued when MFA is required.
    /// Cannot be used for regular API access — must be exchanged via MFA verification.
    MfaPartial,
}

impl fmt::Display for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auth => write!(f, "auth"),
            Self::Refresh => write!(f, "refresh"),
            Self::File => write!(f, "file"),
            Self::Verification => write!(f, "verification"),
            Self::PasswordReset => write!(f, "password_reset"),
            Self::EmailChange => write!(f, "email_change"),
            Self::MfaPartial => write!(f, "mfa_partial"),
        }
    }
}

/// Claims embedded in every Zerobase JWT.
///
/// Mirrors PocketBase's token structure:
/// - `id`  — the user's record ID
/// - `collectionId` — the collection the user belongs to
/// - `type` — token purpose (auth, refresh, file)
/// - `tokenKey` — a per-user random key; changing it invalidates all tokens
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenClaims {
    /// User record ID.
    pub id: String,
    /// Collection the user belongs to (e.g. `"users"` or `"_superusers"`).
    pub collection_id: String,
    /// Token purpose.
    #[serde(rename = "type")]
    pub token_type: TokenType,
    /// Per-user invalidation key. If the stored key changes, all issued
    /// tokens for this user become invalid.
    pub token_key: String,
    /// New email address — only present in email-change tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_email: Option<String>,
    /// Issued-at timestamp (seconds since UNIX epoch).
    pub iat: u64,
    /// Expiry timestamp (seconds since UNIX epoch).
    pub exp: u64,
}

/// A successfully validated token, pairing the raw JWT string with its
/// decoded claims.
#[derive(Debug, Clone)]
pub struct ValidatedToken {
    pub claims: TokenClaims,
}

// ── Token service trait ───────────────────────────────────────────────────

/// Abstraction over JWT token generation and validation.
///
/// Keeps the core and API layers independent of the concrete JWT library.
/// The production implementation lives in `zerobase-auth::token`.
pub trait TokenService: Send + Sync {
    /// Generate a signed JWT for the given claims.
    ///
    /// The implementation is responsible for setting `iat` and `exp`
    /// based on the configured duration (or the supplied override).
    fn generate(
        &self,
        user_id: &str,
        collection_id: &str,
        token_type: TokenType,
        token_key: &str,
        duration_secs: Option<u64>,
    ) -> Result<String, ZerobaseError>;

    /// Generate a signed JWT for an email-change flow.
    ///
    /// Like [`generate`](Self::generate) but embeds the `new_email` claim.
    /// The default implementation delegates to `generate` (losing the
    /// new-email claim), so concrete services should override this.
    fn generate_with_new_email(
        &self,
        user_id: &str,
        collection_id: &str,
        token_type: TokenType,
        token_key: &str,
        new_email: &str,
        duration_secs: Option<u64>,
    ) -> Result<String, ZerobaseError> {
        // Default: fall through to generate (suboptimal — override me).
        let _ = new_email;
        self.generate(user_id, collection_id, token_type, token_key, duration_secs)
    }

    /// Validate a JWT string and return the decoded claims.
    ///
    /// The implementation must verify the signature, check expiry, and
    /// confirm the token type matches `expected_type`.
    fn validate(
        &self,
        token: &str,
        expected_type: TokenType,
    ) -> Result<ValidatedToken, ZerobaseError>;

    /// Validate a JWT and additionally check that the `tokenKey` claim
    /// matches the user's current stored key.
    ///
    /// This is the standard validation path: even if the signature and
    /// expiry are valid, a token whose `tokenKey` no longer matches the
    /// database value is considered revoked.
    fn validate_with_key(
        &self,
        token: &str,
        expected_type: TokenType,
        current_token_key: &str,
    ) -> Result<ValidatedToken, ZerobaseError> {
        let validated = self.validate(token, expected_type)?;
        if validated.claims.token_key != current_token_key {
            return Err(ZerobaseError::auth("token has been invalidated"));
        }
        Ok(validated)
    }
}

// ── Test helpers ──────────────────────────────────────────────────────────

/// A no-op hasher that stores passwords as-is. **Only for testing.**
#[cfg(test)]
pub struct NoOpHasher;

#[cfg(test)]
impl PasswordHasher for NoOpHasher {
    fn hash(&self, plain: &str) -> Result<String, ZerobaseError> {
        Ok(format!("hashed:{plain}"))
    }

    fn verify(&self, plain: &str, hash: &str) -> Result<bool, ZerobaseError> {
        Ok(hash == format!("hashed:{plain}"))
    }
}
