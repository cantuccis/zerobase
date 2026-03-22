//! Record CRUD service.
//!
//! [`RecordService`] is the primary interface for creating, reading, updating,
//! and deleting records within collections. It validates record data against
//! the collection schema, manages system fields (id, created, updated), and
//! delegates persistence to a [`RecordRepository`] implementor.
//!
//! # Design
//!
//! - **Input validation** happens at this layer (via [`RecordValidator`]).
//! - **Auto-date injection** for `created`/`updated` timestamps.
//! - **ID generation** for new records.
//! - The service is generic over both `R: RecordRepository` and
//!   `S: SchemaLookup` for easy testing with mocks.

use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::auth::PasswordHasher;
use crate::error::{Result, ZerobaseError};
use crate::hooks::{HookContext, HookPhase, HookRegistry, RecordOperation};
use crate::id::generate_id;
use crate::schema::{
    Collection, CollectionType, Field, FieldType, OnDeleteAction, OperationContext,
    RecordValidator, RelationOptions,
};

// ── Repository trait ──────────────────────────────────────────────────────────

/// Record-level persistence contract used by the service.
///
/// Defined in core so the service doesn't depend on `zerobase-db` directly.
/// The DB crate implements this trait on `Database`.
pub trait RecordRepository: Send + Sync {
    /// Retrieve a single record by its ID.
    fn find_one(
        &self,
        collection: &str,
        id: &str,
    ) -> std::result::Result<HashMap<String, Value>, RecordRepoError>;

    /// List records with pagination.
    fn find_many(
        &self,
        collection: &str,
        query: &RecordQuery,
    ) -> std::result::Result<RecordList, RecordRepoError>;

    /// Insert a new record. The data already includes id, created, updated.
    fn insert(
        &self,
        collection: &str,
        data: &HashMap<String, Value>,
    ) -> std::result::Result<(), RecordRepoError>;

    /// Update an existing record. Returns true if a row was modified.
    fn update(
        &self,
        collection: &str,
        id: &str,
        data: &HashMap<String, Value>,
    ) -> std::result::Result<bool, RecordRepoError>;

    /// Delete a record by its ID. Returns true if a row was deleted.
    fn delete(&self, collection: &str, id: &str) -> std::result::Result<bool, RecordRepoError>;

    /// Count records matching an optional filter.
    fn count(
        &self,
        collection: &str,
        filter: Option<&str>,
    ) -> std::result::Result<u64, RecordRepoError>;

    /// Check whether a record exists in the given collection.
    fn record_exists(
        &self,
        collection: &str,
        id: &str,
    ) -> std::result::Result<bool, RecordRepoError> {
        match self.find_one(collection, id) {
            Ok(_) => Ok(true),
            Err(RecordRepoError::NotFound { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Find records in a collection where a specific field contains the given value.
    ///
    /// Used for cascade operations: find all records that reference a given record ID.
    fn find_referencing_records(
        &self,
        collection: &str,
        field_name: &str,
        referenced_id: &str,
    ) -> std::result::Result<Vec<HashMap<String, Value>>, RecordRepoError>;

    /// Find referencing records with a maximum result limit.
    ///
    /// Used by back-relation expansion to cap the number of expanded records.
    /// The default implementation delegates to [`find_referencing_records`] and
    /// truncates the result. Database implementations should override this to
    /// push the LIMIT into the SQL query for efficiency.
    fn find_referencing_records_limited(
        &self,
        collection: &str,
        field_name: &str,
        referenced_id: &str,
        limit: usize,
    ) -> std::result::Result<Vec<HashMap<String, Value>>, RecordRepoError> {
        let mut results = self.find_referencing_records(collection, field_name, referenced_id)?;
        results.truncate(limit);
        Ok(results)
    }
}

/// Default number of items per page.
pub const DEFAULT_PER_PAGE: u32 = 30;

/// Maximum number of items per page.
pub const MAX_PER_PAGE: u32 = 500;

/// Query parameters for listing records.
#[derive(Debug, Clone, Default)]
pub struct RecordQuery {
    /// Filter expression (PocketBase-style).
    pub filter: Option<String>,
    /// Sort instructions: `(column, direction)` pairs.
    pub sort: Vec<(String, SortDirection)>,
    /// Page number (1-based). Defaults to 1.
    pub page: u32,
    /// Items per page. Defaults to 30, max 500.
    pub per_page: u32,
    /// Optional field selection — only return these fields in response.
    /// The `id` field is always included regardless of this list.
    pub fields: Option<Vec<String>>,
    /// Full-text search query. When present, results are filtered and ranked
    /// by relevance against the collection's searchable fields (FTS5).
    pub search: Option<String>,
}

/// Sort direction for query ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl std::fmt::Display for SortDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Asc => write!(f, "ASC"),
            Self::Desc => write!(f, "DESC"),
        }
    }
}

/// Parse a PocketBase-style sort parameter into sort instructions.
///
/// The sort parameter is a comma-separated list of field names.
/// Prefix a field name with `-` for descending order; ascending is the default.
///
/// # Examples
///
/// ```
/// use zerobase_core::services::record_service::{parse_sort, SortDirection};
///
/// let sort = parse_sort("-created,title").unwrap();
/// assert_eq!(sort, vec![
///     ("created".to_string(), SortDirection::Desc),
///     ("title".to_string(), SortDirection::Asc),
/// ]);
/// ```
///
/// # Errors
///
/// Returns a validation error if the sort string is malformed (e.g., contains
/// empty segments or field names with invalid characters).
pub fn parse_sort(sort_str: &str) -> Result<Vec<(String, SortDirection)>> {
    let trimmed = sort_str.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let mut result = Vec::new();

    for segment in trimmed.split(',') {
        let segment = segment.trim();
        if segment.is_empty() {
            return Err(ZerobaseError::validation(
                "sort parameter contains an empty field",
            ));
        }

        let (field_name, direction) = if let Some(name) = segment.strip_prefix('-') {
            (name.trim(), SortDirection::Desc)
        } else if let Some(name) = segment.strip_prefix('+') {
            (name.trim(), SortDirection::Asc)
        } else {
            (segment, SortDirection::Asc)
        };

        if field_name.is_empty() {
            return Err(ZerobaseError::validation(
                "sort parameter contains an empty field name after prefix",
            ));
        }

        // Validate field name characters (alphanumeric + underscore).
        if !field_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(ZerobaseError::validation(format!(
                "invalid sort field name: '{field_name}'"
            )));
        }

        result.push((field_name.to_string(), direction));
    }

    Ok(result)
}

/// Validate that all sort fields exist in the given collection schema.
///
/// Returns a validation error listing the first unknown field if any sort field
/// is not a system field or user-defined field on the collection.
pub fn validate_sort_fields(
    sort: &[(String, SortDirection)],
    collection: &Collection,
) -> Result<()> {
    for (field_name, _) in sort {
        if !collection.has_field(field_name) {
            return Err(ZerobaseError::validation(format!(
                "unknown sort field '{}' in collection '{}'",
                field_name, collection.name
            )));
        }
    }
    Ok(())
}

/// Parse a comma-separated `fields` query parameter into a list of field names.
///
/// Each field name is trimmed and validated for safe characters (alphanumeric + underscore).
/// The `id` field is always included in the result even if not explicitly listed.
///
/// # Examples
///
/// ```
/// use zerobase_core::services::record_service::parse_fields;
///
/// let fields = parse_fields("title,views").unwrap();
/// assert!(fields.contains(&"id".to_string()));
/// assert!(fields.contains(&"title".to_string()));
/// assert!(fields.contains(&"views".to_string()));
/// ```
///
/// # Errors
///
/// Returns a validation error if any field name contains invalid characters.
pub fn parse_fields(fields_str: &str) -> Result<Vec<String>> {
    let trimmed = fields_str.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let mut result = Vec::new();
    // Always include `id`.
    result.push("id".to_string());

    for segment in trimmed.split(',') {
        let field_name = segment.trim();
        if field_name.is_empty() {
            continue; // Skip empty segments gracefully.
        }

        // Validate field name characters.
        if !field_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(ZerobaseError::validation(format!(
                "invalid field name in fields parameter: '{field_name}'"
            )));
        }

        if !result.contains(&field_name.to_string()) {
            result.push(field_name.to_string());
        }
    }

    Ok(result)
}

/// Validate that all requested fields exist in the given collection schema.
///
/// Unknown fields are silently ignored (graceful handling) rather than
/// producing an error, matching PocketBase behaviour.
/// Returns only the fields that exist in the collection.
pub fn validate_and_filter_fields(fields: &[String], collection: &Collection) -> Vec<String> {
    fields
        .iter()
        .filter(|f| collection.has_field(f))
        .cloned()
        .collect()
}

/// Project a record to only include the specified fields.
///
/// The `id` field is always preserved. Fields not present in the record
/// are silently skipped.
pub fn project_fields(
    record: &HashMap<String, Value>,
    fields: &[String],
) -> HashMap<String, Value> {
    let mut projected = HashMap::new();
    for field_name in fields {
        if let Some(value) = record.get(field_name) {
            projected.insert(field_name.clone(), value.clone());
        }
    }
    // Always include `id`.
    if let Some(id_val) = record.get("id") {
        projected
            .entry("id".to_string())
            .or_insert_with(|| id_val.clone());
    }
    projected
}

/// Project all records in a [`RecordList`] to only include the specified fields.
pub fn project_record_list(mut record_list: RecordList, fields: &[String]) -> RecordList {
    record_list.items = record_list
        .items
        .iter()
        .map(|record| project_fields(record, fields))
        .collect();
    record_list
}

/// A paginated list of records.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordList {
    /// The records for the current page.
    pub items: Vec<HashMap<String, Value>>,
    /// Total number of records matching the filter.
    pub total_items: u64,
    /// Current page number.
    pub page: u32,
    /// Items per page.
    pub per_page: u32,
    /// Total number of pages.
    pub total_pages: u32,
}

/// Errors that a record repository can produce.
#[derive(Debug, thiserror::Error)]
pub enum RecordRepoError {
    #[error("{resource_type} not found")]
    NotFound {
        resource_type: String,
        resource_id: Option<String>,
    },
    #[error("conflict: {message}")]
    Conflict { message: String },
    #[error("database error: {message}")]
    Database { message: String },
}

impl From<RecordRepoError> for ZerobaseError {
    fn from(err: RecordRepoError) -> Self {
        match err {
            RecordRepoError::NotFound {
                resource_type,
                resource_id,
            } => match resource_id {
                Some(id) => ZerobaseError::not_found_with_id(resource_type, id),
                None => ZerobaseError::not_found(resource_type),
            },
            RecordRepoError::Conflict { message } => ZerobaseError::conflict(message),
            RecordRepoError::Database { message } => ZerobaseError::database(message),
        }
    }
}

/// Lookup contract for retrieving collection schemas.
///
/// The record service needs to know a collection's fields in order to validate
/// records. This trait abstracts that lookup so the service doesn't depend on
/// the full `CollectionService` or `SchemaRepository`.
pub trait SchemaLookup: Send + Sync {
    /// Get the collection definition by name.
    fn get_collection(&self, name: &str) -> Result<Collection>;

    /// Get the collection definition by its unique ID.
    ///
    /// Used by the auth middleware to resolve the collection from a JWT's
    /// `collectionId` claim. The default implementation is a linear scan
    /// over all collections — implementors may override with an indexed lookup.
    fn get_collection_by_id(&self, id: &str) -> Result<Collection> {
        let _ = id;
        Err(ZerobaseError::not_found_with_id("Collection", id))
    }

    /// List all collections. Used for cascade operations that need to scan
    /// all collections for relation fields pointing to a given target.
    ///
    /// The default implementation returns an empty list. Implementors should
    /// override this to return all known collections.
    fn list_all_collections(&self) -> Result<Vec<Collection>> {
        Ok(Vec::new())
    }
}

impl<T: SchemaLookup> SchemaLookup for std::sync::Arc<T> {
    fn get_collection(&self, name: &str) -> Result<Collection> {
        (**self).get_collection(name)
    }

    fn get_collection_by_id(&self, id: &str) -> Result<Collection> {
        (**self).get_collection_by_id(id)
    }

    fn list_all_collections(&self) -> Result<Vec<Collection>> {
        (**self).list_all_collections()
    }
}

// ── RecordService ─────────────────────────────────────────────────────────────

/// Service for CRUD operations on records.
///
/// Generic over `R: RecordRepository` and `S: SchemaLookup` so tests can
/// inject mocks. An optional [`PasswordHasher`] is used to hash passwords
/// when creating or updating records in auth collections.
///
/// An optional [`HookRegistry`] enables before/after hooks on record operations.
/// When present, hooks are invoked around create, update, and delete operations.
pub struct RecordService<R: RecordRepository, S: SchemaLookup> {
    repo: R,
    schema: S,
    password_hasher: Option<Box<dyn PasswordHasher>>,
    hooks: Option<HookRegistry>,
}

impl<R: RecordRepository, S: SchemaLookup> RecordService<R, S> {
    /// Create a new service wrapping the given repository and schema lookup.
    pub fn new(repo: R, schema: S) -> Self {
        Self {
            repo,
            schema,
            password_hasher: None,
            hooks: None,
        }
    }

    /// Create a new service with a password hasher for auth collection support.
    pub fn with_password_hasher(repo: R, schema: S, hasher: impl PasswordHasher + 'static) -> Self {
        Self {
            repo,
            schema,
            password_hasher: Some(Box::new(hasher)),
            hooks: None,
        }
    }

    /// Attach a hook registry to this service.
    ///
    /// When set, hooks are invoked around create, update, and delete operations.
    /// Before-hooks can modify data or abort operations; after-hooks perform
    /// side effects.
    pub fn with_hooks(mut self, hooks: HookRegistry) -> Self {
        self.hooks = Some(hooks);
        self
    }

    /// Set the hook registry on this service (mutable reference variant).
    pub fn set_hooks(&mut self, hooks: HookRegistry) {
        self.hooks = Some(hooks);
    }

    /// Access the hook registry, if any.
    pub fn hooks(&self) -> Option<&HookRegistry> {
        self.hooks.as_ref()
    }

    /// Access the underlying record repository.
    ///
    /// Used by the expand layer to fetch related records.
    pub fn repo(&self) -> &R {
        &self.repo
    }

    /// Access the underlying schema lookup.
    ///
    /// Used by the expand layer to resolve relation target collections.
    pub fn schema(&self) -> &S {
        &self.schema
    }

    /// Look up a collection by name, delegating to the [`SchemaLookup`] backend.
    pub fn get_collection(&self, name: &str) -> Result<Collection> {
        self.schema.get_collection(name)
    }

    /// Create a new record in the given collection.
    ///
    /// 1. Looks up the collection schema.
    /// 2. Generates a unique ID.
    /// 3. For auth collections: hashes password, generates tokenKey, sets defaults.
    /// 4. Injects `created` and `updated` timestamps via AutoDate fields.
    /// 5. Validates and sanitizes all field values.
    /// 6. Persists the record.
    /// 7. Returns the full record (with password stripped for auth collections).
    pub fn create_record(
        &self,
        collection_name: &str,
        data: Value,
    ) -> Result<HashMap<String, Value>> {
        let collection = self.schema.get_collection(collection_name)?;

        if collection.collection_type == CollectionType::View {
            return Err(ZerobaseError::validation(
                "view collections are read-only; create is not allowed",
            ));
        }

        let is_auth = collection.collection_type == CollectionType::Auth;

        // Build the full field list including system fields.
        let all_fields = all_fields_for_validation(&collection);

        // Ensure input is an object.
        let mut obj = data
            .as_object()
            .cloned()
            .ok_or_else(|| ZerobaseError::validation("record data must be a JSON object"))?;

        // Generate and inject the record ID.
        let record_id = generate_id();
        obj.insert("id".to_string(), Value::String(record_id.clone()));

        // Auth collection: process auth system fields before validation.
        if is_auth {
            self.prepare_auth_fields_for_create(&mut obj)?;
        }

        // Build a mutable Value for the validator.
        let record_value = Value::Object(obj);

        // Validate, prepare (sanitize), and inject auto-dates.
        let validator = RecordValidator::new(&all_fields);
        let prepared =
            validator.validate_and_prepare_with_context(&record_value, OperationContext::Create)?;

        // Validate that all relation references point to existing records.
        self.validate_relation_references(&collection, &prepared)?;

        // Convert back to HashMap for the repository.
        let mut record_map = value_to_record_data(&prepared)?;

        // Run before-create hooks (may modify record_map or abort).
        if let Some(hooks) = &self.hooks {
            let mut hook_ctx = HookContext::new(
                RecordOperation::Create,
                HookPhase::Before,
                collection_name,
                &record_id,
                record_map.clone(),
            );
            hooks.run_before(&mut hook_ctx)?;
            record_map = hook_ctx.record;
        }

        // Persist.
        self.repo.insert(collection_name, &record_map)?;

        // Run after-create hooks (side effects only, errors are logged).
        if let Some(hooks) = &self.hooks {
            let hook_ctx = HookContext::new(
                RecordOperation::Create,
                HookPhase::After,
                collection_name,
                &record_id,
                record_map.clone(),
            );
            let _errors = hooks.run_after(&hook_ctx);
        }

        // Strip password from response for auth collections.
        if is_auth {
            strip_password(&mut record_map);
        }

        Ok(record_map)
    }

    /// Get a single record by ID.
    ///
    /// For auth collections, the password field is stripped from the response.
    pub fn get_record(&self, collection_name: &str, id: &str) -> Result<HashMap<String, Value>> {
        let collection = self.schema.get_collection(collection_name)?;
        let mut record = self.repo.find_one(collection_name, id)?;

        if collection.collection_type == CollectionType::Auth {
            strip_password(&mut record);
        }

        Ok(record)
    }

    /// List records with pagination, filtering, sorting, and optional field
    /// selection.
    ///
    /// Sort fields are validated against the collection schema. Unknown fields
    /// result in a 400 validation error. Requested `fields` that don't exist
    /// in the schema are silently dropped.
    pub fn list_records(&self, collection_name: &str, query: &RecordQuery) -> Result<RecordList> {
        // Verify collection exists.
        let collection = self.schema.get_collection(collection_name)?;

        // Validate sort fields against the schema.
        validate_sort_fields(&query.sort, &collection)?;

        // Validate and filter requested fields against the schema.
        let validated_fields = query
            .fields
            .as_ref()
            .map(|f| validate_and_filter_fields(f, &collection));

        // Validate search: collection must have searchable fields.
        if let Some(ref search) = query.search {
            if !search.trim().is_empty() && !collection.has_searchable_fields() {
                return Err(ZerobaseError::validation(format!(
                    "collection '{}' has no searchable fields; mark text fields as searchable to enable search",
                    collection.name,
                )));
            }
        }

        // Normalize pagination defaults.
        let normalized = RecordQuery {
            filter: query.filter.clone(),
            sort: query.sort.clone(),
            page: if query.page == 0 { 1 } else { query.page },
            per_page: if query.per_page == 0 {
                DEFAULT_PER_PAGE
            } else {
                query.per_page.min(MAX_PER_PAGE)
            },
            fields: None, // DB layer fetches all; we project after.
            search: query.search.clone(),
        };

        let mut record_list = self.repo.find_many(collection_name, &normalized)?;

        // Strip passwords from auth collection responses.
        if collection.collection_type == CollectionType::Auth {
            for record in &mut record_list.items {
                strip_password(record);
            }
        }

        // Apply field projection if requested.
        match validated_fields {
            Some(fields) if !fields.is_empty() => Ok(project_record_list(record_list, &fields)),
            _ => Ok(record_list),
        }
    }

    /// Get a single record by ID, optionally projecting to specific fields.
    ///
    /// For auth collections, the password field is stripped from the response.
    pub fn get_record_with_fields(
        &self,
        collection_name: &str,
        id: &str,
        fields: Option<&[String]>,
    ) -> Result<HashMap<String, Value>> {
        let collection = self.schema.get_collection(collection_name)?;
        let mut record = self.repo.find_one(collection_name, id)?;

        if collection.collection_type == CollectionType::Auth {
            strip_password(&mut record);
        }

        match fields {
            Some(f) => {
                let validated = validate_and_filter_fields(f, &collection);
                if validated.is_empty() {
                    Ok(record)
                } else {
                    Ok(project_fields(&record, &validated))
                }
            }
            None => Ok(record),
        }
    }

    /// Update an existing record.
    ///
    /// 1. Looks up the collection schema.
    /// 2. Verifies the record exists.
    /// 3. For auth collections: hashes password if changed, regenerates tokenKey.
    /// 4. Validates the provided fields (partial validation for PATCH semantics).
    /// 5. Injects the `updated` timestamp.
    /// 6. Merges with existing data and persists.
    /// 7. Returns the record with password stripped for auth collections.
    pub fn update_record(
        &self,
        collection_name: &str,
        id: &str,
        data: Value,
    ) -> Result<HashMap<String, Value>> {
        let collection = self.schema.get_collection(collection_name)?;

        if collection.collection_type == CollectionType::View {
            return Err(ZerobaseError::validation(
                "view collections are read-only; update is not allowed",
            ));
        }

        let is_auth = collection.collection_type == CollectionType::Auth;

        // Verify the record exists and get current data.
        let existing = self.repo.find_one(collection_name, id)?;

        let all_fields = all_fields_for_validation(&collection);

        // Ensure input is an object.
        let update_obj = data
            .as_object()
            .ok_or_else(|| ZerobaseError::validation("record data must be a JSON object"))?;

        // Prevent changing the 'id' field.
        if update_obj.contains_key("id") {
            return Err(ZerobaseError::validation("cannot modify the 'id' field"));
        }

        // Process relation modifiers (field+ / field-) before merging.
        let resolved_modifiers = apply_relation_modifiers(update_obj, &existing, &collection)?;

        // Merge: start from existing, overlay with updates.
        // First apply resolved modifier values, then overlay plain keys.
        let mut merged = existing.clone();
        for (key, value) in &resolved_modifiers {
            merged.insert(key.clone(), value.clone());
        }
        for (key, value) in update_obj {
            // Skip modifier keys — they were already resolved above.
            if key.ends_with('+') || key.ends_with('-') {
                continue;
            }
            merged.insert(key.clone(), value.clone());
        }

        // Auth collection: hash password if it was updated.
        if is_auth {
            self.prepare_auth_fields_for_update(&mut merged, update_obj)?;
        }

        // Convert to Value for validation.
        let merged_value = record_data_to_value(&merged);

        // Validate, prepare, and inject auto-dates for update context.
        let validator = RecordValidator::new(&all_fields);
        let prepared =
            validator.validate_and_prepare_with_context(&merged_value, OperationContext::Update)?;

        // Validate that all relation references point to existing records.
        self.validate_relation_references(&collection, &prepared)?;

        let mut record_map = value_to_record_data(&prepared)?;

        // Run before-update hooks (may modify record_map or abort).
        if let Some(hooks) = &self.hooks {
            let mut hook_ctx = HookContext::new(
                RecordOperation::Update,
                HookPhase::Before,
                collection_name,
                id,
                record_map.clone(),
            );
            hooks.run_before(&mut hook_ctx)?;
            record_map = hook_ctx.record;
        }

        // Persist — only the updated fields + auto-dates.
        let updated = self.repo.update(collection_name, id, &record_map)?;
        if !updated {
            return Err(ZerobaseError::not_found_with_id("Record", id));
        }

        // Run after-update hooks (side effects only, errors are logged).
        if let Some(hooks) = &self.hooks {
            let hook_ctx = HookContext::new(
                RecordOperation::Update,
                HookPhase::After,
                collection_name,
                id,
                record_map.clone(),
            );
            let _errors = hooks.run_after(&hook_ctx);
        }

        // Strip password from response for auth collections.
        if is_auth {
            strip_password(&mut record_map);
        }

        Ok(record_map)
    }

    /// Delete a record by ID.
    ///
    /// Before deleting, processes all relation fields across all collections
    /// that reference this record, applying the appropriate cascade behavior:
    ///
    /// - **Cascade**: Delete all referencing records.
    /// - **SetNull**: Set the relation field to null (or remove the ID from arrays).
    /// - **Restrict**: Prevent deletion if any references exist.
    /// - **NoAction**: Leave dangling references (default).
    pub fn delete_record(&self, collection_name: &str, id: &str) -> Result<()> {
        // Verify collection exists.
        let collection = self.schema.get_collection(collection_name)?;

        if collection.collection_type == CollectionType::View {
            return Err(ZerobaseError::validation(
                "view collections are read-only; delete is not allowed",
            ));
        }

        // Verify the record exists before processing cascades.
        let record_data = match self.repo.find_one(collection_name, id) {
            Ok(data) => data,
            Err(_) => return Err(ZerobaseError::not_found_with_id("Record", id)),
        };

        // Run before-delete hooks (may abort the delete).
        if let Some(hooks) = &self.hooks {
            let mut hook_ctx = HookContext::new(
                RecordOperation::Delete,
                HookPhase::Before,
                collection_name,
                id,
                record_data.clone(),
            );
            hooks.run_before(&mut hook_ctx)?;
        }

        // Process cascade behaviors for all collections that reference this one.
        self.process_on_delete_actions(collection_name, &collection, id)?;

        let deleted = self.repo.delete(collection_name, id)?;
        if !deleted {
            return Err(ZerobaseError::not_found_with_id("Record", id));
        }

        // Run after-delete hooks (side effects only, errors are logged).
        if let Some(hooks) = &self.hooks {
            let hook_ctx = HookContext::new(
                RecordOperation::Delete,
                HookPhase::After,
                collection_name,
                id,
                record_data,
            );
            let _errors = hooks.run_after(&hook_ctx);
        }

        Ok(())
    }

    /// Count records in a collection, optionally applying a filter.
    ///
    /// Verifies the collection exists before counting. The filter uses the
    /// same PocketBase-style expression syntax as `list_records`.
    pub fn count_records(&self, collection_name: &str, filter: Option<&str>) -> Result<u64> {
        // Verify collection exists.
        let _collection = self.schema.get_collection(collection_name)?;
        Ok(self.repo.count(collection_name, filter)?)
    }

    // ── Auth collection helpers ──────────────────────────────────────────

    /// Prepare auth system fields for a new record in an auth collection.
    ///
    /// - Hash the password (required for create).
    /// - Generate a random tokenKey for token invalidation.
    /// - Set defaults for emailVisibility and verified if not provided.
    fn prepare_auth_fields_for_create(&self, obj: &mut Map<String, Value>) -> Result<()> {
        // Hash password if provided.
        if let Some(password_val) = obj.get("password") {
            if let Some(plain) = password_val.as_str() {
                if !plain.is_empty() {
                    let hashed = self.hash_password(plain)?;
                    obj.insert("password".to_string(), Value::String(hashed));
                }
            }
        }

        // Generate tokenKey if not provided.
        if !obj.contains_key("tokenKey") || obj["tokenKey"].as_str().map_or(true, |s| s.is_empty())
        {
            obj.insert("tokenKey".to_string(), Value::String(generate_token_key()));
        }

        // Default emailVisibility to false if not provided.
        if !obj.contains_key("emailVisibility") {
            obj.insert("emailVisibility".to_string(), Value::Bool(false));
        }

        // Default verified to false if not provided.
        if !obj.contains_key("verified") {
            obj.insert("verified".to_string(), Value::Bool(false));
        }

        // Default email to empty string if not provided.
        if !obj.contains_key("email") {
            obj.insert("email".to_string(), Value::String(String::new()));
        }

        Ok(())
    }

    /// Prepare auth system fields for an update to a record in an auth collection.
    ///
    /// - Hash the password if it was changed (i.e., present in the update payload).
    /// - Regenerate tokenKey when password changes (invalidates existing tokens).
    fn prepare_auth_fields_for_update(
        &self,
        merged: &mut HashMap<String, Value>,
        update_obj: &Map<String, Value>,
    ) -> Result<()> {
        // Hash password if it was included in the update.
        if let Some(password_val) = update_obj.get("password") {
            if let Some(plain) = password_val.as_str() {
                if !plain.is_empty() {
                    let hashed = self.hash_password(plain)?;
                    merged.insert("password".to_string(), Value::String(hashed));

                    // Regenerate tokenKey to invalidate existing tokens.
                    merged.insert("tokenKey".to_string(), Value::String(generate_token_key()));
                }
            }
        }

        Ok(())
    }

    /// Hash a password using the configured hasher.
    fn hash_password(&self, plain: &str) -> Result<String> {
        match &self.password_hasher {
            Some(hasher) => hasher.hash(plain),
            None => Err(ZerobaseError::internal(
                "password hasher not configured; cannot create records in auth collections",
            )),
        }
    }

    /// Authenticate a user by identity (e.g. email) and password.
    ///
    /// 1. Verifies the collection is an auth collection with email auth enabled.
    /// 2. Looks up the user record by identity field using the filter engine.
    /// 3. Verifies the plaintext password against the stored hash.
    /// 4. Returns the user record (password stripped) on success.
    ///
    /// The returned record retains `tokenKey` so the caller can generate a JWT.
    /// The caller should remove `tokenKey` before sending the response to the client.
    pub fn authenticate_with_password(
        &self,
        collection_name: &str,
        identity: &str,
        password: &str,
    ) -> Result<HashMap<String, Value>> {
        let collection = self.schema.get_collection(collection_name)?;

        // Must be an auth collection.
        if collection.collection_type != CollectionType::Auth {
            return Err(ZerobaseError::validation(format!(
                "collection '{}' is not an auth collection",
                collection_name
            )));
        }

        // Check that email auth is enabled.
        let auth_options = collection
            .auth_options
            .as_ref()
            .cloned()
            .unwrap_or_default();
        if !auth_options.allow_email_auth {
            return Err(ZerobaseError::validation(
                "email/password authentication is not enabled for this collection",
            ));
        }

        // Try each identity field until we find a match.
        let identity_fields = &auth_options.identity_fields;
        let mut user_record = None;

        for field_name in identity_fields {
            // Build a filter to find the user by this identity field.
            let filter = format!("{} = {:?}", field_name, identity);
            let query = RecordQuery {
                filter: Some(filter),
                page: 1,
                per_page: 1,
                ..Default::default()
            };

            match self.repo.find_many(collection_name, &query) {
                Ok(list) if !list.items.is_empty() => {
                    user_record = Some(list.items.into_iter().next().unwrap());
                    break;
                }
                Ok(_) => continue,
                Err(e) => return Err(e.into()),
            }
        }

        let record = user_record.ok_or_else(|| ZerobaseError::auth("Failed to authenticate."))?;

        // Verify password.
        let stored_hash = record
            .get("password")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if stored_hash.is_empty() {
            return Err(ZerobaseError::auth("Failed to authenticate."));
        }

        let hasher = self
            .password_hasher
            .as_ref()
            .ok_or_else(|| ZerobaseError::internal("password hasher not configured"))?;

        let valid = hasher.verify(password, stored_hash)?;
        if !valid {
            return Err(ZerobaseError::auth("Failed to authenticate."));
        }

        // Strip password but keep tokenKey for JWT generation.
        let mut result = record;
        strip_password(&mut result);
        Ok(result)
    }

    // ── Relation helpers ─────────────────────────────────────────────────

    /// Validate that all relation field values in the record point to existing
    /// records in the referenced collections.
    fn validate_relation_references(&self, collection: &Collection, data: &Value) -> Result<()> {
        let obj = match data.as_object() {
            Some(obj) => obj,
            None => return Ok(()),
        };

        let mut field_errors: HashMap<String, String> = HashMap::new();

        for field in &collection.fields {
            if let FieldType::Relation(ref opts) = field.field_type {
                if let Some(value) = obj.get(&field.name) {
                    if value.is_null() {
                        continue;
                    }
                    let ids = RelationOptions::extract_ids(value);
                    for ref_id in &ids {
                        match self.repo.record_exists(&opts.collection_id, ref_id) {
                            Ok(true) => {}
                            Ok(false) => {
                                field_errors.insert(
                                    field.name.clone(),
                                    format!(
                                        "referenced record '{}' not found in collection '{}'",
                                        ref_id, opts.collection_id
                                    ),
                                );
                                break; // Report first missing ID per field.
                            }
                            Err(e) => {
                                // If the target collection doesn't exist,
                                // report as a validation error.
                                field_errors.insert(
                                    field.name.clone(),
                                    format!("cannot validate reference: {}", e),
                                );
                                break;
                            }
                        }
                    }
                }
            }
        }

        if field_errors.is_empty() {
            Ok(())
        } else {
            Err(ZerobaseError::Validation {
                message: "one or more relation references are invalid".to_string(),
                field_errors,
            })
        }
    }

    /// Process on-delete cascade actions for all collections that have relation
    /// fields pointing to the given collection.
    ///
    /// This is called *before* the actual delete. It scans all collections for
    /// relation fields that target `collection_name` and applies the appropriate
    /// action based on `on_delete`.
    fn process_on_delete_actions(
        &self,
        collection_name: &str,
        _collection: &Collection,
        record_id: &str,
    ) -> Result<()> {
        // Find all collections that have relation fields pointing to this collection.
        // We need to iterate all known collections via SchemaLookup. Since SchemaLookup
        // only provides get_collection (by name), we use a broader approach: we look at
        // the relation fields in all collections we know about.
        //
        // For now, we use the trait method that returns a single collection. A full
        // implementation would iterate all collections. We handle this by requiring
        // the caller to provide the referencing fields via a helper.
        let referencing = self.find_referencing_fields(collection_name)?;

        for (ref_collection_name, ref_field_name, on_delete) in &referencing {
            match on_delete {
                OnDeleteAction::NoAction => {
                    // Do nothing — leave dangling references.
                }
                OnDeleteAction::Restrict => {
                    // Check if any records reference this record.
                    let refs = self.repo.find_referencing_records(
                        ref_collection_name,
                        ref_field_name,
                        record_id,
                    )?;
                    if !refs.is_empty() {
                        return Err(ZerobaseError::conflict(format!(
                            "cannot delete record '{}': referenced by {} record(s) in collection '{}' (field '{}')",
                            record_id,
                            refs.len(),
                            ref_collection_name,
                            ref_field_name,
                        )));
                    }
                }
                OnDeleteAction::Cascade => {
                    // Delete all referencing records.
                    let refs = self.repo.find_referencing_records(
                        ref_collection_name,
                        ref_field_name,
                        record_id,
                    )?;
                    for ref_record in &refs {
                        if let Some(ref_id) = ref_record.get("id").and_then(|v| v.as_str()) {
                            // Recursive delete to handle cascades of cascades.
                            let _ = self.delete_record(ref_collection_name, ref_id);
                        }
                    }
                }
                OnDeleteAction::SetNull => {
                    // Set the relation field to null (single) or remove the ID from arrays.
                    let refs = self.repo.find_referencing_records(
                        ref_collection_name,
                        ref_field_name,
                        record_id,
                    )?;
                    for ref_record in &refs {
                        if let Some(ref_id) = ref_record.get("id").and_then(|v| v.as_str()) {
                            let current_value = ref_record.get(ref_field_name);
                            let new_value = match current_value {
                                Some(Value::Array(arr)) => {
                                    // Multi-relation: remove the specific ID.
                                    let filtered: Vec<Value> = arr
                                        .iter()
                                        .filter(|v| v.as_str() != Some(record_id))
                                        .cloned()
                                        .collect();
                                    Value::Array(filtered)
                                }
                                _ => Value::Null,
                            };
                            // Build full record with the field updated (repo.update
                            // replaces the entire row).
                            let mut update_data = ref_record.clone();
                            update_data.insert(ref_field_name.clone(), new_value);
                            let _ = self.repo.update(ref_collection_name, ref_id, &update_data);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Find all (collection_name, field_name, on_delete_action) triples for
    /// relation fields that reference the given collection.
    fn find_referencing_fields(
        &self,
        target_collection: &str,
    ) -> Result<Vec<(String, String, OnDeleteAction)>> {
        // Use SchemaLookup to scan collections. We rely on the list_all_collections
        // method if available, or fall back to known collection names.
        // For now, we expose this through the trait's capabilities.
        let collections = self.schema.list_all_collections()?;
        let mut result = Vec::new();

        for coll in &collections {
            for field in &coll.fields {
                if let FieldType::Relation(ref opts) = field.field_type {
                    if opts.collection_id == target_collection {
                        result.push((
                            coll.name.clone(),
                            field.name.clone(),
                            opts.effective_on_delete(),
                        ));
                    }
                }
            }
        }

        Ok(result)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build the complete field list for validation, including system fields
/// (id, created, updated) that are implicit in every collection.
///
/// For auth collections, also includes: email, emailVisibility, verified,
/// password, and tokenKey.
fn all_fields_for_validation(collection: &Collection) -> Vec<Field> {
    use crate::schema::{AutoDateOptions, BoolOptions, EmailOptions, FieldType, TextOptions};

    let mut fields = Vec::new();

    // System field: id (text, required)
    fields.push(
        Field::new(
            "id",
            FieldType::Text(TextOptions {
                min_length: 15,
                max_length: 15,
                pattern: None,
                searchable: false,
            }),
        )
        .required(true),
    );

    // Auth collection system fields.
    if collection.collection_type == CollectionType::Auth {
        // email — unique, required (enforced at DB level via UNIQUE constraint)
        fields.push(
            Field::new("email", FieldType::Email(EmailOptions::default())).required(false), // Allow empty default; DB has UNIQUE constraint
        );

        // emailVisibility — boolean, defaults to false
        fields.push(Field::new(
            "emailVisibility",
            FieldType::Bool(BoolOptions::default()),
        ));

        // verified — boolean, defaults to false
        fields.push(Field::new(
            "verified",
            FieldType::Bool(BoolOptions::default()),
        ));

        // password — stored as hashed text (already hashed before reaching validator)
        fields.push(Field::new(
            "password",
            FieldType::Text(TextOptions {
                min_length: 0,
                max_length: 0, // No max for hashed value
                pattern: None,
                searchable: false,
            }),
        ));

        // tokenKey — random string for token invalidation
        fields.push(Field::new(
            "tokenKey",
            FieldType::Text(TextOptions {
                min_length: 0,
                max_length: 0,
                pattern: None,
                searchable: false,
            }),
        ));
    }

    // System field: created (auto-date, on create)
    fields.push(Field::new(
        "created",
        FieldType::AutoDate(AutoDateOptions {
            on_create: true,
            on_update: false,
        }),
    ));

    // System field: updated (auto-date, on create + update)
    fields.push(Field::new(
        "updated",
        FieldType::AutoDate(AutoDateOptions {
            on_create: true,
            on_update: true,
        }),
    ));

    // User-defined fields.
    fields.extend(collection.fields.iter().cloned());

    fields
}

/// Convert a `serde_json::Value` (object) to a `HashMap<String, Value>`.
fn value_to_record_data(value: &Value) -> Result<HashMap<String, Value>> {
    match value.as_object() {
        Some(obj) => Ok(obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        None => Err(ZerobaseError::internal("expected JSON object")),
    }
}

/// Remove the `password` field from a record map.
///
/// Called on all auth collection responses to ensure password hashes
/// are never exposed through the API.
fn strip_password(record: &mut HashMap<String, Value>) {
    record.remove("password");
}

/// Generate a random token key for auth record token invalidation.
///
/// Returns a 50-character alphanumeric string used as the `tokenKey` field
/// in auth collections. When a user changes their password, the tokenKey is
/// regenerated, invalidating all previously issued tokens.
fn generate_token_key() -> String {
    nanoid::nanoid!(50, &crate::id::ALPHABET)
}

/// Convert a `HashMap<String, Value>` to a `serde_json::Value::Object`.
fn record_data_to_value(data: &HashMap<String, Value>) -> Value {
    let map: Map<String, Value> = data.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    Value::Object(map)
}

// ── Relation modifier helpers ─────────────────────────────────────────────────

/// Process `field+` (add) and `field-` (remove) modifier keys in an update
/// payload for multi-relation fields.
///
/// For each modifier key found:
/// - `"tags+": "id"` or `"tags+": ["id1", "id2"]` — appends to existing array,
///   deduplicating.
/// - `"tags-": "id"` or `"tags-": ["id1", "id2"]` — removes from existing array.
///   Removing a non-existent ID is a no-op.
///
/// Returns a map of `field_name → resolved_value` for each field that had
/// modifiers applied. The caller should merge these into the record *before*
/// applying plain key updates.
///
/// Only multi-relation fields (max_select != 1) support modifiers.
/// Modifier keys for non-relation fields or single-relation fields produce
/// a validation error.
fn apply_relation_modifiers(
    update_obj: &Map<String, Value>,
    existing: &HashMap<String, Value>,
    collection: &Collection,
) -> Result<HashMap<String, Value>> {
    let mut resolved: HashMap<String, Value> = HashMap::new();

    for (key, modifier_value) in update_obj {
        let (base_field, is_add) = if let Some(base) = key.strip_suffix('+') {
            (base, true)
        } else if let Some(base) = key.strip_suffix('-') {
            (base, false)
        } else {
            continue; // Not a modifier key.
        };

        // Find the field in the collection schema.
        let field = collection
            .fields
            .iter()
            .find(|f| f.name == base_field)
            .ok_or_else(|| {
                ZerobaseError::validation(format!(
                    "modifier key '{}' references unknown field '{}'",
                    key, base_field
                ))
            })?;

        // Only multi-relation fields support modifiers.
        let rel_opts = match &field.field_type {
            FieldType::Relation(opts) if opts.max_select != 1 => opts,
            _ => {
                return Err(ZerobaseError::validation(format!(
                    "modifier '{}' is only supported on multi-relation fields",
                    key
                )));
            }
        };

        // Extract IDs to add or remove from the modifier value.
        let modifier_ids = extract_modifier_ids(modifier_value, key)?;

        // Get the current array for this field (may already have been modified
        // by a previous modifier on the same field, e.g. both tags+ and tags-).
        let current = resolved
            .get(base_field)
            .or_else(|| existing.get(base_field));

        let mut current_ids: Vec<String> = match current {
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            Some(Value::Null) | None => Vec::new(),
            _ => Vec::new(),
        };

        if is_add {
            // Append, deduplicating.
            for id in modifier_ids {
                if !current_ids.contains(&id) {
                    current_ids.push(id);
                }
            }
        } else {
            // Remove matching IDs (non-existent is no-op).
            current_ids.retain(|id| !modifier_ids.contains(id));
        }

        // Enforce max_select if set (0 = unlimited).
        if rel_opts.max_select > 0 && current_ids.len() as u32 > rel_opts.max_select {
            return Err(ZerobaseError::validation(format!(
                "field '{}' would exceed max_select limit of {}",
                base_field, rel_opts.max_select
            )));
        }

        let new_value = Value::Array(current_ids.into_iter().map(Value::String).collect());
        resolved.insert(base_field.to_string(), new_value);
    }

    Ok(resolved)
}

/// Extract a list of string IDs from a modifier value.
///
/// Accepts either a single string `"id"` or an array `["id1", "id2"]`.
fn extract_modifier_ids(value: &Value, key: &str) -> Result<Vec<String>> {
    match value {
        Value::String(s) => {
            if s.is_empty() {
                return Err(ZerobaseError::validation(format!(
                    "modifier '{}' value must be a non-empty string or array of strings",
                    key
                )));
            }
            Ok(vec![s.clone()])
        }
        Value::Array(arr) => {
            let mut ids = Vec::with_capacity(arr.len());
            for item in arr {
                let s = item.as_str().ok_or_else(|| {
                    ZerobaseError::validation(format!(
                        "modifier '{}' array elements must be strings",
                        key
                    ))
                })?;
                if s.is_empty() {
                    return Err(ZerobaseError::validation(format!(
                        "modifier '{}' array elements must be non-empty strings",
                        key
                    )));
                }
                ids.push(s.to_string());
            }
            Ok(ids)
        }
        _ => Err(ZerobaseError::validation(format!(
            "modifier '{}' value must be a string or array of strings",
            key
        ))),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{
        Collection, Field, FieldType, NumberOptions, RelationOptions, TextOptions,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Mutex;

    // ── Mock implementations ──────────────────────────────────────────────

    /// In-memory record repository for testing.
    struct MockRecordRepo {
        records: Mutex<HashMap<String, Vec<HashMap<String, Value>>>>,
    }

    impl MockRecordRepo {
        fn new() -> Self {
            Self {
                records: Mutex::new(HashMap::new()),
            }
        }
    }

    impl RecordRepository for MockRecordRepo {
        fn find_one(
            &self,
            collection: &str,
            id: &str,
        ) -> std::result::Result<HashMap<String, Value>, RecordRepoError> {
            let store = self.records.lock().unwrap();
            let rows = store
                .get(collection)
                .ok_or_else(|| RecordRepoError::NotFound {
                    resource_type: "Record".to_string(),
                    resource_id: Some(id.to_string()),
                })?;
            rows.iter()
                .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
                .cloned()
                .ok_or_else(|| RecordRepoError::NotFound {
                    resource_type: "Record".to_string(),
                    resource_id: Some(id.to_string()),
                })
        }

        fn find_many(
            &self,
            collection: &str,
            query: &RecordQuery,
        ) -> std::result::Result<RecordList, RecordRepoError> {
            let store = self.records.lock().unwrap();
            let rows = store.get(collection).cloned().unwrap_or_default();
            let total = rows.len() as u64;
            let page = query.page.max(1);
            let per_page = query.per_page.max(1);
            let total_pages = if total == 0 {
                1
            } else {
                ((total as f64) / (per_page as f64)).ceil() as u32
            };
            let start = ((page - 1) * per_page) as usize;
            let items: Vec<_> = rows
                .into_iter()
                .skip(start)
                .take(per_page as usize)
                .collect();
            Ok(RecordList {
                items,
                total_items: total,
                page,
                per_page,
                total_pages,
            })
        }

        fn insert(
            &self,
            collection: &str,
            data: &HashMap<String, Value>,
        ) -> std::result::Result<(), RecordRepoError> {
            let mut store = self.records.lock().unwrap();
            store
                .entry(collection.to_string())
                .or_default()
                .push(data.clone());
            Ok(())
        }

        fn update(
            &self,
            collection: &str,
            id: &str,
            data: &HashMap<String, Value>,
        ) -> std::result::Result<bool, RecordRepoError> {
            let mut store = self.records.lock().unwrap();
            if let Some(rows) = store.get_mut(collection) {
                if let Some(row) = rows
                    .iter_mut()
                    .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
                {
                    *row = data.clone();
                    return Ok(true);
                }
            }
            Ok(false)
        }

        fn delete(&self, collection: &str, id: &str) -> std::result::Result<bool, RecordRepoError> {
            let mut store = self.records.lock().unwrap();
            if let Some(rows) = store.get_mut(collection) {
                let before = rows.len();
                rows.retain(|r| r.get("id").and_then(|v| v.as_str()) != Some(id));
                return Ok(rows.len() < before);
            }
            Ok(false)
        }

        fn count(
            &self,
            collection: &str,
            _filter: Option<&str>,
        ) -> std::result::Result<u64, RecordRepoError> {
            let store = self.records.lock().unwrap();
            Ok(store.get(collection).map(|r| r.len() as u64).unwrap_or(0))
        }

        fn find_referencing_records(
            &self,
            collection: &str,
            field_name: &str,
            referenced_id: &str,
        ) -> std::result::Result<Vec<HashMap<String, Value>>, RecordRepoError> {
            let store = self.records.lock().unwrap();
            let rows = match store.get(collection) {
                Some(r) => r,
                None => return Ok(Vec::new()),
            };
            let results = rows
                .iter()
                .filter(|record| {
                    if let Some(val) = record.get(field_name) {
                        match val {
                            Value::String(s) => s == referenced_id,
                            Value::Array(arr) => {
                                arr.iter().any(|v| v.as_str() == Some(referenced_id))
                            }
                            _ => false,
                        }
                    } else {
                        false
                    }
                })
                .cloned()
                .collect();
            Ok(results)
        }
    }

    /// Mock schema lookup that returns a fixed collection.
    struct MockSchema {
        collections: HashMap<String, Collection>,
    }

    impl MockSchema {
        fn with_collection(name: &str, collection: Collection) -> Self {
            let mut collections = HashMap::new();
            collections.insert(name.to_string(), collection);
            Self { collections }
        }
    }

    impl SchemaLookup for MockSchema {
        fn get_collection(&self, name: &str) -> Result<Collection> {
            self.collections
                .get(name)
                .cloned()
                .ok_or_else(|| ZerobaseError::not_found_with_id("Collection", name))
        }

        fn list_all_collections(&self) -> Result<Vec<Collection>> {
            Ok(self.collections.values().cloned().collect())
        }
    }

    // ── Helper ────────────────────────────────────────────────────────────

    fn test_collection() -> Collection {
        Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())).required(true),
                Field::new(
                    "views",
                    FieldType::Number(NumberOptions {
                        min: Some(0.0),
                        max: None,
                        only_int: false,
                    }),
                ),
            ],
        )
    }

    fn make_service() -> RecordService<MockRecordRepo, MockSchema> {
        let collection = test_collection();
        RecordService::new(
            MockRecordRepo::new(),
            MockSchema::with_collection("posts", collection),
        )
    }

    // ── Create tests ──────────────────────────────────────────────────────

    #[test]
    fn create_record_generates_id_and_timestamps() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Hello World", "views": 0 });

        let record = service.create_record("posts", data).unwrap();

        assert!(record.contains_key("id"));
        assert!(record.contains_key("created"));
        assert!(record.contains_key("updated"));

        let id = record["id"].as_str().unwrap();
        assert_eq!(id.len(), 15);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));

        assert!(record["created"].as_str().is_some());
        assert!(record["updated"].as_str().is_some());
    }

    #[test]
    fn create_record_validates_required_fields() {
        let service = make_service();
        // Missing required "title" field.
        let data = serde_json::json!({ "views": 5 });

        let err = service.create_record("posts", data).unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn create_record_rejects_unknown_fields() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Hi", "unknown_field": "bad" });

        let err = service.create_record("posts", data).unwrap_err();
        assert_eq!(err.status_code(), 400);
        if let ZerobaseError::Validation { field_errors, .. } = &err {
            assert!(field_errors.contains_key("unknown_field"));
        } else {
            panic!("expected Validation error");
        }
    }

    #[test]
    fn create_record_validates_field_types() {
        let service = make_service();
        // "views" should be a number, not a string.
        let data = serde_json::json!({ "title": "Hello", "views": "not_a_number" });

        let err = service.create_record("posts", data).unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn create_record_rejects_non_object() {
        let service = make_service();
        let data = serde_json::json!("just a string");

        let err = service.create_record("posts", data).unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn create_record_fails_for_unknown_collection() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Hello" });

        let err = service.create_record("nonexistent", data).unwrap_err();
        assert_eq!(err.status_code(), 404);
    }

    // ── Get tests ─────────────────────────────────────────────────────────

    #[test]
    fn get_record_returns_created_record() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Test Post", "views": 10 });
        let created = service.create_record("posts", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let fetched = service.get_record("posts", id).unwrap();
        assert_eq!(fetched["title"], "Test Post");
        assert_eq!(fetched["id"], created["id"]);
    }

    #[test]
    fn get_record_returns_not_found() {
        let service = make_service();
        let err = service.get_record("posts", "nonexistent12345").unwrap_err();
        assert_eq!(err.status_code(), 404);
    }

    // ── List tests ────────────────────────────────────────────────────────

    #[test]
    fn list_records_returns_paginated_results() {
        let service = make_service();

        // Create 5 records.
        for i in 0..5 {
            let data = serde_json::json!({ "title": format!("Post {i}"), "views": i });
            service.create_record("posts", data).unwrap();
        }

        let query = RecordQuery {
            page: 1,
            per_page: 2,
            ..Default::default()
        };
        let result = service.list_records("posts", &query).unwrap();

        assert_eq!(result.items.len(), 2);
        assert_eq!(result.total_items, 5);
        assert_eq!(result.page, 1);
        assert_eq!(result.per_page, 2);
        assert_eq!(result.total_pages, 3);
    }

    #[test]
    fn list_records_uses_defaults_for_zero_pagination() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Post", "views": 0 });
        service.create_record("posts", data).unwrap();

        let query = RecordQuery::default(); // page=0, per_page=0
        let result = service.list_records("posts", &query).unwrap();

        assert_eq!(result.page, 1);
        assert_eq!(result.per_page, DEFAULT_PER_PAGE);
    }

    #[test]
    fn list_records_clamps_per_page() {
        let service = make_service();
        let query = RecordQuery {
            per_page: 1000,
            ..Default::default()
        };
        let result = service.list_records("posts", &query).unwrap();
        assert_eq!(result.per_page, MAX_PER_PAGE);
    }

    // ── Update tests ──────────────────────────────────────────────────────

    #[test]
    fn update_record_modifies_fields() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Original", "views": 0 });
        let created = service.create_record("posts", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let update_data = serde_json::json!({ "title": "Updated" });
        let updated = service.update_record("posts", id, update_data).unwrap();

        assert_eq!(updated["title"], "Updated");
        assert_eq!(updated["id"], created["id"]);
    }

    #[test]
    fn update_record_preserves_unmodified_fields() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Hello", "views": 42 });
        let created = service.create_record("posts", data).unwrap();
        let id = created["id"].as_str().unwrap();

        // Only update title, views should be preserved.
        let update_data = serde_json::json!({ "title": "New Title" });
        let updated = service.update_record("posts", id, update_data).unwrap();

        assert_eq!(updated["title"], "New Title");
        assert_eq!(updated["views"], 42);
    }

    #[test]
    fn update_record_refreshes_updated_timestamp() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Hello" });
        let created = service.create_record("posts", data).unwrap();
        let id = created["id"].as_str().unwrap();
        let original_updated = created["updated"].as_str().unwrap().to_string();

        // Sleep long enough for second-precision timestamps to differ.
        std::thread::sleep(std::time::Duration::from_secs(1));

        let update_data = serde_json::json!({ "title": "Changed" });
        let updated = service.update_record("posts", id, update_data).unwrap();

        // The `updated` timestamp should change.
        let new_updated = updated["updated"].as_str().unwrap();
        assert_ne!(new_updated, original_updated);
    }

    #[test]
    fn update_record_prevents_id_change() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Hello" });
        let created = service.create_record("posts", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let update_data = serde_json::json!({ "id": "newid1234567890" });
        let err = service.update_record("posts", id, update_data).unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn update_record_returns_not_found() {
        let service = make_service();
        let update_data = serde_json::json!({ "title": "Updated" });
        let err = service
            .update_record("posts", "nonexistent12345", update_data)
            .unwrap_err();
        assert_eq!(err.status_code(), 404);
    }

    #[test]
    fn update_record_validates_field_values() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Hello" });
        let created = service.create_record("posts", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let update_data = serde_json::json!({ "views": "not_a_number" });
        let err = service.update_record("posts", id, update_data).unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    // ── Delete tests ──────────────────────────────────────────────────────

    #[test]
    fn delete_record_removes_record() {
        let service = make_service();
        let data = serde_json::json!({ "title": "To Delete" });
        let created = service.create_record("posts", data).unwrap();
        let id = created["id"].as_str().unwrap();

        service.delete_record("posts", id).unwrap();

        let err = service.get_record("posts", id).unwrap_err();
        assert_eq!(err.status_code(), 404);
    }

    #[test]
    fn delete_record_returns_not_found() {
        let service = make_service();
        let err = service
            .delete_record("posts", "nonexistent12345")
            .unwrap_err();
        assert_eq!(err.status_code(), 404);
    }

    #[test]
    fn delete_record_fails_for_unknown_collection() {
        let service = make_service();
        let err = service
            .delete_record("nonexistent", "someid12345abcd")
            .unwrap_err();
        assert_eq!(err.status_code(), 404);
    }

    // ── Multiple records ──────────────────────────────────────────────────

    #[test]
    fn multiple_records_have_unique_ids() {
        let service = make_service();
        let mut ids = std::collections::HashSet::new();

        for i in 0..10 {
            let data = serde_json::json!({ "title": format!("Post {i}") });
            let record = service.create_record("posts", data).unwrap();
            let id = record["id"].as_str().unwrap().to_string();
            ids.insert(id);
        }

        assert_eq!(ids.len(), 10, "all IDs should be unique");
    }

    // ── parse_sort tests ─────────────────────────────────────────────────

    #[test]
    fn parse_sort_empty_string_returns_empty() {
        let result = parse_sort("").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_sort_whitespace_only_returns_empty() {
        let result = parse_sort("   ").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_sort_single_field_ascending() {
        let result = parse_sort("title").unwrap();
        assert_eq!(result, vec![("title".to_string(), SortDirection::Asc)]);
    }

    #[test]
    fn parse_sort_single_field_descending() {
        let result = parse_sort("-created").unwrap();
        assert_eq!(result, vec![("created".to_string(), SortDirection::Desc)]);
    }

    #[test]
    fn parse_sort_explicit_ascending_with_plus() {
        let result = parse_sort("+title").unwrap();
        assert_eq!(result, vec![("title".to_string(), SortDirection::Asc)]);
    }

    #[test]
    fn parse_sort_multiple_fields() {
        let result = parse_sort("-created,title").unwrap();
        assert_eq!(
            result,
            vec![
                ("created".to_string(), SortDirection::Desc),
                ("title".to_string(), SortDirection::Asc),
            ]
        );
    }

    #[test]
    fn parse_sort_multiple_fields_mixed_directions() {
        let result = parse_sort("-updated,+title,-views").unwrap();
        assert_eq!(
            result,
            vec![
                ("updated".to_string(), SortDirection::Desc),
                ("title".to_string(), SortDirection::Asc),
                ("views".to_string(), SortDirection::Desc),
            ]
        );
    }

    #[test]
    fn parse_sort_trims_whitespace_around_fields() {
        let result = parse_sort(" -created , title ").unwrap();
        assert_eq!(
            result,
            vec![
                ("created".to_string(), SortDirection::Desc),
                ("title".to_string(), SortDirection::Asc),
            ]
        );
    }

    #[test]
    fn parse_sort_rejects_empty_segment() {
        let err = parse_sort("title,,created").unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn parse_sort_rejects_trailing_comma() {
        let err = parse_sort("title,").unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn parse_sort_rejects_lone_minus() {
        let err = parse_sort("-").unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn parse_sort_rejects_lone_plus() {
        let err = parse_sort("+").unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn parse_sort_rejects_invalid_characters() {
        let err = parse_sort("title; DROP TABLE posts").unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn parse_sort_rejects_field_with_spaces() {
        let err = parse_sort("bad field").unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn parse_sort_allows_underscores() {
        let result = parse_sort("my_field").unwrap();
        assert_eq!(result, vec![("my_field".to_string(), SortDirection::Asc)]);
    }

    #[test]
    fn parse_sort_allows_alphanumeric() {
        let result = parse_sort("field123").unwrap();
        assert_eq!(result, vec![("field123".to_string(), SortDirection::Asc)]);
    }

    // ── validate_sort_fields tests ───────────────────────────────────────

    #[test]
    fn validate_sort_fields_accepts_user_defined_field() {
        let collection = test_collection();
        let sort = vec![("title".to_string(), SortDirection::Asc)];
        assert!(validate_sort_fields(&sort, &collection).is_ok());
    }

    #[test]
    fn validate_sort_fields_accepts_system_fields() {
        let collection = test_collection();
        let sort = vec![
            ("id".to_string(), SortDirection::Asc),
            ("created".to_string(), SortDirection::Desc),
            ("updated".to_string(), SortDirection::Asc),
        ];
        assert!(validate_sort_fields(&sort, &collection).is_ok());
    }

    #[test]
    fn validate_sort_fields_accepts_empty_sort() {
        let collection = test_collection();
        assert!(validate_sort_fields(&[], &collection).is_ok());
    }

    #[test]
    fn validate_sort_fields_rejects_unknown_field() {
        let collection = test_collection();
        let sort = vec![("nonexistent".to_string(), SortDirection::Asc)];
        let err = validate_sort_fields(&sort, &collection).unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("nonexistent"));
    }

    #[test]
    fn validate_sort_fields_rejects_if_any_field_unknown() {
        let collection = test_collection();
        let sort = vec![
            ("title".to_string(), SortDirection::Asc),
            ("unknown_field".to_string(), SortDirection::Desc),
        ];
        let err = validate_sort_fields(&sort, &collection).unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn validate_sort_fields_accepts_auth_system_fields() {
        let collection = Collection::auth(
            "users",
            vec![Field::new("name", FieldType::Text(TextOptions::default()))],
        );
        let sort = vec![
            ("email".to_string(), SortDirection::Asc),
            ("verified".to_string(), SortDirection::Desc),
            ("name".to_string(), SortDirection::Asc),
        ];
        assert!(validate_sort_fields(&sort, &collection).is_ok());
    }

    // ── Pagination edge cases via service ────────────────────────────────

    #[test]
    fn list_records_default_per_page_is_30() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Post" });
        service.create_record("posts", data).unwrap();

        let query = RecordQuery {
            page: 1,
            per_page: 0, // should default
            ..Default::default()
        };
        let result = service.list_records("posts", &query).unwrap();
        assert_eq!(result.per_page, 30);
    }

    #[test]
    fn list_records_max_per_page_is_500() {
        let service = make_service();
        let query = RecordQuery {
            per_page: 501,
            ..Default::default()
        };
        let result = service.list_records("posts", &query).unwrap();
        assert_eq!(result.per_page, 500);
    }

    #[test]
    fn list_records_page_beyond_last_returns_empty() {
        let service = make_service();
        for i in 0..3 {
            let data = serde_json::json!({ "title": format!("Post {i}") });
            service.create_record("posts", data).unwrap();
        }

        let query = RecordQuery {
            page: 100,
            per_page: 10,
            ..Default::default()
        };
        let result = service.list_records("posts", &query).unwrap();
        assert!(result.items.is_empty());
        assert_eq!(result.total_items, 3);
    }

    #[test]
    fn list_records_empty_collection_returns_metadata() {
        let service = make_service();
        let query = RecordQuery {
            page: 1,
            per_page: 10,
            ..Default::default()
        };
        let result = service.list_records("posts", &query).unwrap();

        assert!(result.items.is_empty());
        assert_eq!(result.total_items, 0);
        assert_eq!(result.total_pages, 1);
        assert_eq!(result.page, 1);
        assert_eq!(result.per_page, 10);
    }

    #[test]
    fn list_records_per_page_at_boundary_500() {
        let service = make_service();
        let query = RecordQuery {
            per_page: 500,
            ..Default::default()
        };
        let result = service.list_records("posts", &query).unwrap();
        assert_eq!(result.per_page, 500);
    }

    #[test]
    fn record_list_serializes_to_camel_case_json() {
        let list = RecordList {
            items: vec![],
            total_items: 42,
            page: 2,
            per_page: 10,
            total_pages: 5,
        };
        let json = serde_json::to_value(&list).unwrap();
        assert_eq!(json["totalItems"], 42);
        assert_eq!(json["totalPages"], 5);
        assert_eq!(json["perPage"], 10);
        assert!(json["items"].as_array().unwrap().is_empty());
    }

    // ── Integration: list_records with sort validation ───────────────────

    #[test]
    fn list_records_with_valid_sort_succeeds() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Test", "views": 0 });
        service.create_record("posts", data).unwrap();

        let query = RecordQuery {
            sort: vec![("title".to_string(), SortDirection::Asc)],
            ..Default::default()
        };
        let result = service.list_records("posts", &query).unwrap();
        assert_eq!(result.total_items, 1);
    }

    #[test]
    fn list_records_with_system_field_sort_succeeds() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Test" });
        service.create_record("posts", data).unwrap();

        let query = RecordQuery {
            sort: vec![("created".to_string(), SortDirection::Desc)],
            ..Default::default()
        };
        let result = service.list_records("posts", &query).unwrap();
        assert_eq!(result.total_items, 1);
    }

    #[test]
    fn list_records_rejects_invalid_sort_field() {
        let service = make_service();
        let query = RecordQuery {
            sort: vec![("nonexistent".to_string(), SortDirection::Asc)],
            ..Default::default()
        };
        let err = service.list_records("posts", &query).unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("nonexistent"));
    }

    #[test]
    fn list_records_with_multi_field_sort_succeeds() {
        let service = make_service();
        for i in 0..3 {
            let data = serde_json::json!({ "title": format!("Post {i}"), "views": i });
            service.create_record("posts", data).unwrap();
        }

        let query = RecordQuery {
            sort: vec![
                ("views".to_string(), SortDirection::Desc),
                ("title".to_string(), SortDirection::Asc),
            ],
            ..Default::default()
        };
        let result = service.list_records("posts", &query).unwrap();
        assert_eq!(result.total_items, 3);
    }

    // ── parse_fields tests ───────────────────────────────────────────────

    #[test]
    fn parse_fields_empty_string_returns_empty() {
        let result = parse_fields("").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_fields_whitespace_only_returns_empty() {
        let result = parse_fields("   ").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_fields_single_field() {
        let result = parse_fields("title").unwrap();
        assert!(result.contains(&"id".to_string()));
        assert!(result.contains(&"title".to_string()));
    }

    #[test]
    fn parse_fields_multiple_fields() {
        let result = parse_fields("title,views,created").unwrap();
        assert!(result.contains(&"id".to_string()));
        assert!(result.contains(&"title".to_string()));
        assert!(result.contains(&"views".to_string()));
        assert!(result.contains(&"created".to_string()));
    }

    #[test]
    fn parse_fields_always_includes_id() {
        let result = parse_fields("title").unwrap();
        assert!(result.contains(&"id".to_string()));
    }

    #[test]
    fn parse_fields_deduplicates_id() {
        let result = parse_fields("id,title,id").unwrap();
        let id_count = result.iter().filter(|f| *f == "id").count();
        assert_eq!(id_count, 1);
    }

    #[test]
    fn parse_fields_trims_whitespace() {
        let result = parse_fields(" title , views ").unwrap();
        assert!(result.contains(&"title".to_string()));
        assert!(result.contains(&"views".to_string()));
    }

    #[test]
    fn parse_fields_skips_empty_segments() {
        let result = parse_fields("title,,views").unwrap();
        assert!(result.contains(&"title".to_string()));
        assert!(result.contains(&"views".to_string()));
    }

    #[test]
    fn parse_fields_rejects_invalid_characters() {
        let err = parse_fields("title; DROP TABLE").unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn parse_fields_allows_underscores() {
        let result = parse_fields("my_field").unwrap();
        assert!(result.contains(&"my_field".to_string()));
    }

    // ── validate_and_filter_fields tests ─────────────────────────────────

    #[test]
    fn validate_fields_filters_unknown_fields() {
        let collection = test_collection();
        let fields = vec![
            "id".to_string(),
            "title".to_string(),
            "nonexistent".to_string(),
        ];
        let valid = validate_and_filter_fields(&fields, &collection);
        assert!(valid.contains(&"id".to_string()));
        assert!(valid.contains(&"title".to_string()));
        assert!(!valid.contains(&"nonexistent".to_string()));
    }

    #[test]
    fn validate_fields_accepts_system_fields() {
        let collection = test_collection();
        let fields = vec![
            "id".to_string(),
            "created".to_string(),
            "updated".to_string(),
        ];
        let valid = validate_and_filter_fields(&fields, &collection);
        assert_eq!(valid.len(), 3);
    }

    #[test]
    fn validate_fields_returns_empty_when_all_unknown() {
        let collection = test_collection();
        let fields = vec!["nonexistent".to_string(), "also_missing".to_string()];
        let valid = validate_and_filter_fields(&fields, &collection);
        assert!(valid.is_empty());
    }

    // ── project_fields tests ─────────────────────────────────────────────

    #[test]
    fn project_fields_keeps_only_requested() {
        let mut record = HashMap::new();
        record.insert("id".to_string(), Value::String("abc".into()));
        record.insert("title".to_string(), Value::String("Hello".into()));
        record.insert("views".to_string(), Value::Number(42.into()));
        record.insert("created".to_string(), Value::String("2024-01-01".into()));

        let fields = vec!["id".to_string(), "title".to_string()];
        let projected = project_fields(&record, &fields);

        assert_eq!(projected.len(), 2);
        assert!(projected.contains_key("id"));
        assert!(projected.contains_key("title"));
        assert!(!projected.contains_key("views"));
        assert!(!projected.contains_key("created"));
    }

    #[test]
    fn project_fields_always_includes_id() {
        let mut record = HashMap::new();
        record.insert("id".to_string(), Value::String("abc".into()));
        record.insert("title".to_string(), Value::String("Hello".into()));

        // Request only title — id should still be included.
        let fields = vec!["title".to_string()];
        let projected = project_fields(&record, &fields);

        assert!(projected.contains_key("id"));
        assert!(projected.contains_key("title"));
    }

    #[test]
    fn project_fields_silently_skips_missing() {
        let mut record = HashMap::new();
        record.insert("id".to_string(), Value::String("abc".into()));
        record.insert("title".to_string(), Value::String("Hello".into()));

        let fields = vec![
            "id".to_string(),
            "title".to_string(),
            "nonexistent".to_string(),
        ];
        let projected = project_fields(&record, &fields);

        assert_eq!(projected.len(), 2);
        assert!(!projected.contains_key("nonexistent"));
    }

    // ── project_record_list tests ────────────────────────────────────────

    #[test]
    fn project_record_list_applies_to_all_items() {
        let mut r1 = HashMap::new();
        r1.insert("id".to_string(), Value::String("a".into()));
        r1.insert("title".to_string(), Value::String("Post A".into()));
        r1.insert("views".to_string(), Value::Number(10.into()));

        let mut r2 = HashMap::new();
        r2.insert("id".to_string(), Value::String("b".into()));
        r2.insert("title".to_string(), Value::String("Post B".into()));
        r2.insert("views".to_string(), Value::Number(20.into()));

        let record_list = RecordList {
            items: vec![r1, r2],
            total_items: 2,
            page: 1,
            per_page: 10,
            total_pages: 1,
        };

        let fields = vec!["id".to_string(), "title".to_string()];
        let projected = project_record_list(record_list, &fields);

        assert_eq!(projected.items.len(), 2);
        for item in &projected.items {
            assert!(item.contains_key("id"));
            assert!(item.contains_key("title"));
            assert!(!item.contains_key("views"));
        }
        // Metadata preserved.
        assert_eq!(projected.total_items, 2);
        assert_eq!(projected.page, 1);
    }

    // ── Integration: list_records with field selection ────────────────────

    #[test]
    fn list_records_with_fields_projects_results() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Hello", "views": 42 });
        service.create_record("posts", data).unwrap();

        let query = RecordQuery {
            fields: Some(vec!["id".to_string(), "title".to_string()]),
            ..Default::default()
        };
        let result = service.list_records("posts", &query).unwrap();

        assert_eq!(result.items.len(), 1);
        let item = &result.items[0];
        assert!(item.contains_key("id"));
        assert!(item.contains_key("title"));
        assert!(!item.contains_key("views"));
        assert!(!item.contains_key("created"));
        assert!(!item.contains_key("updated"));
    }

    #[test]
    fn list_records_without_fields_returns_all() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Hello", "views": 42 });
        service.create_record("posts", data).unwrap();

        let query = RecordQuery::default();
        let result = service.list_records("posts", &query).unwrap();

        assert_eq!(result.items.len(), 1);
        let item = &result.items[0];
        assert!(item.contains_key("id"));
        assert!(item.contains_key("title"));
        assert!(item.contains_key("views"));
        assert!(item.contains_key("created"));
        assert!(item.contains_key("updated"));
    }

    #[test]
    fn list_records_with_unknown_fields_ignores_them() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Hello", "views": 42 });
        service.create_record("posts", data).unwrap();

        let query = RecordQuery {
            fields: Some(vec![
                "id".to_string(),
                "title".to_string(),
                "nonexistent".to_string(),
            ]),
            ..Default::default()
        };
        let result = service.list_records("posts", &query).unwrap();

        let item = &result.items[0];
        assert!(item.contains_key("id"));
        assert!(item.contains_key("title"));
        assert!(!item.contains_key("nonexistent"));
        assert!(!item.contains_key("views"));
    }

    // ── get_record_with_fields tests ─────────────────────────────────────

    #[test]
    fn get_record_with_fields_projects_result() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Hello", "views": 42 });
        let created = service.create_record("posts", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let fields = vec!["id".to_string(), "title".to_string()];
        let record = service
            .get_record_with_fields("posts", id, Some(&fields))
            .unwrap();

        assert!(record.contains_key("id"));
        assert!(record.contains_key("title"));
        assert!(!record.contains_key("views"));
        assert!(!record.contains_key("created"));
    }

    #[test]
    fn get_record_with_fields_none_returns_all() {
        let service = make_service();
        let data = serde_json::json!({ "title": "Hello", "views": 42 });
        let created = service.create_record("posts", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let record = service.get_record_with_fields("posts", id, None).unwrap();

        assert!(record.contains_key("id"));
        assert!(record.contains_key("title"));
        assert!(record.contains_key("views"));
        assert!(record.contains_key("created"));
        assert!(record.contains_key("updated"));
    }

    // ── count_records tests ───────────────────────────────────────────────

    #[test]
    fn count_records_empty_collection() {
        let service = make_service();
        let count = service.count_records("posts", None).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn count_records_after_inserts() {
        let service = make_service();
        service
            .create_record("posts", serde_json::json!({ "title": "A", "views": 1 }))
            .unwrap();
        service
            .create_record("posts", serde_json::json!({ "title": "B", "views": 2 }))
            .unwrap();
        service
            .create_record("posts", serde_json::json!({ "title": "C", "views": 3 }))
            .unwrap();

        let count = service.count_records("posts", None).unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn count_records_unknown_collection_returns_error() {
        let service = make_service();
        let err = service.count_records("nonexistent", None).unwrap_err();
        assert_eq!(err.status_code(), 404);
    }

    // ── Auth collection helpers ──────────────────────────────────────────

    fn auth_collection() -> Collection {
        Collection::auth(
            "users",
            vec![Field::new("name", FieldType::Text(TextOptions::default()))],
        )
    }

    fn make_auth_service() -> RecordService<MockRecordRepo, MockSchema> {
        use crate::auth::NoOpHasher;
        let collection = auth_collection();
        RecordService::with_password_hasher(
            MockRecordRepo::new(),
            MockSchema::with_collection("users", collection),
            NoOpHasher,
        )
    }

    // ── Auth collection tests ────────────────────────────────────────────

    #[test]
    fn auth_create_hashes_password() {
        let service = make_auth_service();
        let data = serde_json::json!({
            "email": "user@example.com",
            "password": "secret123",
            "name": "Alice",
        });

        let record = service.create_record("users", data).unwrap();

        // Password should be stripped from response.
        assert!(!record.contains_key("password"));

        // Verify the stored record has the hashed password.
        let id = record["id"].as_str().unwrap();
        let store = service.repo.records.lock().unwrap();
        let stored = store["users"]
            .iter()
            .find(|r| r["id"].as_str() == Some(id))
            .unwrap();
        let stored_pw = stored["password"].as_str().unwrap();
        assert!(
            stored_pw.starts_with("hashed:"),
            "password should be hashed, got: {stored_pw}"
        );
        assert_eq!(stored_pw, "hashed:secret123");
    }

    #[test]
    fn auth_create_generates_token_key() {
        let service = make_auth_service();
        let data = serde_json::json!({
            "email": "user@example.com",
            "password": "secret123",
        });

        let record = service.create_record("users", data).unwrap();

        let token_key = record["tokenKey"].as_str().unwrap();
        assert_eq!(token_key.len(), 50, "tokenKey should be 50 chars");
        assert!(
            token_key.chars().all(|c| c.is_ascii_alphanumeric()),
            "tokenKey should be alphanumeric"
        );
    }

    #[test]
    fn auth_create_sets_defaults() {
        let service = make_auth_service();
        let data = serde_json::json!({
            "email": "user@example.com",
            "password": "secret123",
        });

        let record = service.create_record("users", data).unwrap();

        // emailVisibility defaults to false.
        assert_eq!(record["emailVisibility"], Value::Bool(false));
        // verified defaults to false.
        assert_eq!(record["verified"], Value::Bool(false));
    }

    #[test]
    fn auth_create_respects_provided_email_visibility() {
        let service = make_auth_service();
        let data = serde_json::json!({
            "email": "user@example.com",
            "password": "secret123",
            "emailVisibility": true,
        });

        let record = service.create_record("users", data).unwrap();
        assert_eq!(record["emailVisibility"], Value::Bool(true));
    }

    #[test]
    fn auth_get_record_strips_password() {
        let service = make_auth_service();
        let data = serde_json::json!({
            "email": "user@example.com",
            "password": "secret123",
        });
        let created = service.create_record("users", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let fetched = service.get_record("users", id).unwrap();
        assert!(!fetched.contains_key("password"));
        assert_eq!(fetched["email"], "user@example.com");
    }

    #[test]
    fn auth_list_records_strips_passwords() {
        let service = make_auth_service();
        for i in 0..3 {
            let data = serde_json::json!({
                "email": format!("user{i}@example.com"),
                "password": format!("pass{i}"),
            });
            service.create_record("users", data).unwrap();
        }

        let query = RecordQuery::default();
        let result = service.list_records("users", &query).unwrap();

        assert_eq!(result.total_items, 3);
        for record in &result.items {
            assert!(
                !record.contains_key("password"),
                "password should be stripped from list response"
            );
        }
    }

    #[test]
    fn auth_update_rehashes_password() {
        let service = make_auth_service();
        let data = serde_json::json!({
            "email": "user@example.com",
            "password": "original",
        });
        let created = service.create_record("users", data).unwrap();
        let id = created["id"].as_str().unwrap();
        let original_token_key = created["tokenKey"].as_str().unwrap().to_string();

        // Update password.
        let update = serde_json::json!({ "password": "newpassword" });
        let updated = service.update_record("users", id, update).unwrap();

        // Password should be stripped from response.
        assert!(!updated.contains_key("password"));

        // tokenKey should be regenerated.
        let new_token_key = updated["tokenKey"].as_str().unwrap();
        assert_ne!(
            new_token_key, original_token_key,
            "tokenKey should change when password changes"
        );

        // Verify stored password is the new hashed value.
        let store = service.repo.records.lock().unwrap();
        let stored = store["users"]
            .iter()
            .find(|r| r["id"].as_str() == Some(id))
            .unwrap();
        assert_eq!(stored["password"].as_str().unwrap(), "hashed:newpassword");
    }

    #[test]
    fn auth_update_without_password_preserves_token_key() {
        let service = make_auth_service();
        let data = serde_json::json!({
            "email": "user@example.com",
            "password": "secret",
        });
        let created = service.create_record("users", data).unwrap();
        let id = created["id"].as_str().unwrap();
        let original_token_key = created["tokenKey"].as_str().unwrap().to_string();

        // Update non-password field.
        let update = serde_json::json!({ "name": "Alice" });
        let updated = service.update_record("users", id, update).unwrap();

        // tokenKey should remain the same.
        assert_eq!(
            updated["tokenKey"].as_str().unwrap(),
            original_token_key,
            "tokenKey should not change when password is not updated"
        );
    }

    #[test]
    fn auth_get_record_with_fields_strips_password() {
        let service = make_auth_service();
        let data = serde_json::json!({
            "email": "user@example.com",
            "password": "secret",
        });
        let created = service.create_record("users", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let fields = vec!["email".to_string(), "password".to_string()];
        let fetched = service
            .get_record_with_fields("users", id, Some(&fields))
            .unwrap();

        // password should still be stripped even if explicitly requested
        assert!(!fetched.contains_key("password"));
        assert!(fetched.contains_key("email"));
    }

    #[test]
    fn auth_create_without_password_hasher_fails() {
        let collection = auth_collection();
        let service = RecordService::new(
            MockRecordRepo::new(),
            MockSchema::with_collection("users", collection),
        );
        let data = serde_json::json!({
            "email": "user@example.com",
            "password": "secret",
        });

        let err = service.create_record("users", data).unwrap_err();
        assert_eq!(err.status_code(), 500);
    }

    #[test]
    fn strip_password_removes_password_field() {
        let mut record = HashMap::new();
        record.insert("id".to_string(), Value::String("abc".into()));
        record.insert("password".to_string(), Value::String("secret".into()));
        record.insert("email".to_string(), Value::String("a@b.com".into()));

        strip_password(&mut record);

        assert!(!record.contains_key("password"));
        assert!(record.contains_key("email"));
        assert!(record.contains_key("id"));
    }

    #[test]
    fn generate_token_key_produces_valid_key() {
        let key = generate_token_key();
        assert_eq!(key.len(), 50);
        assert!(key.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn generate_token_key_is_unique() {
        let k1 = generate_token_key();
        let k2 = generate_token_key();
        assert_ne!(k1, k2, "consecutive token keys should differ");
    }

    // ── Relation & cascade tests ─────────────────────────────────────────

    /// Helper: create a MockSchema with multiple collections.
    fn multi_schema(collections: Vec<(&str, Collection)>) -> MockSchema {
        let mut map = HashMap::new();
        for (name, coll) in collections {
            map.insert(name.to_string(), coll);
        }
        MockSchema { collections: map }
    }

    /// Helper: build a service with "authors" and "posts" where posts.author -> authors.
    fn make_relation_service(
        on_delete: OnDeleteAction,
    ) -> RecordService<MockRecordRepo, MockSchema> {
        let authors = Collection::base(
            "authors",
            vec![Field::new("name", FieldType::Text(TextOptions::default())).required(true)],
        );
        let posts = Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())).required(true),
                Field::new(
                    "author",
                    FieldType::Relation(RelationOptions {
                        collection_id: "authors".to_string(),
                        max_select: 1,
                        on_delete,
                        ..Default::default()
                    }),
                ),
            ],
        );

        let schema = multi_schema(vec![("authors", authors), ("posts", posts)]);
        RecordService::new(MockRecordRepo::new(), schema)
    }

    /// Helper: build a service with multi-relation (posts.tags -> tags, max_select=5).
    fn make_multi_relation_service(
        on_delete: OnDeleteAction,
    ) -> RecordService<MockRecordRepo, MockSchema> {
        let tags = Collection::base(
            "tags",
            vec![Field::new("label", FieldType::Text(TextOptions::default())).required(true)],
        );
        let posts = Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())).required(true),
                Field::new(
                    "tags",
                    FieldType::Relation(RelationOptions {
                        collection_id: "tags".to_string(),
                        max_select: 5,
                        on_delete,
                        ..Default::default()
                    }),
                ),
            ],
        );

        let schema = multi_schema(vec![("tags", tags), ("posts", posts)]);
        RecordService::new(MockRecordRepo::new(), schema)
    }

    // ── Relation reference validation ────────────────────────────────────

    #[test]
    fn relation_valid_reference_accepted() {
        let service = make_relation_service(OnDeleteAction::NoAction);

        // Create an author first.
        let author = service
            .create_record("authors", serde_json::json!({"name": "Alice"}))
            .unwrap();
        let author_id = author["id"].as_str().unwrap().to_string();

        // Create a post referencing that author — should succeed.
        let post = service
            .create_record(
                "posts",
                serde_json::json!({"title": "Hello", "author": author_id}),
            )
            .unwrap();
        assert_eq!(post["author"].as_str().unwrap(), author_id);
    }

    #[test]
    fn relation_invalid_reference_rejected() {
        let service = make_relation_service(OnDeleteAction::NoAction);

        // Try to create a post referencing a non-existent author.
        let err = service
            .create_record(
                "posts",
                serde_json::json!({"title": "Hello", "author": "nonexistent123"}),
            )
            .unwrap_err();
        assert_eq!(err.status_code(), 400);
        if let ZerobaseError::Validation { field_errors, .. } = &err {
            assert!(
                field_errors.contains_key("author"),
                "expected 'author' field error, got: {field_errors:?}"
            );
        } else {
            panic!("expected Validation error, got: {err:?}");
        }
    }

    #[test]
    fn relation_null_reference_accepted() {
        let service = make_relation_service(OnDeleteAction::NoAction);

        // Null relation should be fine (not required).
        let post = service
            .create_record(
                "posts",
                serde_json::json!({"title": "Hello", "author": null}),
            )
            .unwrap();
        assert!(post["author"].is_null());
    }

    #[test]
    fn relation_multi_valid_references_accepted() {
        let service = make_multi_relation_service(OnDeleteAction::NoAction);

        let t1 = service
            .create_record("tags", serde_json::json!({"label": "rust"}))
            .unwrap();
        let t2 = service
            .create_record("tags", serde_json::json!({"label": "web"}))
            .unwrap();
        let t1_id = t1["id"].as_str().unwrap().to_string();
        let t2_id = t2["id"].as_str().unwrap().to_string();

        let post = service
            .create_record(
                "posts",
                serde_json::json!({"title": "Post", "tags": [t1_id, t2_id]}),
            )
            .unwrap();
        let tags = post["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 2);
    }

    #[test]
    fn relation_multi_invalid_reference_rejected() {
        let service = make_multi_relation_service(OnDeleteAction::NoAction);

        let t1 = service
            .create_record("tags", serde_json::json!({"label": "rust"}))
            .unwrap();
        let t1_id = t1["id"].as_str().unwrap().to_string();

        // One valid, one invalid.
        let err = service
            .create_record(
                "posts",
                serde_json::json!({"title": "Post", "tags": [t1_id, "bad_id_12345678"]}),
            )
            .unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    // ── CASCADE on delete ────────────────────────────────────────────────

    #[test]
    fn cascade_delete_removes_referencing_records() {
        let service = make_relation_service(OnDeleteAction::Cascade);

        let author = service
            .create_record("authors", serde_json::json!({"name": "Alice"}))
            .unwrap();
        let author_id = author["id"].as_str().unwrap().to_string();

        // Create two posts referencing this author.
        let p1 = service
            .create_record(
                "posts",
                serde_json::json!({"title": "Post 1", "author": &author_id}),
            )
            .unwrap();
        let p2 = service
            .create_record(
                "posts",
                serde_json::json!({"title": "Post 2", "author": &author_id}),
            )
            .unwrap();

        // Delete the author — should cascade-delete both posts.
        service.delete_record("authors", &author_id).unwrap();

        // Both posts should be gone.
        let p1_id = p1["id"].as_str().unwrap();
        let p2_id = p2["id"].as_str().unwrap();
        assert!(service.get_record("posts", p1_id).is_err());
        assert!(service.get_record("posts", p2_id).is_err());
    }

    #[test]
    fn cascade_delete_with_no_referencing_records_succeeds() {
        let service = make_relation_service(OnDeleteAction::Cascade);

        let author = service
            .create_record("authors", serde_json::json!({"name": "Alice"}))
            .unwrap();
        let author_id = author["id"].as_str().unwrap().to_string();

        // Delete author with no posts — should just succeed.
        service.delete_record("authors", &author_id).unwrap();
    }

    // ── SET_NULL on delete ───────────────────────────────────────────────

    #[test]
    fn set_null_on_delete_nullifies_single_relation() {
        let service = make_relation_service(OnDeleteAction::SetNull);

        let author = service
            .create_record("authors", serde_json::json!({"name": "Alice"}))
            .unwrap();
        let author_id = author["id"].as_str().unwrap().to_string();

        let post = service
            .create_record(
                "posts",
                serde_json::json!({"title": "Post 1", "author": &author_id}),
            )
            .unwrap();
        let post_id = post["id"].as_str().unwrap().to_string();

        // Delete author — post should survive with author set to null.
        service.delete_record("authors", &author_id).unwrap();

        let updated_post = service.get_record("posts", &post_id).unwrap();
        assert!(
            updated_post["author"].is_null(),
            "expected author to be null after SET_NULL, got: {:?}",
            updated_post["author"]
        );
    }

    #[test]
    fn set_null_on_delete_removes_id_from_multi_relation() {
        let service = make_multi_relation_service(OnDeleteAction::SetNull);

        let t1 = service
            .create_record("tags", serde_json::json!({"label": "rust"}))
            .unwrap();
        let t2 = service
            .create_record("tags", serde_json::json!({"label": "web"}))
            .unwrap();
        let t1_id = t1["id"].as_str().unwrap().to_string();
        let t2_id = t2["id"].as_str().unwrap().to_string();

        let post = service
            .create_record(
                "posts",
                serde_json::json!({"title": "Post", "tags": [&t1_id, &t2_id]}),
            )
            .unwrap();
        let post_id = post["id"].as_str().unwrap().to_string();

        // Delete t1 — post should keep t2 but lose t1.
        service.delete_record("tags", &t1_id).unwrap();

        let updated_post = service.get_record("posts", &post_id).unwrap();
        let remaining = updated_post["tags"].as_array().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].as_str().unwrap(), t2_id);
    }

    // ── RESTRICT on delete ───────────────────────────────────────────────

    #[test]
    fn restrict_prevents_delete_when_referenced() {
        let service = make_relation_service(OnDeleteAction::Restrict);

        let author = service
            .create_record("authors", serde_json::json!({"name": "Alice"}))
            .unwrap();
        let author_id = author["id"].as_str().unwrap().to_string();

        let _post = service
            .create_record(
                "posts",
                serde_json::json!({"title": "Post 1", "author": &author_id}),
            )
            .unwrap();

        // Delete should fail because a post references this author.
        let err = service.delete_record("authors", &author_id).unwrap_err();
        assert_eq!(err.status_code(), 409);

        // Author should still exist.
        assert!(service.get_record("authors", &author_id).is_ok());
    }

    #[test]
    fn restrict_allows_delete_when_not_referenced() {
        let service = make_relation_service(OnDeleteAction::Restrict);

        let author = service
            .create_record("authors", serde_json::json!({"name": "Alice"}))
            .unwrap();
        let author_id = author["id"].as_str().unwrap().to_string();

        // No posts reference this author — delete should succeed.
        service.delete_record("authors", &author_id).unwrap();
    }

    // ── NO_ACTION on delete ──────────────────────────────────────────────

    #[test]
    fn no_action_leaves_dangling_references() {
        let service = make_relation_service(OnDeleteAction::NoAction);

        let author = service
            .create_record("authors", serde_json::json!({"name": "Alice"}))
            .unwrap();
        let author_id = author["id"].as_str().unwrap().to_string();

        let post = service
            .create_record(
                "posts",
                serde_json::json!({"title": "Post 1", "author": &author_id}),
            )
            .unwrap();
        let post_id = post["id"].as_str().unwrap().to_string();

        // Delete author — should succeed, and post still has old reference.
        service.delete_record("authors", &author_id).unwrap();

        let dangling_post = service.get_record("posts", &post_id).unwrap();
        assert_eq!(dangling_post["author"].as_str().unwrap(), author_id);
    }

    // ── OnDeleteAction unit tests ────────────────────────────────────────

    #[test]
    fn effective_on_delete_prefers_on_delete_field() {
        let opts = RelationOptions {
            collection_id: "x".to_string(),
            max_select: 1,
            on_delete: OnDeleteAction::Restrict,
            cascade_delete: true, // should be ignored
        };
        assert_eq!(opts.effective_on_delete(), OnDeleteAction::Restrict);
    }

    #[test]
    fn effective_on_delete_falls_back_to_cascade_delete_bool() {
        let opts = RelationOptions {
            collection_id: "x".to_string(),
            max_select: 1,
            on_delete: OnDeleteAction::NoAction,
            cascade_delete: true,
        };
        assert_eq!(opts.effective_on_delete(), OnDeleteAction::Cascade);
    }

    #[test]
    fn effective_on_delete_defaults_to_no_action() {
        let opts = RelationOptions {
            collection_id: "x".to_string(),
            max_select: 1,
            on_delete: OnDeleteAction::NoAction,
            cascade_delete: false,
        };
        assert_eq!(opts.effective_on_delete(), OnDeleteAction::NoAction);
    }

    #[test]
    fn extract_ids_from_string() {
        let val = serde_json::json!("abc123");
        assert_eq!(RelationOptions::extract_ids(&val), vec!["abc123"]);
    }

    #[test]
    fn extract_ids_from_array() {
        let val = serde_json::json!(["id1", "id2", "id3"]);
        assert_eq!(
            RelationOptions::extract_ids(&val),
            vec!["id1", "id2", "id3"]
        );
    }

    #[test]
    fn extract_ids_from_null_returns_empty() {
        let val = serde_json::json!(null);
        assert!(RelationOptions::extract_ids(&val).is_empty());
    }

    #[test]
    fn extract_ids_from_empty_string_returns_empty() {
        let val = serde_json::json!("");
        assert!(RelationOptions::extract_ids(&val).is_empty());
    }

    #[test]
    fn extract_ids_skips_empty_strings_in_array() {
        let val = serde_json::json!(["id1", "", "id2"]);
        assert_eq!(RelationOptions::extract_ids(&val), vec!["id1", "id2"]);
    }

    #[test]
    fn on_delete_action_serde_round_trip() {
        let actions = vec![
            OnDeleteAction::Cascade,
            OnDeleteAction::SetNull,
            OnDeleteAction::Restrict,
            OnDeleteAction::NoAction,
        ];
        for action in actions {
            let json = serde_json::to_value(action).unwrap();
            let deserialized: OnDeleteAction = serde_json::from_value(json.clone()).unwrap();
            assert_eq!(action, deserialized, "round-trip failed for {json}");
        }
    }

    #[test]
    fn on_delete_action_serializes_screaming_snake_case() {
        assert_eq!(
            serde_json::to_value(OnDeleteAction::Cascade).unwrap(),
            serde_json::json!("CASCADE")
        );
        assert_eq!(
            serde_json::to_value(OnDeleteAction::SetNull).unwrap(),
            serde_json::json!("SET_NULL")
        );
        assert_eq!(
            serde_json::to_value(OnDeleteAction::Restrict).unwrap(),
            serde_json::json!("RESTRICT")
        );
        assert_eq!(
            serde_json::to_value(OnDeleteAction::NoAction).unwrap(),
            serde_json::json!("NO_ACTION")
        );
    }

    // ── Relation modifier tests ──────────────────────────────────────────

    fn collection_with_multi_relation() -> Collection {
        Collection::base(
            "articles",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())).required(true),
                Field::new(
                    "tags",
                    FieldType::Relation(RelationOptions {
                        collection_id: "tags".to_string(),
                        max_select: 0, // unlimited
                        ..Default::default()
                    }),
                ),
                Field::new(
                    "authors",
                    FieldType::Relation(RelationOptions {
                        collection_id: "users".to_string(),
                        max_select: 5,
                        ..Default::default()
                    }),
                ),
            ],
        )
    }

    fn make_modifier_test_service() -> RecordService<MockRecordRepo, MockSchema> {
        let mut collections: HashMap<String, Collection> = HashMap::new();
        collections.insert("articles".to_string(), collection_with_multi_relation());
        // Add target collections so relation validation passes.
        collections.insert(
            "tags".to_string(),
            Collection::base(
                "tags",
                vec![Field::new("name", FieldType::Text(TextOptions::default()))],
            ),
        );
        collections.insert(
            "users".to_string(),
            Collection::base(
                "users",
                vec![Field::new("name", FieldType::Text(TextOptions::default()))],
            ),
        );

        let schema = MockSchema { collections };
        let repo = MockRecordRepo::new();

        // Seed some tag and user records so relation validation passes.
        {
            let mut store = repo.records.lock().unwrap();
            store.insert(
                "tags".to_string(),
                vec![
                    HashMap::from([("id".into(), json!("tag1"))]),
                    HashMap::from([("id".into(), json!("tag2"))]),
                    HashMap::from([("id".into(), json!("tag3"))]),
                    HashMap::from([("id".into(), json!("tag4"))]),
                ],
            );
            store.insert(
                "users".to_string(),
                vec![
                    HashMap::from([("id".into(), json!("user1"))]),
                    HashMap::from([("id".into(), json!("user2"))]),
                    HashMap::from([("id".into(), json!("user3"))]),
                ],
            );
        }

        RecordService::new(repo, schema)
    }

    #[test]
    fn modifier_add_single_id_to_empty_relation() {
        let service = make_modifier_test_service();
        let data = json!({ "title": "Test", "tags": [] });
        let created = service.create_record("articles", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let updated = service
            .update_record("articles", id, json!({ "tags+": "tag1" }))
            .unwrap();

        assert_eq!(updated["tags"], json!(["tag1"]));
    }

    #[test]
    fn modifier_add_multiple_ids() {
        let service = make_modifier_test_service();
        let data = json!({ "title": "Test", "tags": ["tag1"] });
        let created = service.create_record("articles", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let updated = service
            .update_record("articles", id, json!({ "tags+": ["tag2", "tag3"] }))
            .unwrap();

        let tags = updated["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 3);
        assert!(tags.contains(&json!("tag1")));
        assert!(tags.contains(&json!("tag2")));
        assert!(tags.contains(&json!("tag3")));
    }

    #[test]
    fn modifier_add_deduplicates() {
        let service = make_modifier_test_service();
        let data = json!({ "title": "Test", "tags": ["tag1", "tag2"] });
        let created = service.create_record("articles", data).unwrap();
        let id = created["id"].as_str().unwrap();

        // tag1 already exists — should not be duplicated.
        let updated = service
            .update_record("articles", id, json!({ "tags+": ["tag1", "tag3"] }))
            .unwrap();

        let tags = updated["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 3);
        assert_eq!(
            tags.iter().filter(|v| v == &&json!("tag1")).count(),
            1,
            "tag1 should not be duplicated"
        );
    }

    #[test]
    fn modifier_remove_single_id() {
        let service = make_modifier_test_service();
        let data = json!({ "title": "Test", "tags": ["tag1", "tag2", "tag3"] });
        let created = service.create_record("articles", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let updated = service
            .update_record("articles", id, json!({ "tags-": "tag2" }))
            .unwrap();

        assert_eq!(updated["tags"], json!(["tag1", "tag3"]));
    }

    #[test]
    fn modifier_remove_multiple_ids() {
        let service = make_modifier_test_service();
        let data = json!({ "title": "Test", "tags": ["tag1", "tag2", "tag3"] });
        let created = service.create_record("articles", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let updated = service
            .update_record("articles", id, json!({ "tags-": ["tag1", "tag3"] }))
            .unwrap();

        assert_eq!(updated["tags"], json!(["tag2"]));
    }

    #[test]
    fn modifier_remove_nonexistent_id_is_noop() {
        let service = make_modifier_test_service();
        let data = json!({ "title": "Test", "tags": ["tag1", "tag2"] });
        let created = service.create_record("articles", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let updated = service
            .update_record("articles", id, json!({ "tags-": "nonexistent" }))
            .unwrap();

        assert_eq!(updated["tags"], json!(["tag1", "tag2"]));
    }

    #[test]
    fn modifier_add_and_remove_in_same_update() {
        let service = make_modifier_test_service();
        let data = json!({ "title": "Test", "tags": ["tag1", "tag2"] });
        let created = service.create_record("articles", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let updated = service
            .update_record("articles", id, json!({ "tags+": "tag3", "tags-": "tag1" }))
            .unwrap();

        let tags = updated["tags"].as_array().unwrap();
        assert!(tags.contains(&json!("tag2")));
        assert!(tags.contains(&json!("tag3")));
        assert!(!tags.contains(&json!("tag1")));
    }

    #[test]
    fn modifier_add_to_null_field() {
        let service = make_modifier_test_service();
        // Create without tags field — it will be null/absent.
        let data = json!({ "title": "Test" });
        let created = service.create_record("articles", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let updated = service
            .update_record("articles", id, json!({ "tags+": "tag1" }))
            .unwrap();

        assert_eq!(updated["tags"], json!(["tag1"]));
    }

    #[test]
    fn modifier_remove_all_leaves_empty_array() {
        let service = make_modifier_test_service();
        let data = json!({ "title": "Test", "tags": ["tag1"] });
        let created = service.create_record("articles", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let updated = service
            .update_record("articles", id, json!({ "tags-": "tag1" }))
            .unwrap();

        assert_eq!(updated["tags"], json!([]));
    }

    #[test]
    fn modifier_on_unknown_field_returns_error() {
        let service = make_modifier_test_service();
        let data = json!({ "title": "Test" });
        let created = service.create_record("articles", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let err = service
            .update_record("articles", id, json!({ "nonexistent+": "id1" }))
            .unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn modifier_on_non_relation_field_returns_error() {
        let service = make_modifier_test_service();
        let data = json!({ "title": "Test" });
        let created = service.create_record("articles", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let err = service
            .update_record("articles", id, json!({ "title+": "extra" }))
            .unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn modifier_preserves_other_fields() {
        let service = make_modifier_test_service();
        let data = json!({ "title": "Original", "tags": ["tag1"] });
        let created = service.create_record("articles", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let updated = service
            .update_record("articles", id, json!({ "tags+": "tag2" }))
            .unwrap();

        assert_eq!(updated["title"], "Original");
        assert_eq!(updated["tags"], json!(["tag1", "tag2"]));
    }

    #[test]
    fn modifier_with_plain_field_update() {
        let service = make_modifier_test_service();
        let data = json!({ "title": "Original", "tags": ["tag1"] });
        let created = service.create_record("articles", data).unwrap();
        let id = created["id"].as_str().unwrap();

        let updated = service
            .update_record(
                "articles",
                id,
                json!({ "title": "Updated", "tags+": "tag2" }),
            )
            .unwrap();

        assert_eq!(updated["title"], "Updated");
        assert_eq!(updated["tags"], json!(["tag1", "tag2"]));
    }

    #[test]
    fn modifier_exceeding_max_select_returns_error() {
        let service = make_modifier_test_service();
        // authors has max_select=5
        let data = json!({
            "title": "Test",
            "authors": ["user1", "user2", "user3"]
        });
        let created = service.create_record("articles", data).unwrap();
        let id = created["id"].as_str().unwrap();

        // Seed extra user records for the test.
        {
            let mut store = service.repo.records.lock().unwrap();
            let users = store.get_mut("users").unwrap();
            users.push(HashMap::from([("id".into(), json!("user4"))]));
            users.push(HashMap::from([("id".into(), json!("user5"))]));
            users.push(HashMap::from([("id".into(), json!("user6"))]));
        }

        // Try to add 3 more (would be 6, exceeding max_select=5).
        let err = service
            .update_record(
                "articles",
                id,
                json!({ "authors+": ["user4", "user5", "user6"] }),
            )
            .unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    // ── apply_relation_modifiers unit tests ──────────────────────────────

    #[test]
    fn apply_modifiers_no_modifier_keys_returns_empty() {
        let collection = collection_with_multi_relation();
        let update_obj = json!({ "title": "Hello" });
        let existing = HashMap::new();

        let result =
            apply_relation_modifiers(update_obj.as_object().unwrap(), &existing, &collection)
                .unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn apply_modifiers_add_to_existing_array() {
        let collection = collection_with_multi_relation();
        let update_obj = json!({ "tags+": "tag2" });
        let existing = HashMap::from([("tags".to_string(), json!(["tag1"]))]);

        let result =
            apply_relation_modifiers(update_obj.as_object().unwrap(), &existing, &collection)
                .unwrap();

        assert_eq!(result["tags"], json!(["tag1", "tag2"]));
    }

    #[test]
    fn apply_modifiers_remove_from_array() {
        let collection = collection_with_multi_relation();
        let update_obj = json!({ "tags-": "tag1" });
        let existing = HashMap::from([("tags".to_string(), json!(["tag1", "tag2"]))]);

        let result =
            apply_relation_modifiers(update_obj.as_object().unwrap(), &existing, &collection)
                .unwrap();

        assert_eq!(result["tags"], json!(["tag2"]));
    }

    #[test]
    fn apply_modifiers_invalid_value_type_returns_error() {
        let collection = collection_with_multi_relation();
        let update_obj = json!({ "tags+": 123 });
        let existing = HashMap::new();

        let err = apply_relation_modifiers(update_obj.as_object().unwrap(), &existing, &collection)
            .unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn apply_modifiers_empty_string_returns_error() {
        let collection = collection_with_multi_relation();
        let update_obj = json!({ "tags+": "" });
        let existing = HashMap::new();

        let err = apply_relation_modifiers(update_obj.as_object().unwrap(), &existing, &collection)
            .unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    // ── View collection write-guard tests ─────────────────────────────────

    fn make_view_service() -> RecordService<MockRecordRepo, MockSchema> {
        let view = Collection::view("posts_view", "SELECT id, title FROM posts");
        RecordService::new(
            MockRecordRepo::new(),
            MockSchema::with_collection("posts_view", view),
        )
    }

    #[test]
    fn view_collection_rejects_create() {
        let service = make_view_service();
        let data = json!({ "title": "Hello" });
        let err = service.create_record("posts_view", data).unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(
            format!("{err}").contains("read-only"),
            "error should mention read-only: {err}"
        );
    }

    #[test]
    fn view_collection_rejects_update() {
        let service = make_view_service();
        let data = json!({ "title": "Updated" });
        let err = service
            .update_record("posts_view", "some-id", data)
            .unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(
            format!("{err}").contains("read-only"),
            "error should mention read-only: {err}"
        );
    }

    #[test]
    fn view_collection_rejects_delete() {
        let service = make_view_service();
        let err = service
            .delete_record("posts_view", "some-id")
            .unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(
            format!("{err}").contains("read-only"),
            "error should mention read-only: {err}"
        );
    }
}
