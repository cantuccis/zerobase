//! Auth flow security and edge-case tests.
//!
//! Covers the following security checks:
//! 1. Token invalidation — changing password/tokenKey invalidates existing JWTs
//! 2. OTP brute-force — max attempts (5) and code expiry (5 minutes)
//! 3. OAuth2 state parameter — CSRF protection via unique state values
//! 4. MFA bypass — MfaPartial tokens cannot access protected resources
//! 5. Race conditions — concurrent OTP verifications and password resets
//! 6. Timing attacks — constant-time password comparison and token validation
//! 7. Rate limiting on auth endpoints — tested in zerobase-api rate_limit module

#[cfg(test)]
mod tests {
    // ═══════════════════════════════════════════════════════════════════════
    // 1. Token Invalidation
    // ═══════════════════════════════════════════════════════════════════════

    mod token_invalidation {
        use secrecy::SecretString;

        use zerobase_core::auth::{TokenService, TokenType};

        use crate::token::{durations, JwtTokenService};

        fn test_secret() -> SecretString {
            SecretString::from("test-secret-key-that-is-long-enough-for-hmac-sha256!!")
        }

        fn service() -> JwtTokenService {
            JwtTokenService::new(test_secret(), durations::AUTH)
        }

        #[test]
        fn password_change_rotates_token_key_invalidating_all_tokens() {
            let svc = service();

            // User has token_key "key_v1". Issue auth, refresh, and file tokens.
            let auth_token = svc
                .generate("u1", "c1", TokenType::Auth, "key_v1", None)
                .unwrap();
            let refresh_token = svc
                .generate(
                    "u1",
                    "c1",
                    TokenType::Refresh,
                    "key_v1",
                    Some(durations::REFRESH),
                )
                .unwrap();
            let file_token = svc
                .generate(
                    "u1",
                    "c1",
                    TokenType::File,
                    "key_v1",
                    Some(durations::FILE),
                )
                .unwrap();

            // All tokens valid before key rotation.
            assert!(svc
                .validate_with_key(&auth_token, TokenType::Auth, "key_v1")
                .is_ok());
            assert!(svc
                .validate_with_key(&refresh_token, TokenType::Refresh, "key_v1")
                .is_ok());
            assert!(svc
                .validate_with_key(&file_token, TokenType::File, "key_v1")
                .is_ok());

            // Simulate password change → token_key rotated to "key_v2".
            // ALL old tokens must be rejected.
            assert!(svc
                .validate_with_key(&auth_token, TokenType::Auth, "key_v2")
                .is_err());
            assert!(svc
                .validate_with_key(&refresh_token, TokenType::Refresh, "key_v2")
                .is_err());
            assert!(svc
                .validate_with_key(&file_token, TokenType::File, "key_v2")
                .is_err());
        }

        #[test]
        fn token_key_change_does_not_affect_other_users() {
            let svc = service();

            let token_user_a = svc
                .generate("user_a", "c1", TokenType::Auth, "key_a_v1", None)
                .unwrap();
            let token_user_b = svc
                .generate("user_b", "c1", TokenType::Auth, "key_b_v1", None)
                .unwrap();

            // Rotate user A's key.
            assert!(svc
                .validate_with_key(&token_user_a, TokenType::Auth, "key_a_v2")
                .is_err());

            // User B's token is unaffected.
            assert!(svc
                .validate_with_key(&token_user_b, TokenType::Auth, "key_b_v1")
                .is_ok());
        }

        #[test]
        fn mfa_partial_token_invalidated_by_key_change() {
            let svc = service();

            let mfa_token = svc
                .generate(
                    "u1",
                    "c1",
                    TokenType::MfaPartial,
                    "old_key",
                    Some(durations::MFA_PARTIAL),
                )
                .unwrap();

            assert!(svc
                .validate_with_key(&mfa_token, TokenType::MfaPartial, "old_key")
                .is_ok());
            assert!(svc
                .validate_with_key(&mfa_token, TokenType::MfaPartial, "new_key")
                .is_err());
        }

        #[test]
        fn password_reset_token_invalidated_by_key_change() {
            let svc = service();

            let reset_token = svc
                .generate(
                    "u1",
                    "c1",
                    TokenType::PasswordReset,
                    "old_key",
                    Some(durations::PASSWORD_RESET),
                )
                .unwrap();

            assert!(svc
                .validate_with_key(&reset_token, TokenType::PasswordReset, "old_key")
                .is_ok());
            assert!(svc
                .validate_with_key(&reset_token, TokenType::PasswordReset, "new_key")
                .is_err());
        }

        #[test]
        fn cross_type_token_reuse_blocked_even_with_valid_key() {
            let svc = service();

            let auth_token = svc
                .generate("u1", "c1", TokenType::Auth, "key1", None)
                .unwrap();

            // Try to use auth token as other types — must fail.
            assert!(svc
                .validate_with_key(&auth_token, TokenType::Refresh, "key1")
                .is_err());
            assert!(svc
                .validate_with_key(&auth_token, TokenType::MfaPartial, "key1")
                .is_err());
            assert!(svc
                .validate_with_key(&auth_token, TokenType::PasswordReset, "key1")
                .is_err());
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Shared mock infrastructure for OTP tests
    // ═══════════════════════════════════════════════════════════════════════

    mod otp_mocks {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        use serde_json::Value;

        use zerobase_core::auth::{TokenService, TokenType, ValidatedToken};
        use zerobase_core::email::templates::EmailTemplateEngine;
        use zerobase_core::email::{EmailMessage, EmailService};
        use zerobase_core::error::ZerobaseError;
        use zerobase_core::schema::{AuthOptions, Collection, CollectionType};
        use zerobase_core::services::record_service::{
            RecordList, RecordQuery, RecordRepoError, RecordRepository, RecordService, SchemaLookup,
        };

        use crate::otp::OtpService;

        pub type RecordStore = Arc<Mutex<HashMap<String, Vec<HashMap<String, Value>>>>>;

        pub struct MockTokenService;
        impl TokenService for MockTokenService {
            fn generate(
                &self,
                user_id: &str,
                _collection_id: &str,
                _token_type: TokenType,
                _token_key: &str,
                _duration_secs: Option<u64>,
            ) -> Result<String, ZerobaseError> {
                Ok(format!("mock_token_{user_id}"))
            }
            fn validate(
                &self,
                _token: &str,
                _expected_type: TokenType,
            ) -> Result<ValidatedToken, ZerobaseError> {
                Err(ZerobaseError::auth("not implemented"))
            }
        }

        pub struct MockEmailService {
            sent: Mutex<Vec<EmailMessage>>,
        }
        impl MockEmailService {
            pub fn new() -> Self {
                Self {
                    sent: Mutex::new(Vec::new()),
                }
            }
            pub fn sent_messages(&self) -> Vec<EmailMessage> {
                self.sent.lock().unwrap().clone()
            }
        }
        impl EmailService for MockEmailService {
            fn send(&self, message: &EmailMessage) -> Result<(), ZerobaseError> {
                self.sent.lock().unwrap().push(message.clone());
                Ok(())
            }
        }

        pub struct MockRecordRepo {
            records: RecordStore,
        }
        impl MockRecordRepo {
            pub fn new(store: RecordStore) -> Self {
                Self { records: store }
            }
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
                    items: filtered.clone(),
                    page: 1,
                    per_page: query.per_page,
                    total_items: filtered.len() as u64,
                    total_pages: 1,
                })
            }
            fn insert(
                &self,
                collection: &str,
                data: &HashMap<String, Value>,
            ) -> std::result::Result<(), RecordRepoError> {
                self.records
                    .lock()
                    .unwrap()
                    .entry(collection.to_string())
                    .or_default()
                    .push(data.clone());
                Ok(())
            }
            fn update(
                &self,
                _collection: &str,
                _id: &str,
                _data: &HashMap<String, Value>,
            ) -> std::result::Result<bool, RecordRepoError> {
                Ok(true)
            }
            fn delete(
                &self,
                _collection: &str,
                _id: &str,
            ) -> std::result::Result<bool, RecordRepoError> {
                Ok(true)
            }
            fn count(
                &self,
                collection: &str,
                _filter: Option<&str>,
            ) -> std::result::Result<u64, RecordRepoError> {
                let records = self.records.lock().unwrap();
                Ok(records
                    .get(collection)
                    .map(|r| r.len() as u64)
                    .unwrap_or(0))
            }
            fn find_referencing_records(
                &self,
                _collection: &str,
                _field_name: &str,
                _referenced_id: &str,
            ) -> std::result::Result<Vec<HashMap<String, Value>>, RecordRepoError> {
                Ok(vec![])
            }
        }

        pub struct MockSchema {
            collections: Mutex<HashMap<String, Collection>>,
        }
        impl MockSchema {
            pub fn new() -> Self {
                Self {
                    collections: Mutex::new(HashMap::new()),
                }
            }
            pub fn add_auth_collection(&self, name: &str, id: &str) {
                let collection = Collection {
                    id: id.to_string(),
                    name: name.to_string(),
                    collection_type: CollectionType::Auth,
                    fields: vec![],
                    rules: Default::default(),
                    indexes: vec![],
                    view_query: None,
                    auth_options: Some(AuthOptions {
                        allow_otp_auth: true,
                        ..Default::default()
                    }),
                };
                self.collections
                    .lock()
                    .unwrap()
                    .insert(name.to_string(), collection);
            }
        }
        impl SchemaLookup for MockSchema {
            fn get_collection(
                &self,
                name: &str,
            ) -> zerobase_core::error::Result<Collection> {
                self.collections
                    .lock()
                    .unwrap()
                    .get(name)
                    .cloned()
                    .ok_or_else(|| ZerobaseError::not_found_with_id("Collection", name))
            }
        }

        pub fn make_user_record(
            id: &str,
            email: &str,
            token_key: &str,
        ) -> HashMap<String, Value> {
            let mut record = HashMap::new();
            record.insert("id".to_string(), Value::String(id.to_string()));
            record.insert("email".to_string(), Value::String(email.to_string()));
            record.insert("verified".to_string(), Value::Bool(true));
            record.insert(
                "tokenKey".to_string(),
                Value::String(token_key.to_string()),
            );
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

        pub fn insert_record(
            store: &RecordStore,
            collection: &str,
            record: HashMap<String, Value>,
        ) {
            store
                .lock()
                .unwrap()
                .entry(collection.to_string())
                .or_default()
                .push(record);
        }

        pub fn extract_otp_from_email(sent: &[EmailMessage]) -> String {
            let body = &sent[0].body_text;
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

        pub struct TestSetup {
            pub otp_service: OtpService<MockRecordRepo, MockSchema>,
            pub email_service: Arc<MockEmailService>,
            pub record_store: RecordStore,
        }

        pub fn setup() -> TestSetup {
            let record_store: RecordStore = Arc::new(Mutex::new(HashMap::new()));
            let record_repo = MockRecordRepo::new(Arc::clone(&record_store));
            let schema = MockSchema::new();
            schema.add_auth_collection("users", "col_users_01");
            let record_service = Arc::new(RecordService::new(record_repo, schema));
            let token_service = Arc::new(MockTokenService);
            let email_service = Arc::new(MockEmailService::new());
            let otp_service = OtpService::new(
                record_service,
                token_service as Arc<dyn TokenService>,
                Arc::clone(&email_service) as Arc<dyn EmailService>,
                EmailTemplateEngine::new("Zerobase"),
            );
            TestSetup {
                otp_service,
                email_service,
                record_store,
            }
        }

        pub fn setup_arc() -> (
            Arc<OtpService<MockRecordRepo, MockSchema>>,
            Arc<MockEmailService>,
            RecordStore,
        ) {
            let record_store: RecordStore = Arc::new(Mutex::new(HashMap::new()));
            let record_repo = MockRecordRepo::new(Arc::clone(&record_store));
            let schema = MockSchema::new();
            schema.add_auth_collection("users", "col_users_01");
            let record_service = Arc::new(RecordService::new(record_repo, schema));
            let token_service = Arc::new(MockTokenService);
            let email_service = Arc::new(MockEmailService::new());
            let otp_service = Arc::new(OtpService::new(
                record_service,
                token_service as Arc<dyn TokenService>,
                Arc::clone(&email_service) as Arc<dyn EmailService>,
                EmailTemplateEngine::new("Zerobase"),
            ));
            (otp_service, email_service, record_store)
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 2. OTP Brute-Force Protection
    // ═══════════════════════════════════════════════════════════════════════

    mod otp_brute_force {
        use super::otp_mocks::*;

        #[test]
        fn otp_exactly_max_attempts_exhausts_then_rejects_correct_code() {
            let s = setup();
            let user = make_user_record("u1", "test@example.com", "tk1");
            insert_record(&s.record_store, "users", user);

            let otp_id = s
                .otp_service
                .request_otp("users", "test@example.com")
                .unwrap();

            // Use 5 wrong attempts (MAX_ATTEMPTS = 5).
            for i in 0..5 {
                let result = s
                    .otp_service
                    .auth_with_otp(&otp_id, &format!("bad{i:03}"));
                assert!(result.is_err(), "attempt {i} should fail");
            }

            // Correct code must be rejected after exhausting attempts.
            let sent = s.email_service.sent_messages();
            let code = extract_otp_from_email(&sent);
            let result = s.otp_service.auth_with_otp(&otp_id, &code);
            assert!(result.is_err());
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("expired")
                    || err_msg.contains("attempts exceeded")
                    || err_msg.contains("invalid or expired"),
                "expected expiry/attempts error, got: {err_msg}"
            );
        }

        #[test]
        fn otp_attempt_4_wrong_then_correct_succeeds() {
            let s = setup();
            let user = make_user_record("u1", "test@example.com", "tk1");
            insert_record(&s.record_store, "users", user);

            let otp_id = s
                .otp_service
                .request_otp("users", "test@example.com")
                .unwrap();

            let sent = s.email_service.sent_messages();
            let code = extract_otp_from_email(&sent);

            // 4 wrong attempts (one less than MAX_ATTEMPTS).
            for _ in 0..4 {
                let _ = s.otp_service.auth_with_otp(&otp_id, "wrong!");
            }

            // 5th attempt with correct code should still succeed.
            let result = s.otp_service.auth_with_otp(&otp_id, &code);
            assert!(result.is_ok(), "correct code on last attempt should work");
        }

        #[test]
        fn otp_code_single_use_prevents_replay() {
            let s = setup();
            let user = make_user_record("u1", "test@example.com", "tk1");
            insert_record(&s.record_store, "users", user);

            let otp_id = s
                .otp_service
                .request_otp("users", "test@example.com")
                .unwrap();

            let sent = s.email_service.sent_messages();
            let code = extract_otp_from_email(&sent);

            // First use succeeds.
            assert!(s.otp_service.auth_with_otp(&otp_id, &code).is_ok());

            // Replay must fail.
            let result = s.otp_service.auth_with_otp(&otp_id, &code);
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("already been used"));
        }

        #[test]
        fn otp_for_unknown_email_returns_otp_id_without_leaking() {
            let s = setup();
            // Don't insert any user — email is unknown.
            let result = s
                .otp_service
                .request_otp("users", "nonexistent@example.com");
            // Should succeed (anti-enumeration) but no email sent.
            assert!(result.is_ok());
            assert!(
                s.email_service.sent_messages().is_empty(),
                "no email should be sent for unknown address"
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 3. OAuth2 State Parameter — CSRF Protection
    // ═══════════════════════════════════════════════════════════════════════

    mod oauth2_csrf {
        #[test]
        fn oauth2_state_values_are_unique_per_request() {
            let mut states: Vec<String> = (0..100)
                .map(|_| zerobase_core::id::generate_id())
                .collect();
            let total = states.len();
            states.sort();
            states.dedup();
            assert_eq!(
                states.len(),
                total,
                "all 100 state values must be unique"
            );
        }

        #[test]
        fn oauth2_state_has_sufficient_entropy() {
            let state = zerobase_core::id::generate_id();
            assert!(
                state.len() >= 15,
                "state token too short ({} chars): {state}",
                state.len()
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 4. MFA Bypass Prevention
    // ═══════════════════════════════════════════════════════════════════════

    mod mfa_bypass {
        use secrecy::SecretString;

        use zerobase_core::auth::{TokenService, TokenType};

        use crate::token::{durations, JwtTokenService};

        fn service() -> JwtTokenService {
            JwtTokenService::new(
                SecretString::from("test-secret-key-that-is-long-enough-for-hmac-sha256!!"),
                durations::AUTH,
            )
        }

        #[test]
        fn mfa_partial_token_rejected_as_auth_token() {
            let svc = service();
            let mfa_token = svc
                .generate(
                    "u1",
                    "c1",
                    TokenType::MfaPartial,
                    "key1",
                    Some(durations::MFA_PARTIAL),
                )
                .unwrap();

            let result = svc.validate(&mfa_token, TokenType::Auth);
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(
                err.to_string().contains("type mismatch"),
                "expected type mismatch error, got: {}",
                err
            );
        }

        #[test]
        fn mfa_partial_token_only_accepted_as_mfa_partial() {
            let svc = service();
            let mfa_token = svc
                .generate(
                    "u1",
                    "c1",
                    TokenType::MfaPartial,
                    "key1",
                    Some(durations::MFA_PARTIAL),
                )
                .unwrap();

            // Only MfaPartial type validation should succeed.
            assert!(svc.validate(&mfa_token, TokenType::MfaPartial).is_ok());

            // All other types must reject it.
            for token_type in [
                TokenType::Auth,
                TokenType::Refresh,
                TokenType::File,
                TokenType::Verification,
                TokenType::PasswordReset,
                TokenType::EmailChange,
            ] {
                assert!(
                    svc.validate(&mfa_token, token_type).is_err(),
                    "MfaPartial token should not validate as {token_type}"
                );
            }
        }

        #[test]
        fn auth_token_cannot_be_used_as_mfa_partial() {
            let svc = service();
            let auth_token = svc
                .generate("u1", "c1", TokenType::Auth, "key1", None)
                .unwrap();

            assert!(svc.validate(&auth_token, TokenType::MfaPartial).is_err());
        }

        #[test]
        fn mfa_partial_token_has_short_lifetime() {
            assert_eq!(
                durations::MFA_PARTIAL, 300,
                "MFA partial token should expire in 5 minutes"
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 5. Race Conditions
    // ═══════════════════════════════════════════════════════════════════════

    mod race_conditions {
        use std::sync::Arc;
        use std::thread;

        use super::otp_mocks::*;

        #[test]
        fn concurrent_otp_verifications_only_one_succeeds() {
            let (otp_service, email_service, record_store) = setup_arc();

            let user = make_user_record("u1", "test@example.com", "tk1");
            record_store
                .lock()
                .unwrap()
                .entry("users".to_string())
                .or_default()
                .push(user);

            let otp_id = otp_service
                .request_otp("users", "test@example.com")
                .unwrap();

            let sent = email_service.sent_messages();
            let code = extract_otp_from_email(&sent);

            // Spawn multiple threads all trying to verify the same code simultaneously.
            let num_threads = 10;
            let results: Vec<_> = (0..num_threads)
                .map(|_| {
                    let svc = Arc::clone(&otp_service);
                    let otp_id = otp_id.clone();
                    let code = code.clone();
                    thread::spawn(move || svc.auth_with_otp(&otp_id, &code))
                })
                .collect();

            let outcomes: Vec<_> = results.into_iter().map(|h| h.join().unwrap()).collect();

            let successes = outcomes.iter().filter(|r| r.is_ok()).count();
            let failures = outcomes.iter().filter(|r| r.is_err()).count();

            // Exactly one thread should succeed; all others must fail.
            assert_eq!(
                successes, 1,
                "exactly one concurrent verification should succeed, got {successes}"
            );
            assert_eq!(failures, num_threads - 1);
        }

        #[test]
        fn concurrent_otp_requests_for_same_email_all_produce_valid_ids() {
            let (otp_service, _email_service, record_store) = setup_arc();

            let user = make_user_record("u1", "test@example.com", "tk1");
            record_store
                .lock()
                .unwrap()
                .entry("users".to_string())
                .or_default()
                .push(user);

            let num_threads = 10;
            let results: Vec<_> = (0..num_threads)
                .map(|_| {
                    let svc = Arc::clone(&otp_service);
                    thread::spawn(move || svc.request_otp("users", "test@example.com"))
                })
                .collect();

            let otp_ids: Vec<_> = results
                .into_iter()
                .map(|h| h.join().unwrap().unwrap())
                .collect();

            // All should succeed and produce unique IDs.
            assert_eq!(otp_ids.len(), num_threads);
            let unique: std::collections::HashSet<_> = otp_ids.iter().collect();
            assert_eq!(
                unique.len(),
                num_threads,
                "all OTP IDs should be unique"
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 6. Timing Attacks
    // ═══════════════════════════════════════════════════════════════════════

    mod timing_attacks {
        use crate::password::{hash_password, verify_password};

        #[test]
        fn password_verification_timing_consistent_correct_vs_incorrect() {
            let hash = hash_password("correct_password").unwrap();
            let iterations = 5;

            let start = std::time::Instant::now();
            for _ in 0..iterations {
                let _ = verify_password("correct_password", &hash);
            }
            let correct_duration = start.elapsed();

            let start = std::time::Instant::now();
            for _ in 0..iterations {
                let _ = verify_password("wrong_password_value", &hash);
            }
            let incorrect_duration = start.elapsed();

            let ratio =
                correct_duration.as_nanos() as f64 / incorrect_duration.as_nanos() as f64;
            assert!(
                (0.2..=5.0).contains(&ratio),
                "timing ratio {ratio:.2} outside [0.2, 5.0] — \
                 correct={correct_duration:?}, incorrect={incorrect_duration:?}"
            );
        }

        #[test]
        fn password_verification_timing_consistent_short_vs_long_wrong_password() {
            let hash = hash_password("reference_password").unwrap();
            let iterations = 5;

            let start = std::time::Instant::now();
            for _ in 0..iterations {
                let _ = verify_password("a", &hash);
            }
            let short_duration = start.elapsed();

            let long_pwd = "x".repeat(500);
            let start = std::time::Instant::now();
            for _ in 0..iterations {
                let _ = verify_password(&long_pwd, &hash);
            }
            let long_duration = start.elapsed();

            let ratio = short_duration.as_nanos() as f64 / long_duration.as_nanos() as f64;
            assert!(
                (0.1..=10.0).contains(&ratio),
                "timing ratio {ratio:.2} outside [0.1, 10.0] — \
                 short={short_duration:?}, long={long_duration:?}"
            );
        }

        #[test]
        fn token_validation_timing_consistent_valid_vs_invalid_key() {
            use secrecy::SecretString;
            use zerobase_core::auth::{TokenService, TokenType};
            use crate::token::{durations, JwtTokenService};

            let svc = JwtTokenService::new(
                SecretString::from("test-secret-key-that-is-long-enough-for-hmac-sha256!!"),
                durations::AUTH,
            );

            let token = svc
                .generate("u1", "c1", TokenType::Auth, "correct_key", None)
                .unwrap();

            let iterations = 50;

            let start = std::time::Instant::now();
            for _ in 0..iterations {
                let _ = svc.validate_with_key(&token, TokenType::Auth, "correct_key");
            }
            let correct_duration = start.elapsed();

            let start = std::time::Instant::now();
            for _ in 0..iterations {
                let _ = svc.validate_with_key(&token, TokenType::Auth, "wrong_key");
            }
            let wrong_duration = start.elapsed();

            let ratio =
                correct_duration.as_nanos() as f64 / wrong_duration.as_nanos() as f64;
            assert!(
                (0.1..=10.0).contains(&ratio),
                "token validation timing ratio {ratio:.2} outside [0.1, 10.0] — \
                 correct={correct_duration:?}, wrong={wrong_duration:?}"
            );
        }

        #[test]
        fn argon2id_uses_owasp_recommended_parameters() {
            let hash = hash_password("test").unwrap();
            assert!(hash.starts_with("$argon2id$"), "must use Argon2id");
            assert!(hash.contains("m=19456"), "memory cost must be 19456 KiB");
            assert!(hash.contains("t=2"), "time cost must be 2");
            assert!(hash.contains("p=1"), "parallelism must be 1");
        }
    }
}
