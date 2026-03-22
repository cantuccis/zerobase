//! JWT token generation and validation.
//!
//! Implements [`TokenService`] using the `jsonwebtoken` crate with HMAC-SHA256
//! signing. Tokens follow PocketBase's structure:
//!
//! - `id` — user record ID
//! - `collectionId` — auth collection the user belongs to
//! - `type` — token purpose (auth, refresh, file)
//! - `tokenKey` — per-user invalidation key
//! - `iat` / `exp` — standard issued-at and expiry timestamps

use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use secrecy::{ExposeSecret, SecretString};
use tracing::warn;

use zerobase_core::auth::{TokenClaims, TokenService, TokenType, ValidatedToken};
use zerobase_core::error::ZerobaseError;

/// Production JWT token service using HMAC-SHA256.
///
/// Holds the signing secret and default token duration. Thread-safe and
/// cheaply cloneable (the secret is behind an `Arc` inside `SecretString`).
#[derive(Clone)]
pub struct JwtTokenService {
    /// HMAC secret used for both signing and verification.
    secret: SecretString,
    /// Default token validity in seconds (used when no override is given).
    default_duration_secs: u64,
}

impl JwtTokenService {
    /// Create a new token service.
    ///
    /// # Arguments
    /// - `secret` — HMAC signing key. Must not be empty.
    /// - `default_duration_secs` — fallback expiry when callers don't
    ///   specify one. Must be > 0.
    pub fn new(secret: SecretString, default_duration_secs: u64) -> Self {
        Self {
            secret,
            default_duration_secs,
        }
    }

    /// Build from [`AuthSettings`](zerobase_core::configuration::AuthSettings).
    pub fn from_settings(settings: &zerobase_core::configuration::AuthSettings) -> Self {
        Self::new(settings.token_secret.clone(), settings.token_duration_secs)
    }

    fn encoding_key(&self) -> EncodingKey {
        EncodingKey::from_secret(self.secret.expose_secret().as_bytes())
    }

    fn decoding_key(&self) -> DecodingKey {
        DecodingKey::from_secret(self.secret.expose_secret().as_bytes())
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_secs()
    }
}

impl TokenService for JwtTokenService {
    fn generate(
        &self,
        user_id: &str,
        collection_id: &str,
        token_type: TokenType,
        token_key: &str,
        duration_secs: Option<u64>,
    ) -> Result<String, ZerobaseError> {
        let now = Self::now_secs();
        let exp = now + duration_secs.unwrap_or(self.default_duration_secs);

        let claims = TokenClaims {
            id: user_id.to_string(),
            collection_id: collection_id.to_string(),
            token_type,
            token_key: token_key.to_string(),
            new_email: None,
            iat: now,
            exp,
        };

        let header = Header::new(Algorithm::HS256);
        encode(&header, &claims, &self.encoding_key()).map_err(|e| {
            warn!(error = %e, "failed to encode JWT");
            ZerobaseError::internal(format!("token generation failed: {e}"))
        })
    }

    fn generate_with_new_email(
        &self,
        user_id: &str,
        collection_id: &str,
        token_type: TokenType,
        token_key: &str,
        new_email: &str,
        duration_secs: Option<u64>,
    ) -> Result<String, ZerobaseError> {
        let now = Self::now_secs();
        let exp = now + duration_secs.unwrap_or(self.default_duration_secs);

        let claims = TokenClaims {
            id: user_id.to_string(),
            collection_id: collection_id.to_string(),
            token_type,
            token_key: token_key.to_string(),
            new_email: Some(new_email.to_string()),
            iat: now,
            exp,
        };

        let header = Header::new(Algorithm::HS256);
        encode(&header, &claims, &self.encoding_key()).map_err(|e| {
            warn!(error = %e, "failed to encode JWT");
            ZerobaseError::internal(format!("token generation failed: {e}"))
        })
    }

    fn validate(
        &self,
        token: &str,
        expected_type: TokenType,
    ) -> Result<ValidatedToken, ZerobaseError> {
        let mut validation = Validation::new(Algorithm::HS256);
        // We handle expiry checking ourselves via the `exp` claim, but
        // jsonwebtoken also validates it — keep that enabled.
        validation.validate_exp = true;
        // No leeway — tokens expire exactly at `exp`.
        validation.leeway = 0;
        // We don't use `sub` / `iss` / `aud` standard claims.
        validation.required_spec_claims.clear();

        let token_data =
            decode::<TokenClaims>(token, &self.decoding_key(), &validation).map_err(|e| {
                use jsonwebtoken::errors::ErrorKind;
                match e.kind() {
                    ErrorKind::ExpiredSignature => ZerobaseError::auth("token has expired"),
                    ErrorKind::InvalidSignature => ZerobaseError::auth("invalid token signature"),
                    _ => {
                        warn!(error = %e, "JWT validation failed");
                        ZerobaseError::auth(format!("invalid token: {e}"))
                    }
                }
            })?;

        let claims = token_data.claims;

        // Verify the token was issued for the expected purpose.
        if claims.token_type != expected_type {
            return Err(ZerobaseError::auth(format!(
                "token type mismatch: expected {expected_type}, got {}",
                claims.token_type
            )));
        }

        Ok(ValidatedToken { claims })
    }
}

/// Default token durations for each token type (in seconds).
///
/// These mirror PocketBase defaults.
pub mod durations {
    /// Auth token: 14 days.
    pub const AUTH: u64 = 14 * 24 * 60 * 60;
    /// Refresh token: 90 days.
    pub const REFRESH: u64 = 90 * 24 * 60 * 60;
    /// File access token: 3 minutes.
    pub const FILE: u64 = 3 * 60;
    /// Verification token: 7 days.
    pub const VERIFICATION: u64 = 7 * 24 * 60 * 60;
    /// Password reset token: 1 hour.
    pub const PASSWORD_RESET: u64 = 60 * 60;
    /// Email change token: 1 hour.
    pub const EMAIL_CHANGE: u64 = 60 * 60;
    /// OTP code validity: 5 minutes.
    pub const OTP: u64 = 5 * 60;
    /// MFA partial token validity: 5 minutes.
    pub const MFA_PARTIAL: u64 = 5 * 60;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_secret() -> SecretString {
        SecretString::from("test-secret-key-that-is-long-enough-for-hmac-sha256!!")
    }

    fn service() -> JwtTokenService {
        JwtTokenService::new(test_secret(), durations::AUTH)
    }

    // ── Generation ────────────────────────────────────────────────────────

    #[test]
    fn generates_valid_auth_token() {
        let svc = service();
        let token = svc
            .generate("user123", "col_abc", TokenType::Auth, "tk_key1", None)
            .unwrap();
        assert!(!token.is_empty());

        // Should be a three-part JWT
        assert_eq!(token.split('.').count(), 3);
    }

    #[test]
    fn generates_valid_refresh_token() {
        let svc = service();
        let token = svc
            .generate(
                "user123",
                "col_abc",
                TokenType::Refresh,
                "tk_key1",
                Some(durations::REFRESH),
            )
            .unwrap();
        assert!(!token.is_empty());
    }

    #[test]
    fn generates_valid_file_token() {
        let svc = service();
        let token = svc
            .generate(
                "user123",
                "col_abc",
                TokenType::File,
                "tk_key1",
                Some(durations::FILE),
            )
            .unwrap();
        assert!(!token.is_empty());
    }

    // ── Validation (happy path) ───────────────────────────────────────────

    #[test]
    fn validates_auth_token() {
        let svc = service();
        let token = svc
            .generate("u1", "c1", TokenType::Auth, "key1", None)
            .unwrap();

        let validated = svc.validate(&token, TokenType::Auth).unwrap();
        assert_eq!(validated.claims.id, "u1");
        assert_eq!(validated.claims.collection_id, "c1");
        assert_eq!(validated.claims.token_type, TokenType::Auth);
        assert_eq!(validated.claims.token_key, "key1");
    }

    #[test]
    fn validates_refresh_token() {
        let svc = service();
        let token = svc
            .generate("u1", "c1", TokenType::Refresh, "key1", Some(3600))
            .unwrap();

        let validated = svc.validate(&token, TokenType::Refresh).unwrap();
        assert_eq!(validated.claims.token_type, TokenType::Refresh);
    }

    #[test]
    fn validates_file_token() {
        let svc = service();
        let token = svc
            .generate("u1", "c1", TokenType::File, "key1", Some(180))
            .unwrap();

        let validated = svc.validate(&token, TokenType::File).unwrap();
        assert_eq!(validated.claims.token_type, TokenType::File);
    }

    // ── Claims content ────────────────────────────────────────────────────

    #[test]
    fn claims_contain_correct_timestamps() {
        let svc = service();
        let before = JwtTokenService::now_secs();
        let token = svc
            .generate("u1", "c1", TokenType::Auth, "key1", Some(3600))
            .unwrap();
        let after = JwtTokenService::now_secs();

        let validated = svc.validate(&token, TokenType::Auth).unwrap();
        assert!(validated.claims.iat >= before);
        assert!(validated.claims.iat <= after);
        assert_eq!(validated.claims.exp, validated.claims.iat + 3600);
    }

    #[test]
    fn custom_duration_overrides_default() {
        let svc = JwtTokenService::new(test_secret(), 1_000_000);
        let token = svc
            .generate("u1", "c1", TokenType::Auth, "key1", Some(42))
            .unwrap();

        let validated = svc.validate(&token, TokenType::Auth).unwrap();
        assert_eq!(validated.claims.exp - validated.claims.iat, 42);
    }

    #[test]
    fn default_duration_used_when_none() {
        let default_dur = 7200_u64;
        let svc = JwtTokenService::new(test_secret(), default_dur);
        let token = svc
            .generate("u1", "c1", TokenType::Auth, "key1", None)
            .unwrap();

        let validated = svc.validate(&token, TokenType::Auth).unwrap();
        assert_eq!(validated.claims.exp - validated.claims.iat, default_dur);
    }

    // ── Expired tokens ────────────────────────────────────────────────────

    #[test]
    fn expired_token_is_rejected() {
        let svc = service();
        // Generate a token that expired 1 second ago.
        // We do this by manually crafting claims with exp in the past.
        let now = JwtTokenService::now_secs();
        let claims = TokenClaims {
            id: "u1".to_string(),
            collection_id: "c1".to_string(),
            token_type: TokenType::Auth,
            token_key: "key1".to_string(),
            new_email: None,
            iat: now - 3600,
            exp: now - 1, // already expired
        };
        let header = Header::new(Algorithm::HS256);
        let token = encode(&header, &claims, &svc.encoding_key()).unwrap();

        let result = svc.validate(&token, TokenType::Auth);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 401);
        assert!(err.to_string().contains("expired"));
    }

    // ── Invalid signature ─────────────────────────────────────────────────

    #[test]
    fn wrong_secret_rejects_token() {
        let svc1 = JwtTokenService::new(
            SecretString::from("secret-one-long-enough-for-testing!!"),
            3600,
        );
        let svc2 = JwtTokenService::new(
            SecretString::from("secret-two-long-enough-for-testing!!"),
            3600,
        );

        let token = svc1
            .generate("u1", "c1", TokenType::Auth, "key1", None)
            .unwrap();

        let result = svc2.validate(&token, TokenType::Auth);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 401);
        assert!(err.to_string().contains("signature") || err.to_string().contains("invalid"));
    }

    #[test]
    fn tampered_token_is_rejected() {
        let svc = service();
        let token = svc
            .generate("u1", "c1", TokenType::Auth, "key1", None)
            .unwrap();

        // Tamper with the payload by flipping a character
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let mut payload = parts[1].to_string();
        // Flip the first character of the payload
        let first_char = payload.chars().next().unwrap();
        let flipped = if first_char == 'A' { 'B' } else { 'A' };
        payload.replace_range(0..1, &flipped.to_string());
        let tampered = format!("{}.{}.{}", parts[0], payload, parts[2]);

        let result = svc.validate(&tampered, TokenType::Auth);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 401);
    }

    // ── Type mismatch ─────────────────────────────────────────────────────

    #[test]
    fn auth_token_rejected_when_refresh_expected() {
        let svc = service();
        let token = svc
            .generate("u1", "c1", TokenType::Auth, "key1", None)
            .unwrap();

        let result = svc.validate(&token, TokenType::Refresh);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("type mismatch"));
    }

    #[test]
    fn refresh_token_rejected_when_auth_expected() {
        let svc = service();
        let token = svc
            .generate("u1", "c1", TokenType::Refresh, "key1", Some(3600))
            .unwrap();

        let result = svc.validate(&token, TokenType::Auth);
        assert!(result.is_err());
    }

    #[test]
    fn file_token_rejected_when_auth_expected() {
        let svc = service();
        let token = svc
            .generate("u1", "c1", TokenType::File, "key1", Some(180))
            .unwrap();

        let result = svc.validate(&token, TokenType::Auth);
        assert!(result.is_err());
    }

    // ── Token key invalidation ────────────────────────────────────────────

    #[test]
    fn validate_with_key_succeeds_when_key_matches() {
        let svc = service();
        let token = svc
            .generate("u1", "c1", TokenType::Auth, "current_key", None)
            .unwrap();

        let result = svc.validate_with_key(&token, TokenType::Auth, "current_key");
        assert!(result.is_ok());
    }

    #[test]
    fn validate_with_key_rejects_when_key_changed() {
        let svc = service();
        let token = svc
            .generate("u1", "c1", TokenType::Auth, "old_key", None)
            .unwrap();

        // User's token key was rotated — old tokens should be invalid.
        let result = svc.validate_with_key(&token, TokenType::Auth, "new_key");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 401);
        assert!(err.to_string().contains("invalidated"));
    }

    #[test]
    fn token_key_invalidation_works_per_user() {
        let svc = service();

        // User A's token with key "key_a"
        let token_a = svc
            .generate("user_a", "c1", TokenType::Auth, "key_a", None)
            .unwrap();

        // User B's token with key "key_b"
        let token_b = svc
            .generate("user_b", "c1", TokenType::Auth, "key_b", None)
            .unwrap();

        // Invalidate user A's tokens by changing their key.
        assert!(svc
            .validate_with_key(&token_a, TokenType::Auth, "key_a_v2")
            .is_err());

        // User B's tokens are still valid.
        assert!(svc
            .validate_with_key(&token_b, TokenType::Auth, "key_b")
            .is_ok());
    }

    // ── Refresh flow ──────────────────────────────────────────────────────

    #[test]
    fn refresh_flow_issues_new_auth_token() {
        let svc = service();

        // Step 1: Generate initial auth + refresh tokens
        let _auth_token = svc
            .generate("u1", "c1", TokenType::Auth, "key1", Some(3600))
            .unwrap();
        let refresh_token = svc
            .generate(
                "u1",
                "c1",
                TokenType::Refresh,
                "key1",
                Some(durations::REFRESH),
            )
            .unwrap();

        // Step 2: Validate the refresh token
        let refresh_validated = svc.validate(&refresh_token, TokenType::Refresh).unwrap();
        assert_eq!(refresh_validated.claims.id, "u1");
        assert_eq!(refresh_validated.claims.collection_id, "c1");
        assert_eq!(refresh_validated.claims.token_key, "key1");

        // Step 3: Use refresh claims to issue a new auth token
        let new_auth_token = svc
            .generate(
                &refresh_validated.claims.id,
                &refresh_validated.claims.collection_id,
                TokenType::Auth,
                &refresh_validated.claims.token_key,
                Some(7200), // different duration to distinguish
            )
            .unwrap();

        // Step 4: New auth token is valid with correct claims
        let new_validated = svc.validate(&new_auth_token, TokenType::Auth).unwrap();
        assert_eq!(new_validated.claims.id, "u1");
        assert_eq!(new_validated.claims.collection_id, "c1");
        assert_eq!(new_validated.claims.token_type, TokenType::Auth);
        assert_eq!(new_validated.claims.token_key, "key1");
        // New token has different duration
        assert_eq!(new_validated.claims.exp - new_validated.claims.iat, 7200);
    }

    // ── Malformed input ───────────────────────────────────────────────────

    #[test]
    fn empty_token_is_rejected() {
        let svc = service();
        let result = svc.validate("", TokenType::Auth);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 401);
    }

    #[test]
    fn garbage_token_is_rejected() {
        let svc = service();
        let result = svc.validate("not.a.jwt", TokenType::Auth);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 401);
    }

    #[test]
    fn random_base64_is_rejected() {
        let svc = service();
        let result = svc.validate(
            "eyJhbGciOiJIUzI1NiJ9.eyJmb28iOiJiYXIifQ.dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            TokenType::Auth,
        );
        assert!(result.is_err());
    }

    // ── from_settings ─────────────────────────────────────────────────────

    #[test]
    fn from_settings_uses_configured_values() {
        let settings = zerobase_core::configuration::AuthSettings {
            token_secret: SecretString::from("settings-secret-long-enough-for-testing!!"),
            token_duration_secs: 7200,
        };

        let svc = JwtTokenService::from_settings(&settings);
        let token = svc
            .generate("u1", "c1", TokenType::Auth, "key1", None)
            .unwrap();

        let validated = svc.validate(&token, TokenType::Auth).unwrap();
        assert_eq!(validated.claims.exp - validated.claims.iat, 7200);
    }

    // ── TokenType Display ─────────────────────────────────────────────────

    #[test]
    fn token_type_display() {
        assert_eq!(TokenType::Auth.to_string(), "auth");
        assert_eq!(TokenType::Refresh.to_string(), "refresh");
        assert_eq!(TokenType::File.to_string(), "file");
        assert_eq!(TokenType::PasswordReset.to_string(), "password_reset");
        assert_eq!(TokenType::EmailChange.to_string(), "email_change");
    }

    // ── TokenType serialization ───────────────────────────────────────────

    #[test]
    fn token_type_serializes_to_snake_case() {
        let json = serde_json::to_string(&TokenType::Auth).unwrap();
        assert_eq!(json, r#""auth""#);

        let json = serde_json::to_string(&TokenType::Refresh).unwrap();
        assert_eq!(json, r#""refresh""#);

        let json = serde_json::to_string(&TokenType::File).unwrap();
        assert_eq!(json, r#""file""#);

        let json = serde_json::to_string(&TokenType::PasswordReset).unwrap();
        assert_eq!(json, r#""password_reset""#);
    }

    #[test]
    fn token_type_deserializes_from_snake_case() {
        let t: TokenType = serde_json::from_str(r#""auth""#).unwrap();
        assert_eq!(t, TokenType::Auth);

        let t: TokenType = serde_json::from_str(r#""refresh""#).unwrap();
        assert_eq!(t, TokenType::Refresh);

        let t: TokenType = serde_json::from_str(r#""file""#).unwrap();
        assert_eq!(t, TokenType::File);

        let t: TokenType = serde_json::from_str(r#""password_reset""#).unwrap();
        assert_eq!(t, TokenType::PasswordReset);
    }

    // ── Claims roundtrip ──────────────────────────────────────────────────

    #[test]
    fn claims_serialize_roundtrip() {
        let claims = TokenClaims {
            id: "user123".to_string(),
            collection_id: "col_abc".to_string(),
            token_type: TokenType::Auth,
            token_key: "tk_xyz".to_string(),
            new_email: None,
            iat: 1700000000,
            exp: 1700003600,
        };

        let json = serde_json::to_string(&claims).unwrap();
        // Verify camelCase serialization
        assert!(json.contains("\"collectionId\""));
        assert!(json.contains("\"tokenKey\""));
        assert!(json.contains("\"type\""));

        let deserialized: TokenClaims = serde_json::from_str(&json).unwrap();
        assert_eq!(claims, deserialized);
    }
}
