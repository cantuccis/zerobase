//! WebAuthn credential types and repository trait.
//!
//! Manages the relationship between local user records and their WebAuthn
//! (passkey) credentials. Each credential is stored in the `_webauthn_credentials`
//! system table.

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// A stored WebAuthn credential linking a passkey to a user record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebauthnCredential {
    pub id: String,
    pub collection_id: String,
    pub record_id: String,
    /// A human-readable label for this passkey (e.g. "MacBook Touch ID").
    pub name: String,
    /// The credential data serialized as JSON (from webauthn-rs).
    pub credential_data: String,
    /// The credential ID in base64url encoding, for lookups during authentication.
    pub credential_id: String,
    pub created: String,
    pub updated: String,
}

/// Repository trait for WebAuthn credential persistence.
///
/// Defined in core so the service layer doesn't depend on `zerobase-db` directly.
pub trait WebauthnCredentialRepository: Send + Sync {
    /// Find a credential by its WebAuthn credential ID (base64url-encoded).
    fn find_by_credential_id(&self, credential_id: &str) -> Result<Option<WebauthnCredential>>;

    /// Find all credentials for a given local record.
    fn find_by_record(
        &self,
        collection_id: &str,
        record_id: &str,
    ) -> Result<Vec<WebauthnCredential>>;

    /// Find all credentials for a given collection (needed for authentication
    /// discovery — we need to know which credentials belong to which users).
    fn find_by_collection(&self, collection_id: &str) -> Result<Vec<WebauthnCredential>>;

    /// Create a new credential.
    fn create(&self, credential: &WebauthnCredential) -> Result<()>;

    /// Delete a credential by its ID.
    fn delete(&self, id: &str) -> Result<()>;

    /// Delete all credentials for a given local record.
    fn delete_by_record(&self, collection_id: &str, record_id: &str) -> Result<()>;
}
