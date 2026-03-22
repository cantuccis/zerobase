//! OAuth2 service — account linking, creation, and authentication.
//!
//! Implements the server-side of the OAuth2 authorization code flow:
//! 1. List enabled providers for a collection
//! 2. Generate authorization URLs
//! 3. Handle callbacks: exchange code, fetch user info, link/create accounts

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use zerobase_core::auth::{TokenService, TokenType};
use zerobase_core::error::{Result, ZerobaseError};
use zerobase_core::id::generate_id;
use zerobase_core::oauth::{OAuthProviderRegistry, OAuthUserInfo};
use zerobase_core::schema::CollectionType;
use zerobase_core::services::external_auth::{ExternalAuth, ExternalAuthRepository};
use zerobase_core::services::record_service::{
    RecordQuery, RecordRepository, RecordService, SchemaLookup,
};

/// OAuth2 authentication service.
///
/// Orchestrates the full OAuth2 flow: provider lookup, authorization URL
/// generation, code exchange, user info retrieval, and account
/// linking/creation with JWT token issuance.
pub struct OAuth2Service<R: RecordRepository, S: SchemaLookup, E: ExternalAuthRepository> {
    pub record_service: Arc<RecordService<R, S>>,
    pub token_service: Arc<dyn TokenService>,
    pub external_auth_repo: Arc<E>,
    pub provider_registry: Arc<OAuthProviderRegistry>,
}

impl<R: RecordRepository, S: SchemaLookup, E: ExternalAuthRepository> OAuth2Service<R, S, E> {
    pub fn new(
        record_service: Arc<RecordService<R, S>>,
        token_service: Arc<dyn TokenService>,
        external_auth_repo: Arc<E>,
        provider_registry: Arc<OAuthProviderRegistry>,
    ) -> Self {
        Self {
            record_service,
            token_service,
            external_auth_repo,
            provider_registry,
        }
    }

    /// List available authentication methods for a collection.
    ///
    /// Returns a structured response with all auth method categories
    /// (password, OAuth2, OTP, MFA), matching the PocketBase format.
    /// OAuth2 providers include pre-generated authorization URLs.
    pub fn list_auth_methods(&self, collection_name: &str) -> Result<AuthMethodsResponse> {
        let collection = self.record_service.get_collection(collection_name)?;

        if collection.collection_type != CollectionType::Auth {
            return Err(ZerobaseError::validation(format!(
                "collection '{}' is not an auth collection",
                collection_name
            )));
        }

        let auth_options = collection
            .auth_options
            .as_ref()
            .cloned()
            .unwrap_or_default();

        // Password auth section
        let password = PasswordAuthMethod {
            enabled: auth_options.allow_email_auth,
            identity_fields: auth_options.identity_fields.clone(),
        };

        // OAuth2 section with provider auth URLs
        let mut oauth2_providers = Vec::new();
        if auth_options.allow_oauth2_auth {
            for provider_name in self.provider_registry.available_providers() {
                if let Some(provider) = self.provider_registry.get(provider_name) {
                    // Generate a unique state for CSRF protection
                    let state = zerobase_core::id::generate_id();
                    // Use a placeholder redirect URL — the client will supply the real one
                    let auth_url_result = provider.auth_url(&state, "");

                    let provider_info = match auth_url_result {
                        Ok(auth_resp) => OAuth2ProviderInfo {
                            name: provider.name().to_string(),
                            display_name: provider.display_name().to_string(),
                            state: auth_resp.state,
                            auth_url: auth_resp.url,
                            code_verifier: auth_resp.code_verifier.unwrap_or_default(),
                        },
                        Err(_) => OAuth2ProviderInfo {
                            name: provider.name().to_string(),
                            display_name: provider.display_name().to_string(),
                            state,
                            auth_url: String::new(),
                            code_verifier: String::new(),
                        },
                    };

                    oauth2_providers.push(provider_info);
                }
            }
        }

        let oauth2 = OAuth2AuthMethod {
            enabled: auth_options.allow_oauth2_auth,
            providers: oauth2_providers,
        };

        // OTP section
        let otp = OtpAuthMethod {
            enabled: auth_options.allow_otp_auth,
        };

        // MFA section
        let mfa = MfaAuthMethod {
            enabled: auth_options.mfa_enabled,
            duration: auth_options.mfa_duration,
        };

        Ok(AuthMethodsResponse {
            password,
            oauth2,
            otp,
            mfa,
        })
    }

    /// Complete the OAuth2 flow: exchange code, fetch user info, link or create
    /// account, and return a JWT token.
    ///
    /// This method handles the callback phase of the OAuth2 authorization code flow.
    ///
    /// # Flow
    ///
    /// 1. Verify the collection is an auth collection with OAuth2 enabled.
    /// 2. Look up the provider from the registry.
    /// 3. Exchange the authorization code for tokens.
    /// 4. Fetch the user's profile from the provider.
    /// 5. Check if the external identity is already linked to a local account.
    ///    - If linked: authenticate as that user.
    ///    - If not linked: search for a matching local user by email.
    ///      - If found: link the external identity and authenticate.
    ///      - If not found: create a new user account and link.
    /// 6. Generate and return a JWT auth token.
    pub async fn authenticate_with_oauth2(
        &self,
        collection_name: &str,
        provider_name: &str,
        code: &str,
        redirect_url: &str,
        code_verifier: Option<&str>,
    ) -> Result<OAuth2AuthResult> {
        let collection = self.record_service.get_collection(collection_name)?;

        // Must be an auth collection.
        if collection.collection_type != CollectionType::Auth {
            return Err(ZerobaseError::validation(format!(
                "collection '{}' is not an auth collection",
                collection_name
            )));
        }

        // Check that OAuth2 auth is enabled.
        let auth_options = collection
            .auth_options
            .as_ref()
            .cloned()
            .unwrap_or_default();
        if !auth_options.allow_oauth2_auth {
            return Err(ZerobaseError::validation(
                "OAuth2 authentication is not enabled for this collection",
            ));
        }

        // Look up the provider.
        let provider = self.provider_registry.get(provider_name).ok_or_else(|| {
            ZerobaseError::validation(format!("unknown OAuth2 provider: {provider_name}"))
        })?;

        // Exchange code for tokens.
        let oauth_token = provider
            .exchange_code(code, redirect_url, code_verifier)
            .await?;

        // Fetch user info from the provider.
        let user_info = provider.get_user_info(&oauth_token).await?;

        // Try to find an existing external auth link.
        let existing_link = self
            .external_auth_repo
            .find_by_provider(provider_name, &user_info.id)?;

        let (record, is_new) = if let Some(link) = existing_link {
            // Existing link — authenticate as the linked user.
            let record = self
                .record_service
                .get_record(collection_name, &link.record_id)?;
            (record, false)
        } else {
            // No existing link — try to find by email or create new.
            self.link_or_create_user(collection_name, &collection.id, provider_name, &user_info)?
        };

        // Generate JWT.
        let user_id = record
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let token_key = record
            .get("tokenKey")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let token = self.token_service.generate(
            &user_id,
            &collection.id,
            TokenType::Auth,
            &token_key,
            None,
        )?;

        // Strip sensitive fields from the response record.
        let mut response_record = record;
        response_record.remove("tokenKey");
        response_record.remove("password");

        Ok(OAuth2AuthResult {
            token,
            record: response_record,
            collection_id: collection.id,
            collection_name: collection.name,
            is_new_user: is_new,
        })
    }

    /// Find an existing user by email or create a new one, then link the external identity.
    fn link_or_create_user(
        &self,
        collection_name: &str,
        collection_id: &str,
        provider_name: &str,
        user_info: &OAuthUserInfo,
    ) -> Result<(HashMap<String, Value>, bool)> {
        // Try to find an existing user by email.
        if let Some(email) = &user_info.email {
            let filter = format!("email = {:?}", email);
            let query = RecordQuery {
                filter: Some(filter),
                page: 1,
                per_page: 1,
                ..Default::default()
            };

            if let Ok(list) = self.record_service.list_records(collection_name, &query) {
                if let Some(existing_user) = list.items.into_iter().next() {
                    let record_id = existing_user
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    // Link external identity to existing user.
                    self.create_external_auth_link(
                        collection_id,
                        &record_id,
                        provider_name,
                        &user_info.id,
                    )?;

                    // If provider says email is verified, mark it verified locally.
                    if user_info.email_verified {
                        let _ = self.record_service.update_record(
                            collection_name,
                            &record_id,
                            Value::Object({
                                let mut m = serde_json::Map::new();
                                m.insert("verified".to_string(), Value::Bool(true));
                                m
                            }),
                        );
                    }

                    return Ok((existing_user, false));
                }
            }
        }

        // No existing user found — create a new one.
        let mut new_user_data = serde_json::Map::new();

        if let Some(email) = &user_info.email {
            new_user_data.insert("email".to_string(), Value::String(email.clone()));
        }

        // Mark as verified if the provider says so.
        if user_info.email_verified {
            new_user_data.insert("verified".to_string(), Value::Bool(true));
        }

        // OAuth2 users don't need a password — set emailVisibility to false by default.
        new_user_data.insert("emailVisibility".to_string(), Value::Bool(false));

        let new_record = self
            .record_service
            .create_record(collection_name, Value::Object(new_user_data))?;

        let record_id = new_record
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Link external identity.
        self.create_external_auth_link(collection_id, &record_id, provider_name, &user_info.id)?;

        Ok((new_record, true))
    }

    /// Create an external auth link record.
    fn create_external_auth_link(
        &self,
        collection_id: &str,
        record_id: &str,
        provider: &str,
        provider_id: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let auth = ExternalAuth {
            id: generate_id(),
            collection_id: collection_id.to_string(),
            record_id: record_id.to_string(),
            provider: provider.to_string(),
            provider_id: provider_id.to_string(),
            created: now.clone(),
            updated: now,
        };

        self.external_auth_repo.create(&auth)
    }
}

/// Full response for the auth-methods endpoint.
///
/// Groups authentication methods by category (password, OAuth2, OTP, MFA),
/// matching the PocketBase response format.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthMethodsResponse {
    pub password: PasswordAuthMethod,
    pub oauth2: OAuth2AuthMethod,
    pub otp: OtpAuthMethod,
    pub mfa: MfaAuthMethod,
}

/// Password authentication method details.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordAuthMethod {
    pub enabled: bool,
    pub identity_fields: Vec<String>,
}

/// OAuth2 authentication method details.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2AuthMethod {
    pub enabled: bool,
    pub providers: Vec<OAuth2ProviderInfo>,
}

/// Individual OAuth2 provider information with pre-generated auth URL.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2ProviderInfo {
    pub name: String,
    pub display_name: String,
    pub state: String,
    pub auth_url: String,
    pub code_verifier: String,
}

/// OTP authentication method details.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtpAuthMethod {
    pub enabled: bool,
}

/// MFA authentication method details.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MfaAuthMethod {
    pub enabled: bool,
    pub duration: u64,
}

/// Result of a successful OAuth2 authentication.
pub struct OAuth2AuthResult {
    pub token: String,
    pub record: HashMap<String, Value>,
    pub collection_id: String,
    pub collection_name: String,
    pub is_new_user: bool,
}
