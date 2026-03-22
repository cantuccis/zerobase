//! Zerobase Core — domain types, validation, and shared abstractions.
//!
//! This crate contains no I/O and no framework dependencies.
//! It defines the vocabulary used by all other crates in the workspace.

pub mod auth;
pub mod configuration;
pub mod email;
pub mod error;
pub mod hooks;
pub mod id;
pub mod oauth;
pub mod schema;
pub mod services;
pub mod storage;
pub mod telemetry;
pub mod webhooks;

// Re-export the most commonly used types at the crate root.
pub use auth::{TokenClaims, TokenService, TokenType, ValidatedToken};
pub use configuration::Settings;
pub use error::{ErrorResponseBody, FieldError, ZerobaseError};
pub use id::{generate_id, InvalidRecordId, RecordId};
pub use oauth::{OAuthProvider, OAuthProviderConfig, OAuthProviderRegistry, OAuthUserInfo};
pub use schema::{ApiRules, Collection, CollectionType, Field, FieldType, RecordValidator};
pub use services::BackupService;
pub use services::CollectionService;
pub use services::LogService;
pub use services::RecordService;
pub use services::SettingsService;
pub use services::SuperuserService;
pub use hooks::{Hook, HookAuthInfo, HookContext, HookPhase, HookRegistry, HookResult, RecordOperation};
pub use storage::{FileStorage, StorageError};
pub use telemetry::{init_tracing, LogFormat};
