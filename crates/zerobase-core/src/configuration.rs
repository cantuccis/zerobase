//! Application configuration.
//!
//! Configuration is loaded from three sources, in order of increasing priority:
//!
//! 1. Compiled-in defaults (see [`Default`] impls below).
//! 2. A TOML config file — by default `zerobase.toml` in the working directory,
//!    overridden via `ZEROBASE_CONFIG` env var.
//! 3. Environment variables prefixed with `ZEROBASE__` and using `__` as a
//!    separator for nested keys (e.g. `ZEROBASE__SERVER__PORT=9090`).
//!
//! The [`Settings`] struct is the top-level entry point.  Call
//! [`Settings::load`] to build a fully-resolved configuration or
//! [`Settings::load_from`] when you want to point at a specific file (useful
//! in tests).

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::error::ZerobaseError;
use crate::telemetry::LogFormat;

// ── Top-level settings ──────────────────────────────────────────────────────

/// Root configuration for the entire Zerobase application.
#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub server: ServerSettings,
    pub database: DatabaseSettings,
    pub storage: StorageSettings,
    pub auth: AuthSettings,
    pub smtp: SmtpSettings,
    pub logs: LogsSettings,
}

impl Settings {
    /// Load configuration using the default layering strategy.
    ///
    /// 1. Compiled-in defaults
    /// 2. `zerobase.toml` (or path in `ZEROBASE_CONFIG`)
    /// 3. Environment variables (`ZEROBASE__*`)
    pub fn load() -> Result<Self, ZerobaseError> {
        let config_path =
            std::env::var("ZEROBASE_CONFIG").unwrap_or_else(|_| "zerobase.toml".to_string());

        Self::build_from_sources(Some(&config_path))
    }

    /// Load configuration from a specific TOML file path (+ defaults + env).
    ///
    /// This is the primary entry point for tests that supply a temporary file.
    pub fn load_from(path: &Path) -> Result<Self, ZerobaseError> {
        Self::build_from_sources(Some(path.to_str().unwrap_or("zerobase.toml")))
    }

    /// Load configuration from defaults and environment only (no file).
    pub fn load_from_env() -> Result<Self, ZerobaseError> {
        Self::build_from_sources(None)
    }

    fn build_from_sources(config_path: Option<&str>) -> Result<Self, ZerobaseError> {
        let mut builder = config::Config::builder()
            // Layer 1: compiled-in defaults
            .set_default("server.host", "127.0.0.1")
            .map_err(config_err)?
            .set_default("server.port", 8090)
            .map_err(config_err)?
            .set_default("server.log_format", "json")
            .map_err(config_err)?
            .set_default("server.body_limit", 10_485_760_i64) // 10 MiB
            .map_err(config_err)?
            .set_default("server.body_limit_upload", 104_857_600_i64) // 100 MiB
            .map_err(config_err)?
            .set_default("server.shutdown_timeout_secs", 30_i64)
            .map_err(config_err)?
            .set_default("database.path", "zerobase_data/data.db")
            .map_err(config_err)?
            .set_default("database.max_read_connections", 8_i64)
            .map_err(config_err)?
            .set_default("database.busy_timeout_ms", 5000_i64)
            .map_err(config_err)?
            .set_default("storage.backend", "local")
            .map_err(config_err)?
            .set_default("storage.local_path", "zerobase_data/storage")
            .map_err(config_err)?
            .set_default("auth.token_secret", "")
            .map_err(config_err)?
            .set_default("auth.token_duration_secs", 1_209_600_i64)
            .map_err(config_err)?
            .set_default("smtp.enabled", false)
            .map_err(config_err)?
            .set_default("smtp.host", "")
            .map_err(config_err)?
            .set_default("smtp.port", 587)
            .map_err(config_err)?
            .set_default("smtp.username", "")
            .map_err(config_err)?
            .set_default("smtp.password", "")
            .map_err(config_err)?
            .set_default("smtp.sender_address", "")
            .map_err(config_err)?
            .set_default("smtp.sender_name", "Zerobase")
            .map_err(config_err)?
            .set_default("smtp.tls", true)
            .map_err(config_err)?
            .set_default("logs.retention_days", 7_i64)
            .map_err(config_err)?;

        // Layer 2: config file (optional — missing file is not an error)
        if let Some(path) = config_path {
            builder = builder.add_source(config::File::with_name(path).required(false));
        }

        // Layer 3: environment variables
        builder = builder.add_source(
            config::Environment::with_prefix("ZEROBASE")
                .separator("__")
                .try_parsing(true),
        );

        let settings: Settings = builder
            .build()
            .map_err(config_err)?
            .try_deserialize()
            .map_err(config_err)?;

        settings.validate()?;

        Ok(settings)
    }

    /// Validate invariants that cannot be expressed through types alone.
    fn validate(&self) -> Result<(), ZerobaseError> {
        if self.server.port == 0 {
            return Err(ZerobaseError::validation("server.port must be > 0"));
        }
        if self.server.host.is_empty() {
            return Err(ZerobaseError::validation("server.host must not be empty"));
        }
        if self.database.path.as_os_str().is_empty() {
            return Err(ZerobaseError::validation("database.path must not be empty"));
        }
        if self.database.max_read_connections == 0 {
            return Err(ZerobaseError::validation(
                "database.max_read_connections must be > 0",
            ));
        }
        if self.database.busy_timeout_ms == 0 {
            return Err(ZerobaseError::validation(
                "database.busy_timeout_ms must be > 0",
            ));
        }
        if self.auth.token_secret.expose_secret().is_empty() {
            return Err(ZerobaseError::validation(
                "auth.token_secret is required — set ZEROBASE__AUTH__TOKEN_SECRET or add it to zerobase.toml",
            ));
        }
        if self.auth.token_duration_secs == 0 {
            return Err(ZerobaseError::validation(
                "auth.token_duration_secs must be > 0",
            ));
        }

        // If storage backend is S3, require bucket and region.
        if self.storage.backend == StorageBackend::S3 {
            let s3 = self.storage.s3.as_ref().ok_or_else(|| {
                ZerobaseError::validation(
                    "storage.s3 section is required when storage.backend = \"s3\"",
                )
            })?;
            if s3.bucket.is_empty() {
                return Err(ZerobaseError::validation(
                    "storage.s3.bucket must not be empty",
                ));
            }
            if s3.region.is_empty() {
                return Err(ZerobaseError::validation(
                    "storage.s3.region must not be empty",
                ));
            }
        }

        // If SMTP is enabled, require host and sender_address.
        if self.smtp.enabled {
            if self.smtp.host.is_empty() {
                return Err(ZerobaseError::validation(
                    "smtp.host is required when smtp.enabled = true",
                ));
            }
            if self.smtp.sender_address.is_empty() {
                return Err(ZerobaseError::validation(
                    "smtp.sender_address is required when smtp.enabled = true",
                ));
            }
        }

        Ok(())
    }
}

// ── Section structs ─────────────────────────────────────────────────────────

/// HTTP server settings.
#[derive(Debug, Deserialize, Clone)]
pub struct ServerSettings {
    /// Address to bind to (default `127.0.0.1`).
    pub host: String,
    /// Port to listen on (default `8090`).
    pub port: u16,
    /// Log output format — `json` for production, `pretty` for development.
    /// Defaults to `json`.
    pub log_format: LogFormat,
    /// Maximum request body size in bytes for regular (non-file) requests.
    /// Defaults to 10 MiB (10_485_760).
    pub body_limit: usize,
    /// Maximum request body size in bytes for file upload (multipart) requests.
    /// Defaults to 100 MiB (104_857_600).
    pub body_limit_upload: usize,
    /// Timeout in seconds for draining in-flight requests during graceful
    /// shutdown. After this period, the server force-exits even if requests
    /// are still in progress. Defaults to 30 seconds.
    pub shutdown_timeout_secs: u64,
}

impl ServerSettings {
    /// Convenience: return the `host:port` string for binding.
    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// SQLite database settings.
#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseSettings {
    /// Path to the SQLite database file (default `zerobase_data/data.db`).
    pub path: PathBuf,
    /// Maximum number of read connections in the pool (default 8).
    pub max_read_connections: u32,
    /// Busy timeout in milliseconds for SQLite (default 5000).
    pub busy_timeout_ms: u32,
}

/// File storage settings.
#[derive(Debug, Deserialize, Clone)]
pub struct StorageSettings {
    /// Which backend to use — `local` or `s3`.
    pub backend: StorageBackend,
    /// Local filesystem directory (used when `backend = "local"`).
    pub local_path: PathBuf,
    /// S3-compatible settings (required when `backend = "s3"`).
    pub s3: Option<S3Settings>,
}

/// Storage backend selector.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    Local,
    S3,
}

/// S3-compatible storage settings.
#[derive(Debug, Deserialize, Clone)]
pub struct S3Settings {
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub access_key: SecretString,
    pub secret_key: SecretString,
    pub force_path_style: Option<bool>,
}

/// Authentication / token settings.
#[derive(Debug, Deserialize, Clone)]
pub struct AuthSettings {
    /// HMAC secret used to sign JWTs.
    pub token_secret: SecretString,
    /// Token validity duration in seconds (default 14 days = 1_209_600).
    pub token_duration_secs: u64,
}

/// SMTP settings for transactional email.
#[derive(Debug, Deserialize, Clone)]
pub struct SmtpSettings {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: SecretString,
    pub sender_address: String,
    pub sender_name: String,
    pub tls: bool,
}

/// Request log settings.
#[derive(Debug, Deserialize, Clone)]
pub struct LogsSettings {
    /// How many days to keep request logs before auto-cleanup (default 7).
    pub retention_days: u32,
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Convert a [`config::ConfigError`] into our domain error type.
fn config_err(e: config::ConfigError) -> ZerobaseError {
    ZerobaseError::Internal {
        message: format!("configuration error: {e}"),
        source: Some(Box::new(e)),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Helper: set env vars scoped to a closure, then clean up.
    struct EnvGuard {
        keys: Vec<String>,
    }

    impl EnvGuard {
        fn set(pairs: &[(&str, &str)]) -> Self {
            let keys = pairs.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>();
            for (k, v) in pairs {
                unsafe { std::env::set_var(k, v) };
            }
            Self { keys }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for k in &self.keys {
                unsafe { std::env::remove_var(k) };
            }
        }
    }

    fn minimal_env() -> EnvGuard {
        EnvGuard::set(&[(
            "ZEROBASE__AUTH__TOKEN_SECRET",
            "test-secret-at-least-32-chars-long!!",
        )])
    }

    // ── Loading from TOML file ──────────────────────────────────────────

    #[test]
    #[serial]
    fn loads_from_toml_file() {
        let toml_content = r#"
[server]
host = "0.0.0.0"
port = 9090

[database]
path = "/tmp/zerobase_test.db"

[storage]
backend = "local"
local_path = "/tmp/zerobase_storage"

[auth]
token_secret = "my-super-secret-key-for-testing-that-is-long-enough"
token_duration_secs = 3600

[smtp]
enabled = false
"#;
        let mut f = NamedTempFile::with_suffix(".toml").unwrap();
        f.write_all(toml_content.as_bytes()).unwrap();

        let settings = Settings::load_from(f.path()).unwrap();

        assert_eq!(settings.server.host, "0.0.0.0");
        assert_eq!(settings.server.port, 9090);
        assert_eq!(
            settings.database.path,
            PathBuf::from("/tmp/zerobase_test.db")
        );
        assert_eq!(settings.storage.backend, StorageBackend::Local);
        assert_eq!(
            settings.storage.local_path,
            PathBuf::from("/tmp/zerobase_storage")
        );
        assert_eq!(
            settings.auth.token_secret.expose_secret(),
            "my-super-secret-key-for-testing-that-is-long-enough"
        );
        assert_eq!(settings.auth.token_duration_secs, 3600);
        assert!(!settings.smtp.enabled);
    }

    // ── Loading from environment variables ──────────────────────────────

    #[test]
    #[serial]
    fn env_vars_override_defaults() {
        let _guard = EnvGuard::set(&[
            ("ZEROBASE__SERVER__HOST", "0.0.0.0"),
            ("ZEROBASE__SERVER__PORT", "7777"),
            ("ZEROBASE__DATABASE__PATH", "/custom/db.sqlite"),
            (
                "ZEROBASE__AUTH__TOKEN_SECRET",
                "env-secret-that-is-long-enough-for-testing!!",
            ),
            ("ZEROBASE__AUTH__TOKEN_DURATION_SECS", "7200"),
        ]);

        let settings = Settings::load_from_env().unwrap();

        assert_eq!(settings.server.host, "0.0.0.0");
        assert_eq!(settings.server.port, 7777);
        assert_eq!(settings.database.path, PathBuf::from("/custom/db.sqlite"));
        assert_eq!(
            settings.auth.token_secret.expose_secret(),
            "env-secret-that-is-long-enough-for-testing!!"
        );
        assert_eq!(settings.auth.token_duration_secs, 7200);
    }

    // ── Env overrides file ──────────────────────────────────────────────

    #[test]
    #[serial]
    fn env_takes_precedence_over_file() {
        let toml_content = r#"
[server]
host = "127.0.0.1"
port = 8080

[auth]
token_secret = "file-secret-long-enough-for-testing-purposes!!"
"#;
        let mut f = NamedTempFile::with_suffix(".toml").unwrap();
        f.write_all(toml_content.as_bytes()).unwrap();

        let _guard = EnvGuard::set(&[("ZEROBASE__SERVER__PORT", "9999")]);

        let settings = Settings::load_from(f.path()).unwrap();

        // host comes from file
        assert_eq!(settings.server.host, "127.0.0.1");
        // port is overridden by env
        assert_eq!(settings.server.port, 9999);
    }

    // ── Defaults ────────────────────────────────────────────────────────

    #[test]
    #[serial]
    fn defaults_applied_for_unset_values() {
        let _guard = minimal_env();

        let settings = Settings::load_from_env().unwrap();

        assert_eq!(settings.server.host, "127.0.0.1");
        assert_eq!(settings.server.port, 8090);
        assert_eq!(
            settings.database.path,
            PathBuf::from("zerobase_data/data.db")
        );
        assert_eq!(settings.storage.backend, StorageBackend::Local);
        assert_eq!(
            settings.storage.local_path,
            PathBuf::from("zerobase_data/storage")
        );
        assert_eq!(settings.auth.token_duration_secs, 1_209_600);
        assert!(!settings.smtp.enabled);
    }

    // ── Validation: missing required fields ─────────────────────────────

    #[test]
    #[serial]
    fn missing_token_secret_produces_clear_error() {
        // No env vars, no file → token_secret is empty
        let result = Settings::load_from_env();
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("token_secret"),
            "error should mention token_secret: {msg}"
        );
    }

    #[test]
    #[serial]
    fn empty_host_rejected() {
        let _guard = EnvGuard::set(&[
            ("ZEROBASE__SERVER__HOST", ""),
            (
                "ZEROBASE__AUTH__TOKEN_SECRET",
                "test-secret-at-least-32-chars-long!!",
            ),
        ]);
        let result = Settings::load_from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("host"));
    }

    #[test]
    #[serial]
    fn zero_port_rejected() {
        let _guard = EnvGuard::set(&[
            ("ZEROBASE__SERVER__PORT", "0"),
            (
                "ZEROBASE__AUTH__TOKEN_SECRET",
                "test-secret-at-least-32-chars-long!!",
            ),
        ]);
        let result = Settings::load_from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("port"));
    }

    #[test]
    #[serial]
    fn zero_token_duration_rejected() {
        let _guard = EnvGuard::set(&[
            (
                "ZEROBASE__AUTH__TOKEN_SECRET",
                "test-secret-at-least-32-chars-long!!",
            ),
            ("ZEROBASE__AUTH__TOKEN_DURATION_SECS", "0"),
        ]);
        let result = Settings::load_from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("token_duration"));
    }

    // ── S3 validation ───────────────────────────────────────────────────

    #[test]
    #[serial]
    fn s3_backend_requires_s3_section() {
        let toml_content = r#"
[server]
host = "127.0.0.1"

[storage]
backend = "s3"

[auth]
token_secret = "test-secret-at-least-32-chars-long!!"
"#;
        let mut f = NamedTempFile::with_suffix(".toml").unwrap();
        f.write_all(toml_content.as_bytes()).unwrap();

        let result = Settings::load_from(f.path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("s3"), "error should mention s3: {msg}");
    }

    #[test]
    #[serial]
    fn s3_backend_with_valid_config_succeeds() {
        let toml_content = r#"
[storage]
backend = "s3"
local_path = "/unused"

[storage.s3]
bucket = "my-bucket"
region = "us-east-1"
access_key = "AKID"
secret_key = "SKEY"

[auth]
token_secret = "test-secret-at-least-32-chars-long!!"
"#;
        let mut f = NamedTempFile::with_suffix(".toml").unwrap();
        f.write_all(toml_content.as_bytes()).unwrap();

        let settings = Settings::load_from(f.path()).unwrap();
        assert_eq!(settings.storage.backend, StorageBackend::S3);
        let s3 = settings.storage.s3.as_ref().unwrap();
        assert_eq!(s3.bucket, "my-bucket");
        assert_eq!(s3.region, "us-east-1");
    }

    // ── SMTP validation ─────────────────────────────────────────────────

    #[test]
    #[serial]
    fn smtp_enabled_requires_host() {
        let toml_content = r#"
[smtp]
enabled = true
host = ""
sender_address = "noreply@example.com"

[auth]
token_secret = "test-secret-at-least-32-chars-long!!"
"#;
        let mut f = NamedTempFile::with_suffix(".toml").unwrap();
        f.write_all(toml_content.as_bytes()).unwrap();

        let result = Settings::load_from(f.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("smtp.host"));
    }

    #[test]
    #[serial]
    fn smtp_enabled_requires_sender_address() {
        let toml_content = r#"
[smtp]
enabled = true
host = "smtp.example.com"
sender_address = ""

[auth]
token_secret = "test-secret-at-least-32-chars-long!!"
"#;
        let mut f = NamedTempFile::with_suffix(".toml").unwrap();
        f.write_all(toml_content.as_bytes()).unwrap();

        let result = Settings::load_from(f.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("sender_address"));
    }

    #[test]
    #[serial]
    fn smtp_disabled_does_not_require_host() {
        let _guard = minimal_env();

        let settings = Settings::load_from_env().unwrap();
        assert!(!settings.smtp.enabled);
        // no error even though host is empty
    }

    // ── Full SMTP config ────────────────────────────────────────────────

    #[test]
    #[serial]
    fn full_smtp_config_loads() {
        let toml_content = r#"
[smtp]
enabled = true
host = "smtp.example.com"
port = 465
username = "user"
password = "pass"
sender_address = "noreply@example.com"
sender_name = "ZeroApp"
tls = true

[auth]
token_secret = "test-secret-at-least-32-chars-long!!"
"#;
        let mut f = NamedTempFile::with_suffix(".toml").unwrap();
        f.write_all(toml_content.as_bytes()).unwrap();

        let settings = Settings::load_from(f.path()).unwrap();
        assert!(settings.smtp.enabled);
        assert_eq!(settings.smtp.host, "smtp.example.com");
        assert_eq!(settings.smtp.port, 465);
        assert_eq!(settings.smtp.username, "user");
        assert_eq!(settings.smtp.sender_address, "noreply@example.com");
        assert_eq!(settings.smtp.sender_name, "ZeroApp");
        assert!(settings.smtp.tls);
    }

    // ── ServerSettings::address ─────────────────────────────────────────

    #[test]
    fn server_address_formats_correctly() {
        let s = ServerSettings {
            host: "0.0.0.0".to_string(),
            port: 3000,
            log_format: LogFormat::default(),
            body_limit: 10_485_760,
            body_limit_upload: 104_857_600,
            shutdown_timeout_secs: 30,
        };
        assert_eq!(s.address(), "0.0.0.0:3000");
    }

    // ── Invalid TOML produces a descriptive error ───────────────────────

    #[test]
    #[serial]
    fn invalid_toml_produces_descriptive_error() {
        let mut f = NamedTempFile::with_suffix(".toml").unwrap();
        f.write_all(b"[[[bad toml").unwrap();

        let result = Settings::load_from(f.path());
        assert!(result.is_err());
    }
}
