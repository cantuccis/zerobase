//! External auth (OAuth2 account linking) types and repository trait.
//!
//! Manages the relationship between local user records and external OAuth2
//! provider identities. Each link is stored in the `_externalAuths` system
//! table.

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// A link between a local user record and an external OAuth2 identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAuth {
    pub id: String,
    pub collection_id: String,
    pub record_id: String,
    pub provider: String,
    pub provider_id: String,
    pub created: String,
    pub updated: String,
}

/// Repository trait for external auth link persistence.
///
/// Defined in core so the service layer doesn't depend on `zerobase-db` directly.
pub trait ExternalAuthRepository: Send + Sync {
    /// Find an external auth link by provider and provider-side user ID.
    ///
    /// Returns `None` if no link exists for this provider identity.
    fn find_by_provider(&self, provider: &str, provider_id: &str) -> Result<Option<ExternalAuth>>;

    /// Find all external auth links for a given local record.
    fn find_by_record(&self, collection_id: &str, record_id: &str) -> Result<Vec<ExternalAuth>>;

    /// Create a new external auth link.
    fn create(&self, auth: &ExternalAuth) -> Result<()>;

    /// Delete an external auth link by its ID.
    fn delete(&self, id: &str) -> Result<()>;

    /// Delete all external auth links for a given local record.
    fn delete_by_record(&self, collection_id: &str, record_id: &str) -> Result<()>;
}
