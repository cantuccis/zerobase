//! Password reset flow.
//!
//! Provides [`PasswordResetService`] which handles:
//! - Generating time-limited password reset tokens (JWT)
//! - Sending reset emails via [`EmailService`]
//! - Confirming reset tokens and updating the user's password
//! - Invalidating existing auth tokens on password change (via tokenKey rotation)

use std::sync::Arc;

use tracing::info;

use zerobase_core::auth::{TokenService, TokenType};
use zerobase_core::email::templates::{EmailTemplateEngine, PasswordResetContext};
use zerobase_core::email::EmailService;
use zerobase_core::error::ZerobaseError;
use zerobase_core::services::record_service::{RecordRepository, SchemaLookup};
use zerobase_core::RecordService;

use crate::token::durations;

/// Service responsible for the password reset flow.
///
/// Encapsulates token generation, email sending, and password update logic.
/// Generic over repositories for testability.
pub struct PasswordResetService<R: RecordRepository, S: SchemaLookup> {
    record_service: Arc<RecordService<R, S>>,
    token_service: Arc<dyn TokenService>,
    email_service: Arc<dyn EmailService>,
    template_engine: EmailTemplateEngine,
    /// Base URL for constructing reset links (e.g. "https://myapp.com").
    app_url: String,
}

impl<R: RecordRepository, S: SchemaLookup> PasswordResetService<R, S> {
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

    /// Request a password reset for a user in the given auth collection.
    ///
    /// 1. Looks up the user record by email.
    /// 2. If the email doesn't exist, returns success silently (prevents enumeration).
    /// 3. Generates a time-limited password reset JWT.
    /// 4. Sends the reset email.
    pub fn request_password_reset(
        &self,
        collection_name: &str,
        email: &str,
    ) -> Result<(), ZerobaseError> {
        let email = email.trim();
        if email.is_empty() {
            return Err(ZerobaseError::validation("email is required"));
        }

        // Validate auth collection.
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
            info!(
                email = %email,
                collection = %collection_name,
                "password reset requested for unknown email (silent success)"
            );
            return Ok(());
        }

        let record = &list.items[0];

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

        // Generate password reset token.
        let token = self.token_service.generate(
            &user_id,
            &collection.id,
            TokenType::PasswordReset,
            &token_key,
            Some(durations::PASSWORD_RESET),
        )?;

        // Send reset email.
        let reset_url = format!(
            "{}/_/api/password-reset?token={}&collection={}",
            self.app_url, token, collection_name
        );

        let message = self.template_engine.password_reset(&PasswordResetContext {
            to: email.to_string(),
            reset_url,
            expiry_text: "1 hour".into(),
        });

        self.email_service.send(&message)?;

        info!(
            email = %email,
            user_id = %user_id,
            collection = %collection_name,
            "password reset email sent"
        );

        Ok(())
    }

    /// Confirm a password reset using a token from the reset email.
    ///
    /// 1. Validates the password reset JWT (signature, expiry, type).
    /// 2. Loads the user record and checks the tokenKey matches.
    /// 3. Validates and updates the password.
    /// 4. The password update (via RecordService) automatically rotates the
    ///    tokenKey, invalidating all existing auth/refresh tokens.
    pub fn confirm_password_reset(
        &self,
        collection_name: &str,
        token: &str,
        password: &str,
        password_confirm: &str,
    ) -> Result<(), ZerobaseError> {
        if token.trim().is_empty() {
            return Err(ZerobaseError::validation("token is required"));
        }

        if password.is_empty() {
            return Err(ZerobaseError::validation("password is required"));
        }

        if password != password_confirm {
            return Err(ZerobaseError::validation("passwords do not match"));
        }

        if password.len() < 8 {
            return Err(ZerobaseError::validation(
                "password must be at least 8 characters",
            ));
        }

        // Validate the token structure, signature, and expiry.
        let validated = self
            .token_service
            .validate(token, TokenType::PasswordReset)?;

        // Verify collection is auth and matches the token's collection.
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
                "password reset token has been invalidated",
            ));
        }

        // Update the password via RecordService.
        // RecordService::update_record handles password hashing and tokenKey rotation
        // automatically when a `password` field is included in the update data.
        let update_data = serde_json::json!({
            "password": password,
        });

        self.record_service
            .update_record(collection_name, user_id, update_data)?;

        info!(
            user_id = %user_id,
            collection = %collection_name,
            "password reset confirmed — password updated and tokens invalidated"
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

    type RecordStore = Arc<Mutex<HashMap<String, Vec<HashMap<String, Value>>>>>;

    // ── Mock TokenService ──────────────────────────────────────────────────

    struct MockTokenService {
        generated_tokens: Mutex<Vec<(String, String, TokenType, String, Option<u64>)>>,
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

    fn insert_record(store: &RecordStore, collection: &str, record: HashMap<String, Value>) {
        store
            .lock()
            .unwrap()
            .entry(collection.to_string())
            .or_default()
            .push(record);
    }

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

    // ── No-op password hasher for tests ────────────────────────────────────

    struct TestHasher;

    impl zerobase_core::auth::PasswordHasher for TestHasher {
        fn hash(&self, plain: &str) -> Result<String, ZerobaseError> {
            Ok(format!("hashed:{plain}"))
        }

        fn verify(&self, plain: &str, hash: &str) -> Result<bool, ZerobaseError> {
            Ok(hash == format!("hashed:{plain}"))
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
            Value::String("hashed:oldpass123".to_string()),
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
        service: PasswordResetService<MockRecordRepo, MockSchema>,
        token_service: Arc<MockTokenService>,
        email_service: Arc<MockEmailService>,
        record_store: RecordStore,
    }

    fn setup() -> TestSetup {
        let record_store: RecordStore = Arc::new(Mutex::new(HashMap::new()));
        let record_repo = MockRecordRepo::new(Arc::clone(&record_store));
        let schema = MockSchema::new();
        schema.add_auth_collection("users", COL_USERS_ID);

        let record_service = Arc::new(RecordService::with_password_hasher(
            record_repo,
            schema,
            TestHasher,
        ));

        let token_service = Arc::new(MockTokenService::new());
        let email_service = Arc::new(MockEmailService::new());

        let service = PasswordResetService::new(
            record_service,
            Arc::clone(&token_service) as Arc<dyn TokenService>,
            Arc::clone(&email_service) as Arc<dyn EmailService>,
            EmailTemplateEngine::new("Zerobase"),
            "https://myapp.com".to_string(),
        );

        TestSetup {
            service,
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

        let record_service = Arc::new(RecordService::with_password_hasher(
            record_repo,
            schema,
            TestHasher,
        ));

        let token_service = Arc::new(MockTokenService::new());
        let email_service = Arc::new(MockEmailService::new());

        let service = PasswordResetService::new(
            record_service,
            Arc::clone(&token_service) as Arc<dyn TokenService>,
            Arc::clone(&email_service) as Arc<dyn EmailService>,
            EmailTemplateEngine::new("Zerobase"),
            "https://myapp.com".to_string(),
        );

        TestSetup {
            service,
            token_service,
            email_service,
            record_store,
        }
    }

    fn make_validated_token(user_id: &str, collection_id: &str, token_key: &str) -> ValidatedToken {
        ValidatedToken {
            claims: TokenClaims {
                id: user_id.to_string(),
                collection_id: collection_id.to_string(),
                token_type: TokenType::PasswordReset,
                token_key: token_key.to_string(),
                new_email: None,
                iat: 0,
                exp: 999999999999,
            },
        }
    }

    // ── Request password reset tests ────────────────────────────────────────

    #[test]
    fn request_reset_sends_email_for_existing_user() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        let result = s
            .service
            .request_password_reset("users", "test@example.com");
        assert!(result.is_ok());

        let sent = s.email_service.sent_messages();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "test@example.com");
        assert!(sent[0].subject.contains("Reset"));
        assert!(sent[0]
            .body_text
            .contains(&format!("mock_token_{USER1_ID}")));

        // Token was generated with correct params.
        let tokens = s.token_service.generated_tokens();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, USER1_ID);
        assert_eq!(tokens[0].1, COL_USERS_ID);
        assert_eq!(tokens[0].2, TokenType::PasswordReset);
        assert_eq!(tokens[0].3, "tk_123");
        assert_eq!(tokens[0].4, Some(durations::PASSWORD_RESET));
    }

    #[test]
    fn request_reset_silent_success_for_unknown_email() {
        let s = setup();

        let result = s
            .service
            .request_password_reset("users", "unknown@example.com");
        assert!(result.is_ok());

        assert!(s.email_service.sent_messages().is_empty());
        assert!(s.token_service.generated_tokens().is_empty());
    }

    #[test]
    fn request_reset_fails_for_empty_email() {
        let s = setup();

        let result = s.service.request_password_reset("users", "");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn request_reset_fails_for_non_auth_collection() {
        let s = setup_with_base_collection();

        let result = s
            .service
            .request_password_reset("posts", "test@example.com");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("not an auth collection"));
    }

    #[test]
    fn request_reset_fails_for_unknown_collection() {
        let s = setup();

        let result = s
            .service
            .request_password_reset("nonexistent", "test@example.com");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 404);
    }

    #[test]
    fn request_reset_email_contains_reset_url() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        s.service
            .request_password_reset("users", "test@example.com")
            .unwrap();

        let sent = s.email_service.sent_messages();
        assert!(sent[0].body_text.contains("https://myapp.com"));
        assert!(sent[0]
            .body_text
            .contains(&format!("mock_token_{USER1_ID}")));
        let html = sent[0].body_html.as_ref().unwrap();
        assert!(html.contains("Reset Password"));
        assert!(html.contains(&format!("mock_token_{USER1_ID}")));
    }

    #[test]
    fn request_reset_email_failure_returns_error() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);
        s.email_service.set_should_fail(true);

        let result = s
            .service
            .request_password_reset("users", "test@example.com");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 500);
    }

    // ── Confirm password reset tests ───────────────────────────────────────

    #[test]
    fn confirm_reset_updates_password() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        s.token_service
            .set_validate_ok(make_validated_token(USER1_ID, COL_USERS_ID, "tk_123"));

        let result =
            s.service
                .confirm_password_reset("users", "some_token", "newpass123", "newpass123");
        assert!(result.is_ok(), "confirm failed: {:?}", result.unwrap_err());

        // Verify the password was hashed and updated in the record store.
        let updated = find_one_in_store(&s.record_store, "users", USER1_ID).unwrap();
        assert_eq!(
            updated.get("password").and_then(|v| v.as_str()),
            Some("hashed:newpass123")
        );

        // Verify tokenKey was rotated (invalidates existing auth tokens).
        let new_token_key = updated.get("tokenKey").and_then(|v| v.as_str()).unwrap();
        assert_ne!(
            new_token_key, "tk_123",
            "tokenKey should be rotated after password change"
        );
    }

    #[test]
    fn confirm_reset_fails_with_empty_token() {
        let s = setup();

        let result = s
            .service
            .confirm_password_reset("users", "", "newpass123", "newpass123");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn confirm_reset_fails_with_empty_password() {
        let s = setup();

        s.token_service
            .set_validate_ok(make_validated_token(USER1_ID, COL_USERS_ID, "tk_123"));

        let result = s
            .service
            .confirm_password_reset("users", "some_token", "", "");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn confirm_reset_fails_when_passwords_dont_match() {
        let s = setup();

        s.token_service
            .set_validate_ok(make_validated_token(USER1_ID, COL_USERS_ID, "tk_123"));

        let result =
            s.service
                .confirm_password_reset("users", "some_token", "newpass123", "different1");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("do not match"));
    }

    #[test]
    fn confirm_reset_fails_when_password_too_short() {
        let s = setup();

        s.token_service
            .set_validate_ok(make_validated_token(USER1_ID, COL_USERS_ID, "tk_123"));

        let result = s
            .service
            .confirm_password_reset("users", "some_token", "short", "short");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("at least 8"));
    }

    #[test]
    fn confirm_reset_fails_with_invalid_token() {
        let s = setup();

        s.token_service.set_validate_err("token has expired");

        let result =
            s.service
                .confirm_password_reset("users", "expired_token", "newpass123", "newpass123");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 401);
    }

    #[test]
    fn confirm_reset_fails_with_collection_mismatch() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        s.token_service.set_validate_ok(make_validated_token(
            USER1_ID,
            "col_OTHER_test01",
            "tk_123",
        ));

        let result =
            s.service
                .confirm_password_reset("users", "some_token", "newpass123", "newpass123");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn confirm_reset_fails_with_invalidated_token_key() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "new_key");
        insert_record(&s.record_store, "users", user);

        s.token_service
            .set_validate_ok(make_validated_token(USER1_ID, COL_USERS_ID, "old_key"));

        let result =
            s.service
                .confirm_password_reset("users", "some_token", "newpass123", "newpass123");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 401);
        assert!(err.to_string().contains("invalidated"));
    }

    #[test]
    fn confirm_reset_fails_for_nonexistent_user() {
        let s = setup();

        s.token_service.set_validate_ok(make_validated_token(
            "nonexistent_user1",
            COL_USERS_ID,
            "tk_123",
        ));

        let result =
            s.service
                .confirm_password_reset("users", "some_token", "newpass123", "newpass123");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 404);
    }

    #[test]
    fn confirm_reset_fails_for_non_auth_collection() {
        let s = setup_with_base_collection();

        s.token_service.set_validate_ok(make_validated_token(
            "post_test_id_01",
            "col_posts_test01",
            "tk_____________",
        ));

        let result =
            s.service
                .confirm_password_reset("posts", "some_token", "newpass123", "newpass123");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn confirm_reset_token_used_once_only() {
        let s = setup();
        let user = make_user_record(USER1_ID, "test@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        s.token_service
            .set_validate_ok(make_validated_token(USER1_ID, COL_USERS_ID, "tk_123"));

        // First use succeeds.
        let result =
            s.service
                .confirm_password_reset("users", "some_token", "newpass123", "newpass123");
        assert!(result.is_ok());

        // RecordService::update_record rotates tokenKey when password changes.
        // The token still carries the old tokenKey ("tk_123"), so a second
        // attempt with the same validated claims will fail.
        let updated = find_one_in_store(&s.record_store, "users", USER1_ID).unwrap();
        let new_token_key = updated.get("tokenKey").and_then(|v| v.as_str()).unwrap();
        assert_ne!(new_token_key, "tk_123", "tokenKey should have rotated");

        // Second use fails because tokenKey was rotated.
        let result =
            s.service
                .confirm_password_reset("users", "some_token", "anotherp1", "anotherp1");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 401);
        assert!(err.to_string().contains("invalidated"));
    }
}
