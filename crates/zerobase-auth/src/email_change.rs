//! Email change flow.
//!
//! Provides [`EmailChangeService`] which handles:
//! - Generating time-limited email-change tokens (JWT with `newEmail` claim)
//! - Sending confirmation emails to the **new** address
//! - Confirming the change: validating the token, checking new-email uniqueness,
//!   updating the record, and rotating `tokenKey` to invalidate old tokens

use std::sync::Arc;

use tracing::info;

use zerobase_core::auth::{TokenService, TokenType};
use zerobase_core::email::templates::{EmailChangeContext, EmailTemplateEngine};
use zerobase_core::email::EmailService;
use zerobase_core::error::ZerobaseError;
use zerobase_core::services::record_service::{RecordRepository, SchemaLookup};
use zerobase_core::RecordService;

use crate::token::durations;

/// Service responsible for the email change flow.
///
/// Follows the same request/confirm pattern as verification and password reset:
/// 1. **Request** — authenticated user asks to change their email.
/// 2. A confirmation link is sent to the **new** email address.
/// 3. **Confirm** — the user clicks the link, the email is updated, and all
///    existing tokens are invalidated (via `tokenKey` rotation).
pub struct EmailChangeService<R: RecordRepository, S: SchemaLookup> {
    record_service: Arc<RecordService<R, S>>,
    token_service: Arc<dyn TokenService>,
    email_service: Arc<dyn EmailService>,
    template_engine: EmailTemplateEngine,
    /// Base URL for constructing confirmation links (e.g. "https://myapp.com").
    app_url: String,
}

impl<R: RecordRepository, S: SchemaLookup> EmailChangeService<R, S> {
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

    /// Request an email change for a user in the given auth collection.
    ///
    /// 1. Validates that the collection is an auth collection.
    /// 2. Looks up the user by `user_id` (the caller must be authenticated).
    /// 3. Validates the new email is different and not already taken.
    /// 4. Generates an `EmailChange` JWT containing the `newEmail` claim.
    /// 5. Sends a confirmation email to the **new** address.
    pub fn request_email_change(
        &self,
        collection_name: &str,
        user_id: &str,
        new_email: &str,
    ) -> Result<(), ZerobaseError> {
        let new_email = new_email.trim();
        if new_email.is_empty() {
            return Err(ZerobaseError::validation("newEmail is required"));
        }

        // Validate auth collection.
        let collection = self.record_service.get_collection(collection_name)?;
        if collection.collection_type != zerobase_core::CollectionType::Auth {
            return Err(ZerobaseError::validation(format!(
                "collection '{}' is not an auth collection",
                collection_name
            )));
        }

        // Load the authenticated user's record.
        let record = self.record_service.get_record(collection_name, user_id)?;

        let current_email = record.get("email").and_then(|v| v.as_str()).unwrap_or("");

        // New email must be different from current.
        if current_email == new_email {
            return Err(ZerobaseError::validation(
                "the new email must be different from the current one",
            ));
        }

        // Check uniqueness: ensure the new email is not already used.
        let query = zerobase_core::services::record_service::RecordQuery {
            filter: Some(format!("email = {:?}", new_email)),
            page: 1,
            per_page: 1,
            ..Default::default()
        };

        let existing = self.record_service.list_records(collection_name, &query)?;

        if !existing.items.is_empty() {
            return Err(ZerobaseError::validation(
                "the requested new email is already in use",
            ));
        }

        let token_key = record
            .get("tokenKey")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Generate email-change token with the newEmail claim.
        let token = self.token_service.generate_with_new_email(
            user_id,
            &collection.id,
            TokenType::EmailChange,
            &token_key,
            new_email,
            Some(durations::EMAIL_CHANGE),
        )?;

        // Send confirmation email to the NEW address.
        let confirm_url = format!(
            "{}/_/api/confirm-email-change?token={}&collection={}",
            self.app_url, token, collection_name
        );

        let message = self.template_engine.email_change(&EmailChangeContext {
            to: new_email.to_string(),
            confirm_url,
            expiry_text: "1 hour".into(),
        });

        self.email_service.send(&message)?;

        info!(
            user_id = %user_id,
            new_email = %new_email,
            collection = %collection_name,
            "email change confirmation sent to new address"
        );

        Ok(())
    }

    /// Confirm an email change using a token from the confirmation email.
    ///
    /// 1. Validates the email-change JWT (signature, expiry, type).
    /// 2. Extracts the `newEmail` claim from the token.
    /// 3. Verifies the collection matches.
    /// 4. Loads the user record and checks the tokenKey matches.
    /// 5. Re-validates that the new email is still unique.
    /// 6. Updates the user's email and rotates `tokenKey` (invalidating old tokens).
    pub fn confirm_email_change(
        &self,
        collection_name: &str,
        token: &str,
    ) -> Result<(), ZerobaseError> {
        if token.trim().is_empty() {
            return Err(ZerobaseError::validation("token is required"));
        }

        // Validate the token structure, signature, and expiry.
        let validated = self.token_service.validate(token, TokenType::EmailChange)?;

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

        // Extract the new email from the token claims.
        let new_email = validated
            .claims
            .new_email
            .as_deref()
            .ok_or_else(|| ZerobaseError::validation("token does not contain a new email"))?;

        if new_email.is_empty() {
            return Err(ZerobaseError::validation(
                "token contains an empty new email",
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
                "email change token has been invalidated",
            ));
        }

        // Re-check uniqueness at confirmation time (could have been taken since request).
        let query = zerobase_core::services::record_service::RecordQuery {
            filter: Some(format!("email = {:?}", new_email)),
            page: 1,
            per_page: 1,
            ..Default::default()
        };

        let existing = self.record_service.list_records(collection_name, &query)?;

        if !existing.items.is_empty() {
            return Err(ZerobaseError::validation(
                "the requested new email is already in use",
            ));
        }

        // Update the email. Include a password-related sentinel so RecordService
        // rotates the tokenKey (invalidating all existing tokens).
        // We set `email` and trigger tokenKey rotation by updating a special field.
        let update_data = serde_json::json!({
            "email": new_email,
            "verified": true,
        });

        self.record_service
            .update_record(collection_name, user_id, update_data)?;

        // Explicitly rotate tokenKey to invalidate old tokens.
        let new_token_key = uuid::Uuid::new_v4().to_string();
        let key_update = serde_json::json!({
            "tokenKey": new_token_key,
        });
        self.record_service
            .update_record(collection_name, user_id, key_update)?;

        info!(
            user_id = %user_id,
            new_email = %new_email,
            collection = %collection_name,
            "email change confirmed — email updated and tokens invalidated"
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
        generated_new_emails: Mutex<Vec<String>>,
        validate_ok: Mutex<Option<ValidatedToken>>,
        validate_err: Mutex<Option<String>>,
    }

    impl MockTokenService {
        fn new() -> Self {
            Self {
                generated_tokens: Mutex::new(Vec::new()),
                generated_new_emails: Mutex::new(Vec::new()),
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

        fn generated_new_emails(&self) -> Vec<String> {
            self.generated_new_emails.lock().unwrap().clone()
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

        fn generate_with_new_email(
            &self,
            user_id: &str,
            collection_id: &str,
            token_type: TokenType,
            token_key: &str,
            new_email: &str,
            duration_secs: Option<u64>,
        ) -> Result<String, ZerobaseError> {
            self.generated_new_emails
                .lock()
                .unwrap()
                .push(new_email.to_string());
            self.generate(user_id, collection_id, token_type, token_key, duration_secs)
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

    // ── Test helpers ───────────────────────────────────────────────────────

    const USER1_ID: &str = "usr_test_user01";
    const USER2_ID: &str = "usr_test_user02";
    const COL_USERS_ID: &str = "col_users_test1";

    fn make_user_record(id: &str, email: &str, token_key: &str) -> HashMap<String, Value> {
        let mut record = HashMap::new();
        record.insert("id".to_string(), Value::String(id.to_string()));
        record.insert("email".to_string(), Value::String(email.to_string()));
        record.insert("verified".to_string(), Value::Bool(true));
        record.insert("tokenKey".to_string(), Value::String(token_key.to_string()));
        record.insert(
            "password".to_string(),
            Value::String("hashed:pass123".to_string()),
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
        service: EmailChangeService<MockRecordRepo, MockSchema>,
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

        let service = EmailChangeService::new(
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

        let record_service = Arc::new(RecordService::new(record_repo, schema));

        let token_service = Arc::new(MockTokenService::new());
        let email_service = Arc::new(MockEmailService::new());

        let service = EmailChangeService::new(
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

    fn make_validated_token(
        user_id: &str,
        collection_id: &str,
        token_key: &str,
        new_email: &str,
    ) -> ValidatedToken {
        ValidatedToken {
            claims: TokenClaims {
                id: user_id.to_string(),
                collection_id: collection_id.to_string(),
                token_type: TokenType::EmailChange,
                token_key: token_key.to_string(),
                new_email: Some(new_email.to_string()),
                iat: 0,
                exp: 999999999999,
            },
        }
    }

    // ── Request email change tests ────────────────────────────────────────

    #[test]
    fn request_email_change_sends_confirmation_to_new_email() {
        let s = setup();
        let user = make_user_record(USER1_ID, "old@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        let result = s
            .service
            .request_email_change("users", USER1_ID, "new@example.com");
        assert!(result.is_ok());

        // Email sent to the NEW address.
        let sent = s.email_service.sent_messages();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "new@example.com");
        assert!(sent[0].subject.contains("Confirm"));
        assert!(sent[0]
            .body_text
            .contains(&format!("mock_token_{USER1_ID}")));

        // Token was generated with correct params.
        let tokens = s.token_service.generated_tokens();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, USER1_ID);
        assert_eq!(tokens[0].1, COL_USERS_ID);
        assert_eq!(tokens[0].2, TokenType::EmailChange);
        assert_eq!(tokens[0].3, "tk_123");
        assert_eq!(tokens[0].4, Some(durations::EMAIL_CHANGE));

        // New email was embedded in the token.
        let new_emails = s.token_service.generated_new_emails();
        assert_eq!(new_emails, vec!["new@example.com"]);
    }

    #[test]
    fn request_email_change_fails_for_empty_new_email() {
        let s = setup();
        let user = make_user_record(USER1_ID, "old@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        let result = s.service.request_email_change("users", USER1_ID, "");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn request_email_change_fails_when_same_as_current() {
        let s = setup();
        let user = make_user_record(USER1_ID, "old@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        let result = s
            .service
            .request_email_change("users", USER1_ID, "old@example.com");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("different"));
    }

    #[test]
    fn request_email_change_fails_when_new_email_already_taken() {
        let s = setup();
        let user1 = make_user_record(USER1_ID, "user1@example.com", "tk_123");
        let user2 = make_user_record(USER2_ID, "user2@example.com", "tk_456");
        insert_record(&s.record_store, "users", user1);
        insert_record(&s.record_store, "users", user2);

        let result = s
            .service
            .request_email_change("users", USER1_ID, "user2@example.com");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("already in use"));
    }

    #[test]
    fn request_email_change_fails_for_non_auth_collection() {
        let s = setup_with_base_collection();

        let result = s
            .service
            .request_email_change("posts", "some_id", "new@example.com");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("not an auth collection"));
    }

    #[test]
    fn request_email_change_fails_for_unknown_collection() {
        let s = setup();

        let result = s
            .service
            .request_email_change("nonexistent", USER1_ID, "new@example.com");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 404);
    }

    #[test]
    fn request_email_change_fails_for_nonexistent_user() {
        let s = setup();

        let result =
            s.service
                .request_email_change("users", "nonexistent_user1", "new@example.com");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 404);
    }

    #[test]
    fn request_email_change_email_contains_confirm_url() {
        let s = setup();
        let user = make_user_record(USER1_ID, "old@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        s.service
            .request_email_change("users", USER1_ID, "new@example.com")
            .unwrap();

        let sent = s.email_service.sent_messages();
        assert!(sent[0].body_text.contains("https://myapp.com"));
        assert!(sent[0]
            .body_text
            .contains(&format!("mock_token_{USER1_ID}")));
        let html = sent[0].body_html.as_ref().unwrap();
        assert!(html.contains("Confirm Email Change"));
        assert!(html.contains(&format!("mock_token_{USER1_ID}")));
    }

    #[test]
    fn request_email_change_email_failure_returns_error() {
        let s = setup();
        let user = make_user_record(USER1_ID, "old@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);
        s.email_service.set_should_fail(true);

        let result = s
            .service
            .request_email_change("users", USER1_ID, "new@example.com");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 500);
    }

    // ── Confirm email change tests ───────────────────────────────────────

    #[test]
    fn confirm_email_change_updates_email() {
        let s = setup();
        let user = make_user_record(USER1_ID, "old@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        s.token_service.set_validate_ok(make_validated_token(
            USER1_ID,
            COL_USERS_ID,
            "tk_123",
            "new@example.com",
        ));

        let result = s.service.confirm_email_change("users", "some_token");
        assert!(result.is_ok(), "confirm failed: {:?}", result.unwrap_err());

        // Verify the email was updated.
        let updated = find_one_in_store(&s.record_store, "users", USER1_ID).unwrap();
        assert_eq!(
            updated.get("email").and_then(|v| v.as_str()),
            Some("new@example.com")
        );

        // Verify tokenKey was rotated (invalidates existing tokens).
        let new_token_key = updated.get("tokenKey").and_then(|v| v.as_str()).unwrap();
        assert_ne!(
            new_token_key, "tk_123",
            "tokenKey should be rotated after email change"
        );
    }

    #[test]
    fn confirm_email_change_sets_verified_true() {
        let s = setup();
        let mut user = make_user_record(USER1_ID, "old@example.com", "tk_123");
        user.insert("verified".to_string(), Value::Bool(false));
        insert_record(&s.record_store, "users", user);

        s.token_service.set_validate_ok(make_validated_token(
            USER1_ID,
            COL_USERS_ID,
            "tk_123",
            "new@example.com",
        ));

        s.service
            .confirm_email_change("users", "some_token")
            .unwrap();

        let updated = find_one_in_store(&s.record_store, "users", USER1_ID).unwrap();
        assert_eq!(
            updated.get("verified").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn confirm_email_change_fails_with_empty_token() {
        let s = setup();

        let result = s.service.confirm_email_change("users", "");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn confirm_email_change_fails_with_invalid_token() {
        let s = setup();

        s.token_service.set_validate_err("token has expired");

        let result = s.service.confirm_email_change("users", "expired_token");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 401);
    }

    #[test]
    fn confirm_email_change_fails_with_collection_mismatch() {
        let s = setup();
        let user = make_user_record(USER1_ID, "old@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        s.token_service.set_validate_ok(make_validated_token(
            USER1_ID,
            "col_OTHER_test01",
            "tk_123",
            "new@example.com",
        ));

        let result = s.service.confirm_email_change("users", "some_token");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn confirm_email_change_fails_with_invalidated_token_key() {
        let s = setup();
        let user = make_user_record(USER1_ID, "old@example.com", "new_key");
        insert_record(&s.record_store, "users", user);

        s.token_service.set_validate_ok(make_validated_token(
            USER1_ID,
            COL_USERS_ID,
            "old_key",
            "new@example.com",
        ));

        let result = s.service.confirm_email_change("users", "some_token");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 401);
        assert!(err.to_string().contains("invalidated"));
    }

    #[test]
    fn confirm_email_change_fails_when_new_email_taken_at_confirm_time() {
        let s = setup();
        let user1 = make_user_record(USER1_ID, "old@example.com", "tk_123");
        // Another user took the desired email between request and confirm.
        let user2 = make_user_record(USER2_ID, "new@example.com", "tk_456");
        insert_record(&s.record_store, "users", user1);
        insert_record(&s.record_store, "users", user2);

        s.token_service.set_validate_ok(make_validated_token(
            USER1_ID,
            COL_USERS_ID,
            "tk_123",
            "new@example.com",
        ));

        let result = s.service.confirm_email_change("users", "some_token");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("already in use"));
    }

    #[test]
    fn confirm_email_change_fails_for_nonexistent_user() {
        let s = setup();

        s.token_service.set_validate_ok(make_validated_token(
            "nonexistent_user1",
            COL_USERS_ID,
            "tk_123",
            "new@example.com",
        ));

        let result = s.service.confirm_email_change("users", "some_token");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 404);
    }

    #[test]
    fn confirm_email_change_fails_for_non_auth_collection() {
        let s = setup_with_base_collection();

        s.token_service.set_validate_ok(make_validated_token(
            "post_test_id_01",
            "col_posts_test01",
            "tk_____________",
            "new@example.com",
        ));

        let result = s.service.confirm_email_change("posts", "some_token");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn confirm_email_change_fails_when_token_missing_new_email() {
        let s = setup();
        let user = make_user_record(USER1_ID, "old@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        // Token without new_email claim.
        let token = ValidatedToken {
            claims: TokenClaims {
                id: USER1_ID.to_string(),
                collection_id: COL_USERS_ID.to_string(),
                token_type: TokenType::EmailChange,
                token_key: "tk_123".to_string(),
                new_email: None,
                iat: 0,
                exp: 999999999999,
            },
        };
        s.token_service.set_validate_ok(token);

        let result = s.service.confirm_email_change("users", "some_token");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn confirm_email_change_token_used_once_only() {
        let s = setup();
        let user = make_user_record(USER1_ID, "old@example.com", "tk_123");
        insert_record(&s.record_store, "users", user);

        s.token_service.set_validate_ok(make_validated_token(
            USER1_ID,
            COL_USERS_ID,
            "tk_123",
            "new@example.com",
        ));

        // First use succeeds.
        let result = s.service.confirm_email_change("users", "some_token");
        assert!(result.is_ok());

        // The tokenKey was rotated, so the same token (carrying old key) fails.
        let updated = find_one_in_store(&s.record_store, "users", USER1_ID).unwrap();
        let new_token_key = updated.get("tokenKey").and_then(|v| v.as_str()).unwrap();
        assert_ne!(new_token_key, "tk_123", "tokenKey should have rotated");

        // Second use fails.
        let result = s.service.confirm_email_change("users", "some_token");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 401);
        assert!(err.to_string().contains("invalidated"));
    }
}
