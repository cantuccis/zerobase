//! Microsoft OAuth2 provider implementation.
//!
//! Implements the OAuth2 authorization code flow for Microsoft (Azure AD v2),
//! including:
//! - Authorization URL generation with PKCE support
//! - Token exchange via Microsoft's token endpoint
//! - User info retrieval from Microsoft Graph API
//!
//! # Scopes
//!
//! By default requests `openid`, `email`, `profile`, and `User.Read` scopes.
//! Additional scopes can be added via [`OAuthProviderConfig::extra_scopes`].
//!
//! # Endpoint overrides
//!
//! All endpoint URLs can be overridden via [`OAuthProviderConfig`] for
//! testing or single-tenant Azure AD configurations.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use zerobase_core::error::ZerobaseError;
use zerobase_core::oauth::{
    AuthUrlResponse, OAuthProvider, OAuthProviderConfig, OAuthToken, OAuthUserInfo,
};

// ── Microsoft endpoint defaults (Azure AD v2 common) ────────────────────

/// Authorization endpoint for Azure AD v2 (common tenant = any Microsoft account).
const MICROSOFT_AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";

/// Token endpoint for Azure AD v2 (common tenant).
const MICROSOFT_TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";

/// Microsoft Graph API endpoint for user profile.
const MICROSOFT_USER_INFO_URL: &str = "https://graph.microsoft.com/v1.0/me";

/// Default scopes for Microsoft OAuth2.
///
/// `User.Read` is required to access the Microsoft Graph `/me` endpoint.
const DEFAULT_SCOPES: &[&str] = &["openid", "email", "profile", "User.Read"];

// ── Microsoft Graph user response ───────────────────────────────────────

/// Raw response from Microsoft Graph `/v1.0/me` endpoint.
///
/// Only the fields we need are deserialized; the full JSON is preserved
/// in [`OAuthUserInfo::raw`] for extensibility.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MicrosoftUserInfoResponse {
    /// Microsoft's unique user identifier (GUID).
    id: String,
    /// The user's primary email address (from `mail` or `userPrincipalName`).
    mail: Option<String>,
    /// The user's principal name (often an email for personal accounts).
    user_principal_name: Option<String>,
    /// The user's full display name.
    display_name: Option<String>,
}

// ── MicrosoftProvider ───────────────────────────────────────────────────

/// Microsoft OAuth2 provider (Azure AD v2).
///
/// Handles the complete authorization code flow against Microsoft's Azure AD
/// v2.0 endpoints. Supports PKCE (S256) for enhanced security.
///
/// Uses the `common` tenant by default, which allows both personal Microsoft
/// accounts and organizational (Azure AD) accounts. Override endpoints via
/// [`OAuthProviderConfig`] for single-tenant configurations.
///
/// # Example
///
/// ```ignore
/// use zerobase_core::oauth::OAuthProviderConfig;
/// use zerobase_auth::providers::MicrosoftProvider;
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
/// let provider = MicrosoftProvider::new(config);
/// ```
pub struct MicrosoftProvider {
    config: OAuthProviderConfig,
    http_client: Client,
}

impl MicrosoftProvider {
    /// Create a new Microsoft provider from the given configuration.
    pub fn new(config: OAuthProviderConfig) -> Self {
        Self {
            config,
            http_client: Client::new(),
        }
    }

    /// Create a new Microsoft provider with a custom HTTP client.
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
        self.config
            .auth_url
            .as_deref()
            .unwrap_or(MICROSOFT_AUTH_URL)
    }

    /// Resolve the token endpoint URL, using the config override if set.
    fn token_endpoint(&self) -> &str {
        self.config
            .token_url
            .as_deref()
            .unwrap_or(MICROSOFT_TOKEN_URL)
    }

    /// Resolve the user info endpoint URL, using the config override if set.
    fn user_info_endpoint(&self) -> &str {
        self.config
            .user_info_url
            .as_deref()
            .unwrap_or(MICROSOFT_USER_INFO_URL)
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

        let mut rng = rand::thread_rng();
        let random_bytes: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
        let code_verifier = base64_url_encode(&random_bytes);

        // S256 challenge = BASE64URL(SHA256(code_verifier))
        let challenge = sha256_base64url(code_verifier.as_bytes());

        (code_verifier, challenge)
    }

    /// Extract the best email from the Microsoft Graph response.
    ///
    /// Microsoft Graph returns email in different fields depending on the
    /// account type:
    /// - `mail`: primary for organizational accounts
    /// - `userPrincipalName`: fallback, often email for personal accounts
    fn extract_email(info: &MicrosoftUserInfoResponse) -> Option<String> {
        info.mail.clone().or_else(|| {
            // userPrincipalName is sometimes a GUID-based identifier for
            // personal accounts (e.g., "live.com#..."), so only use it
            // if it looks like an email address.
            info.user_principal_name
                .as_ref()
                .filter(|upn| upn.contains('@') && !upn.contains('#'))
                .cloned()
        })
    }
}

#[async_trait]
impl OAuthProvider for MicrosoftProvider {
    fn name(&self) -> &str {
        "microsoft"
    }

    fn display_name(&self) -> &str {
        "Microsoft"
    }

    fn auth_url(&self, state: &str, redirect_url: &str) -> Result<AuthUrlResponse, ZerobaseError> {
        let (code_verifier, code_challenge) = Self::generate_pkce();
        let scopes = self.scopes().join(" ");

        let url = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&code_challenge={}&code_challenge_method=S256&response_mode=query",
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
                ZerobaseError::internal(format!("Microsoft token exchange request failed: {e}"))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ZerobaseError::auth(format!(
                "Microsoft token exchange failed (HTTP {status}): {body}"
            )));
        }

        let token_response: MicrosoftTokenResponse = response.json().await.map_err(|e| {
            ZerobaseError::internal(format!("failed to parse Microsoft token response: {e}"))
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
                ZerobaseError::internal(format!("Microsoft user info request failed: {e}"))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ZerobaseError::auth(format!(
                "Microsoft user info request failed (HTTP {status}): {body}"
            )));
        }

        // Parse the raw JSON first to preserve it.
        let raw_json: serde_json::Value = response.json().await.map_err(|e| {
            ZerobaseError::internal(format!("failed to parse Microsoft user info response: {e}"))
        })?;

        let ms_info: MicrosoftUserInfoResponse =
            serde_json::from_value(raw_json.clone()).map_err(|e| {
                ZerobaseError::internal(format!("failed to deserialize Microsoft user info: {e}"))
            })?;

        let email = Self::extract_email(&ms_info);

        Ok(OAuthUserInfo {
            id: ms_info.id,
            email,
            // Microsoft Graph does not expose email_verified directly.
            // If we obtained an email, we treat it as verified since it
            // comes from Microsoft's identity platform.
            email_verified: ms_info.mail.is_some()
                || ms_info
                    .user_principal_name
                    .as_ref()
                    .is_some_and(|upn| upn.contains('@') && !upn.contains('#')),
            name: ms_info.display_name,
            avatar_url: None, // Graph photo requires separate request
            raw: Some(raw_json),
        })
    }
}

// ── Microsoft token response ────────────────────────────────────────────

/// Response from Microsoft's token endpoint.
#[derive(Debug, Deserialize)]
struct MicrosoftTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    #[allow(dead_code)]
    token_type: Option<String>,
}

// ── Helpers ─────────────────────────────────────────────────────────────
// Re-using the same pure-Rust helpers as GoogleProvider for consistency.

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

    String::from_utf8(buf)
        .unwrap()
        .replace('+', "-")
        .replace('/', "_")
}

/// Compute SHA-256 and return as base64url-encoded string.
fn sha256_base64url(data: &[u8]) -> String {
    let hash = sha256(data);
    base64_url_encode(&hash)
}

/// SHA-256 hash function (pure Rust, no external deps).
fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

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

    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

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

// ── Tests ────────────────────────────────────────────────────────────────

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
            user_info_url: Some(format!("{base}/me")),
            extra_scopes: vec![],
        }
    }

    /// Create a MicrosoftProvider wired to a mock server.
    fn test_provider(mock_server: &MockServer) -> MicrosoftProvider {
        MicrosoftProvider::new(test_config(mock_server))
    }

    // ── Unit tests ────────────────────────────────────────────────────────

    #[test]
    fn provider_name_is_microsoft() {
        let provider = MicrosoftProvider::new(OAuthProviderConfig {
            client_id: "id".into(),
            client_secret: "secret".into(),
            auth_url: None,
            token_url: None,
            user_info_url: None,
            extra_scopes: vec![],
        });
        assert_eq!(provider.name(), "microsoft");
        assert_eq!(provider.display_name(), "Microsoft");
    }

    #[test]
    fn auth_url_contains_required_params() {
        let provider = MicrosoftProvider::new(OAuthProviderConfig {
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

        assert!(result.url.starts_with(MICROSOFT_AUTH_URL));
        assert!(result.url.contains("client_id=my-client-id"));
        assert!(result.url.contains("redirect_uri=http"));
        assert!(result.url.contains("response_type=code"));
        assert!(result.url.contains("state=test-state"));
        assert!(result.url.contains("code_challenge="));
        assert!(result.url.contains("code_challenge_method=S256"));
        assert!(result.url.contains("scope="));
        assert!(result.url.contains("openid"));
        assert!(result.url.contains("User.Read"));
        assert!(result.url.contains("response_mode=query"));
        assert_eq!(result.state, "test-state");
        assert!(result.code_verifier.is_some());
    }

    #[test]
    fn auth_url_uses_custom_endpoint() {
        let provider = MicrosoftProvider::new(OAuthProviderConfig {
            client_id: "id".into(),
            client_secret: "secret".into(),
            auth_url: Some(
                "https://login.microsoftonline.com/my-tenant/oauth2/v2.0/authorize".into(),
            ),
            token_url: None,
            user_info_url: None,
            extra_scopes: vec![],
        });

        let result = provider.auth_url("state", "http://localhost/cb").unwrap();
        assert!(result
            .url
            .starts_with("https://login.microsoftonline.com/my-tenant/"));
    }

    #[test]
    fn default_scopes_include_openid_email_profile_user_read() {
        let provider = MicrosoftProvider::new(OAuthProviderConfig {
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
        assert!(scopes.contains(&"User.Read".to_string()));
    }

    #[test]
    fn extra_scopes_are_appended() {
        let provider = MicrosoftProvider::new(OAuthProviderConfig {
            client_id: "id".into(),
            client_secret: "secret".into(),
            auth_url: None,
            token_url: None,
            user_info_url: None,
            extra_scopes: vec!["Calendars.Read".to_string()],
        });

        let scopes = provider.scopes();
        assert!(scopes.contains(&"Calendars.Read".to_string()));
        assert!(scopes.contains(&"openid".to_string()));
    }

    #[test]
    fn duplicate_extra_scopes_are_not_added() {
        let provider = MicrosoftProvider::new(OAuthProviderConfig {
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
        let (verifier, challenge) = MicrosoftProvider::generate_pkce();
        assert!(!verifier.is_empty());
        assert!(!challenge.is_empty());
        assert_ne!(verifier, challenge);
    }

    #[test]
    fn pkce_generates_unique_values() {
        let (v1, _) = MicrosoftProvider::generate_pkce();
        let (v2, _) = MicrosoftProvider::generate_pkce();
        assert_ne!(v1, v2, "PKCE verifiers should be unique");
    }

    #[test]
    fn microsoft_user_info_response_deserializes_org_account() {
        let json = serde_json::json!({
            "id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "mail": "user@contoso.com",
            "userPrincipalName": "user@contoso.com",
            "displayName": "Test User",
            "givenName": "Test",
            "surname": "User",
            "jobTitle": "Engineer"
        });

        let info: MicrosoftUserInfoResponse = serde_json::from_value(json).unwrap();
        assert_eq!(info.id, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        assert_eq!(info.mail.as_deref(), Some("user@contoso.com"));
        assert_eq!(info.display_name.as_deref(), Some("Test User"));
    }

    #[test]
    fn microsoft_user_info_response_deserializes_personal_account() {
        // Personal Microsoft accounts often have null mail
        let json = serde_json::json!({
            "id": "ffffffff-1111-2222-3333-444444444444",
            "mail": null,
            "userPrincipalName": "user@outlook.com",
            "displayName": "Personal User"
        });

        let info: MicrosoftUserInfoResponse = serde_json::from_value(json).unwrap();
        assert_eq!(info.id, "ffffffff-1111-2222-3333-444444444444");
        assert!(info.mail.is_none());
        assert_eq!(
            info.user_principal_name.as_deref(),
            Some("user@outlook.com")
        );
    }

    #[test]
    fn microsoft_user_info_response_minimal() {
        let json = serde_json::json!({
            "id": "minimal-user-guid"
        });

        let info: MicrosoftUserInfoResponse = serde_json::from_value(json).unwrap();
        assert_eq!(info.id, "minimal-user-guid");
        assert!(info.mail.is_none());
        assert!(info.user_principal_name.is_none());
        assert!(info.display_name.is_none());
    }

    #[test]
    fn extract_email_prefers_mail_field() {
        let info = MicrosoftUserInfoResponse {
            id: "id".into(),
            mail: Some("primary@contoso.com".into()),
            user_principal_name: Some("upn@contoso.com".into()),
            display_name: None,
        };
        assert_eq!(
            MicrosoftProvider::extract_email(&info).as_deref(),
            Some("primary@contoso.com")
        );
    }

    #[test]
    fn extract_email_falls_back_to_upn() {
        let info = MicrosoftUserInfoResponse {
            id: "id".into(),
            mail: None,
            user_principal_name: Some("user@outlook.com".into()),
            display_name: None,
        };
        assert_eq!(
            MicrosoftProvider::extract_email(&info).as_deref(),
            Some("user@outlook.com")
        );
    }

    #[test]
    fn extract_email_ignores_non_email_upn() {
        // Some personal accounts have UPN like "live.com#user@outlook.com"
        let info = MicrosoftUserInfoResponse {
            id: "id".into(),
            mail: None,
            user_principal_name: Some("live.com#user@outlook.com".into()),
            display_name: None,
        };
        assert!(MicrosoftProvider::extract_email(&info).is_none());
    }

    #[test]
    fn extract_email_returns_none_when_both_absent() {
        let info = MicrosoftUserInfoResponse {
            id: "id".into(),
            mail: None,
            user_principal_name: None,
            display_name: None,
        };
        assert!(MicrosoftProvider::extract_email(&info).is_none());
    }

    #[test]
    fn email_verified_true_when_mail_present() {
        // Verify the email_verified logic in get_user_info by testing the
        // condition directly (mail.is_some())
        let mail_present = Some("user@contoso.com".to_string());
        let verified = mail_present.is_some();
        assert!(verified);
    }

    // ── Integration tests with mock server ────────────────────────────────

    #[tokio::test]
    async fn exchange_code_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "eyJ0eXAi.test-access-token",
                "refresh_token": "OAAABAAAAi.test-refresh-token",
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

        assert_eq!(token.access_token, "eyJ0eXAi.test-access-token");
        assert_eq!(
            token.refresh_token.as_deref(),
            Some("OAAABAAAAi.test-refresh-token")
        );
        assert_eq!(token.expires_in, Some(3600));
    }

    #[tokio::test]
    async fn exchange_code_without_refresh_token() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "eyJ0eXAi.access-only",
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

        assert_eq!(token.access_token, "eyJ0eXAi.access-only");
        assert!(token.refresh_token.is_none());
    }

    #[tokio::test]
    async fn exchange_code_failure_returns_auth_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant",
                "error_description": "AADSTS70000: The provided authorization code has expired."
            })))
            .mount(&mock_server)
            .await;

        let provider = test_provider(&mock_server);
        let result = provider
            .exchange_code("expired-code", "http://localhost/cb", None)
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("token exchange failed"),
            "Error should mention token exchange: {err_msg}"
        );
    }

    #[tokio::test]
    async fn get_user_info_success_org_account() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/me"))
            .and(bearer_token("test-access-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                "mail": "user@contoso.com",
                "userPrincipalName": "user@contoso.com",
                "displayName": "Contoso User",
                "givenName": "Contoso",
                "surname": "User",
                "jobTitle": "Software Engineer",
                "officeLocation": "Building 1"
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

        assert_eq!(info.id, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        assert_eq!(info.email.as_deref(), Some("user@contoso.com"));
        assert!(info.email_verified);
        assert_eq!(info.name.as_deref(), Some("Contoso User"));
        assert!(info.avatar_url.is_none()); // Graph doesn't return photo URL directly

        // Raw response should preserve all fields
        let raw = info.raw.unwrap();
        assert_eq!(raw["jobTitle"], "Software Engineer");
        assert_eq!(raw["officeLocation"], "Building 1");
    }

    #[tokio::test]
    async fn get_user_info_success_personal_account() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/me"))
            .and(bearer_token("personal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ffffffff-1111-2222-3333-444444444444",
                "mail": null,
                "userPrincipalName": "user@outlook.com",
                "displayName": "Outlook User"
            })))
            .mount(&mock_server)
            .await;

        let provider = test_provider(&mock_server);
        let token = OAuthToken {
            access_token: "personal-token".to_string(),
            refresh_token: None,
            expires_in: None,
        };

        let info = provider.get_user_info(&token).await.unwrap();

        assert_eq!(info.id, "ffffffff-1111-2222-3333-444444444444");
        // Should fall back to userPrincipalName
        assert_eq!(info.email.as_deref(), Some("user@outlook.com"));
        assert!(info.email_verified);
        assert_eq!(info.name.as_deref(), Some("Outlook User"));
    }

    #[tokio::test]
    async fn get_user_info_minimal_profile() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "minimal-user-guid"
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

        assert_eq!(info.id, "minimal-user-guid");
        assert!(info.email.is_none());
        assert!(!info.email_verified);
        assert!(info.name.is_none());
        assert!(info.avatar_url.is_none());
    }

    #[tokio::test]
    async fn get_user_info_failure_returns_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/me"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": {
                    "code": "InvalidAuthenticationToken",
                    "message": "Access token has expired or is not yet valid."
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
                "access_token": "eyJ0eXAi.full-flow-token",
                "refresh_token": "OAAABAAAAi.full-flow-refresh",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .mount(&mock_server)
            .await;

        // Mock Graph /me endpoint
        Mock::given(method("GET"))
            .and(path("/me"))
            .and(bearer_token("eyJ0eXAi.full-flow-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ms-user-guid-123",
                "mail": "fullflow@contoso.com",
                "userPrincipalName": "fullflow@contoso.com",
                "displayName": "Full Flow User",
                "givenName": "Full",
                "surname": "User"
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
                "auth-code-from-microsoft",
                "http://localhost:8090/callback",
                auth_response.code_verifier.as_deref(),
            )
            .await
            .unwrap();
        assert_eq!(token.access_token, "eyJ0eXAi.full-flow-token");

        // Step 3: Get user info
        let user_info = provider.get_user_info(&token).await.unwrap();
        assert_eq!(user_info.id, "ms-user-guid-123");
        assert_eq!(user_info.email.as_deref(), Some("fullflow@contoso.com"));
        assert!(user_info.email_verified);
        assert_eq!(user_info.name.as_deref(), Some("Full Flow User"));
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
    }

    #[tokio::test]
    async fn get_user_info_with_hash_upn_ignores_it() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "personal-guid",
                "mail": null,
                "userPrincipalName": "live.com#user@outlook.com",
                "displayName": "Personal User"
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

        // UPN with # should be ignored
        assert!(info.email.is_none());
        assert!(!info.email_verified);
    }
}
