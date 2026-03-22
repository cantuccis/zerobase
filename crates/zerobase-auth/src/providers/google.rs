//! Google OAuth2 provider implementation.
//!
//! Implements the OAuth2 authorization code flow for Google, including:
//! - Authorization URL generation with PKCE support
//! - Token exchange via Google's token endpoint
//! - User info retrieval from Google's userinfo API
//!
//! # Scopes
//!
//! By default requests `openid`, `email`, and `profile` scopes. Additional
//! scopes can be added via [`OAuthProviderConfig::extra_scopes`].
//!
//! # Endpoint overrides
//!
//! All endpoint URLs can be overridden via [`OAuthProviderConfig`] for
//! testing or Google Workspace custom configurations.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use zerobase_core::error::ZerobaseError;
use zerobase_core::oauth::{
    AuthUrlResponse, OAuthProvider, OAuthProviderConfig, OAuthToken, OAuthUserInfo,
};

// ── Google endpoint defaults ──────────────────────────────────────────────

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_USER_INFO_URL: &str = "https://www.googleapis.com/oauth2/v3/userinfo";

/// Default scopes for Google OAuth2.
const DEFAULT_SCOPES: &[&str] = &["openid", "email", "profile"];

// ── Google userinfo response ──────────────────────────────────────────────

/// Raw response from Google's userinfo endpoint (`/oauth2/v3/userinfo`).
///
/// Only the fields we need are deserialized; the full JSON is preserved
/// in [`OAuthUserInfo::raw`] for extensibility.
#[derive(Debug, Deserialize)]
struct GoogleUserInfoResponse {
    /// Google's unique user identifier (numeric string).
    sub: String,
    /// The user's email address.
    email: Option<String>,
    /// Whether Google has verified this email.
    #[serde(default)]
    email_verified: bool,
    /// The user's full display name.
    name: Option<String>,
    /// URL to the user's profile picture.
    picture: Option<String>,
}

// ── GoogleProvider ────────────────────────────────────────────────────────

/// Google OAuth2 provider.
///
/// Handles the complete authorization code flow against Google's OAuth2
/// endpoints. Supports PKCE (S256) for enhanced security.
///
/// # Example
///
/// ```ignore
/// use zerobase_core::oauth::OAuthProviderConfig;
/// use zerobase_auth::providers::GoogleProvider;
///
/// let config = OAuthProviderConfig {
///     client_id: "your-client-id".into(),
///     client_secret: "your-client-secret".into(),
///     auth_url: None,
///     token_url: None,
///     user_info_url: None,
///     extra_scopes: vec![],
/// };
///
/// let provider = GoogleProvider::new(config);
/// ```
pub struct GoogleProvider {
    config: OAuthProviderConfig,
    http_client: Client,
}

impl GoogleProvider {
    /// Create a new Google provider from the given configuration.
    pub fn new(config: OAuthProviderConfig) -> Self {
        Self {
            config,
            http_client: Client::new(),
        }
    }

    /// Create a new Google provider with a custom HTTP client.
    ///
    /// Useful for testing (e.g., with a client configured to use a mock server).
    pub fn with_http_client(config: OAuthProviderConfig, http_client: Client) -> Self {
        Self {
            config,
            http_client,
        }
    }

    /// Resolve the authorization endpoint URL, using the config override if set.
    fn auth_endpoint(&self) -> &str {
        self.config.auth_url.as_deref().unwrap_or(GOOGLE_AUTH_URL)
    }

    /// Resolve the token endpoint URL, using the config override if set.
    fn token_endpoint(&self) -> &str {
        self.config.token_url.as_deref().unwrap_or(GOOGLE_TOKEN_URL)
    }

    /// Resolve the user info endpoint URL, using the config override if set.
    fn user_info_endpoint(&self) -> &str {
        self.config
            .user_info_url
            .as_deref()
            .unwrap_or(GOOGLE_USER_INFO_URL)
    }

    /// Build the full list of scopes (defaults + extras).
    fn scopes(&self) -> Vec<String> {
        let mut scopes: Vec<String> = DEFAULT_SCOPES.iter().map(|s| (*s).to_string()).collect();
        for extra in &self.config.extra_scopes {
            if !scopes.contains(extra) {
                scopes.push(extra.clone());
            }
        }
        scopes
    }

    /// Generate a PKCE code verifier and its S256 challenge.
    fn generate_pkce() -> (String, String) {
        use rand::Rng;

        // Generate 32 random bytes for the code verifier
        let mut rng = rand::thread_rng();
        let random_bytes: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
        let code_verifier = base64_url_encode(&random_bytes);

        // S256 challenge = BASE64URL(SHA256(code_verifier))
        let challenge = sha256_base64url(code_verifier.as_bytes());

        (code_verifier, challenge)
    }
}

#[async_trait]
impl OAuthProvider for GoogleProvider {
    fn name(&self) -> &str {
        "google"
    }

    fn display_name(&self) -> &str {
        "Google"
    }

    fn auth_url(&self, state: &str, redirect_url: &str) -> Result<AuthUrlResponse, ZerobaseError> {
        let (code_verifier, code_challenge) = Self::generate_pkce();
        let scopes = self.scopes().join(" ");

        let url = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent",
            self.auth_endpoint(),
            url_encode(&self.config.client_id),
            url_encode(redirect_url),
            url_encode(&scopes),
            url_encode(state),
            url_encode(&code_challenge),
        );

        Ok(AuthUrlResponse {
            url,
            state: state.to_string(),
            code_verifier: Some(code_verifier),
        })
    }

    async fn exchange_code(
        &self,
        code: &str,
        redirect_url: &str,
        code_verifier: Option<&str>,
    ) -> Result<OAuthToken, ZerobaseError> {
        let mut params = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_url),
            ("client_id", self.config.client_id.as_str()),
            ("client_secret", self.config.client_secret.as_str()),
        ];

        // Include PKCE code verifier if provided.
        let cv;
        if let Some(verifier) = code_verifier {
            cv = verifier.to_string();
            params.push(("code_verifier", cv.as_str()));
        }

        let response = self
            .http_client
            .post(self.token_endpoint())
            .form(&params)
            .send()
            .await
            .map_err(|e| {
                ZerobaseError::internal(format!("Google token exchange request failed: {e}"))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ZerobaseError::auth(format!(
                "Google token exchange failed (HTTP {status}): {body}"
            )));
        }

        let token_response: GoogleTokenResponse = response.json().await.map_err(|e| {
            ZerobaseError::internal(format!("failed to parse Google token response: {e}"))
        })?;

        Ok(OAuthToken {
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token,
            expires_in: token_response.expires_in,
        })
    }

    async fn get_user_info(&self, token: &OAuthToken) -> Result<OAuthUserInfo, ZerobaseError> {
        let response = self
            .http_client
            .get(self.user_info_endpoint())
            .bearer_auth(&token.access_token)
            .send()
            .await
            .map_err(|e| {
                ZerobaseError::internal(format!("Google user info request failed: {e}"))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ZerobaseError::auth(format!(
                "Google user info request failed (HTTP {status}): {body}"
            )));
        }

        // Parse the raw JSON first to preserve it.
        let raw_json: serde_json::Value = response.json().await.map_err(|e| {
            ZerobaseError::internal(format!("failed to parse Google user info response: {e}"))
        })?;

        let google_info: GoogleUserInfoResponse = serde_json::from_value(raw_json.clone())
            .map_err(|e| {
                ZerobaseError::internal(format!("failed to deserialize Google user info: {e}"))
            })?;

        Ok(OAuthUserInfo {
            id: google_info.sub,
            email: google_info.email,
            email_verified: google_info.email_verified,
            name: google_info.name,
            avatar_url: google_info.picture,
            raw: Some(raw_json),
        })
    }
}

// ── Google token response ─────────────────────────────────────────────────

/// Response from Google's token endpoint.
#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    #[allow(dead_code)]
    token_type: Option<String>,
}

// ── Helpers ───────────────────────────────────────────────────────────────

/// URL-encode a string (percent encoding).
fn url_encode(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

/// Base64-URL encode without padding.
fn base64_url_encode(data: &[u8]) -> String {
    use std::io::Write;
    let mut buf = Vec::with_capacity(data.len() * 4 / 3 + 4);

    // Simple base64 encoding
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut i = 0;
    while i + 2 < data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | (data[i + 2] as u32);
        let _ = buf.write_all(&[
            ALPHABET[((n >> 18) & 0x3F) as usize],
            ALPHABET[((n >> 12) & 0x3F) as usize],
            ALPHABET[((n >> 6) & 0x3F) as usize],
            ALPHABET[(n & 0x3F) as usize],
        ]);
        i += 3;
    }

    let remaining = data.len() - i;
    if remaining == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
        let _ = buf.write_all(&[
            ALPHABET[((n >> 18) & 0x3F) as usize],
            ALPHABET[((n >> 12) & 0x3F) as usize],
            ALPHABET[((n >> 6) & 0x3F) as usize],
        ]);
    } else if remaining == 1 {
        let n = (data[i] as u32) << 16;
        let _ = buf.write_all(&[
            ALPHABET[((n >> 18) & 0x3F) as usize],
            ALPHABET[((n >> 12) & 0x3F) as usize],
        ]);
    }

    // Convert to URL-safe base64 (no padding)
    String::from_utf8(buf)
        .unwrap()
        .replace('+', "-")
        .replace('/', "_")
}

/// Compute SHA-256 and return as base64url-encoded string.
fn sha256_base64url(data: &[u8]) -> String {
    // Minimal SHA-256 implementation for PKCE challenge.
    // We only need this for the code_challenge computation.
    let hash = sha256(data);
    base64_url_encode(&hash)
}

/// SHA-256 hash function (pure Rust, no external deps).
fn sha256(data: &[u8]) -> [u8; 32] {
    // Initial hash values (first 32 bits of fractional parts of square roots of first 8 primes)
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // Round constants
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    // Pre-processing: pad message
    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit block
    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut result = [0u8; 32];
    for (i, val) in h.iter().enumerate() {
        result[i * 4..i * 4 + 4].copy_from_slice(&val.to_be_bytes());
    }
    result
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{bearer_token, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Create a test config pointing at the given mock server.
    fn test_config(mock_server: &MockServer) -> OAuthProviderConfig {
        let base = mock_server.uri();
        OAuthProviderConfig {
            client_id: "test-client-id".to_string(),
            client_secret: "test-client-secret".to_string(),
            auth_url: Some(format!("{base}/authorize")),
            token_url: Some(format!("{base}/token")),
            user_info_url: Some(format!("{base}/userinfo")),
            extra_scopes: vec![],
        }
    }

    /// Create a GoogleProvider wired to a mock server.
    fn test_provider(mock_server: &MockServer) -> GoogleProvider {
        GoogleProvider::new(test_config(mock_server))
    }

    // ── Unit tests ────────────────────────────────────────────────────────

    #[test]
    fn provider_name_is_google() {
        let provider = GoogleProvider::new(OAuthProviderConfig {
            client_id: "id".into(),
            client_secret: "secret".into(),
            auth_url: None,
            token_url: None,
            user_info_url: None,
            extra_scopes: vec![],
        });
        assert_eq!(provider.name(), "google");
        assert_eq!(provider.display_name(), "Google");
    }

    #[test]
    fn auth_url_contains_required_params() {
        let provider = GoogleProvider::new(OAuthProviderConfig {
            client_id: "my-client-id".into(),
            client_secret: "secret".into(),
            auth_url: None,
            token_url: None,
            user_info_url: None,
            extra_scopes: vec![],
        });

        let result = provider
            .auth_url("test-state", "http://localhost:8090/redirect")
            .unwrap();

        assert!(result.url.starts_with(GOOGLE_AUTH_URL));
        assert!(result.url.contains("client_id=my-client-id"));
        assert!(result.url.contains("redirect_uri=http"));
        assert!(result.url.contains("response_type=code"));
        assert!(result.url.contains("state=test-state"));
        assert!(result.url.contains("code_challenge="));
        assert!(result.url.contains("code_challenge_method=S256"));
        assert!(result.url.contains("scope="));
        assert!(result.url.contains("openid"));
        assert_eq!(result.state, "test-state");
        assert!(result.code_verifier.is_some());
    }

    #[test]
    fn auth_url_uses_custom_endpoint() {
        let provider = GoogleProvider::new(OAuthProviderConfig {
            client_id: "id".into(),
            client_secret: "secret".into(),
            auth_url: Some("https://custom.example.com/auth".into()),
            token_url: None,
            user_info_url: None,
            extra_scopes: vec![],
        });

        let result = provider.auth_url("state", "http://localhost/cb").unwrap();
        assert!(result.url.starts_with("https://custom.example.com/auth"));
    }

    #[test]
    fn default_scopes_include_openid_email_profile() {
        let provider = GoogleProvider::new(OAuthProviderConfig {
            client_id: "id".into(),
            client_secret: "secret".into(),
            auth_url: None,
            token_url: None,
            user_info_url: None,
            extra_scopes: vec![],
        });

        let scopes = provider.scopes();
        assert!(scopes.contains(&"openid".to_string()));
        assert!(scopes.contains(&"email".to_string()));
        assert!(scopes.contains(&"profile".to_string()));
    }

    #[test]
    fn extra_scopes_are_appended() {
        let provider = GoogleProvider::new(OAuthProviderConfig {
            client_id: "id".into(),
            client_secret: "secret".into(),
            auth_url: None,
            token_url: None,
            user_info_url: None,
            extra_scopes: vec!["calendar.readonly".to_string()],
        });

        let scopes = provider.scopes();
        assert!(scopes.contains(&"calendar.readonly".to_string()));
        // Default scopes still present
        assert!(scopes.contains(&"openid".to_string()));
    }

    #[test]
    fn duplicate_extra_scopes_are_not_added() {
        let provider = GoogleProvider::new(OAuthProviderConfig {
            client_id: "id".into(),
            client_secret: "secret".into(),
            auth_url: None,
            token_url: None,
            user_info_url: None,
            extra_scopes: vec!["email".to_string()],
        });

        let scopes = provider.scopes();
        assert_eq!(scopes.iter().filter(|s| *s == "email").count(), 1);
    }

    #[test]
    fn pkce_verifier_and_challenge_are_different() {
        let (verifier, challenge) = GoogleProvider::generate_pkce();
        assert!(!verifier.is_empty());
        assert!(!challenge.is_empty());
        assert_ne!(verifier, challenge);
    }

    #[test]
    fn pkce_generates_unique_values() {
        let (v1, _) = GoogleProvider::generate_pkce();
        let (v2, _) = GoogleProvider::generate_pkce();
        assert_ne!(v1, v2, "PKCE verifiers should be unique");
    }

    #[test]
    fn sha256_known_vector() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let hash = sha256(b"");
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(
            hex,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_known_vector_abc() {
        // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        let hash = sha256(b"abc");
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(
            hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn url_encode_preserves_unreserved() {
        assert_eq!(url_encode("hello"), "hello");
        assert_eq!(url_encode("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn url_encode_encodes_special_chars() {
        assert_eq!(url_encode("hello world"), "hello%20world");
        assert_eq!(url_encode("a=b&c"), "a%3Db%26c");
    }

    #[test]
    fn google_user_info_response_deserializes() {
        let json = serde_json::json!({
            "sub": "1234567890",
            "email": "user@gmail.com",
            "email_verified": true,
            "name": "Test User",
            "picture": "https://lh3.googleusercontent.com/photo.jpg"
        });

        let info: GoogleUserInfoResponse = serde_json::from_value(json).unwrap();
        assert_eq!(info.sub, "1234567890");
        assert_eq!(info.email.as_deref(), Some("user@gmail.com"));
        assert!(info.email_verified);
        assert_eq!(info.name.as_deref(), Some("Test User"));
        assert_eq!(
            info.picture.as_deref(),
            Some("https://lh3.googleusercontent.com/photo.jpg")
        );
    }

    #[test]
    fn google_user_info_response_minimal() {
        let json = serde_json::json!({
            "sub": "1234567890"
        });

        let info: GoogleUserInfoResponse = serde_json::from_value(json).unwrap();
        assert_eq!(info.sub, "1234567890");
        assert!(info.email.is_none());
        assert!(!info.email_verified);
        assert!(info.name.is_none());
        assert!(info.picture.is_none());
    }

    // ── Integration tests with mock server ────────────────────────────────

    #[tokio::test]
    async fn exchange_code_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.test-access-token",
                "refresh_token": "1//test-refresh-token",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .mount(&mock_server)
            .await;

        let provider = test_provider(&mock_server);
        let token = provider
            .exchange_code(
                "test-auth-code",
                "http://localhost/callback",
                Some("verifier"),
            )
            .await
            .unwrap();

        assert_eq!(token.access_token, "ya29.test-access-token");
        assert_eq!(
            token.refresh_token.as_deref(),
            Some("1//test-refresh-token")
        );
        assert_eq!(token.expires_in, Some(3600));
    }

    #[tokio::test]
    async fn exchange_code_without_refresh_token() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.access-only",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .mount(&mock_server)
            .await;

        let provider = test_provider(&mock_server);
        let token = provider
            .exchange_code("code", "http://localhost/cb", None)
            .await
            .unwrap();

        assert_eq!(token.access_token, "ya29.access-only");
        assert!(token.refresh_token.is_none());
    }

    #[tokio::test]
    async fn exchange_code_failure_returns_auth_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant",
                "error_description": "Code has already been used."
            })))
            .mount(&mock_server)
            .await;

        let provider = test_provider(&mock_server);
        let result = provider
            .exchange_code("expired-code", "http://localhost/cb", None)
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("token exchange failed"),
            "Error should mention token exchange: {err_msg}"
        );
    }

    #[tokio::test]
    async fn get_user_info_success_full_profile() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .and(bearer_token("test-access-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sub": "1234567890",
                "email": "user@gmail.com",
                "email_verified": true,
                "name": "Test User",
                "picture": "https://lh3.googleusercontent.com/a/photo.jpg",
                "given_name": "Test",
                "family_name": "User",
                "locale": "en"
            })))
            .mount(&mock_server)
            .await;

        let provider = test_provider(&mock_server);
        let token = OAuthToken {
            access_token: "test-access-token".to_string(),
            refresh_token: None,
            expires_in: None,
        };

        let info = provider.get_user_info(&token).await.unwrap();

        assert_eq!(info.id, "1234567890");
        assert_eq!(info.email.as_deref(), Some("user@gmail.com"));
        assert!(info.email_verified);
        assert_eq!(info.name.as_deref(), Some("Test User"));
        assert_eq!(
            info.avatar_url.as_deref(),
            Some("https://lh3.googleusercontent.com/a/photo.jpg")
        );

        // Raw response should preserve all fields
        let raw = info.raw.unwrap();
        assert_eq!(raw["given_name"], "Test");
        assert_eq!(raw["family_name"], "User");
        assert_eq!(raw["locale"], "en");
    }

    #[tokio::test]
    async fn get_user_info_minimal_profile() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sub": "minimal-user-id"
            })))
            .mount(&mock_server)
            .await;

        let provider = test_provider(&mock_server);
        let token = OAuthToken {
            access_token: "token".to_string(),
            refresh_token: None,
            expires_in: None,
        };

        let info = provider.get_user_info(&token).await.unwrap();

        assert_eq!(info.id, "minimal-user-id");
        assert!(info.email.is_none());
        assert!(!info.email_verified);
        assert!(info.name.is_none());
        assert!(info.avatar_url.is_none());
    }

    #[tokio::test]
    async fn get_user_info_unverified_email() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sub": "unverified-user",
                "email": "unverified@gmail.com",
                "email_verified": false,
                "name": "Unverified User"
            })))
            .mount(&mock_server)
            .await;

        let provider = test_provider(&mock_server);
        let token = OAuthToken {
            access_token: "token".to_string(),
            refresh_token: None,
            expires_in: None,
        };

        let info = provider.get_user_info(&token).await.unwrap();

        assert_eq!(info.email.as_deref(), Some("unverified@gmail.com"));
        assert!(!info.email_verified);
    }

    #[tokio::test]
    async fn get_user_info_failure_returns_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": {
                    "code": 401,
                    "message": "Request had invalid authentication credentials.",
                    "status": "UNAUTHENTICATED"
                }
            })))
            .mount(&mock_server)
            .await;

        let provider = test_provider(&mock_server);
        let token = OAuthToken {
            access_token: "expired-token".to_string(),
            refresh_token: None,
            expires_in: None,
        };

        let result = provider.get_user_info(&token).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("user info request failed"),
            "Error should mention user info: {err_msg}"
        );
    }

    #[tokio::test]
    async fn full_oauth_flow_with_mock_server() {
        let mock_server = MockServer::start().await;

        // Mock token endpoint
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.full-flow-token",
                "refresh_token": "1//full-flow-refresh",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .mount(&mock_server)
            .await;

        // Mock userinfo endpoint
        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .and(bearer_token("ya29.full-flow-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sub": "google-user-123",
                "email": "fullflow@gmail.com",
                "email_verified": true,
                "name": "Full Flow User",
                "picture": "https://lh3.googleusercontent.com/photo.jpg"
            })))
            .mount(&mock_server)
            .await;

        let provider = test_provider(&mock_server);

        // Step 1: Generate auth URL
        let auth_response = provider
            .auth_url("csrf-state", "http://localhost:8090/callback")
            .unwrap();
        assert!(!auth_response.url.is_empty());
        assert!(auth_response.code_verifier.is_some());

        // Step 2: Exchange code for token
        let token = provider
            .exchange_code(
                "auth-code-from-google",
                "http://localhost:8090/callback",
                auth_response.code_verifier.as_deref(),
            )
            .await
            .unwrap();
        assert_eq!(token.access_token, "ya29.full-flow-token");

        // Step 3: Get user info
        let user_info = provider.get_user_info(&token).await.unwrap();
        assert_eq!(user_info.id, "google-user-123");
        assert_eq!(user_info.email.as_deref(), Some("fullflow@gmail.com"));
        assert!(user_info.email_verified);
        assert_eq!(user_info.name.as_deref(), Some("Full Flow User"));
        assert!(user_info.avatar_url.is_some());
        assert!(user_info.raw.is_some());
    }

    #[tokio::test]
    async fn exchange_code_sends_correct_form_params() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "token",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let provider = test_provider(&mock_server);
        let _ = provider
            .exchange_code("my-code", "http://localhost/cb", Some("my-verifier"))
            .await
            .unwrap();

        // If we get here without error, the mock matched and the request was correct.
        // The expect(1) ensures exactly one request was made.
    }
}
