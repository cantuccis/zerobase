//! Superuser management service.
//!
//! [`SuperuserService`] handles CRUD operations on superuser accounts stored
//! in the `_superusers` system table. Superusers bypass all collection access
//! rules and can manage the system (collections, settings, etc.).
//!
//! # Design
//!
//! - Passwords are hashed via the [`PasswordHasher`] trait before storage.
//! - Each superuser has a `tokenKey` for JWT invalidation.
//! - The service is generic over `SuperuserRepository` for testability.

use std::collections::HashMap;

use serde_json::Value;

use crate::auth::PasswordHasher;
use crate::error::{Result, ZerobaseError};
use crate::id::generate_id;

/// Well-known collection ID for the `_superusers` system collection.
///
/// Used in JWT claims so the auth middleware can resolve superuser tokens
/// back to the `_superusers` collection.
pub const SUPERUSERS_COLLECTION_ID: &str = "pbc_superusers0";

/// The name of the superuser system collection.
pub const SUPERUSERS_COLLECTION_NAME: &str = "_superusers";

// ── Repository trait ──────────────────────────────────────────────────────────

/// Persistence contract for superuser accounts.
///
/// Defined in core so the service doesn't depend on `zerobase-db` directly.
/// The DB crate implements this trait on `Database`.
pub trait SuperuserRepository: Send + Sync {
    /// Find a superuser by their ID.
    fn find_by_id(&self, id: &str) -> Result<Option<HashMap<String, Value>>>;

    /// Find a superuser by email address.
    fn find_by_email(&self, email: &str) -> Result<Option<HashMap<String, Value>>>;

    /// Insert a new superuser record.
    fn insert(&self, data: &HashMap<String, Value>) -> Result<()>;

    /// Update an existing superuser record.
    fn update(&self, id: &str, data: &HashMap<String, Value>) -> Result<()>;

    /// Delete a superuser by ID.
    fn delete(&self, id: &str) -> Result<bool>;

    /// List all superusers.
    fn list_all(&self) -> Result<Vec<HashMap<String, Value>>>;

    /// Count the number of superusers.
    fn count(&self) -> Result<u64>;
}

// ── SuperuserService ─────────────────────────────────────────────────────────

/// Service for managing superuser accounts.
///
/// Generic over `R: SuperuserRepository` for testability with mocks.
pub struct SuperuserService<R: SuperuserRepository> {
    repo: R,
    hasher: Box<dyn PasswordHasher>,
}

impl<R: SuperuserRepository> SuperuserService<R> {
    /// Create a new service wrapping the given repository and password hasher.
    pub fn new(repo: R, hasher: impl PasswordHasher + 'static) -> Self {
        Self {
            repo,
            hasher: Box::new(hasher),
        }
    }

    /// Create a new superuser account.
    ///
    /// Generates a unique ID, hashes the password, and creates a tokenKey
    /// for JWT invalidation. Returns the created record (password stripped).
    pub fn create_superuser(&self, email: &str, password: &str) -> Result<HashMap<String, Value>> {
        // Validate inputs.
        let email = email.trim().to_lowercase();
        if email.is_empty() {
            return Err(ZerobaseError::validation("email is required"));
        }
        if password.is_empty() {
            return Err(ZerobaseError::validation("password is required"));
        }
        if password.len() < 8 {
            return Err(ZerobaseError::validation(
                "password must be at least 8 characters",
            ));
        }

        // Check for duplicate email.
        if self.repo.find_by_email(&email)?.is_some() {
            return Err(ZerobaseError::conflict(format!(
                "superuser with email '{email}' already exists"
            )));
        }

        let id = generate_id();
        let hashed_password = self.hasher.hash(password)?;
        let token_key = generate_id(); // Random token key for invalidation.

        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let mut record = HashMap::new();
        record.insert("id".to_string(), Value::String(id.clone()));
        record.insert("email".to_string(), Value::String(email));
        record.insert("password".to_string(), Value::String(hashed_password));
        record.insert("tokenKey".to_string(), Value::String(token_key));
        record.insert("created".to_string(), Value::String(now.clone()));
        record.insert("updated".to_string(), Value::String(now));

        self.repo.insert(&record)?;

        // Strip password from the returned record.
        let mut result = record;
        result.remove("password");
        Ok(result)
    }

    /// Authenticate a superuser by email and password.
    ///
    /// Returns the superuser record with `tokenKey` intact (for JWT generation).
    /// The caller should strip `tokenKey` before sending the response.
    pub fn authenticate(&self, email: &str, password: &str) -> Result<HashMap<String, Value>> {
        let email = email.trim().to_lowercase();
        if email.is_empty() || password.is_empty() {
            return Err(ZerobaseError::auth("Failed to authenticate."));
        }

        let record = self
            .repo
            .find_by_email(&email)?
            .ok_or_else(|| ZerobaseError::auth("Failed to authenticate."))?;

        let stored_hash = record
            .get("password")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if stored_hash.is_empty() {
            return Err(ZerobaseError::auth("Failed to authenticate."));
        }

        let valid = self.hasher.verify(password, stored_hash)?;
        if !valid {
            return Err(ZerobaseError::auth("Failed to authenticate."));
        }

        // Return record with tokenKey (password stripped).
        let mut result = record;
        result.remove("password");
        Ok(result)
    }

    /// Get a superuser by ID (password stripped).
    pub fn get_superuser(&self, id: &str) -> Result<HashMap<String, Value>> {
        let record = self
            .repo
            .find_by_id(id)?
            .ok_or_else(|| ZerobaseError::not_found_with_id("Superuser", id))?;

        let mut result = record;
        result.remove("password");
        result.remove("tokenKey");
        Ok(result)
    }

    /// List all superusers (passwords and tokenKeys stripped).
    pub fn list_superusers(&self) -> Result<Vec<HashMap<String, Value>>> {
        let records = self.repo.list_all()?;
        Ok(records
            .into_iter()
            .map(|mut r| {
                r.remove("password");
                r.remove("tokenKey");
                r
            })
            .collect())
    }

    /// Update an existing superuser's email and/or password.
    ///
    /// Looks up the superuser by their current email. If `new_email` is
    /// provided, checks for duplicates before changing it. If `new_password`
    /// is provided, hashes and stores the new password. A new `tokenKey` is
    /// generated to invalidate existing JWTs.
    ///
    /// Returns the updated record (password stripped).
    pub fn update_superuser(
        &self,
        email: &str,
        new_email: Option<&str>,
        new_password: Option<&str>,
    ) -> Result<HashMap<String, Value>> {
        let email = email.trim().to_lowercase();

        let record = self
            .repo
            .find_by_email(&email)?
            .ok_or_else(|| ZerobaseError::not_found_with_id("Superuser", &email))?;

        let id = record
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ZerobaseError::internal("superuser record missing id"))?
            .to_string();

        let mut updates = HashMap::new();

        if let Some(new_email) = new_email {
            let new_email = new_email.trim().to_lowercase();
            if new_email.is_empty() {
                return Err(ZerobaseError::validation("new email must not be empty"));
            }
            // Check for duplicate email (only if actually changing).
            if new_email != email {
                if self.repo.find_by_email(&new_email)?.is_some() {
                    return Err(ZerobaseError::conflict(format!(
                        "superuser with email '{new_email}' already exists"
                    )));
                }
            }
            updates.insert("email".to_string(), Value::String(new_email));
        }

        if let Some(new_password) = new_password {
            if new_password.is_empty() {
                return Err(ZerobaseError::validation("new password must not be empty"));
            }
            if new_password.len() < 8 {
                return Err(ZerobaseError::validation(
                    "password must be at least 8 characters",
                ));
            }
            let hashed = self.hasher.hash(new_password)?;
            updates.insert("password".to_string(), Value::String(hashed));
        }

        if updates.is_empty() {
            return Err(ZerobaseError::validation(
                "at least one of new_email or new_password must be provided",
            ));
        }

        // Rotate tokenKey to invalidate existing JWTs.
        updates.insert("tokenKey".to_string(), Value::String(generate_id()));

        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        updates.insert("updated".to_string(), Value::String(now));

        self.repo.update(&id, &updates)?;

        // Re-fetch and strip sensitive fields.
        let updated = self
            .repo
            .find_by_id(&id)?
            .ok_or_else(|| ZerobaseError::internal("superuser vanished after update"))?;

        let mut result = updated;
        result.remove("password");
        result.remove("tokenKey");
        Ok(result)
    }

    /// Find a superuser by email. Returns None if not found.
    pub fn find_by_email(&self, email: &str) -> Result<Option<HashMap<String, Value>>> {
        let email = email.trim().to_lowercase();
        let record = self.repo.find_by_email(&email)?;
        Ok(record.map(|mut r| {
            r.remove("password");
            r.remove("tokenKey");
            r
        }))
    }

    /// Delete a superuser by ID.
    pub fn delete_superuser(&self, id: &str) -> Result<()> {
        let deleted = self.repo.delete(id)?;
        if !deleted {
            return Err(ZerobaseError::not_found_with_id("Superuser", id));
        }
        Ok(())
    }

    /// Check if any superusers exist. Used for first-run setup.
    pub fn has_superusers(&self) -> Result<bool> {
        Ok(self.repo.count()? > 0)
    }

    /// Create an initial superuser if none exist. Used during first-run setup.
    ///
    /// Returns `Ok(Some(record))` if a superuser was created, or `Ok(None)`
    /// if superusers already exist.
    pub fn ensure_initial_superuser(
        &self,
        email: &str,
        password: &str,
    ) -> Result<Option<HashMap<String, Value>>> {
        if self.has_superusers()? {
            return Ok(None);
        }
        let record = self.create_superuser(email, password)?;
        Ok(Some(record))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // ── In-memory mock repository ────────────────────────────────────────

    struct MockSuperuserRepo {
        records: Mutex<Vec<HashMap<String, Value>>>,
    }

    impl MockSuperuserRepo {
        fn new() -> Self {
            Self {
                records: Mutex::new(Vec::new()),
            }
        }

        fn with_records(records: Vec<HashMap<String, Value>>) -> Self {
            Self {
                records: Mutex::new(records),
            }
        }
    }

    impl SuperuserRepository for MockSuperuserRepo {
        fn find_by_id(&self, id: &str) -> Result<Option<HashMap<String, Value>>> {
            let store = self.records.lock().unwrap();
            Ok(store
                .iter()
                .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
                .cloned())
        }

        fn find_by_email(&self, email: &str) -> Result<Option<HashMap<String, Value>>> {
            let store = self.records.lock().unwrap();
            Ok(store
                .iter()
                .find(|r| r.get("email").and_then(|v| v.as_str()) == Some(email))
                .cloned())
        }

        fn insert(&self, data: &HashMap<String, Value>) -> Result<()> {
            self.records.lock().unwrap().push(data.clone());
            Ok(())
        }

        fn update(&self, id: &str, data: &HashMap<String, Value>) -> Result<()> {
            let mut store = self.records.lock().unwrap();
            if let Some(record) = store
                .iter_mut()
                .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
            {
                for (k, v) in data {
                    record.insert(k.clone(), v.clone());
                }
            }
            Ok(())
        }

        fn delete(&self, id: &str) -> Result<bool> {
            let mut store = self.records.lock().unwrap();
            let len_before = store.len();
            store.retain(|r| r.get("id").and_then(|v| v.as_str()) != Some(id));
            Ok(store.len() < len_before)
        }

        fn list_all(&self) -> Result<Vec<HashMap<String, Value>>> {
            Ok(self.records.lock().unwrap().clone())
        }

        fn count(&self) -> Result<u64> {
            Ok(self.records.lock().unwrap().len() as u64)
        }
    }

    // ── Test hasher ──────────────────────────────────────────────────────

    struct TestHasher;

    impl PasswordHasher for TestHasher {
        fn hash(&self, plain: &str) -> Result<String> {
            Ok(format!("hashed:{plain}"))
        }

        fn verify(&self, plain: &str, hash: &str) -> Result<bool> {
            Ok(hash == format!("hashed:{plain}"))
        }
    }

    // ── Tests ────────────────────────────────────────────────────────────

    fn make_service() -> SuperuserService<MockSuperuserRepo> {
        SuperuserService::new(MockSuperuserRepo::new(), TestHasher)
    }

    #[test]
    fn create_superuser_success() {
        let svc = make_service();
        let result = svc
            .create_superuser("admin@test.com", "password123")
            .unwrap();
        assert_eq!(
            result.get("email").unwrap().as_str().unwrap(),
            "admin@test.com"
        );
        assert!(result.get("id").is_some());
        assert!(result.get("tokenKey").is_some());
        // Password should be stripped from returned record.
        assert!(result.get("password").is_none());
    }

    #[test]
    fn create_superuser_normalizes_email() {
        let svc = make_service();
        let result = svc
            .create_superuser("  Admin@Test.COM  ", "password123")
            .unwrap();
        assert_eq!(
            result.get("email").unwrap().as_str().unwrap(),
            "admin@test.com"
        );
    }

    #[test]
    fn create_superuser_rejects_empty_email() {
        let svc = make_service();
        let err = svc.create_superuser("", "password123").unwrap_err();
        assert!(matches!(err, ZerobaseError::Validation { .. }));
    }

    #[test]
    fn create_superuser_rejects_short_password() {
        let svc = make_service();
        let err = svc.create_superuser("admin@test.com", "short").unwrap_err();
        assert!(matches!(err, ZerobaseError::Validation { .. }));
    }

    #[test]
    fn create_superuser_rejects_duplicate_email() {
        let svc = make_service();
        svc.create_superuser("admin@test.com", "password123")
            .unwrap();
        let err = svc
            .create_superuser("admin@test.com", "password456")
            .unwrap_err();
        assert!(matches!(err, ZerobaseError::Conflict { .. }));
    }

    #[test]
    fn authenticate_success() {
        let repo = MockSuperuserRepo::with_records(vec![{
            let mut r = HashMap::new();
            r.insert("id".to_string(), Value::String("su1".into()));
            r.insert("email".to_string(), Value::String("admin@test.com".into()));
            r.insert(
                "password".to_string(),
                Value::String("hashed:secret123".into()),
            );
            r.insert("tokenKey".to_string(), Value::String("tk1".into()));
            r
        }]);
        let svc = SuperuserService::new(repo, TestHasher);

        let result = svc.authenticate("admin@test.com", "secret123").unwrap();
        assert_eq!(result.get("id").unwrap().as_str().unwrap(), "su1");
        assert_eq!(result.get("tokenKey").unwrap().as_str().unwrap(), "tk1");
        // Password should be stripped.
        assert!(result.get("password").is_none());
    }

    #[test]
    fn authenticate_wrong_password() {
        let repo = MockSuperuserRepo::with_records(vec![{
            let mut r = HashMap::new();
            r.insert("id".to_string(), Value::String("su1".into()));
            r.insert("email".to_string(), Value::String("admin@test.com".into()));
            r.insert(
                "password".to_string(),
                Value::String("hashed:secret123".into()),
            );
            r.insert("tokenKey".to_string(), Value::String("tk1".into()));
            r
        }]);
        let svc = SuperuserService::new(repo, TestHasher);

        let err = svc.authenticate("admin@test.com", "wrong").unwrap_err();
        assert!(matches!(err, ZerobaseError::Auth { .. }));
    }

    #[test]
    fn authenticate_unknown_email() {
        let svc = make_service();
        let err = svc
            .authenticate("nobody@test.com", "password123")
            .unwrap_err();
        assert!(matches!(err, ZerobaseError::Auth { .. }));
    }

    #[test]
    fn authenticate_normalizes_email() {
        let repo = MockSuperuserRepo::with_records(vec![{
            let mut r = HashMap::new();
            r.insert("id".to_string(), Value::String("su1".into()));
            r.insert("email".to_string(), Value::String("admin@test.com".into()));
            r.insert(
                "password".to_string(),
                Value::String("hashed:secret123".into()),
            );
            r.insert("tokenKey".to_string(), Value::String("tk1".into()));
            r
        }]);
        let svc = SuperuserService::new(repo, TestHasher);

        let result = svc.authenticate("  Admin@Test.COM  ", "secret123");
        assert!(result.is_ok());
    }

    #[test]
    fn has_superusers_false_when_empty() {
        let svc = make_service();
        assert!(!svc.has_superusers().unwrap());
    }

    #[test]
    fn has_superusers_true_after_create() {
        let svc = make_service();
        svc.create_superuser("admin@test.com", "password123")
            .unwrap();
        assert!(svc.has_superusers().unwrap());
    }

    #[test]
    fn ensure_initial_superuser_creates_when_none_exist() {
        let svc = make_service();
        let result = svc
            .ensure_initial_superuser("admin@test.com", "password123")
            .unwrap();
        assert!(result.is_some());
        assert!(svc.has_superusers().unwrap());
    }

    #[test]
    fn ensure_initial_superuser_skips_when_exists() {
        let svc = make_service();
        svc.create_superuser("admin@test.com", "password123")
            .unwrap();
        let result = svc
            .ensure_initial_superuser("other@test.com", "password456")
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn list_superusers_strips_sensitive_fields() {
        let svc = make_service();
        svc.create_superuser("admin@test.com", "password123")
            .unwrap();
        let list = svc.list_superusers().unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].get("password").is_none());
        assert!(list[0].get("tokenKey").is_none());
    }

    #[test]
    fn delete_superuser_success() {
        let svc = make_service();
        let created = svc
            .create_superuser("admin@test.com", "password123")
            .unwrap();
        let id = created.get("id").unwrap().as_str().unwrap().to_string();
        svc.delete_superuser(&id).unwrap();
        assert!(!svc.has_superusers().unwrap());
    }

    #[test]
    fn delete_superuser_not_found() {
        let svc = make_service();
        let err = svc.delete_superuser("nonexistent").unwrap_err();
        assert!(matches!(err, ZerobaseError::NotFound { .. }));
    }

    #[test]
    fn get_superuser_strips_sensitive_fields() {
        let svc = make_service();
        let created = svc
            .create_superuser("admin@test.com", "password123")
            .unwrap();
        let id = created.get("id").unwrap().as_str().unwrap().to_string();
        let fetched = svc.get_superuser(&id).unwrap();
        assert!(fetched.get("password").is_none());
        assert!(fetched.get("tokenKey").is_none());
        assert_eq!(
            fetched.get("email").unwrap().as_str().unwrap(),
            "admin@test.com"
        );
    }
}
