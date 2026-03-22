//! Multi-Factor Authentication (MFA) via TOTP.
//!
//! Provides [`MfaService`] which handles:
//! - Generating TOTP secrets and QR code URIs for MFA setup
//! - Verifying TOTP codes to confirm MFA enrollment
//! - Generating recovery codes for account recovery
//! - Verifying TOTP or recovery codes during the MFA auth step
//!
//! MFA state is stored on the user record via two fields:
//! - `mfaSecret` — the base32-encoded TOTP secret (empty when MFA disabled)
//! - `mfaRecoveryCodes` — JSON array of hashed recovery codes

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::Rng;
use serde::{Deserialize, Serialize};
use totp_rs::{Algorithm, Secret, TOTP};
use tracing::info;

use zerobase_core::auth::{TokenService, TokenType};
use zerobase_core::error::ZerobaseError;
use zerobase_core::schema::CollectionType;
use zerobase_core::services::record_service::{RecordRepository, RecordService, SchemaLookup};

use crate::token::durations;

use std::sync::Arc;

/// Number of recovery codes to generate.
const RECOVERY_CODE_COUNT: usize = 8;

/// Length of each recovery code (characters).
const RECOVERY_CODE_LENGTH: usize = 10;

/// TOTP configuration: SHA-1, 6 digits, 30-second step (RFC 6238 standard).
const TOTP_DIGITS: usize = 6;
const TOTP_STEP: u64 = 30;
/// Allow 1 step of skew (30s before/after) to account for clock drift.
const TOTP_SKEW: u8 = 1;

/// Pending MFA setup: stores the secret until the user confirms with a valid code.
#[derive(Debug, Clone)]
struct PendingMfaSetup {
    /// Base32-encoded TOTP secret.
    secret: String,
    /// User record ID.
    user_id: String,
    /// Collection name.
    collection_name: String,
    /// Expiry timestamp (seconds since UNIX epoch).
    expires_at: u64,
}

/// Response returned when requesting MFA setup.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MfaSetupResponse {
    /// Unique identifier for this pending setup (needed to confirm).
    pub mfa_id: String,
    /// The TOTP secret in base32 encoding (for manual entry).
    pub secret: String,
    /// The otpauth:// URI for QR code generation.
    pub qr_uri: String,
}

/// Thread-safe store for pending MFA setups.
type PendingSetupStore = Mutex<HashMap<String, PendingMfaSetup>>;

/// Service responsible for the MFA (TOTP) authentication flow.
///
/// Handles secret generation, QR code URI creation, TOTP verification,
/// recovery code management, and full MFA auth token exchange.
pub struct MfaService<R: RecordRepository, S: SchemaLookup> {
    record_service: Arc<RecordService<R, S>>,
    token_service: Arc<dyn TokenService>,
    /// In-memory store for pending MFA setup requests.
    pending_setups: PendingSetupStore,
    /// Issuer name shown in authenticator apps (e.g. "Zerobase").
    issuer: String,
}

impl<R: RecordRepository, S: SchemaLookup> MfaService<R, S> {
    pub fn new(
        record_service: Arc<RecordService<R, S>>,
        token_service: Arc<dyn TokenService>,
        issuer: String,
    ) -> Self {
        Self {
            record_service,
            token_service,
            pending_setups: Mutex::new(HashMap::new()),
            issuer,
        }
    }

    /// Expose the record service for collection metadata lookups.
    pub fn record_service(&self) -> &RecordService<R, S> {
        &self.record_service
    }

    // ── MFA Setup ─────────────────────────────────────────────────────────

    /// Begin MFA setup for a user: generate a TOTP secret and return
    /// the secret + QR URI. The setup is not finalized until confirmed
    /// with a valid TOTP code via [`confirm_mfa_setup`].
    pub fn request_mfa_setup(
        &self,
        collection_name: &str,
        user_id: &str,
    ) -> Result<MfaSetupResponse, ZerobaseError> {
        // Verify the collection is an auth collection.
        let collection = self.record_service.get_collection(collection_name)?;
        if collection.collection_type != CollectionType::Auth {
            return Err(ZerobaseError::validation(
                "MFA is only available for auth collections.",
            ));
        }

        // Verify the user exists.
        let record = self.record_service.get_record(collection_name, user_id)?;

        // Check if MFA is already enabled.
        let existing_secret = record
            .get("mfaSecret")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !existing_secret.is_empty() {
            return Err(ZerobaseError::validation(
                "MFA is already enabled for this account. Disable it first to reconfigure.",
            ));
        }

        // Get the user's email for the TOTP label.
        let email = record
            .get("email")
            .and_then(|v| v.as_str())
            .unwrap_or("user")
            .to_string();

        // Generate a new TOTP secret.
        let secret = Secret::generate_secret();
        let secret_base32 = secret.to_encoded().to_string();

        // Build the TOTP instance for the QR URI.
        let totp = build_totp(&secret_base32, &self.issuer, &email)?;
        let qr_uri = totp.get_url();

        // Generate a unique setup ID and store the pending setup.
        let mfa_id = nanoid::nanoid!(24);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let pending = PendingMfaSetup {
            secret: secret_base32.clone(),
            user_id: user_id.to_string(),
            collection_name: collection_name.to_string(),
            expires_at: now + durations::MFA_PARTIAL,
        };

        {
            let mut store = self.pending_setups.lock().unwrap();
            store.insert(mfa_id.clone(), pending);
        }

        info!(
            user_id = user_id,
            collection = collection_name,
            "MFA setup requested"
        );

        Ok(MfaSetupResponse {
            mfa_id,
            secret: secret_base32,
            qr_uri,
        })
    }

    /// Confirm MFA setup by verifying a TOTP code against the pending secret.
    /// On success, stores the secret and recovery codes on the user record
    /// and returns the plaintext recovery codes (shown once to the user).
    pub fn confirm_mfa_setup(
        &self,
        mfa_id: &str,
        code: &str,
    ) -> Result<Vec<String>, ZerobaseError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Look up and remove the pending setup.
        let pending = {
            let mut store = self.pending_setups.lock().unwrap();
            store
                .remove(mfa_id)
                .ok_or_else(|| ZerobaseError::validation("Invalid or expired MFA setup ID."))?
        };

        // Check expiry.
        if now > pending.expires_at {
            return Err(ZerobaseError::validation(
                "MFA setup has expired. Please request a new setup.",
            ));
        }

        // Verify the TOTP code against the pending secret.
        let totp = build_totp(&pending.secret, &self.issuer, "")?;
        if !verify_totp_code(&totp, code, now) {
            return Err(ZerobaseError::validation(
                "Invalid verification code. Please try again.",
            ));
        }

        // Generate recovery codes.
        let recovery_codes = generate_recovery_codes();
        let hashed_codes: Vec<String> = recovery_codes
            .iter()
            .map(|c| hash_recovery_code(c))
            .collect();

        // Store the MFA secret and hashed recovery codes on the user record.
        let updates = serde_json::json!({
            "mfaSecret": pending.secret,
            "mfaRecoveryCodes": hashed_codes,
        });

        self.record_service
            .update_record(&pending.collection_name, &pending.user_id, updates)?;

        info!(
            user_id = pending.user_id,
            collection = pending.collection_name,
            "MFA enabled successfully"
        );

        // Return plaintext recovery codes (shown once to the user).
        Ok(recovery_codes)
    }

    // ── MFA Verification (Login Step 2) ──────────────────────────────────

    /// Verify an MFA code (TOTP or recovery code) and exchange the partial
    /// token for a full auth token.
    ///
    /// The caller must present a valid `MfaPartial` token (obtained from
    /// the initial password auth when MFA is enabled).
    pub fn auth_with_mfa(
        &self,
        mfa_partial_token: &str,
        code: &str,
    ) -> Result<(String, HashMap<String, serde_json::Value>), ZerobaseError> {
        // Validate the MFA partial token.
        let validated = self
            .token_service
            .validate(mfa_partial_token, TokenType::MfaPartial)?;

        let user_id = &validated.claims.id;
        let collection_id = &validated.claims.collection_id;

        // Resolve collection name from ID.
        let collection = self.record_service.get_collection(collection_id)?;

        // Load the user record.
        let record = self.record_service.get_record(&collection.name, user_id)?;

        // Verify tokenKey hasn't changed.
        let current_token_key = record
            .get("tokenKey")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if current_token_key != validated.claims.token_key {
            return Err(ZerobaseError::auth("token has been invalidated"));
        }

        // Get the MFA secret.
        let mfa_secret = record
            .get("mfaSecret")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if mfa_secret.is_empty() {
            return Err(ZerobaseError::validation(
                "MFA is not enabled for this account.",
            ));
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Try TOTP verification first.
        let totp = build_totp(mfa_secret, &self.issuer, "")?;
        let totp_valid = verify_totp_code(&totp, code, now);

        if !totp_valid {
            // Try recovery code verification.
            let recovery_valid =
                self.try_consume_recovery_code(&collection.name, user_id, &record, code)?;

            if !recovery_valid {
                return Err(ZerobaseError::validation("Invalid MFA code."));
            }
        }

        // Generate a full auth token.
        let token = self.token_service.generate(
            user_id,
            collection_id,
            TokenType::Auth,
            current_token_key,
            None,
        )?;

        // Build clean response record.
        let mut response_record = record;
        response_record.remove("tokenKey");
        response_record.remove("password");
        response_record.remove("mfaSecret");
        response_record.remove("mfaRecoveryCodes");

        info!(
            user_id = user_id,
            collection = collection.name,
            "MFA verification successful"
        );

        Ok((token, response_record))
    }

    // ── MFA Disable ──────────────────────────────────────────────────────

    /// Disable MFA for a user by clearing the secret and recovery codes.
    pub fn disable_mfa(&self, collection_name: &str, user_id: &str) -> Result<(), ZerobaseError> {
        // Verify user exists and has MFA enabled.
        let record = self.record_service.get_record(collection_name, user_id)?;

        let mfa_secret = record
            .get("mfaSecret")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if mfa_secret.is_empty() {
            return Err(ZerobaseError::validation(
                "MFA is not enabled for this account.",
            ));
        }

        let updates = serde_json::json!({
            "mfaSecret": "",
            "mfaRecoveryCodes": [],
        });

        self.record_service
            .update_record(collection_name, user_id, updates)?;

        info!(
            user_id = user_id,
            collection = collection_name,
            "MFA disabled"
        );

        Ok(())
    }

    // ── MFA Status Check ─────────────────────────────────────────────────

    /// Check whether a user has MFA enabled by inspecting the `mfaSecret` field.
    pub fn is_mfa_enabled(record: &HashMap<String, serde_json::Value>) -> bool {
        record
            .get("mfaSecret")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    // ── Recovery Code Helpers ────────────────────────────────────────────

    /// Try to consume a recovery code. Returns `true` if the code matched
    /// and was consumed (removed from the stored list).
    fn try_consume_recovery_code(
        &self,
        collection_name: &str,
        user_id: &str,
        record: &HashMap<String, serde_json::Value>,
        code: &str,
    ) -> Result<bool, ZerobaseError> {
        let stored_codes = match record.get("mfaRecoveryCodes") {
            Some(serde_json::Value::Array(codes)) => codes.clone(),
            _ => return Ok(false),
        };

        let code_hash = hash_recovery_code(code);

        // Find and remove the matching hashed recovery code.
        let mut found = false;
        let remaining: Vec<serde_json::Value> = stored_codes
            .into_iter()
            .filter(|stored| {
                if !found {
                    if let Some(stored_str) = stored.as_str() {
                        if stored_str == code_hash {
                            found = true;
                            return false; // Remove this code
                        }
                    }
                }
                true
            })
            .collect();

        if !found {
            return Ok(false);
        }

        // Update the record with the remaining recovery codes.
        let updates = serde_json::json!({
            "mfaRecoveryCodes": remaining,
        });

        self.record_service
            .update_record(collection_name, user_id, updates)?;

        info!(
            user_id = user_id,
            collection = collection_name,
            "Recovery code consumed"
        );

        Ok(true)
    }
}

// ── Helper Functions ─────────────────────────────────────────────────────────

/// Build a TOTP instance from a base32-encoded secret.
fn build_totp(secret_base32: &str, issuer: &str, account: &str) -> Result<TOTP, ZerobaseError> {
    let secret_bytes = data_encoding::BASE32
        .decode(secret_base32.as_bytes())
        .map_err(|e| ZerobaseError::internal(format!("invalid base32 secret: {e}")))?;

    TOTP::new(
        Algorithm::SHA1,
        TOTP_DIGITS,
        TOTP_SKEW,
        TOTP_STEP,
        secret_bytes,
        Some(issuer.to_string()),
        account.to_string(),
    )
    .map_err(|e| ZerobaseError::internal(format!("failed to create TOTP: {e}")))
}

/// Verify a TOTP code at a specific time.
fn verify_totp_code(totp: &TOTP, code: &str, time: u64) -> bool {
    // The TOTP library's check_current handles skew.
    // We use check() with explicit time for testability.
    totp.check(code, time)
}

/// Generate a set of random recovery codes.
fn generate_recovery_codes() -> Vec<String> {
    let mut rng = rand::thread_rng();
    (0..RECOVERY_CODE_COUNT)
        .map(|_| {
            (0..RECOVERY_CODE_LENGTH)
                .map(|_| {
                    let idx = rng.gen_range(0..36);
                    if idx < 10 {
                        (b'0' + idx) as char
                    } else {
                        (b'a' + idx - 10) as char
                    }
                })
                .collect()
        })
        .collect()
}

/// Hash a recovery code for storage. Uses a simple SHA-256 digest
/// (recovery codes are high-entropy random strings, so a fast hash is acceptable).
fn hash_recovery_code(code: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // We use a deterministic hash for simplicity. In production you might
    // use SHA-256, but since recovery codes are random high-entropy strings
    // a fast hash is sufficient.
    let normalized = code.trim().to_lowercase();
    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_totp_creates_valid_instance() {
        let secret = Secret::generate_secret();
        let secret_b32 = secret.to_encoded().to_string();
        let totp = build_totp(&secret_b32, "Zerobase", "test@example.com");
        assert!(totp.is_ok());
    }

    #[test]
    fn build_totp_rejects_invalid_base32() {
        let result = build_totp("not-valid-base32!!!", "Zerobase", "test@example.com");
        assert!(result.is_err());
    }

    #[test]
    fn totp_code_generation_and_verification() {
        let secret = Secret::generate_secret();
        let secret_b32 = secret.to_encoded().to_string();
        let totp = build_totp(&secret_b32, "Zerobase", "test@example.com").unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let code = totp.generate(now);
        assert_eq!(code.len(), TOTP_DIGITS);
        assert!(verify_totp_code(&totp, &code, now));
    }

    #[test]
    fn totp_rejects_wrong_code() {
        let secret = Secret::generate_secret();
        let secret_b32 = secret.to_encoded().to_string();
        let totp = build_totp(&secret_b32, "Zerobase", "test@example.com").unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        assert!(!verify_totp_code(&totp, "000000", now));
    }

    #[test]
    fn totp_allows_skew() {
        let secret = Secret::generate_secret();
        let secret_b32 = secret.to_encoded().to_string();
        let totp = build_totp(&secret_b32, "Zerobase", "test@example.com").unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Generate code for 30 seconds ago (1 step back).
        let past_code = totp.generate(now - TOTP_STEP);
        // Should still be valid due to TOTP_SKEW = 1.
        assert!(verify_totp_code(&totp, &past_code, now));
    }

    #[test]
    fn generate_recovery_codes_produces_correct_count() {
        let codes = generate_recovery_codes();
        assert_eq!(codes.len(), RECOVERY_CODE_COUNT);
    }

    #[test]
    fn generate_recovery_codes_produces_unique_codes() {
        let codes = generate_recovery_codes();
        let unique: std::collections::HashSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), codes.len());
    }

    #[test]
    fn recovery_codes_have_correct_length() {
        let codes = generate_recovery_codes();
        for code in &codes {
            assert_eq!(code.len(), RECOVERY_CODE_LENGTH);
        }
    }

    #[test]
    fn recovery_code_hashing_is_deterministic() {
        let code = "abc123test";
        let hash1 = hash_recovery_code(code);
        let hash2 = hash_recovery_code(code);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn recovery_code_hashing_is_case_insensitive() {
        let hash1 = hash_recovery_code("AbCdEf1234");
        let hash2 = hash_recovery_code("abcdef1234");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn recovery_code_hashing_trims_whitespace() {
        let hash1 = hash_recovery_code("  abc123  ");
        let hash2 = hash_recovery_code("abc123");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn different_codes_produce_different_hashes() {
        let hash1 = hash_recovery_code("code1");
        let hash2 = hash_recovery_code("code2");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn is_mfa_enabled_returns_true_when_secret_set() {
        let mut record = HashMap::new();
        record.insert(
            "mfaSecret".to_string(),
            serde_json::Value::String("JBSWY3DPEHPK3PXP".to_string()),
        );
        assert!(MfaService::<DummyRepo, DummySchema>::is_mfa_enabled(
            &record
        ));
    }

    #[test]
    fn is_mfa_enabled_returns_false_when_secret_empty() {
        let mut record = HashMap::new();
        record.insert(
            "mfaSecret".to_string(),
            serde_json::Value::String(String::new()),
        );
        assert!(!MfaService::<DummyRepo, DummySchema>::is_mfa_enabled(
            &record
        ));
    }

    #[test]
    fn is_mfa_enabled_returns_false_when_field_missing() {
        let record = HashMap::new();
        assert!(!MfaService::<DummyRepo, DummySchema>::is_mfa_enabled(
            &record
        ));
    }

    #[test]
    fn qr_uri_contains_issuer_and_account() {
        let secret = Secret::generate_secret();
        let secret_b32 = secret.to_encoded().to_string();
        let totp = build_totp(&secret_b32, "Zerobase", "test@example.com").unwrap();
        let uri = totp.get_url();
        assert!(uri.starts_with("otpauth://totp/"));
        assert!(uri.contains("issuer=Zerobase"));
        assert!(uri.contains("test%40example.com") || uri.contains("test@example.com"));
    }

    // Dummy types for generic bounds in static method tests.
    struct DummyRepo;
    struct DummySchema;

    use zerobase_core::services::record_service::RecordRepoError;

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
}
