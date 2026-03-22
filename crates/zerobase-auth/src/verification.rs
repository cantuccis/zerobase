//! Email verification flow.
//!
//! Provides [`VerificationService`] which handles:
//! - Generating time-limited verification tokens (JWT)
//! - Sending verification emails via [`EmailService`]
//! - Confirming verification tokens and marking users as verified

use std::sync::Arc;

use tracing::info;

use zerobase_core::auth::{TokenService, TokenType};
use zerobase_core::email::templates::{EmailTemplateEngine, VerificationContext};
use zerobase_core::email::EmailService;
use zerobase_core::error::ZerobaseError;
use zerobase_core::services::record_service::{RecordRepository, SchemaLookup};
use zerobase_core::RecordService;

use crate::token::durations;

/// Service responsible for the email verification flow.
///
/// Encapsulates token generation, email sending, and record update logic.
/// Generic over repositories for testability.
pub struct VerificationService<R: RecordRepository, S: SchemaLookup> {
    record_service: Arc<RecordService<R, S>>,
    token_service: Arc<dyn TokenService>,
    email_service: Arc<dyn EmailService>,
    template_engine: EmailTemplateEngine,
    /// Base URL for constructing verification links (e.g. "https://myapp.com").
    app_url: String,
}

impl<R: RecordRepository, S: SchemaLookup> VerificationService<R, S> {
    pub fn new(
        record_service: Arc<RecordService<R, S>>,
        token_service: Arc<dyn TokenService>,
        email_service: Arc<dyn EmailService>,
        template_engine: EmailTemplateEngine,
        app_url: String,
    ) -> Self {
        Self {
            record_service,
            token_service,
            email_service,
            template_engine,
            app_url,
        }
    }

    /// Request email verification for a user in the given auth collection.
    ///
    /// 1. Looks up the user record by email.
    /// 2. If already verified, returns early (no error, no email).
    /// 3. Generates a time-limited verification JWT.
    /// 4. Sends the verification email.
    pub fn request_verification(
        &self,
        collection_name: &str,
        email: &str,
    ) -> Result<(), ZerobaseError> {
        // Validate email is not empty.
        let email = email.trim();
        if email.is_empty() {
            return Err(ZerobaseError::validation("email is required"));
        }

        // Look up the collection to get its ID and verify it's an auth collection.
        let collection = self.record_service.get_collection(collection_name)?;
        if collection.collection_type != zerobase_core::CollectionType::Auth {
            return Err(ZerobaseError::validation(format!(
                "collection '{}' is not an auth collection",
                collection_name
            )));
        }

        // Find the user by email.
        let query = zerobase_core::services::record_service::RecordQuery {
            filter: Some(format!("email = {:?}", email)),
            page: 1,
            per_page: 1,
            ..Default::default()
        };

        let list = self.record_service.list_records(collection_name, &query)?;

        if list.items.is_empty() {
            // Don't reveal whether the email exists — return success silently.
            // This prevents email enumeration attacks.
            info!(
                email = %email,
                collection = %collection_name,
                "verification requested for unknown email (silent success)"
            );
            return Ok(());
        }

        let record = &list.items[0];

        // Check if already verified.
        let already_verified = record
            .get("verified")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if already_verified {
            info!(
                email = %email,
                collection = %collection_name,
                "verification requested for already-verified user (silent success)"
            );
            return Ok(());
        }

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

        // Generate verification token.
        let token = self.token_service.generate(
            &user_id,
            &collection.id,
            TokenType::Verification,
            &token_key,
            Some(durations::VERIFICATION),
        )?;

        // Send verification email.
        let verification_url = format!(
            "{}/_/api/verify?token={}&collection={}",
            self.app_url, token, collection_name
        );

        let message = self.template_engine.verification(&VerificationContext {
            to: email.to_string(),
            verification_url,
            expiry_text: "7 days".into(),
        });

        self.email_service.send(&message)?;

        info!(
            email = %email,
            user_id = %user_id,
            collection = %collection_name,
            "verification email sent"
        );

        Ok(())
    }

    /// Confirm email verification using a token from the verification email.
    ///
    /// 1. Validates the verification JWT (signature, expiry, type).
    /// 2. Loads the user record and checks the tokenKey matches.
    /// 3. Sets `verified = true` on the record.
    /// 4. Returns the updated user record.
    pub fn confirm_verification(
        &self,
        collection_name: &str,
        token: &str,
    ) -> Result<(), ZerobaseError> {
        if token.trim().is_empty() {
            return Err(ZerobaseError::validation("token is required"));
        }

        // Validate the token structure, signature, and expiry.
        let validated = self
            .token_service
            .validate(token, TokenType::Verification)?;

        // Look up the collection to verify it's auth and matches the token's collection.
        let collection = self.record_service.get_collection(collection_name)?;
        if collection.collection_type != zerobase_core::CollectionType::Auth {
            return Err(ZerobaseError::validation(format!(
                "collection '{}' is not an auth collection",
                collection_name
            )));
        }

        if validated.claims.collection_id != collection.id {
            return Err(ZerobaseError::validation(
                "token was not issued for this collection",
            ));
        }

        // Load the user record.
        let user_id = &validated.claims.id;
        let record = self.record_service.get_record(collection_name, user_id)?;

        // Verify the tokenKey hasn't changed (token invalidation check).
        let current_token_key = record
            .get("tokenKey")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if current_token_key != validated.claims.token_key {
            return Err(ZerobaseError::auth(
                "verification token has been invalidated",
            ));
        }

        // Check if already verified.
        let already_verified = record
            .get("verified")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if already_verified {
            // Already verified — success (idempotent).
            return Ok(());
        }

        // Update the record to set verified = true.
        let update_data = serde_json::json!({
            "verified": true,
        });

        self.record_service
            .update_record(collection_name, user_id, update_data)?;

        info!(
            user_id = %user_id,
            collection = %collection_name,
            "user email verified successfully"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    use serde_json::Value;

    use zerobase_core::auth::{TokenClaims, ValidatedToken};
    use zerobase_core::email::templates::EmailTemplateEngine;
    use zerobase_core::email::EmailMessage;
    use zerobase_core::schema::{Collection, CollectionType};
    use zerobase_core::services::record_service::{
        RecordList, RecordQuery, RecordRepoError, RecordRepository, SchemaLookup,
    };

    // ── Shared inner data ─────────────────────────────────────────────────

    /// Inner storage shared between a `MockRecordRepo` and the test harness.
    type RecordStore = Arc<Mutex<HashMap<String, Vec<HashMap<String, Value>>>>>;

    // ── Mock TokenService ──────────────────────────────────────────────────

    /// Stores validate results as `Option<ValidatedToken>` (Ok) or `Option<String>` (Err message).
    struct MockTokenService {
        generated_tokens: Mutex<Vec<(String, String, TokenType, String, Option<u64>)>>,
        /// `Some(Ok(token))` or `Some(Err(msg))`.
        validate_ok: Mutex<Option<ValidatedToken>>,
        validate_err: Mutex<Option<String>>,
    }

    impl MockTokenService {
        fn new() -> Self {
            Self {
                generated_tokens: Mutex::new(Vec::new()),
                validate_ok: Mutex::new(None),
                validate_err: Mutex::new(None),
            }
        }

        fn set_validate_ok(&self, token: ValidatedToken) {
            *self.validate_ok.lock().unwrap() = Some(token);
            *self.validate_err.lock().unwrap() = None;
        }

        fn set_validate_err(&self, msg: &str) {
            *self.validate_err.lock().unwrap() = Some(msg.to_string());
            *self.validate_ok.lock().unwrap() = None;
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
            if let Some(ref msg) = *self.validate_err.lock().unwrap() {
                return Err(ZerobaseError::auth(msg));
            }
            self.validate_ok
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| ZerobaseError::auth("no validate result configured"))
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

    /// Helper to insert into a shared store directly from tests.
    fn insert_record(store: &RecordStore, collection: &str, record: HashMap<String, Value>) {
        store
            .lock()
            .unwrap()
            .entry(collection.to_string())
            .or_default()
            .push(record);
    }

    /// Helper to read a record from the shared store (for assertions).
    fn find_one_in_store(
        store: &RecordStore,
        collection: &str,
        id: &str,
    ) -> Option<HashMap<String, Value>> {
        let records = store.lock().unwrap();
        records.get(collection).and_then(|col| {
            col.iter()
                .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
                .cloned()
        })
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

            // Simple email filter support for tests.
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

        fn add_auth_collection(&self, name: &str, id: &str) {
            let collection = Collection {
                id: id.to_string(),
                name: name.to_string(),
                collection_type: CollectionType::Auth,
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

    /// A valid 15-char ID for tests (matches the id field validation).
    const USER1_ID: &str = "usr_test_user01";
    const COL_USERS_ID: &str = "col_users_test1";

    fn make_user_record(
        id: &str,
        email: &str,
        verified: bool,
        token_key: &str,
    ) -> HashMap<String, Value> {
        let mut record = HashMap::new();
        record.insert("id".to_string(), Value::String(id.to_string()));
        record.insert("email".to_string(), Value::String(email.to_string()));
        record.insert("verified".to_string(), Value::Bool(verified));
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
        verification_service: VerificationService<MockRecordRepo, MockSchema>,
        token_service: Arc<MockTokenService>,
        email_service: Arc<MockEmailService>,
        record_store: RecordStore,
    }

    fn setup() -> TestSetup {
        let record_store: RecordStore = Arc::new(Mutex::new(HashMap::new()));
        let record_repo = MockRecordRepo::new(Arc::clone(&record_store));
        let schema = MockSchema::new();
        schema.add_auth_collection("users", COL_USERS_ID);

        let record_service = Arc::new(RecordService::new(record_repo, schema));

        let token_service = Arc::new(MockTokenService::new());
        let email_service = Arc::new(MockEmailService::new());

        let verification_service = VerificationService::new(
            record_service,
            Arc::clone(&token_service) as Arc<dyn TokenService>,
            Arc::clone(&email_service) as Arc<dyn EmailService>,
            EmailTemplateEngine::new("Zerobase"),
            "https://myapp.com".to_string(),
        );

        TestSetup {
            verification_service,
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

        let verification_service = VerificationService::new(
            record_service,
            Arc::clone(&token_service) as Arc<dyn TokenService>,
            Arc::clone(&email_service) as Arc<dyn EmailService>,
            EmailTemplateEngine::new("Zerobase"),
            "https://myapp.com".to_string(),
        );

        TestSetup {
            verification_service,
            token_service,
            email_service,
            record_store,
        }
    }

    // ── Request verification tests ─────────────────────────────────────────

    #[test]
    fn request_verification_sends_email_for_unverified_user() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", false, "tk_123");
        insert_record(&s.record_store, "users", user);

        let result = s
            .verification_service
            .request_verification("users", "test@example.com");
        assert!(result.is_ok());

        let sent = s.email_service.sent_messages();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "test@example.com");
        assert!(sent[0].subject.contains("Verify"));
        assert!(sent[0]
            .body_text
            .contains(&format!("mock_token_{USER1_ID}")));

        // Token was generated with correct params.
        let tokens = s.token_service.generated_tokens();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, USER1_ID); // user_id
        assert_eq!(tokens[0].1, COL_USERS_ID); // collection_id
        assert_eq!(tokens[0].2, TokenType::Verification);
        assert_eq!(tokens[0].3, "tk_123"); // token_key
        assert_eq!(tokens[0].4, Some(durations::VERIFICATION));
    }

    #[test]
    fn request_verification_silent_success_for_already_verified() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", true, "tk_123");
        insert_record(&s.record_store, "users", user);

        let result = s
            .verification_service
            .request_verification("users", "test@example.com");
        assert!(result.is_ok());

        // No email should be sent.
        assert!(s.email_service.sent_messages().is_empty());
        // No token should be generated.
        assert!(s.token_service.generated_tokens().is_empty());
    }

    #[test]
    fn request_verification_silent_success_for_unknown_email() {
        let s = setup();

        let result = s
            .verification_service
            .request_verification("users", "unknown@example.com");
        assert!(result.is_ok());

        // No email sent, no token generated — prevents enumeration.
        assert!(s.email_service.sent_messages().is_empty());
        assert!(s.token_service.generated_tokens().is_empty());
    }

    #[test]
    fn request_verification_fails_for_empty_email() {
        let s = setup();

        let result = s.verification_service.request_verification("users", "");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn request_verification_fails_for_non_auth_collection() {
        let s = setup_with_base_collection();

        let result = s
            .verification_service
            .request_verification("posts", "test@example.com");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("not an auth collection"));
    }

    #[test]
    fn request_verification_fails_for_unknown_collection() {
        let s = setup();

        let result = s
            .verification_service
            .request_verification("nonexistent", "test@example.com");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 404);
    }

    #[test]
    fn request_verification_email_contains_verification_url() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", false, "tk_123");
        insert_record(&s.record_store, "users", user);

        s.verification_service
            .request_verification("users", "test@example.com")
            .unwrap();

        let sent = s.email_service.sent_messages();
        assert!(sent[0].body_text.contains("https://myapp.com"));
        assert!(sent[0]
            .body_text
            .contains(&format!("mock_token_{USER1_ID}")));
        let html = sent[0].body_html.as_ref().unwrap();
        assert!(html.contains("Verify Email"));
        assert!(html.contains(&format!("mock_token_{USER1_ID}")));
    }

    #[test]
    fn request_verification_email_failure_returns_error() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", false, "tk_123");
        insert_record(&s.record_store, "users", user);
        s.email_service.set_should_fail(true);

        let result = s
            .verification_service
            .request_verification("users", "test@example.com");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 500);
    }

    // ── Confirm verification tests ─────────────────────────────────────────

    fn make_validated_token(user_id: &str, collection_id: &str, token_key: &str) -> ValidatedToken {
        ValidatedToken {
            claims: TokenClaims {
                id: user_id.to_string(),
                collection_id: collection_id.to_string(),
                token_type: TokenType::Verification,
                token_key: token_key.to_string(),
                new_email: None,
                iat: 0,
                exp: 999999999999,
            },
        }
    }

    #[test]
    fn confirm_verification_marks_user_as_verified() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", false, "tk_123");
        insert_record(&s.record_store, "users", user);

        s.token_service
            .set_validate_ok(make_validated_token(USER1_ID, COL_USERS_ID, "tk_123"));

        let result = s
            .verification_service
            .confirm_verification("users", "some_token");
        assert!(
            result.is_ok(),
            "confirm_verification failed: {:?}",
            result.unwrap_err()
        );

        // Verify the record was updated.
        let updated = find_one_in_store(&s.record_store, "users", USER1_ID).unwrap();
        assert_eq!(
            updated.get("verified").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn confirm_verification_fails_with_empty_token() {
        let s = setup();

        let result = s.verification_service.confirm_verification("users", "");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn confirm_verification_fails_with_invalid_token() {
        let s = setup();

        s.token_service.set_validate_err("token has expired");

        let result = s
            .verification_service
            .confirm_verification("users", "expired_token");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 401);
    }

    #[test]
    fn confirm_verification_fails_with_collection_mismatch() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", false, "tk_123");
        insert_record(&s.record_store, "users", user);

        s.token_service.set_validate_ok(make_validated_token(
            USER1_ID,
            "col_OTHER_test01",
            "tk_123",
        ));

        let result = s
            .verification_service
            .confirm_verification("users", "some_token");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn confirm_verification_fails_with_invalidated_token_key() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", false, "new_key");
        insert_record(&s.record_store, "users", user);

        s.token_service
            .set_validate_ok(make_validated_token(USER1_ID, COL_USERS_ID, "old_key"));

        let result = s
            .verification_service
            .confirm_verification("users", "some_token");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 401);
        assert!(err.to_string().contains("invalidated"));
    }

    #[test]
    fn confirm_verification_idempotent_for_already_verified() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", true, "tk_123");
        insert_record(&s.record_store, "users", user);

        s.token_service
            .set_validate_ok(make_validated_token(USER1_ID, COL_USERS_ID, "tk_123"));

        let result = s
            .verification_service
            .confirm_verification("users", "some_token");
        assert!(result.is_ok());
    }

    #[test]
    fn confirm_verification_fails_for_nonexistent_user() {
        let s = setup();

        s.token_service.set_validate_ok(make_validated_token(
            "nonexistent_user1",
            COL_USERS_ID,
            "tk_123",
        ));

        let result = s
            .verification_service
            .confirm_verification("users", "some_token");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 404);
    }

    #[test]
    fn confirm_verification_fails_for_non_auth_collection() {
        let s = setup_with_base_collection();

        s.token_service.set_validate_ok(make_validated_token(
            "post_test_id_01",
            "col_posts_test01",
            "tk_____________",
        ));

        let result = s
            .verification_service
            .confirm_verification("posts", "some_token");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }
}
