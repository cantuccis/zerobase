//! OTP (One-Time Password) authentication flow.
//!
//! Provides [`OtpService`] which handles:
//! - Generating 6-digit OTP codes with 5-minute expiry
//! - Sending OTP codes via email using [`EmailService`]
//! - Verifying OTP codes with attempt limiting
//! - Returning auth tokens upon successful verification
//!
//! OTP records are stored in an in-memory concurrent map, keyed by a unique
//! OTP ID. Each record tracks the code, user identity, expiry, attempt count,
//! and whether the code has been consumed.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::Rng;
use tracing::info;

use zerobase_core::auth::{TokenService, TokenType};
use zerobase_core::email::templates::{EmailTemplateEngine, OtpContext};
use zerobase_core::email::EmailService;
use zerobase_core::error::ZerobaseError;
use zerobase_core::services::record_service::{RecordRepository, SchemaLookup};
use zerobase_core::RecordService;

use crate::token::durations;

use std::sync::Arc;

/// Maximum number of verification attempts per OTP code.
const MAX_ATTEMPTS: u32 = 5;

/// Length of the generated OTP code.
const OTP_CODE_LENGTH: usize = 6;

/// An in-flight OTP record tracking code, identity, expiry, and attempts.
#[derive(Debug, Clone)]
struct OtpRecord {
    /// The 6-digit OTP code.
    code: String,
    /// Email address the OTP was sent to.
    email: String,
    /// Collection name the user belongs to.
    collection_name: String,
    /// Expiry timestamp (seconds since UNIX epoch).
    expires_at: u64,
    /// Number of failed verification attempts.
    attempts: u32,
    /// Whether this OTP has been successfully consumed.
    used: bool,
}

/// Thread-safe OTP store mapping otp_id → OtpRecord.
type OtpStore = Mutex<HashMap<String, OtpRecord>>;

/// Service responsible for the OTP authentication flow.
///
/// Encapsulates OTP generation, email sending, code verification, and
/// auth token issuance. Generic over repositories for testability.
pub struct OtpService<R: RecordRepository, S: SchemaLookup> {
    record_service: Arc<RecordService<R, S>>,
    token_service: Arc<dyn TokenService>,
    email_service: Arc<dyn EmailService>,
    template_engine: EmailTemplateEngine,
    /// In-memory store for pending OTP records.
    store: OtpStore,
    /// OTP code validity duration in seconds.
    otp_duration_secs: u64,
}

impl<R: RecordRepository, S: SchemaLookup> OtpService<R, S> {
    pub fn new(
        record_service: Arc<RecordService<R, S>>,
        token_service: Arc<dyn TokenService>,
        email_service: Arc<dyn EmailService>,
        template_engine: EmailTemplateEngine,
    ) -> Self {
        Self {
            record_service,
            token_service,
            email_service,
            template_engine,
            store: Mutex::new(HashMap::new()),
            otp_duration_secs: durations::OTP,
        }
    }

    /// Request an OTP code for the given email in the specified auth collection.
    ///
    /// 1. Validates the collection is an auth collection with OTP enabled.
    /// 2. Generates a cryptographically random 6-digit code.
    /// 3. Stores the OTP record with a 5-minute expiry.
    /// 4. Sends the code via email.
    /// 5. Returns the OTP ID (needed to verify the code later).
    ///
    /// Returns the `otp_id` regardless of whether the email exists, to prevent
    /// email enumeration attacks. If the email doesn't exist, no email is sent
    /// but a valid-looking `otp_id` is still returned.
    pub fn request_otp(&self, collection_name: &str, email: &str) -> Result<String, ZerobaseError> {
        let email = email.trim();
        if email.is_empty() {
            return Err(ZerobaseError::validation("email is required"));
        }

        // Validate collection is auth type.
        let collection = self.record_service.get_collection(collection_name)?;
        if collection.collection_type != zerobase_core::CollectionType::Auth {
            return Err(ZerobaseError::validation(format!(
                "collection '{}' is not an auth collection",
                collection_name
            )));
        }

        // Check if OTP auth is enabled on this collection.
        if let Some(ref auth_opts) = collection.auth_options {
            if !auth_opts.allow_otp_auth {
                return Err(ZerobaseError::validation(format!(
                    "OTP authentication is not enabled for collection '{}'",
                    collection_name
                )));
            }
        } else {
            return Err(ZerobaseError::validation(format!(
                "OTP authentication is not enabled for collection '{}'",
                collection_name
            )));
        }

        // Generate the OTP code and ID.
        let otp_code = generate_otp_code();
        let otp_id = nanoid::nanoid!(15);
        let now = now_secs();

        // Look up the user by email to decide whether to send the email.
        let query = zerobase_core::services::record_service::RecordQuery {
            filter: Some(format!("email = {:?}", email)),
            page: 1,
            per_page: 1,
            ..Default::default()
        };

        let list = self.record_service.list_records(collection_name, &query)?;

        if list.items.is_empty() {
            // Don't reveal whether the email exists — return a fake otp_id.
            info!(
                email = %email,
                collection = %collection_name,
                "OTP requested for unknown email (silent success)"
            );
            return Ok(otp_id);
        }

        // Store the OTP record.
        let record = OtpRecord {
            code: otp_code.clone(),
            email: email.to_string(),
            collection_name: collection_name.to_string(),
            expires_at: now + self.otp_duration_secs,
            attempts: 0,
            used: false,
        };

        {
            let mut store = self.store.lock().unwrap();
            // Clean up expired entries while we have the lock.
            store.retain(|_, r| r.expires_at > now && !r.used);
            store.insert(otp_id.clone(), record);
        }

        // Send the OTP email.
        let message = self.template_engine.otp(&OtpContext {
            to: email.to_string(),
            otp_code: otp_code.clone(),
            expiry_text: format!("{} minutes", self.otp_duration_secs / 60),
        });

        self.email_service.send(&message)?;

        info!(
            email = %email,
            collection = %collection_name,
            otp_id = %otp_id,
            "OTP code sent"
        );

        Ok(otp_id)
    }

    /// Verify an OTP code and return an auth token on success.
    ///
    /// 1. Looks up the OTP record by `otp_id`.
    /// 2. Checks expiry, usage, and attempt limits.
    /// 3. Compares the provided code against the stored code.
    /// 4. On success, marks the OTP as used and returns a JWT auth token
    ///    plus the user record.
    ///
    /// Returns `(token, record)` on success.
    pub fn auth_with_otp(
        &self,
        otp_id: &str,
        code: &str,
    ) -> Result<(String, HashMap<String, serde_json::Value>), ZerobaseError>
    where
        R: RecordRepository,
        S: SchemaLookup,
    {
        let otp_id = otp_id.trim();
        let code = code.trim();

        if otp_id.is_empty() {
            return Err(ZerobaseError::validation("otpId is required"));
        }
        if code.is_empty() {
            return Err(ZerobaseError::validation("code is required"));
        }

        let now = now_secs();

        // Look up and validate the OTP record.
        let (email, collection_name) = {
            let mut store = self.store.lock().unwrap();

            let otp_record = store
                .get_mut(otp_id)
                .ok_or_else(|| ZerobaseError::validation("invalid or expired OTP"))?;

            // Check if already used.
            if otp_record.used {
                return Err(ZerobaseError::validation("OTP code has already been used"));
            }

            // Check expiry.
            if now > otp_record.expires_at {
                // Remove expired record.
                store.remove(otp_id);
                return Err(ZerobaseError::validation("OTP code has expired"));
            }

            // Check attempt limit.
            if otp_record.attempts >= MAX_ATTEMPTS {
                // Remove exhausted record.
                store.remove(otp_id);
                return Err(ZerobaseError::validation(
                    "maximum verification attempts exceeded",
                ));
            }

            // Increment attempts.
            otp_record.attempts += 1;

            // Verify the code.
            if otp_record.code != code {
                let remaining = MAX_ATTEMPTS - otp_record.attempts;
                if remaining == 0 {
                    store.remove(otp_id);
                    return Err(ZerobaseError::validation(
                        "maximum verification attempts exceeded",
                    ));
                }
                return Err(ZerobaseError::validation("invalid OTP code"));
            }

            // Mark as used.
            otp_record.used = true;

            (otp_record.email.clone(), otp_record.collection_name.clone())
        };

        // Look up the user by email.
        let query = zerobase_core::services::record_service::RecordQuery {
            filter: Some(format!("email = {:?}", email)),
            page: 1,
            per_page: 1,
            ..Default::default()
        };

        let list = self.record_service.list_records(&collection_name, &query)?;

        if list.items.is_empty() {
            return Err(ZerobaseError::validation("user not found"));
        }

        let record = list.items.into_iter().next().unwrap();

        let collection = self.record_service.get_collection(&collection_name)?;

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

        // Generate auth token.
        let token = self.token_service.generate(
            &user_id,
            &collection.id,
            TokenType::Auth,
            &token_key,
            None,
        )?;

        info!(
            email = %email,
            user_id = %user_id,
            collection = %collection_name,
            "OTP authentication successful"
        );

        Ok((token, record))
    }

    /// Access the underlying record service (e.g. for collection lookups in handlers).
    pub fn record_service(&self) -> &RecordService<R, S> {
        &self.record_service
    }
}

/// Generate a cryptographically random 6-digit OTP code.
fn generate_otp_code() -> String {
    let mut rng = rand::thread_rng();
    let code: u32 = rng.gen_range(0..10u32.pow(OTP_CODE_LENGTH as u32));
    format!("{:0>width$}", code, width = OTP_CODE_LENGTH)
}

/// Get current time as seconds since UNIX epoch.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    use serde_json::Value;

    use zerobase_core::auth::{TokenClaims, TokenType, ValidatedToken};
    use zerobase_core::email::templates::EmailTemplateEngine;
    use zerobase_core::email::EmailMessage;
    use zerobase_core::schema::{AuthOptions, Collection, CollectionType};
    use zerobase_core::services::record_service::{
        RecordList, RecordQuery, RecordRepoError, RecordRepository, SchemaLookup,
    };

    // ── Shared inner data ─────────────────────────────────────────────────

    type RecordStore = Arc<Mutex<HashMap<String, Vec<HashMap<String, Value>>>>>;

    // ── Mock TokenService ──────────────────────────────────────────────────

    struct MockTokenService {
        generated_tokens: Mutex<Vec<(String, String, TokenType, String, Option<u64>)>>,
    }

    impl MockTokenService {
        fn new() -> Self {
            Self {
                generated_tokens: Mutex::new(Vec::new()),
            }
        }

        fn generated_tokens(&self) -> Vec<(String, String, TokenType, String, Option<u64>)> {
            self.generated_tokens.lock().unwrap().clone()
        }
    }

    impl TokenService for MockTokenService {
        fn generate(
            &self,
            user_id: &str,
            collection_id: &str,
            token_type: TokenType,
            token_key: &str,
            duration_secs: Option<u64>,
        ) -> Result<String, ZerobaseError> {
            self.generated_tokens.lock().unwrap().push((
                user_id.to_string(),
                collection_id.to_string(),
                token_type,
                token_key.to_string(),
                duration_secs,
            ));
            Ok(format!("mock_token_{}", user_id))
        }

        fn validate(
            &self,
            _token: &str,
            _expected_type: TokenType,
        ) -> Result<ValidatedToken, ZerobaseError> {
            Err(ZerobaseError::auth("not implemented in mock"))
        }
    }

    // ── Mock EmailService ──────────────────────────────────────────────────

    struct MockEmailService {
        sent: Mutex<Vec<EmailMessage>>,
        should_fail: Mutex<bool>,
    }

    impl MockEmailService {
        fn new() -> Self {
            Self {
                sent: Mutex::new(Vec::new()),
                should_fail: Mutex::new(false),
            }
        }

        fn sent_messages(&self) -> Vec<EmailMessage> {
            self.sent.lock().unwrap().clone()
        }

        fn set_should_fail(&self, fail: bool) {
            *self.should_fail.lock().unwrap() = fail;
        }
    }

    impl EmailService for MockEmailService {
        fn send(&self, message: &EmailMessage) -> Result<(), ZerobaseError> {
            if *self.should_fail.lock().unwrap() {
                return Err(ZerobaseError::internal("SMTP connection failed"));
            }
            self.sent.lock().unwrap().push(message.clone());
            Ok(())
        }
    }

    // ── Mock RecordRepository ──────────────────────────────────────────────

    struct MockRecordRepo {
        records: RecordStore,
    }

    impl MockRecordRepo {
        fn new(store: RecordStore) -> Self {
            Self { records: store }
        }
    }

    fn insert_record(store: &RecordStore, collection: &str, record: HashMap<String, Value>) {
        store
            .lock()
            .unwrap()
            .entry(collection.to_string())
            .or_default()
            .push(record);
    }

    impl RecordRepository for MockRecordRepo {
        fn find_one(
            &self,
            collection: &str,
            id: &str,
        ) -> std::result::Result<HashMap<String, Value>, RecordRepoError> {
            let records = self.records.lock().unwrap();
            if let Some(col_records) = records.get(collection) {
                for record in col_records {
                    if record.get("id").and_then(|v| v.as_str()) == Some(id) {
                        return Ok(record.clone());
                    }
                }
            }
            Err(RecordRepoError::NotFound {
                resource_type: "Record".to_string(),
                resource_id: Some(id.to_string()),
            })
        }

        fn find_many(
            &self,
            collection: &str,
            query: &RecordQuery,
        ) -> std::result::Result<RecordList, RecordRepoError> {
            let records = self.records.lock().unwrap();
            let col_records = records.get(collection).cloned().unwrap_or_default();

            let filtered: Vec<_> = if let Some(ref filter) = query.filter {
                col_records
                    .into_iter()
                    .filter(|r| {
                        if let Some(email_val) = filter.strip_prefix("email = ") {
                            let expected = email_val.trim_matches('"');
                            r.get("email").and_then(|v| v.as_str()) == Some(expected)
                        } else {
                            true
                        }
                    })
                    .collect()
            } else {
                col_records
            };

            Ok(RecordList {
                items: filtered,
                total_items: 0,
                total_pages: 0,
                page: query.page,
                per_page: query.per_page,
            })
        }

        fn insert(
            &self,
            collection: &str,
            data: &HashMap<String, Value>,
        ) -> std::result::Result<(), RecordRepoError> {
            insert_record(&self.records, collection, data.clone());
            Ok(())
        }

        fn update(
            &self,
            collection: &str,
            id: &str,
            data: &HashMap<String, Value>,
        ) -> std::result::Result<bool, RecordRepoError> {
            let mut records = self.records.lock().unwrap();
            if let Some(col_records) = records.get_mut(collection) {
                for record in col_records.iter_mut() {
                    if record.get("id").and_then(|v| v.as_str()) == Some(id) {
                        for (key, value) in data {
                            record.insert(key.clone(), value.clone());
                        }
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }

        fn delete(&self, collection: &str, id: &str) -> std::result::Result<bool, RecordRepoError> {
            let mut records = self.records.lock().unwrap();
            if let Some(col_records) = records.get_mut(collection) {
                let len_before = col_records.len();
                col_records.retain(|r| r.get("id").and_then(|v| v.as_str()) != Some(id));
                return Ok(col_records.len() < len_before);
            }
            Ok(false)
        }

        fn count(
            &self,
            collection: &str,
            _filter: Option<&str>,
        ) -> std::result::Result<u64, RecordRepoError> {
            let records = self.records.lock().unwrap();
            Ok(records.get(collection).map_or(0, |r| r.len() as u64))
        }

        fn find_referencing_records(
            &self,
            _collection: &str,
            _field_name: &str,
            _referenced_id: &str,
        ) -> std::result::Result<Vec<HashMap<String, Value>>, RecordRepoError> {
            Ok(Vec::new())
        }
    }

    // ── Mock SchemaLookup ──────────────────────────────────────────────────

    struct MockSchema {
        collections: Mutex<HashMap<String, Collection>>,
    }

    impl MockSchema {
        fn new() -> Self {
            Self {
                collections: Mutex::new(HashMap::new()),
            }
        }

        fn add_auth_collection_with_otp(&self, name: &str, id: &str, otp_enabled: bool) {
            let collection = Collection {
                id: id.to_string(),
                name: name.to_string(),
                collection_type: CollectionType::Auth,
                fields: vec![],
                rules: Default::default(),
                indexes: vec![],
                view_query: None,
                auth_options: Some(AuthOptions {
                    allow_otp_auth: otp_enabled,
                    ..Default::default()
                }),
            };
            self.collections
                .lock()
                .unwrap()
                .insert(name.to_string(), collection);
        }

        fn add_base_collection(&self, name: &str, id: &str) {
            let collection = Collection {
                id: id.to_string(),
                name: name.to_string(),
                collection_type: CollectionType::Base,
                fields: vec![],
                rules: Default::default(),
                indexes: vec![],
                view_query: None,
                auth_options: None,
            };
            self.collections
                .lock()
                .unwrap()
                .insert(name.to_string(), collection);
        }
    }

    impl SchemaLookup for MockSchema {
        fn get_collection(&self, name: &str) -> zerobase_core::error::Result<Collection> {
            self.collections
                .lock()
                .unwrap()
                .get(name)
                .cloned()
                .ok_or_else(|| ZerobaseError::not_found_with_id("Collection", name))
        }
    }

    // ── Test helpers ───────────────────────────────────────────────────────

    const USER1_ID: &str = "usr_test_user01";
    const COL_USERS_ID: &str = "col_users_test1";

    fn make_user_record(id: &str, email: &str, token_key: &str) -> HashMap<String, Value> {
        let mut record = HashMap::new();
        record.insert("id".to_string(), Value::String(id.to_string()));
        record.insert("email".to_string(), Value::String(email.to_string()));
        record.insert("verified".to_string(), Value::Bool(true));
        record.insert("tokenKey".to_string(), Value::String(token_key.to_string()));
        record.insert(
            "password".to_string(),
            Value::String("hashed:pass".to_string()),
        );
        record.insert("emailVisibility".to_string(), Value::Bool(false));
        record.insert(
            "created".to_string(),
            Value::String("2024-01-01T00:00:00Z".to_string()),
        );
        record.insert(
            "updated".to_string(),
            Value::String("2024-01-01T00:00:00Z".to_string()),
        );
        record
    }

    struct TestSetup {
        otp_service: OtpService<MockRecordRepo, MockSchema>,
        token_service: Arc<MockTokenService>,
        email_service: Arc<MockEmailService>,
        record_store: RecordStore,
    }

    fn setup() -> TestSetup {
        let record_store: RecordStore = Arc::new(Mutex::new(HashMap::new()));
        let record_repo = MockRecordRepo::new(Arc::clone(&record_store));
        let schema = MockSchema::new();
        schema.add_auth_collection_with_otp("users", COL_USERS_ID, true);

        let record_service = Arc::new(RecordService::new(record_repo, schema));

        let token_service = Arc::new(MockTokenService::new());
        let email_service = Arc::new(MockEmailService::new());

        let otp_service = OtpService::new(
            record_service,
            Arc::clone(&token_service) as Arc<dyn TokenService>,
            Arc::clone(&email_service) as Arc<dyn EmailService>,
            EmailTemplateEngine::new("Zerobase"),
        );

        TestSetup {
            otp_service,
            token_service,
            email_service,
            record_store,
        }
    }

    fn setup_otp_disabled() -> TestSetup {
        let record_store: RecordStore = Arc::new(Mutex::new(HashMap::new()));
        let record_repo = MockRecordRepo::new(Arc::clone(&record_store));
        let schema = MockSchema::new();
        schema.add_auth_collection_with_otp("users", COL_USERS_ID, false);

        let record_service = Arc::new(RecordService::new(record_repo, schema));

        let token_service = Arc::new(MockTokenService::new());
        let email_service = Arc::new(MockEmailService::new());

        let otp_service = OtpService::new(
            record_service,
            Arc::clone(&token_service) as Arc<dyn TokenService>,
            Arc::clone(&email_service) as Arc<dyn EmailService>,
            EmailTemplateEngine::new("Zerobase"),
        );

        TestSetup {
            otp_service,
            token_service,
            email_service,
            record_store,
        }
    }

    fn setup_with_base_collection() -> TestSetup {
        let record_store: RecordStore = Arc::new(Mutex::new(HashMap::new()));
        let record_repo = MockRecordRepo::new(Arc::clone(&record_store));
        let schema = MockSchema::new();
        schema.add_base_collection("posts", "col_posts_test01");

        let record_service = Arc::new(RecordService::new(record_repo, schema));

        let token_service = Arc::new(MockTokenService::new());
        let email_service = Arc::new(MockEmailService::new());

        let otp_service = OtpService::new(
            record_service,
            Arc::clone(&token_service) as Arc<dyn TokenService>,
            Arc::clone(&email_service) as Arc<dyn EmailService>,
            EmailTemplateEngine::new("Zerobase"),
        );

        TestSetup {
            otp_service,
            token_service,
            email_service,
            record_store,
        }
    }

    /// Extract the OTP code from the sent email body.
    fn extract_otp_from_email(sent: &[EmailMessage]) -> String {
        let body = &sent[0].body_text;
        // The template engine puts "verification code is:" on one line
        // and the code on the next non-empty line.
        let mut found_marker = false;
        for line in body.lines() {
            if found_marker {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
            if line.contains("verification code is:") {
                if let Some(rest) = line.split("is:").nth(1) {
                    let rest = rest.trim();
                    if !rest.is_empty() {
                        return rest.to_string();
                    }
                }
                found_marker = true;
            }
        }
        String::new()
    }

    // ── generate_otp_code tests ─────────────────────────────────────────

    #[test]
    fn generated_code_has_correct_length() {
        let code = generate_otp_code();
        assert_eq!(code.len(), OTP_CODE_LENGTH);
    }

    #[test]
    fn generated_code_is_all_digits() {
        for _ in 0..100 {
            let code = generate_otp_code();
            assert!(code.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn generated_code_is_zero_padded() {
        // The code "000001" should remain 6 chars with leading zeros.
        let code = format!("{:0>6}", 1);
        assert_eq!(code, "000001");
        assert_eq!(code.len(), 6);
    }

    // ── request_otp tests ───────────────────────────────────────────────

    #[test]
    fn request_otp_sends_email_and_returns_otp_id() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        let result = s.otp_service.request_otp("users", "test@example.com");
        assert!(result.is_ok());
        let otp_id = result.unwrap();
        assert!(!otp_id.is_empty());

        // Email was sent with the OTP code.
        let sent = s.email_service.sent_messages();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "test@example.com");
        assert!(sent[0].subject.contains("verification code"));
        let code = extract_otp_from_email(&sent);
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn request_otp_returns_otp_id_for_unknown_email() {
        let s = setup();

        let result = s.otp_service.request_otp("users", "unknown@example.com");
        assert!(result.is_ok());
        let otp_id = result.unwrap();
        assert!(!otp_id.is_empty());

        // No email should be sent.
        assert!(s.email_service.sent_messages().is_empty());
    }

    #[test]
    fn request_otp_fails_for_empty_email() {
        let s = setup();

        let result = s.otp_service.request_otp("users", "");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn request_otp_fails_for_non_auth_collection() {
        let s = setup_with_base_collection();

        let result = s.otp_service.request_otp("posts", "test@example.com");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("not an auth collection"));
    }

    #[test]
    fn request_otp_fails_for_unknown_collection() {
        let s = setup();

        let result = s.otp_service.request_otp("nonexistent", "test@example.com");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 404);
    }

    #[test]
    fn request_otp_fails_when_otp_disabled() {
        let s = setup_otp_disabled();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        let result = s.otp_service.request_otp("users", "test@example.com");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("not enabled"));
    }

    #[test]
    fn request_otp_email_failure_returns_error() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);
        s.email_service.set_should_fail(true);

        let result = s.otp_service.request_otp("users", "test@example.com");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 500);
    }

    #[test]
    fn request_otp_email_contains_html_code() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        s.otp_service
            .request_otp("users", "test@example.com")
            .unwrap();

        let sent = s.email_service.sent_messages();
        let html = sent[0].body_html.as_ref().unwrap();
        assert!(html.contains("verification code"));
        // HTML should contain the same code.
        let code = extract_otp_from_email(&sent);
        assert!(html.contains(&code));
    }

    // ── auth_with_otp tests ─────────────────────────────────────────────

    #[test]
    fn auth_with_otp_succeeds_with_valid_code() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        // Request OTP.
        let otp_id = s
            .otp_service
            .request_otp("users", "test@example.com")
            .unwrap();

        // Extract code from email.
        let sent = s.email_service.sent_messages();
        let code = extract_otp_from_email(&sent);

        // Verify OTP.
        let result = s.otp_service.auth_with_otp(&otp_id, &code);
        assert!(result.is_ok());

        let (token, record) = result.unwrap();
        assert!(token.starts_with("mock_token_"));
        assert_eq!(
            record.get("email").and_then(|v| v.as_str()),
            Some("test@example.com")
        );

        // Token was generated with correct params.
        let tokens = s.token_service.generated_tokens();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, USER1_ID); // user_id
        assert_eq!(tokens[0].1, COL_USERS_ID); // collection_id
        assert_eq!(tokens[0].2, TokenType::Auth);
        assert_eq!(tokens[0].3, "tk_123"); // token_key
    }

    #[test]
    fn auth_with_otp_fails_with_wrong_code() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        let otp_id = s
            .otp_service
            .request_otp("users", "test@example.com")
            .unwrap();

        let result = s.otp_service.auth_with_otp(&otp_id, "000000");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("invalid OTP code"));
    }

    #[test]
    fn auth_with_otp_fails_with_invalid_otp_id() {
        let s = setup();

        let result = s.otp_service.auth_with_otp("nonexistent_id__", "123456");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("invalid or expired"));
    }

    #[test]
    fn auth_with_otp_fails_with_empty_otp_id() {
        let s = setup();

        let result = s.otp_service.auth_with_otp("", "123456");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn auth_with_otp_fails_with_empty_code() {
        let s = setup();

        let result = s.otp_service.auth_with_otp("some_id________", "");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn auth_with_otp_code_cannot_be_reused() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        let otp_id = s
            .otp_service
            .request_otp("users", "test@example.com")
            .unwrap();

        let sent = s.email_service.sent_messages();
        let code = extract_otp_from_email(&sent);

        // First use succeeds.
        let result = s.otp_service.auth_with_otp(&otp_id, &code);
        assert!(result.is_ok());

        // Second use fails.
        let result = s.otp_service.auth_with_otp(&otp_id, &code);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("already been used"));
    }

    #[test]
    fn auth_with_otp_max_attempts_enforced() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        let otp_id = s
            .otp_service
            .request_otp("users", "test@example.com")
            .unwrap();

        // Exhaust all attempts with wrong codes.
        for i in 0..MAX_ATTEMPTS {
            let result = s.otp_service.auth_with_otp(&otp_id, &format!("wrong{}", i));
            assert!(result.is_err());
        }

        // Even the correct code should fail now.
        let sent = s.email_service.sent_messages();
        let code = extract_otp_from_email(&sent);
        let result = s.otp_service.auth_with_otp(&otp_id, &code);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("expired")
                || err.to_string().contains("attempts exceeded")
                || err.to_string().contains("invalid or expired")
        );
    }

    #[test]
    fn auth_with_otp_expired_code_rejected() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        let otp_id = s
            .otp_service
            .request_otp("users", "test@example.com")
            .unwrap();

        // Manually expire the OTP by setting expires_at to the past.
        {
            let mut store = s.otp_service.store.lock().unwrap();
            if let Some(record) = store.get_mut(&otp_id) {
                record.expires_at = 0; // Expired in the past.
            }
        }

        let sent = s.email_service.sent_messages();
        let code = extract_otp_from_email(&sent);
        let result = s.otp_service.auth_with_otp(&otp_id, &code);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("expired"));
    }

    #[test]
    fn auth_with_otp_wrong_attempts_counted_correctly() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        let otp_id = s
            .otp_service
            .request_otp("users", "test@example.com")
            .unwrap();

        let sent = s.email_service.sent_messages();
        let code = extract_otp_from_email(&sent);

        // Use wrong code a few times (less than max).
        for _ in 0..3 {
            let result = s.otp_service.auth_with_otp(&otp_id, "wrong!");
            assert!(result.is_err());
        }

        // Correct code should still work.
        let result = s.otp_service.auth_with_otp(&otp_id, &code);
        assert!(result.is_ok());
    }

    #[test]
    fn multiple_otp_requests_for_same_email_work() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        let otp_id1 = s
            .otp_service
            .request_otp("users", "test@example.com")
            .unwrap();
        let otp_id2 = s
            .otp_service
            .request_otp("users", "test@example.com")
            .unwrap();

        // Both should be different IDs.
        assert_ne!(otp_id1, otp_id2);

        // Both emails should have been sent.
        assert_eq!(s.email_service.sent_messages().len(), 2);
    }

    #[test]
    fn request_otp_trims_email() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        let result = s.otp_service.request_otp("users", "  test@example.com  ");
        assert!(result.is_ok());

        let sent = s.email_service.sent_messages();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "test@example.com");
    }
}
