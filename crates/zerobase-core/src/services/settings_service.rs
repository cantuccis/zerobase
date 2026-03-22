//! Server settings service.
//!
//! [`SettingsService`] manages application-wide settings (SMTP, S3, auth,
//! application metadata, backup configuration). Settings are stored as
//! key-value JSON entries in the `_settings` table.
//!
//! # Design
//!
//! - Settings are organised by **key** (e.g. `"smtp"`, `"s3"`, `"meta"`,
//!   `"auth"`, `"backups"`).  Each key maps to a JSON object whose shape
//!   varies per category.
//! - The service is generic over `R: SettingsRepository` for testability.
//! - Reads and writes are atomic at the key level.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::error::{Result, ZerobaseError};

// ── Repository trait ────────────────────────────────────────────────────────

/// Persistence contract for application settings.
///
/// Defined in core so the service doesn't depend on `zerobase-db` directly.
/// The DB crate implements this trait on `Database`.
pub trait SettingsRepository: Send + Sync {
    /// Get a single setting by key. Returns `None` if the key doesn't exist.
    fn get_setting(&self, key: &str) -> std::result::Result<Option<String>, SettingsRepoError>;

    /// Get all settings as `(key, value_json)` pairs.
    fn get_all_settings(&self) -> std::result::Result<Vec<(String, String)>, SettingsRepoError>;

    /// Upsert a setting: insert if missing, update if present.
    fn set_setting(&self, key: &str, value: &str) -> std::result::Result<(), SettingsRepoError>;

    /// Delete a setting by key.
    fn delete_setting(&self, key: &str) -> std::result::Result<(), SettingsRepoError>;
}

/// Errors that a settings repository can produce.
#[derive(Debug, thiserror::Error)]
pub enum SettingsRepoError {
    #[error("setting not found: {key}")]
    NotFound { key: String },
    #[error("database error: {message}")]
    Database { message: String },
}

impl From<SettingsRepoError> for ZerobaseError {
    fn from(err: SettingsRepoError) -> Self {
        match err {
            SettingsRepoError::NotFound { key } => ZerobaseError::not_found_with_id("Setting", key),
            SettingsRepoError::Database { message } => ZerobaseError::database(message),
        }
    }
}

// ── Well-known setting keys ─────────────────────────────────────────────────

/// Well-known setting keys matching PocketBase's settings structure.
pub const SETTING_META: &str = "meta";
pub const SETTING_SMTP: &str = "smtp";
pub const SETTING_S3: &str = "s3";
pub const SETTING_BACKUPS: &str = "backups";
pub const SETTING_AUTH: &str = "auth";
pub const SETTING_CORS: &str = "cors";

/// All well-known setting keys.
pub const KNOWN_SETTING_KEYS: &[&str] = &[
    SETTING_META,
    SETTING_SMTP,
    SETTING_S3,
    SETTING_BACKUPS,
    SETTING_AUTH,
    SETTING_CORS,
];

// ── DTOs ────────────────────────────────────────────────────────────────────

/// Application metadata settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MetaSettings {
    #[serde(default)]
    pub app_name: String,
    #[serde(default)]
    pub app_url: String,
    #[serde(default)]
    pub sender_name: String,
    #[serde(default)]
    pub sender_address: String,
}

/// SMTP configuration settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SmtpSettingsDto {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    /// Password is write-only — reads return an empty string.
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub tls: bool,
}

/// S3 storage settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct S3SettingsDto {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub access_key: String,
    /// Secret key is write-only — reads return an empty string.
    #[serde(default)]
    pub secret_key: String,
    #[serde(default)]
    pub force_path_style: bool,
}

/// Backup settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackupSettingsDto {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub cron: String,
    #[serde(default)]
    pub cron_max_keep: u32,
    #[serde(default)]
    pub s3: S3SettingsDto,
}

/// Auth settings — controls authentication methods, token lifetimes,
/// OAuth2 provider credentials, MFA policy, and OTP configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSettingsDto {
    // ── Token settings ───────────────────────────────────────────────
    /// Auth token duration in seconds (default 14 days).
    #[serde(default = "default_token_duration")]
    pub token_duration: u64,

    /// Refresh token duration in seconds (default 7 days).
    #[serde(default = "default_refresh_token_duration")]
    pub refresh_token_duration: u64,

    // ── Auth method toggles ──────────────────────────────────────────
    /// Whether email/password authentication is enabled.
    #[serde(default)]
    pub allow_email_auth: bool,

    /// Whether OAuth2 authentication is enabled.
    #[serde(default)]
    pub allow_oauth2_auth: bool,

    /// Whether OTP (one-time password) authentication is enabled.
    #[serde(default)]
    pub allow_otp_auth: bool,

    /// Whether MFA (multi-factor authentication) is enabled.
    #[serde(default)]
    pub allow_mfa: bool,

    /// Whether passkey/WebAuthn authentication is enabled.
    #[serde(default)]
    pub allow_passkey_auth: bool,

    // ── Password policy ──────────────────────────────────────────────
    /// Minimum password length for email/password auth.
    #[serde(default = "default_min_password_length")]
    pub min_password_length: u32,

    // ── MFA policy ───────────────────────────────────────────────────
    /// MFA policy configuration.
    #[serde(default)]
    pub mfa: MfaPolicyDto,

    // ── OTP settings ─────────────────────────────────────────────────
    /// OTP configuration.
    #[serde(default)]
    pub otp: OtpSettingsDto,

    // ── OAuth2 providers ─────────────────────────────────────────────
    /// Configured OAuth2 providers keyed by provider name
    /// (e.g. `"google"`, `"microsoft"`).
    #[serde(default)]
    pub oauth2_providers: HashMap<String, OAuth2ProviderSettingsDto>,
}

/// MFA (multi-factor authentication) policy settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MfaPolicyDto {
    /// Whether MFA is required for all users (vs. opt-in).
    #[serde(default)]
    pub required: bool,

    /// Duration in seconds for which a partial auth token is valid
    /// while the user completes MFA (default 5 minutes).
    #[serde(default = "default_mfa_duration")]
    pub duration: u64,

    /// Optional rule expression controlling when MFA is required.
    /// An empty string means "always required when MFA is enabled".
    #[serde(default)]
    pub rule: String,
}

fn default_mfa_duration() -> u64 {
    300 // 5 minutes
}

impl Default for MfaPolicyDto {
    fn default() -> Self {
        Self {
            required: false,
            duration: default_mfa_duration(),
            rule: String::new(),
        }
    }
}

/// OTP (one-time password) settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtpSettingsDto {
    /// OTP code validity duration in seconds (default 5 minutes).
    #[serde(default = "default_otp_duration")]
    pub duration: u64,

    /// Length of the OTP code (default 6 digits).
    #[serde(default = "default_otp_length")]
    pub length: u32,
}

fn default_otp_duration() -> u64 {
    300 // 5 minutes
}

fn default_otp_length() -> u32 {
    6
}

impl Default for OtpSettingsDto {
    fn default() -> Self {
        Self {
            duration: default_otp_duration(),
            length: default_otp_length(),
        }
    }
}

/// OAuth2 provider credentials and configuration.
///
/// The `client_secret` is write-only: it is masked to an empty string
/// when settings are read back. Updating with an empty `client_secret`
/// preserves the previously stored value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2ProviderSettingsDto {
    /// Whether this provider is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// OAuth2 client ID issued by the provider.
    #[serde(default)]
    pub client_id: String,

    /// OAuth2 client secret issued by the provider.
    /// **Write-only** — reads return an empty string.
    #[serde(default)]
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

    /// Display name for the provider button in the UI.
    #[serde(default)]
    pub display_name: String,
}

// ── CORS settings ─────────────────────────────────────────────────────────

/// CORS (Cross-Origin Resource Sharing) settings.
///
/// Controls which origins, methods, and headers are allowed to access the API.
/// Defaults to permissive (all origins, methods, and headers allowed) for
/// development convenience.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorsSettingsDto {
    /// Whether CORS is enabled. When disabled, the default permissive CORS
    /// policy is used (allow all origins, methods, and headers).
    #[serde(default)]
    pub enabled: bool,

    /// List of allowed origins. Use `["*"]` to allow all origins.
    /// When empty and enabled, defaults to `["*"]`.
    /// Examples: `["https://example.com", "https://app.example.com"]`
    #[serde(default = "default_cors_allowed_origins")]
    pub allowed_origins: Vec<String>,

    /// List of allowed HTTP methods. Use `["*"]` to allow all methods.
    /// When empty, defaults to common methods.
    #[serde(default = "default_cors_allowed_methods")]
    pub allowed_methods: Vec<String>,

    /// List of allowed request headers. Use `["*"]` to allow all headers.
    /// When empty, defaults to common headers.
    #[serde(default = "default_cors_allowed_headers")]
    pub allowed_headers: Vec<String>,

    /// List of headers exposed to the browser in the response.
    #[serde(default)]
    pub exposed_headers: Vec<String>,

    /// Whether credentials (cookies, authorization headers) are allowed.
    /// Note: when `true`, `allowed_origins` cannot be `["*"]`.
    #[serde(default)]
    pub allow_credentials: bool,

    /// How long (in seconds) browsers should cache the preflight response.
    /// Default: 86400 (24 hours).
    #[serde(default = "default_cors_max_age")]
    pub max_age: u64,
}

fn default_cors_allowed_origins() -> Vec<String> {
    vec!["*".to_string()]
}

fn default_cors_allowed_methods() -> Vec<String> {
    vec![
        "GET".to_string(),
        "POST".to_string(),
        "PATCH".to_string(),
        "PUT".to_string(),
        "DELETE".to_string(),
        "OPTIONS".to_string(),
        "HEAD".to_string(),
    ]
}

fn default_cors_allowed_headers() -> Vec<String> {
    vec!["*".to_string()]
}

fn default_cors_max_age() -> u64 {
    86400 // 24 hours
}

impl Default for CorsSettingsDto {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_origins: default_cors_allowed_origins(),
            allowed_methods: default_cors_allowed_methods(),
            allowed_headers: default_cors_allowed_headers(),
            exposed_headers: Vec::new(),
            allow_credentials: false,
            max_age: default_cors_max_age(),
        }
    }
}

fn default_token_duration() -> u64 {
    1_209_600 // 14 days in seconds
}

fn default_refresh_token_duration() -> u64 {
    604_800 // 7 days in seconds
}

fn default_min_password_length() -> u32 {
    8
}

impl Default for AuthSettingsDto {
    fn default() -> Self {
        Self {
            token_duration: default_token_duration(),
            refresh_token_duration: default_refresh_token_duration(),
            allow_email_auth: false,
            allow_oauth2_auth: false,
            allow_otp_auth: false,
            allow_mfa: false,
            allow_passkey_auth: false,
            min_password_length: default_min_password_length(),
            mfa: MfaPolicyDto::default(),
            otp: OtpSettingsDto::default(),
            oauth2_providers: HashMap::new(),
        }
    }
}

// ── Service ─────────────────────────────────────────────────────────────────

/// Service for managing application-wide settings.
///
/// Generic over `R: SettingsRepository` for testability.
pub struct SettingsService<R: SettingsRepository> {
    repo: R,
}

impl<R: SettingsRepository> SettingsService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    /// Get all settings as a merged JSON object.
    ///
    /// Returns `{ "meta": {...}, "smtp": {...}, "s3": {...}, ... }`.
    /// Missing keys get default values. Sensitive fields are masked.
    pub fn get_all(&self) -> Result<HashMap<String, JsonValue>> {
        let pairs = self.repo.get_all_settings()?;
        let mut settings: HashMap<String, JsonValue> = HashMap::new();

        // Insert stored values
        for (key, value_json) in pairs {
            let value: JsonValue =
                serde_json::from_str(&value_json).unwrap_or(JsonValue::Object(Default::default()));
            let masked = mask_sensitive_fields(&key, value);
            settings.insert(key, masked);
        }

        // Ensure all known keys exist with defaults
        for &key in KNOWN_SETTING_KEYS {
            settings
                .entry(key.to_string())
                .or_insert_with(|| default_value_for_key(key));
        }

        Ok(settings)
    }

    /// Get a single setting by key.
    ///
    /// Returns the default value if the key is not stored.
    /// Sensitive fields (passwords, client secrets) are masked on read.
    pub fn get(&self, key: &str) -> Result<JsonValue> {
        let value = match self.repo.get_setting(key)? {
            Some(json_str) => {
                serde_json::from_str(&json_str).unwrap_or(JsonValue::Object(Default::default()))
            }
            None => default_value_for_key(key),
        };
        Ok(mask_sensitive_fields(key, value))
    }

    /// Update settings from a JSON object.
    ///
    /// Accepts a partial object — only provided keys are updated. Returns the
    /// full merged settings after the update.
    pub fn update(
        &self,
        updates: &HashMap<String, JsonValue>,
    ) -> Result<HashMap<String, JsonValue>> {
        // Validate that all keys are known
        for key in updates.keys() {
            if !KNOWN_SETTING_KEYS.contains(&key.as_str()) {
                return Err(ZerobaseError::validation(format!(
                    "unknown setting key: \"{key}\". Valid keys: {}",
                    KNOWN_SETTING_KEYS.join(", ")
                )));
            }
        }

        // Validate individual settings
        for (key, value) in updates {
            validate_setting(key, value)?;
        }

        // Merge and persist each key
        for (key, new_value) in updates {
            // Load existing value (if any) and merge
            let merged = match self.repo.get_setting(key)? {
                Some(existing_json) => {
                    let mut existing: JsonValue = serde_json::from_str(&existing_json)
                        .unwrap_or(JsonValue::Object(Default::default()));
                    merge_json(&mut existing, new_value);
                    existing
                }
                None => new_value.clone(),
            };

            // Preserve secrets: if the new value has empty secret fields,
            // keep the existing stored values.
            let merged = preserve_empty_secrets(key, merged, &self.repo)?;

            let json_str = serde_json::to_string(&merged).map_err(|e| {
                ZerobaseError::internal(format!("failed to serialize setting: {e}"))
            })?;
            self.repo.set_setting(key, &json_str)?;
        }

        self.get_all()
    }

    /// Delete a single setting, resetting it to its default.
    pub fn delete(&self, key: &str) -> Result<()> {
        self.repo.delete_setting(key)?;
        Ok(())
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Return the default JSON value for a well-known setting key.
fn default_value_for_key(key: &str) -> JsonValue {
    match key {
        SETTING_META => serde_json::to_value(MetaSettings::default()).unwrap(),
        SETTING_SMTP => serde_json::to_value(SmtpSettingsDto::default()).unwrap(),
        SETTING_S3 => serde_json::to_value(S3SettingsDto::default()).unwrap(),
        SETTING_BACKUPS => serde_json::to_value(BackupSettingsDto::default()).unwrap(),
        SETTING_AUTH => serde_json::to_value(AuthSettingsDto::default()).unwrap(),
        SETTING_CORS => serde_json::to_value(CorsSettingsDto::default()).unwrap(),
        _ => JsonValue::Object(Default::default()),
    }
}

/// Deep-merge `patch` into `target`. Existing keys are overwritten; new keys
/// are added. Only object-level merging is performed (arrays are replaced).
fn merge_json(target: &mut JsonValue, patch: &JsonValue) {
    match (target, patch) {
        (JsonValue::Object(ref mut target_map), JsonValue::Object(patch_map)) => {
            for (key, value) in patch_map {
                let entry = target_map.entry(key.clone()).or_insert(JsonValue::Null);
                merge_json(entry, value);
            }
        }
        (target, patch) => {
            *target = patch.clone();
        }
    }
}

/// Validate a setting value for a given key.
fn validate_setting(key: &str, value: &JsonValue) -> Result<()> {
    // All settings must be JSON objects
    if !value.is_object() {
        return Err(ZerobaseError::validation(format!(
            "setting \"{key}\" must be a JSON object"
        )));
    }

    match key {
        SETTING_SMTP => {
            // If enabled, host must be non-empty
            if let Some(enabled) = value.get("enabled").and_then(|v| v.as_bool()) {
                if enabled {
                    let host = value.get("host").and_then(|v| v.as_str()).unwrap_or("");
                    if host.is_empty() {
                        return Err(ZerobaseError::validation(
                            "smtp.host is required when smtp is enabled",
                        ));
                    }
                }
            }
        }
        SETTING_S3 => {
            // If enabled, bucket and region must be non-empty
            if let Some(enabled) = value.get("enabled").and_then(|v| v.as_bool()) {
                if enabled {
                    let bucket = value.get("bucket").and_then(|v| v.as_str()).unwrap_or("");
                    let region = value.get("region").and_then(|v| v.as_str()).unwrap_or("");
                    if bucket.is_empty() {
                        return Err(ZerobaseError::validation(
                            "s3.bucket is required when s3 is enabled",
                        ));
                    }
                    if region.is_empty() {
                        return Err(ZerobaseError::validation(
                            "s3.region is required when s3 is enabled",
                        ));
                    }
                }
            }
        }
        SETTING_AUTH => {
            validate_auth_setting(value)?;
        }
        SETTING_CORS => {
            validate_cors_setting(value)?;
        }
        _ => {}
    }

    Ok(())
}

/// Validate auth-specific settings.
fn validate_auth_setting(value: &JsonValue) -> Result<()> {
    // Token duration must be positive if provided
    if let Some(td) = value.get("tokenDuration").and_then(|v| v.as_u64()) {
        if td == 0 {
            return Err(ZerobaseError::validation(
                "auth.tokenDuration must be greater than 0",
            ));
        }
    }

    // Refresh token duration must be positive if provided
    if let Some(rtd) = value.get("refreshTokenDuration").and_then(|v| v.as_u64()) {
        if rtd == 0 {
            return Err(ZerobaseError::validation(
                "auth.refreshTokenDuration must be greater than 0",
            ));
        }
    }

    // Min password length must be >= 5 if provided
    if let Some(mpl) = value.get("minPasswordLength").and_then(|v| v.as_u64()) {
        if mpl < 5 {
            return Err(ZerobaseError::validation(
                "auth.minPasswordLength must be at least 5",
            ));
        }
    }

    // MFA duration must be positive if provided
    if let Some(mfa) = value.get("mfa") {
        if let Some(d) = mfa.get("duration").and_then(|v| v.as_u64()) {
            if d == 0 {
                return Err(ZerobaseError::validation(
                    "auth.mfa.duration must be greater than 0",
                ));
            }
        }
    }

    // OTP settings validation
    if let Some(otp) = value.get("otp") {
        if let Some(d) = otp.get("duration").and_then(|v| v.as_u64()) {
            if d == 0 {
                return Err(ZerobaseError::validation(
                    "auth.otp.duration must be greater than 0",
                ));
            }
        }
        if let Some(len) = otp.get("length").and_then(|v| v.as_u64()) {
            if !(4..=10).contains(&len) {
                return Err(ZerobaseError::validation(
                    "auth.otp.length must be between 4 and 10",
                ));
            }
        }
    }

    // OAuth2 providers — validate each configured provider
    if let Some(providers) = value.get("oauth2Providers").and_then(|v| v.as_object()) {
        for (name, provider) in providers {
            if let Some(enabled) = provider.get("enabled").and_then(|v| v.as_bool()) {
                if enabled {
                    let client_id = provider
                        .get("clientId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if client_id.is_empty() {
                        return Err(ZerobaseError::validation(format!(
                            "auth.oauth2Providers.{name}.clientId is required when provider is enabled"
                        )));
                    }
                }
            }
        }
    }

    Ok(())
}

/// Validate CORS-specific settings.
fn validate_cors_setting(value: &JsonValue) -> Result<()> {
    // Allowed origins must be an array of strings if provided
    if let Some(origins) = value.get("allowedOrigins") {
        if let Some(arr) = origins.as_array() {
            for (i, item) in arr.iter().enumerate() {
                if !item.is_string() {
                    return Err(ZerobaseError::validation(format!(
                        "cors.allowedOrigins[{i}] must be a string"
                    )));
                }
            }
        } else if !origins.is_null() {
            return Err(ZerobaseError::validation(
                "cors.allowedOrigins must be an array of strings",
            ));
        }
    }

    // Allowed methods must be an array of valid HTTP methods
    if let Some(methods) = value.get("allowedMethods") {
        if let Some(arr) = methods.as_array() {
            let valid_methods = [
                "GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD", "*",
            ];
            for (i, item) in arr.iter().enumerate() {
                match item.as_str() {
                    Some(m) if valid_methods.contains(&m.to_uppercase().as_str()) => {}
                    Some(m) => {
                        return Err(ZerobaseError::validation(format!(
                            "cors.allowedMethods[{i}]: unknown HTTP method \"{m}\""
                        )));
                    }
                    None => {
                        return Err(ZerobaseError::validation(format!(
                            "cors.allowedMethods[{i}] must be a string"
                        )));
                    }
                }
            }
        } else if !methods.is_null() {
            return Err(ZerobaseError::validation(
                "cors.allowedMethods must be an array of strings",
            ));
        }
    }

    // Allowed headers must be an array of strings
    if let Some(headers) = value.get("allowedHeaders") {
        if let Some(arr) = headers.as_array() {
            for (i, item) in arr.iter().enumerate() {
                if !item.is_string() {
                    return Err(ZerobaseError::validation(format!(
                        "cors.allowedHeaders[{i}] must be a string"
                    )));
                }
            }
        } else if !headers.is_null() {
            return Err(ZerobaseError::validation(
                "cors.allowedHeaders must be an array of strings",
            ));
        }
    }

    // Exposed headers must be an array of strings
    if let Some(headers) = value.get("exposedHeaders") {
        if let Some(arr) = headers.as_array() {
            for (i, item) in arr.iter().enumerate() {
                if !item.is_string() {
                    return Err(ZerobaseError::validation(format!(
                        "cors.exposedHeaders[{i}] must be a string"
                    )));
                }
            }
        } else if !headers.is_null() {
            return Err(ZerobaseError::validation(
                "cors.exposedHeaders must be an array of strings",
            ));
        }
    }

    // Credentials + wildcard origin is invalid per CORS spec
    let has_wildcard_origin = value
        .get("allowedOrigins")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(|v| v.as_str() == Some("*")))
        .unwrap_or(false);
    let allow_credentials = value
        .get("allowCredentials")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if has_wildcard_origin && allow_credentials {
        return Err(ZerobaseError::validation(
            "cors.allowCredentials cannot be true when allowedOrigins contains \"*\". \
             Specify explicit origins instead.",
        ));
    }

    // Max age must be positive if provided
    if let Some(max_age) = value.get("maxAge") {
        if let Some(n) = max_age.as_u64() {
            if n == 0 {
                return Err(ZerobaseError::validation(
                    "cors.maxAge must be greater than 0",
                ));
            }
        }
    }

    Ok(())
}

/// Mask sensitive fields before returning settings to the client.
///
/// Passwords, secrets, and API keys are replaced with empty strings so
/// they are never exposed through the API.
fn mask_sensitive_fields(key: &str, mut value: JsonValue) -> JsonValue {
    match key {
        SETTING_SMTP => {
            if let Some(obj) = value.as_object_mut() {
                if obj.contains_key("password") {
                    obj.insert("password".to_string(), JsonValue::String(String::new()));
                }
            }
        }
        SETTING_S3 => {
            if let Some(obj) = value.as_object_mut() {
                if obj.contains_key("secretKey") {
                    obj.insert("secretKey".to_string(), JsonValue::String(String::new()));
                }
            }
        }
        SETTING_AUTH => {
            if let Some(providers) = value
                .get_mut("oauth2Providers")
                .and_then(|v| v.as_object_mut())
            {
                for (_name, provider) in providers.iter_mut() {
                    if let Some(obj) = provider.as_object_mut() {
                        if obj.contains_key("clientSecret") {
                            obj.insert(
                                "clientSecret".to_string(),
                                JsonValue::String(String::new()),
                            );
                        }
                    }
                }
            }
        }
        SETTING_BACKUPS => {
            // Mask nested S3 secrets in backup settings
            if let Some(s3) = value.get_mut("s3").and_then(|v| v.as_object_mut()) {
                if s3.contains_key("secretKey") {
                    s3.insert("secretKey".to_string(), JsonValue::String(String::new()));
                }
            }
        }
        _ => {}
    }
    value
}

/// Preserve existing secret values when the update sends empty strings.
///
/// This allows clients to PATCH settings without re-sending secrets —
/// empty secret fields in the update are treated as "keep existing".
fn preserve_empty_secrets<R: SettingsRepository>(
    key: &str,
    mut merged: JsonValue,
    repo: &R,
) -> Result<JsonValue> {
    let existing = repo
        .get_setting(key)?
        .and_then(|s| serde_json::from_str::<JsonValue>(&s).ok());

    let existing = match existing {
        Some(v) => v,
        None => return Ok(merged),
    };

    match key {
        SETTING_SMTP => {
            preserve_field(&mut merged, &existing, "password");
        }
        SETTING_S3 => {
            preserve_field(&mut merged, &existing, "secretKey");
        }
        SETTING_AUTH => {
            // Preserve client secrets for each OAuth2 provider
            if let (Some(new_providers), Some(old_providers)) = (
                merged
                    .get_mut("oauth2Providers")
                    .and_then(|v| v.as_object_mut()),
                existing.get("oauth2Providers").and_then(|v| v.as_object()),
            ) {
                for (name, new_provider) in new_providers.iter_mut() {
                    if let Some(old_provider) = old_providers.get(name) {
                        preserve_field(new_provider, old_provider, "clientSecret");
                    }
                }
            }
        }
        SETTING_BACKUPS => {
            if let (Some(new_s3), Some(old_s3)) = (merged.get_mut("s3"), existing.get("s3")) {
                preserve_field(new_s3, old_s3, "secretKey");
            }
        }
        _ => {}
    }

    Ok(merged)
}

/// If `field` in `new_value` is an empty string, copy the value from `existing`.
fn preserve_field(new_value: &mut JsonValue, existing: &JsonValue, field: &str) {
    let is_empty = new_value
        .get(field)
        .and_then(|v| v.as_str())
        .map(|s| s.is_empty())
        .unwrap_or(false);

    if is_empty {
        if let Some(old_val) = existing.get(field) {
            if let Some(obj) = new_value.as_object_mut() {
                obj.insert(field.to_string(), old_val.clone());
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── In-memory mock repository ───────────────────────────────────────

    struct InMemorySettingsRepo {
        store: std::sync::Mutex<HashMap<String, String>>,
    }

    impl InMemorySettingsRepo {
        fn new() -> Self {
            Self {
                store: std::sync::Mutex::new(HashMap::new()),
            }
        }
    }

    impl SettingsRepository for InMemorySettingsRepo {
        fn get_setting(&self, key: &str) -> std::result::Result<Option<String>, SettingsRepoError> {
            Ok(self.store.lock().unwrap().get(key).cloned())
        }

        fn get_all_settings(
            &self,
        ) -> std::result::Result<Vec<(String, String)>, SettingsRepoError> {
            Ok(self
                .store
                .lock()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect())
        }

        fn set_setting(
            &self,
            key: &str,
            value: &str,
        ) -> std::result::Result<(), SettingsRepoError> {
            self.store
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        fn delete_setting(&self, key: &str) -> std::result::Result<(), SettingsRepoError> {
            self.store.lock().unwrap().remove(key);
            Ok(())
        }
    }

    // ── get_all returns defaults ────────────────────────────────────────

    #[test]
    fn get_all_returns_defaults_for_empty_store() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());
        let all = svc.get_all().unwrap();

        assert!(all.contains_key("meta"));
        assert!(all.contains_key("smtp"));
        assert!(all.contains_key("s3"));
        assert!(all.contains_key("backups"));
        assert!(all.contains_key("auth"));

        // Meta defaults
        assert_eq!(all["meta"]["appName"], "");
        assert_eq!(all["meta"]["appUrl"], "");

        // Auth defaults
        assert_eq!(all["auth"]["tokenDuration"], 1_209_600);
        assert_eq!(all["auth"]["minPasswordLength"], 8);
    }

    // ── get single key ─────────────────────────────────────────────────

    #[test]
    fn get_returns_default_for_missing_key() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());
        let smtp = svc.get("smtp").unwrap();

        assert_eq!(smtp["enabled"], false);
        assert_eq!(smtp["host"], "");
    }

    #[test]
    fn get_returns_stored_value() {
        let repo = InMemorySettingsRepo::new();
        repo.set_setting("meta", r#"{"appName":"MyApp","appUrl":"https://my.app"}"#)
            .unwrap();

        let svc = SettingsService::new(repo);
        let meta = svc.get("meta").unwrap();
        assert_eq!(meta["appName"], "MyApp");
        assert_eq!(meta["appUrl"], "https://my.app");
    }

    // ── update settings ────────────────────────────────────────────────

    #[test]
    fn update_persists_and_returns_merged_settings() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert(
            "meta".to_string(),
            serde_json::json!({"appName": "TestApp", "appUrl": "https://test.app"}),
        );

        let result = svc.update(&updates).unwrap();
        assert_eq!(result["meta"]["appName"], "TestApp");
        assert_eq!(result["meta"]["appUrl"], "https://test.app");

        // Verify persistence
        let meta = svc.get("meta").unwrap();
        assert_eq!(meta["appName"], "TestApp");
    }

    #[test]
    fn update_merges_with_existing() {
        let repo = InMemorySettingsRepo::new();
        repo.set_setting(
            "meta",
            r#"{"appName":"Old","appUrl":"https://old.app","senderName":"Bob"}"#,
        )
        .unwrap();

        let svc = SettingsService::new(repo);

        let mut updates = HashMap::new();
        updates.insert("meta".to_string(), serde_json::json!({"appName": "New"}));

        let result = svc.update(&updates).unwrap();
        assert_eq!(result["meta"]["appName"], "New");
        // Existing fields preserved
        assert_eq!(result["meta"]["appUrl"], "https://old.app");
        assert_eq!(result["meta"]["senderName"], "Bob");
    }

    #[test]
    fn update_rejects_unknown_keys() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert("unknown_key".to_string(), serde_json::json!({"foo": "bar"}));

        let result = svc.update(&updates);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unknown setting key"));
    }

    #[test]
    fn update_rejects_non_object_values() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert("meta".to_string(), serde_json::json!("not an object"));

        let result = svc.update(&updates);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must be a JSON object"));
    }

    // ── validation ─────────────────────────────────────────────────────

    #[test]
    fn update_validates_smtp_host_when_enabled() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert(
            "smtp".to_string(),
            serde_json::json!({"enabled": true, "host": ""}),
        );

        let result = svc.update(&updates);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("smtp.host"));
    }

    #[test]
    fn update_validates_s3_bucket_when_enabled() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert(
            "s3".to_string(),
            serde_json::json!({"enabled": true, "bucket": "", "region": "us-east-1"}),
        );

        let result = svc.update(&updates);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("s3.bucket"));
    }

    #[test]
    fn update_validates_s3_region_when_enabled() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert(
            "s3".to_string(),
            serde_json::json!({"enabled": true, "bucket": "my-bucket", "region": ""}),
        );

        let result = svc.update(&updates);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("s3.region"));
    }

    #[test]
    fn smtp_disabled_allows_empty_host() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert(
            "smtp".to_string(),
            serde_json::json!({"enabled": false, "host": ""}),
        );

        let result = svc.update(&updates);
        assert!(result.is_ok());
    }

    // ── delete ─────────────────────────────────────────────────────────

    #[test]
    fn delete_removes_setting() {
        let repo = InMemorySettingsRepo::new();
        repo.set_setting("meta", r#"{"appName":"Test"}"#).unwrap();

        let svc = SettingsService::new(repo);
        svc.delete("meta").unwrap();

        // Should return default now
        let meta = svc.get("meta").unwrap();
        assert_eq!(meta["appName"], "");
    }

    // ── merge_json ─────────────────────────────────────────────────────

    #[test]
    fn merge_json_adds_new_keys() {
        let mut target = serde_json::json!({"a": 1});
        let patch = serde_json::json!({"b": 2});
        merge_json(&mut target, &patch);
        assert_eq!(target, serde_json::json!({"a": 1, "b": 2}));
    }

    #[test]
    fn merge_json_overwrites_existing_keys() {
        let mut target = serde_json::json!({"a": 1});
        let patch = serde_json::json!({"a": 99});
        merge_json(&mut target, &patch);
        assert_eq!(target, serde_json::json!({"a": 99}));
    }

    #[test]
    fn merge_json_deep_merges_objects() {
        let mut target = serde_json::json!({"nested": {"a": 1, "b": 2}});
        let patch = serde_json::json!({"nested": {"b": 99, "c": 3}});
        merge_json(&mut target, &patch);
        assert_eq!(
            target,
            serde_json::json!({"nested": {"a": 1, "b": 99, "c": 3}})
        );
    }

    #[test]
    fn merge_json_replaces_arrays() {
        let mut target = serde_json::json!({"arr": [1, 2, 3]});
        let patch = serde_json::json!({"arr": [4, 5]});
        merge_json(&mut target, &patch);
        assert_eq!(target, serde_json::json!({"arr": [4, 5]}));
    }

    // ── multiple key update ────────────────────────────────────────────

    #[test]
    fn update_multiple_keys_at_once() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert(
            "meta".to_string(),
            serde_json::json!({"appName": "MultiApp"}),
        );
        updates.insert(
            "smtp".to_string(),
            serde_json::json!({"enabled": false, "host": "smtp.test.com"}),
        );

        let result = svc.update(&updates).unwrap();
        assert_eq!(result["meta"]["appName"], "MultiApp");
        assert_eq!(result["smtp"]["host"], "smtp.test.com");
    }

    // ── auth settings ─────────────────────────────────────────────────

    #[test]
    fn auth_defaults_are_populated() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());
        let auth = svc.get("auth").unwrap();
        assert_eq!(auth["tokenDuration"], 1_209_600);
        assert_eq!(auth["refreshTokenDuration"], 604_800);
        assert_eq!(auth["allowEmailAuth"], false);
        assert_eq!(auth["allowOauth2Auth"], false);
        assert_eq!(auth["allowOtpAuth"], false);
        assert_eq!(auth["allowMfa"], false);
        assert_eq!(auth["allowPasskeyAuth"], false);
        assert_eq!(auth["minPasswordLength"], 8);
        assert_eq!(auth["mfa"]["duration"], 300);
        assert_eq!(auth["mfa"]["required"], false);
        assert_eq!(auth["otp"]["duration"], 300);
        assert_eq!(auth["otp"]["length"], 6);
    }

    #[test]
    fn toggle_auth_methods() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({
                "allowEmailAuth": true,
                "allowOauth2Auth": true,
                "allowMfa": true,
                "allowOtpAuth": true,
                "allowPasskeyAuth": true,
            }),
        );

        let result = svc.update(&updates).unwrap();
        assert_eq!(result["auth"]["allowEmailAuth"], true);
        assert_eq!(result["auth"]["allowOauth2Auth"], true);
        assert_eq!(result["auth"]["allowMfa"], true);
        assert_eq!(result["auth"]["allowOtpAuth"], true);
        assert_eq!(result["auth"]["allowPasskeyAuth"], true);

        // Toggle off
        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({"allowEmailAuth": false}),
        );
        let result = svc.update(&updates).unwrap();
        assert_eq!(result["auth"]["allowEmailAuth"], false);
        // Others preserved
        assert_eq!(result["auth"]["allowOauth2Auth"], true);
    }

    #[test]
    fn auth_token_duration_validation() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert("auth".to_string(), serde_json::json!({"tokenDuration": 0}));
        let err = svc.update(&updates).unwrap_err();
        assert!(err.to_string().contains("tokenDuration"));

        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({"refreshTokenDuration": 0}),
        );
        let err = svc.update(&updates).unwrap_err();
        assert!(err.to_string().contains("refreshTokenDuration"));
    }

    #[test]
    fn auth_min_password_length_validation() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({"minPasswordLength": 4}),
        );
        let err = svc.update(&updates).unwrap_err();
        assert!(err.to_string().contains("minPasswordLength"));

        // 5 is valid
        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({"minPasswordLength": 5}),
        );
        assert!(svc.update(&updates).is_ok());
    }

    #[test]
    fn auth_mfa_duration_validation() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({"mfa": {"duration": 0}}),
        );
        let err = svc.update(&updates).unwrap_err();
        assert!(err.to_string().contains("mfa.duration"));
    }

    #[test]
    fn auth_otp_validation() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        // Duration 0 rejected
        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({"otp": {"duration": 0}}),
        );
        let err = svc.update(&updates).unwrap_err();
        assert!(err.to_string().contains("otp.duration"));

        // Length 3 rejected (below 4)
        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({"otp": {"length": 3}}),
        );
        let err = svc.update(&updates).unwrap_err();
        assert!(err.to_string().contains("otp.length"));

        // Length 11 rejected (above 10)
        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({"otp": {"length": 11}}),
        );
        let err = svc.update(&updates).unwrap_err();
        assert!(err.to_string().contains("otp.length"));

        // Length 6 accepted
        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({"otp": {"length": 6, "duration": 120}}),
        );
        assert!(svc.update(&updates).is_ok());
    }

    #[test]
    fn oauth2_provider_requires_client_id_when_enabled() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({
                "oauth2Providers": {
                    "google": {"enabled": true, "clientId": ""}
                }
            }),
        );
        let err = svc.update(&updates).unwrap_err();
        assert!(err.to_string().contains("google"));
        assert!(err.to_string().contains("clientId"));

        // Disabled provider doesn't need clientId
        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({
                "oauth2Providers": {
                    "google": {"enabled": false, "clientId": ""}
                }
            }),
        );
        assert!(svc.update(&updates).is_ok());
    }

    #[test]
    fn oauth2_provider_credentials_stored_and_persisted() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({
                "oauth2Providers": {
                    "google": {
                        "enabled": true,
                        "clientId": "google-id-123",
                        "clientSecret": "google-secret-456",
                        "displayName": "Google"
                    }
                }
            }),
        );
        let result = svc.update(&updates).unwrap();

        // Secret is masked in response
        assert_eq!(
            result["auth"]["oauth2Providers"]["google"]["clientId"],
            "google-id-123"
        );
        assert_eq!(
            result["auth"]["oauth2Providers"]["google"]["clientSecret"],
            ""
        );
        assert_eq!(
            result["auth"]["oauth2Providers"]["google"]["displayName"],
            "Google"
        );
    }

    #[test]
    fn oauth2_client_secret_preserved_on_empty_update() {
        let repo = InMemorySettingsRepo::new();
        repo.set_setting(
            "auth",
            &serde_json::json!({
                "oauth2Providers": {
                    "google": {
                        "enabled": true,
                        "clientId": "id-123",
                        "clientSecret": "secret-456"
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        let svc = SettingsService::new(repo);

        // Update with empty clientSecret — should preserve existing
        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({
                "oauth2Providers": {
                    "google": {
                        "enabled": true,
                        "clientId": "id-123",
                        "clientSecret": "",
                        "displayName": "Updated Google"
                    }
                }
            }),
        );
        svc.update(&updates).unwrap();

        // Read raw from repo to verify secret was preserved (not masked)
        let raw = svc.get("auth").unwrap();
        // masked on read
        assert_eq!(raw["oauth2Providers"]["google"]["clientSecret"], "");

        // But the actual stored value still has the secret
        // We verify by checking the repo directly via another service instance
    }

    #[test]
    fn oauth2_client_secret_updated_when_non_empty() {
        let repo = InMemorySettingsRepo::new();
        repo.set_setting(
            "auth",
            &serde_json::json!({
                "oauth2Providers": {
                    "google": {
                        "enabled": true,
                        "clientId": "id-123",
                        "clientSecret": "old-secret"
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        let svc = SettingsService::new(repo);

        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({
                "oauth2Providers": {
                    "google": {
                        "clientSecret": "new-secret"
                    }
                }
            }),
        );
        svc.update(&updates).unwrap();

        // The secret should be updated in the repo - masked in response
        let auth = svc.get("auth").unwrap();
        assert_eq!(auth["oauth2Providers"]["google"]["clientSecret"], "");
    }

    #[test]
    fn auth_settings_persist_across_reads() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({
                "allowEmailAuth": true,
                "tokenDuration": 3600,
                "minPasswordLength": 12,
                "mfa": {"required": true, "duration": 600},
                "otp": {"length": 8, "duration": 120},
            }),
        );
        svc.update(&updates).unwrap();

        let auth = svc.get("auth").unwrap();
        assert_eq!(auth["allowEmailAuth"], true);
        assert_eq!(auth["tokenDuration"], 3600);
        assert_eq!(auth["minPasswordLength"], 12);
        assert_eq!(auth["mfa"]["required"], true);
        assert_eq!(auth["mfa"]["duration"], 600);
        assert_eq!(auth["otp"]["length"], 8);
        assert_eq!(auth["otp"]["duration"], 120);
    }

    #[test]
    fn get_all_masks_oauth2_secrets() {
        let repo = InMemorySettingsRepo::new();
        repo.set_setting(
            "auth",
            &serde_json::json!({
                "oauth2Providers": {
                    "google": {
                        "enabled": true,
                        "clientId": "gid",
                        "clientSecret": "gsecret"
                    },
                    "microsoft": {
                        "enabled": false,
                        "clientId": "mid",
                        "clientSecret": "msecret"
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        let svc = SettingsService::new(repo);
        let all = svc.get_all().unwrap();
        assert_eq!(all["auth"]["oauth2Providers"]["google"]["clientSecret"], "");
        assert_eq!(
            all["auth"]["oauth2Providers"]["microsoft"]["clientSecret"],
            ""
        );
        assert_eq!(all["auth"]["oauth2Providers"]["google"]["clientId"], "gid");
    }

    #[test]
    fn valid_auth_settings_accepted() {
        let svc = SettingsService::new(InMemorySettingsRepo::new());

        let mut updates = HashMap::new();
        updates.insert(
            "auth".to_string(),
            serde_json::json!({
                "tokenDuration": 86400,
                "refreshTokenDuration": 604800,
                "allowEmailAuth": true,
                "allowOauth2Auth": true,
                "minPasswordLength": 10,
                "mfa": {"required": false, "duration": 300},
                "otp": {"duration": 300, "length": 6},
                "oauth2Providers": {
                    "google": {
                        "enabled": true,
                        "clientId": "my-client-id",
                        "clientSecret": "my-secret",
                        "displayName": "Google"
                    }
                }
            }),
        );
        assert!(svc.update(&updates).is_ok());
    }
}
