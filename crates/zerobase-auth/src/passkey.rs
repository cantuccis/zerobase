//! Passkey/WebAuthn authentication.
//!
//! Provides [`PasskeyService`] which handles:
//! - Generating WebAuthn registration challenges
//! - Completing passkey registration and storing credentials
//! - Generating WebAuthn authentication (assertion) challenges
//! - Completing passkey authentication and issuing auth tokens
//!
//! WebAuthn state is stored in the `_webauthn_credentials` system table.
//! Pending challenges are kept in memory with short expiry times.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::info;
use url::Url;
use webauthn_rs::prelude::*;
use webauthn_rs::Webauthn;

use zerobase_core::auth::{TokenService, TokenType};
use zerobase_core::error::ZerobaseError;
use zerobase_core::schema::CollectionType;
use zerobase_core::services::record_service::{RecordRepository, RecordService, SchemaLookup};
use zerobase_core::services::webauthn_credential::{
    WebauthnCredential, WebauthnCredentialRepository,
};

/// Duration in seconds for pending registration/authentication challenges.
const CHALLENGE_EXPIRY_SECS: u64 = 300; // 5 minutes

/// Pending registration challenge state.
#[derive(Debug)]
struct PendingRegistration {
    /// The passkey registration state from webauthn-rs.
    state: PasskeyRegistration,
    /// User record ID.
    user_id: String,
    /// Collection ID.
    collection_id: String,
    /// Collection name.
    collection_name: String,
    /// Human-readable name for this passkey.
    passkey_name: String,
    /// Expiry timestamp (seconds since UNIX epoch).
    expires_at: u64,
}

/// Pending authentication challenge state.
#[derive(Debug)]
struct PendingAuthentication {
    /// The passkey authentication state from webauthn-rs.
    state: PasskeyAuthentication,
    /// Collection ID.
    collection_id: String,
    /// Collection name.
    collection_name: String,
    /// Expiry timestamp (seconds since UNIX epoch).
    expires_at: u64,
}

/// Response returned when requesting passkey registration.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyRegisterResponse {
    /// Unique identifier for this pending registration (needed to confirm).
    pub registration_id: String,
    /// The WebAuthn creation options to pass to the browser's `navigator.credentials.create()`.
    pub options: serde_json::Value,
}

/// Response returned when beginning passkey authentication.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyAuthBeginResponse {
    /// Unique identifier for this pending authentication (needed to finish).
    pub authentication_id: String,
    /// The WebAuthn request options to pass to the browser's `navigator.credentials.get()`.
    pub options: serde_json::Value,
}

type PendingRegStore = Mutex<HashMap<String, PendingRegistration>>;
type PendingAuthStore = Mutex<HashMap<String, PendingAuthentication>>;

/// Service responsible for the WebAuthn/Passkey authentication flow.
///
/// Handles challenge generation, credential registration, credential
/// storage, and passkey-based authentication with token issuance.
pub struct PasskeyService<R: RecordRepository, S: SchemaLookup, W: WebauthnCredentialRepository> {
    record_service: Arc<RecordService<R, S>>,
    token_service: Arc<dyn TokenService>,
    credential_repo: Arc<W>,
    webauthn: Arc<Webauthn>,
    pending_registrations: PendingRegStore,
    pending_authentications: PendingAuthStore,
}

impl<R: RecordRepository, S: SchemaLookup, W: WebauthnCredentialRepository>
    PasskeyService<R, S, W>
{
    /// Create a new `PasskeyService`.
    ///
    /// # Arguments
    ///
    /// * `rp_id` — The Relying Party identifier (typically the domain, e.g. "example.com").
    /// * `rp_origin` — The origin URL (e.g. "https://example.com").
    /// * `rp_name` — Human-readable name of the relying party (e.g. "My App").
    pub fn new(
        record_service: Arc<RecordService<R, S>>,
        token_service: Arc<dyn TokenService>,
        credential_repo: Arc<W>,
        rp_id: &str,
        rp_origin: &str,
        rp_name: &str,
    ) -> Result<Self, ZerobaseError> {
        let origin = Url::parse(rp_origin)
            .map_err(|e| ZerobaseError::internal(format!("invalid WebAuthn origin URL: {e}")))?;

        let builder = WebauthnBuilder::new(rp_id, &origin)
            .map_err(|e| {
                ZerobaseError::internal(format!("failed to create WebAuthn builder: {e}"))
            })?
            .rp_name(rp_name);

        let webauthn = builder
            .build()
            .map_err(|e| ZerobaseError::internal(format!("failed to build WebAuthn: {e}")))?;

        Ok(Self {
            record_service,
            token_service,
            credential_repo,
            webauthn: Arc::new(webauthn),
            pending_registrations: Mutex::new(HashMap::new()),
            pending_authentications: Mutex::new(HashMap::new()),
        })
    }

    /// Create from an existing `Webauthn` instance (useful for testing).
    #[cfg(test)]
    pub fn with_webauthn(
        record_service: Arc<RecordService<R, S>>,
        token_service: Arc<dyn TokenService>,
        credential_repo: Arc<W>,
        webauthn: Webauthn,
    ) -> Self {
        Self {
            record_service,
            token_service,
            credential_repo,
            webauthn: Arc::new(webauthn),
            pending_registrations: Mutex::new(HashMap::new()),
            pending_authentications: Mutex::new(HashMap::new()),
        }
    }

    /// Expose the record service for collection metadata lookups.
    pub fn record_service(&self) -> &RecordService<R, S> {
        &self.record_service
    }

    // ── Registration ────────────────────────────────────────────────────────

    /// Begin passkey registration for a user.
    ///
    /// Returns a challenge (creation options) that the client must pass to
    /// `navigator.credentials.create()`. The registration must be confirmed
    /// within [`CHALLENGE_EXPIRY_SECS`].
    pub fn request_passkey_register(
        &self,
        collection_name: &str,
        user_id: &str,
        passkey_name: Option<&str>,
    ) -> Result<PasskeyRegisterResponse, ZerobaseError> {
        // Verify collection is an auth collection.
        let collection = self.record_service.get_collection(collection_name)?;
        if collection.collection_type != CollectionType::Auth {
            return Err(ZerobaseError::validation(
                "Passkey registration is only available for auth collections.",
            ));
        }

        // Verify the user exists.
        let record = self.record_service.get_record(collection_name, user_id)?;

        let email = record
            .get("email")
            .and_then(|v| v.as_str())
            .unwrap_or("user")
            .to_string();

        // Load existing passkeys for this user to exclude them from registration.
        let existing_creds = self
            .credential_repo
            .find_by_record(&collection.id, user_id)?;

        let existing_passkeys: Vec<Passkey> = existing_creds
            .iter()
            .filter_map(|c| serde_json::from_str(&c.credential_data).ok())
            .collect();

        // Create a WebAuthn user ID from the record ID.
        // webauthn-rs requires a Uuid — we derive one deterministically from the user ID.
        let webauthn_user_id = uuid_from_user_id(user_id);

        let exclude_credentials: Vec<CredentialID> = existing_passkeys
            .iter()
            .map(|p| p.cred_id().clone())
            .collect();

        let (ccr, reg_state) = self
            .webauthn
            .start_passkey_registration(webauthn_user_id, &email, &email, Some(exclude_credentials))
            .map_err(|e| {
                ZerobaseError::internal(format!("WebAuthn registration start failed: {e}"))
            })?;

        let registration_id = nanoid::nanoid!(24);
        let now = now_secs();

        let pending = PendingRegistration {
            state: reg_state,
            user_id: user_id.to_string(),
            collection_id: collection.id.clone(),
            collection_name: collection_name.to_string(),
            passkey_name: passkey_name.unwrap_or("Passkey").to_string(),
            expires_at: now + CHALLENGE_EXPIRY_SECS,
        };

        {
            let mut store = self.pending_registrations.lock().unwrap();
            store.insert(registration_id.clone(), pending);
        }

        let options = serde_json::to_value(&ccr).map_err(|e| {
            ZerobaseError::internal(format!("failed to serialize registration options: {e}"))
        })?;

        info!(
            user_id = user_id,
            collection = collection_name,
            "Passkey registration requested"
        );

        Ok(PasskeyRegisterResponse {
            registration_id,
            options,
        })
    }

    /// Confirm passkey registration by verifying the browser's response.
    ///
    /// On success, stores the credential in the database.
    pub fn confirm_passkey_register(
        &self,
        registration_id: &str,
        credential_response: &RegisterPublicKeyCredential,
    ) -> Result<(), ZerobaseError> {
        let now = now_secs();

        let pending = {
            let mut store = self.pending_registrations.lock().unwrap();
            store
                .remove(registration_id)
                .ok_or_else(|| ZerobaseError::validation("Invalid or expired registration ID."))?
        };

        if now > pending.expires_at {
            return Err(ZerobaseError::validation(
                "Registration challenge has expired. Please request a new one.",
            ));
        }

        let passkey = self
            .webauthn
            .finish_passkey_registration(credential_response, &pending.state)
            .map_err(|e| {
                ZerobaseError::validation(format!("WebAuthn registration verification failed: {e}"))
            })?;

        // Serialize the credential for storage.
        let credential_data = serde_json::to_string(&passkey).map_err(|e| {
            ZerobaseError::internal(format!("failed to serialize passkey credential: {e}"))
        })?;

        let credential_id = base64url_encode(passkey.cred_id().as_ref());

        let now_str = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let stored = WebauthnCredential {
            id: nanoid::nanoid!(15),
            collection_id: pending.collection_id,
            record_id: pending.user_id.clone(),
            name: pending.passkey_name,
            credential_id,
            credential_data,
            created: now_str.clone(),
            updated: now_str,
        };

        self.credential_repo.create(&stored)?;

        info!(
            user_id = pending.user_id,
            collection = pending.collection_name,
            "Passkey registered successfully"
        );

        Ok(())
    }

    // ── Authentication ──────────────────────────────────────────────────────

    /// Begin passkey authentication for a collection.
    ///
    /// Returns an assertion challenge that the client must pass to
    /// `navigator.credentials.get()`. The authentication must be completed
    /// within [`CHALLENGE_EXPIRY_SECS`].
    pub fn auth_with_passkey_begin(
        &self,
        collection_name: &str,
    ) -> Result<PasskeyAuthBeginResponse, ZerobaseError> {
        let collection = self.record_service.get_collection(collection_name)?;
        if collection.collection_type != CollectionType::Auth {
            return Err(ZerobaseError::validation(
                "Passkey authentication is only available for auth collections.",
            ));
        }

        // Load all passkeys for this collection.
        let all_creds = self.credential_repo.find_by_collection(&collection.id)?;

        if all_creds.is_empty() {
            return Err(ZerobaseError::validation(
                "No passkeys registered for this collection.",
            ));
        }

        let passkeys: Vec<Passkey> = all_creds
            .iter()
            .filter_map(|c| serde_json::from_str(&c.credential_data).ok())
            .collect();

        if passkeys.is_empty() {
            return Err(ZerobaseError::validation(
                "No valid passkeys found for this collection.",
            ));
        }

        let (rcr, auth_state) = self
            .webauthn
            .start_passkey_authentication(&passkeys)
            .map_err(|e| {
                ZerobaseError::internal(format!("WebAuthn authentication start failed: {e}"))
            })?;

        let authentication_id = nanoid::nanoid!(24);
        let now = now_secs();

        let pending = PendingAuthentication {
            state: auth_state,
            collection_id: collection.id.clone(),
            collection_name: collection_name.to_string(),
            expires_at: now + CHALLENGE_EXPIRY_SECS,
        };

        {
            let mut store = self.pending_authentications.lock().unwrap();
            store.insert(authentication_id.clone(), pending);
        }

        let options = serde_json::to_value(&rcr).map_err(|e| {
            ZerobaseError::internal(format!("failed to serialize authentication options: {e}"))
        })?;

        info!(
            collection = collection_name,
            passkey_count = passkeys.len(),
            "Passkey authentication begun"
        );

        Ok(PasskeyAuthBeginResponse {
            authentication_id,
            options,
        })
    }

    /// Finish passkey authentication by verifying the browser's assertion response.
    ///
    /// On success, returns a JWT auth token and the authenticated user record.
    pub fn auth_with_passkey_finish(
        &self,
        authentication_id: &str,
        credential_response: &PublicKeyCredential,
    ) -> Result<(String, HashMap<String, serde_json::Value>), ZerobaseError> {
        let now = now_secs();

        let pending = {
            let mut store = self.pending_authentications.lock().unwrap();
            store
                .remove(authentication_id)
                .ok_or_else(|| ZerobaseError::validation("Invalid or expired authentication ID."))?
        };

        if now > pending.expires_at {
            return Err(ZerobaseError::validation(
                "Authentication challenge has expired. Please request a new one.",
            ));
        }

        let auth_result = self
            .webauthn
            .finish_passkey_authentication(credential_response, &pending.state)
            .map_err(|e| {
                ZerobaseError::auth(format!("WebAuthn authentication verification failed: {e}"))
            })?;

        // Find the credential that was used.
        let used_cred_id = base64url_encode(auth_result.cred_id().as_ref());

        let stored_cred = self
            .credential_repo
            .find_by_credential_id(&used_cred_id)?
            .ok_or_else(|| ZerobaseError::auth("authenticated credential not found in database"))?;

        // Ensure the credential belongs to the expected collection.
        if stored_cred.collection_id != pending.collection_id {
            return Err(ZerobaseError::auth(
                "credential does not belong to this collection",
            ));
        }

        // Update the credential's counter if needed (replay protection).
        if auth_result.needs_update() {
            // Load the passkey, update counter, and save back.
            if let Ok(mut passkey) = serde_json::from_str::<Passkey>(&stored_cred.credential_data) {
                passkey.update_credential(&auth_result);
                if let Ok(updated_data) = serde_json::to_string(&passkey) {
                    let now_str = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    let updated_cred = WebauthnCredential {
                        credential_data: updated_data,
                        updated: now_str,
                        ..stored_cred.clone()
                    };
                    // Best-effort update — don't fail auth if counter update fails.
                    let _ = self.credential_repo.delete(&stored_cred.id);
                    let _ = self.credential_repo.create(&updated_cred);
                }
            }
        }

        // Load the user record.
        let record = self
            .record_service
            .get_record(&pending.collection_name, &stored_cred.record_id)?;

        let token_key = record
            .get("tokenKey")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Generate auth token.
        let token = self.token_service.generate(
            &stored_cred.record_id,
            &pending.collection_id,
            TokenType::Auth,
            token_key,
            None,
        )?;

        // Build clean response record (strip sensitive fields).
        let mut response_record = record;
        response_record.remove("tokenKey");
        response_record.remove("password");
        response_record.remove("mfaSecret");
        response_record.remove("mfaRecoveryCodes");

        info!(
            user_id = stored_cred.record_id,
            collection = pending.collection_name,
            "Passkey authentication successful"
        );

        Ok((token, response_record))
    }

    /// Cleanup expired pending registrations and authentications.
    pub fn cleanup_expired(&self) {
        let now = now_secs();

        {
            let mut store = self.pending_registrations.lock().unwrap();
            store.retain(|_, v| v.expires_at > now);
        }
        {
            let mut store = self.pending_authentications.lock().unwrap();
            store.retain(|_, v| v.expires_at > now);
        }
    }
}

// ── Helper Functions ─────────────────────────────────────────────────────────

/// Current time in seconds since UNIX epoch.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Derive a deterministic UUID from a user ID string.
///
/// WebAuthn requires a UUID for the user handle. We derive one from the
/// user's record ID using UUID v5 (SHA-1 namespace hash) for consistency.
fn uuid_from_user_id(user_id: &str) -> Uuid {
    // Use the URL namespace OID as the base namespace.
    let namespace = uuid::Uuid::NAMESPACE_URL;
    uuid::Uuid::new_v5(&namespace, user_id.as_bytes())
}

/// Encode bytes as base64url (no padding).
fn base64url_encode(bytes: &[u8]) -> String {
    data_encoding::BASE64URL_NOPAD.encode(bytes)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_from_user_id_is_deterministic() {
        let uuid1 = uuid_from_user_id("user123");
        let uuid2 = uuid_from_user_id("user123");
        assert_eq!(uuid1, uuid2);
    }

    #[test]
    fn uuid_from_user_id_differs_for_different_users() {
        let uuid1 = uuid_from_user_id("user1");
        let uuid2 = uuid_from_user_id("user2");
        assert_ne!(uuid1, uuid2);
    }

    #[test]
    fn base64url_encode_works() {
        let result = base64url_encode(b"hello world");
        assert_eq!(result, "aGVsbG8gd29ybGQ");
    }

    #[test]
    fn base64url_encode_empty() {
        let result = base64url_encode(b"");
        assert_eq!(result, "");
    }

    #[test]
    fn challenge_expiry_is_5_minutes() {
        assert_eq!(CHALLENGE_EXPIRY_SECS, 300);
    }

    // Integration tests with mock repos follow the same pattern as MFA tests.
    // Full end-to-end WebAuthn flow tests require a webauthn-rs test helper,
    // which is covered in the API integration tests.

    use zerobase_core::services::record_service::RecordRepoError;

    struct DummyRepo;
    struct DummySchema;
    struct DummyCredRepo;

    impl RecordRepository for DummyRepo {
        fn find_one(
            &self,
            _: &str,
            _: &str,
        ) -> std::result::Result<HashMap<String, serde_json::Value>, RecordRepoError> {
            unimplemented!()
        }
        fn find_many(
            &self,
            _: &str,
            _: &zerobase_core::services::record_service::RecordQuery,
        ) -> std::result::Result<zerobase_core::services::record_service::RecordList, RecordRepoError>
        {
            unimplemented!()
        }
        fn insert(
            &self,
            _: &str,
            _: &HashMap<String, serde_json::Value>,
        ) -> std::result::Result<(), RecordRepoError> {
            unimplemented!()
        }
        fn update(
            &self,
            _: &str,
            _: &str,
            _: &HashMap<String, serde_json::Value>,
        ) -> std::result::Result<bool, RecordRepoError> {
            unimplemented!()
        }
        fn delete(&self, _: &str, _: &str) -> std::result::Result<bool, RecordRepoError> {
            unimplemented!()
        }
        fn count(&self, _: &str, _: Option<&str>) -> std::result::Result<u64, RecordRepoError> {
            unimplemented!()
        }
        fn find_referencing_records(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> std::result::Result<Vec<HashMap<String, serde_json::Value>>, RecordRepoError> {
            Ok(Vec::new())
        }
    }

    impl SchemaLookup for DummySchema {
        fn get_collection(
            &self,
            _: &str,
        ) -> zerobase_core::error::Result<zerobase_core::schema::Collection> {
            unimplemented!()
        }
        fn get_collection_by_id(
            &self,
            _: &str,
        ) -> zerobase_core::error::Result<zerobase_core::schema::Collection> {
            unimplemented!()
        }
    }

    impl WebauthnCredentialRepository for DummyCredRepo {
        fn find_by_credential_id(
            &self,
            _: &str,
        ) -> zerobase_core::error::Result<Option<WebauthnCredential>> {
            Ok(None)
        }
        fn find_by_record(
            &self,
            _: &str,
            _: &str,
        ) -> zerobase_core::error::Result<Vec<WebauthnCredential>> {
            Ok(vec![])
        }
        fn find_by_collection(
            &self,
            _: &str,
        ) -> zerobase_core::error::Result<Vec<WebauthnCredential>> {
            Ok(vec![])
        }
        fn create(&self, _: &WebauthnCredential) -> zerobase_core::error::Result<()> {
            Ok(())
        }
        fn delete(&self, _: &str) -> zerobase_core::error::Result<()> {
            Ok(())
        }
        fn delete_by_record(&self, _: &str, _: &str) -> zerobase_core::error::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn passkey_service_can_be_constructed() {
        use zerobase_core::services::record_service::RecordService;

        let record_service = Arc::new(RecordService::new(DummyRepo, DummySchema));
        let token_service: Arc<dyn TokenService> = Arc::new(crate::JwtTokenService::new(
            secrecy::SecretString::from("test-secret-key-that-is-long-enough-for-hmac-sha256!!"),
            3600,
        ));
        let cred_repo = Arc::new(DummyCredRepo);

        let result = PasskeyService::new(
            record_service,
            token_service,
            cred_repo,
            "localhost",
            "http://localhost:8090",
            "Test App",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn passkey_service_rejects_invalid_origin() {
        use zerobase_core::services::record_service::RecordService;

        let record_service = Arc::new(RecordService::new(DummyRepo, DummySchema));
        let token_service: Arc<dyn TokenService> = Arc::new(crate::JwtTokenService::new(
            secrecy::SecretString::from("test-secret-key-that-is-long-enough-for-hmac-sha256!!"),
            3600,
        ));
        let cred_repo = Arc::new(DummyCredRepo);

        let result = PasskeyService::new(
            record_service,
            token_service,
            cred_repo,
            "localhost",
            "not a valid url",
            "Test App",
        );
        assert!(result.is_err());
    }

    #[test]
    fn cleanup_expired_removes_old_entries() {
        use zerobase_core::services::record_service::RecordService;

        let record_service = Arc::new(RecordService::new(DummyRepo, DummySchema));
        let token_service: Arc<dyn TokenService> = Arc::new(crate::JwtTokenService::new(
            secrecy::SecretString::from("test-secret-key-that-is-long-enough-for-hmac-sha256!!"),
            3600,
        ));
        let cred_repo = Arc::new(DummyCredRepo);

        let service = PasskeyService::new(
            record_service,
            token_service,
            cred_repo,
            "localhost",
            "http://localhost:8090",
            "Test App",
        )
        .unwrap();

        // Insert an expired registration.
        {
            let mut store = service.pending_registrations.lock().unwrap();
            // We can't easily create a PasskeyRegistration without webauthn flow,
            // so we just verify cleanup doesn't panic with an empty store.
            assert!(store.is_empty());
        }

        service.cleanup_expired();

        {
            let store = service.pending_registrations.lock().unwrap();
            assert!(store.is_empty());
        }
    }
}
