//! Collection CRUD service.
//!
//! [`CollectionService`] is the primary interface for managing collections.
//! It validates domain-level `Collection` objects, converts them to/from
//! the DB layer's `CollectionSchema`, and delegates persistence to a
//! [`SchemaRepository`] implementor.
//!
//! # Design
//!
//! - **Input validation** happens at this layer (via `Collection::validate()`).
//! - **Conflict detection** and DDL execution are delegated to the repository.
//! - The service is generic over `R: SchemaRepository` for easy testing with mocks.

use crate::error::{Result, ZerobaseError};
use crate::schema::{
    is_system_collection, AuthOptions, BoolOptions, Collection, CollectionType, Field, FieldType,
    IndexSortDirection, IndexSpec, NumberOptions, TextOptions, AUTH_SYSTEM_FIELDS,
    BASE_SYSTEM_FIELDS,
};
use crate::services::record_service::SchemaLookup;

// ── Repository trait (re-exported from DB) ────────────────────────────────────

/// Minimal schema-persistence contract used by the service.
///
/// This is intentionally defined here (in core) so the service doesn't depend
/// on `zerobase-db` directly. The DB crate implements this trait on `Database`.
pub trait SchemaRepository: Send + Sync {
    /// List all collection schemas.
    fn list_collections(&self) -> std::result::Result<Vec<CollectionSchemaDto>, SchemaRepoError>;

    /// Get a single collection schema by name.
    fn get_collection(
        &self,
        name: &str,
    ) -> std::result::Result<CollectionSchemaDto, SchemaRepoError>;

    /// Create a new collection (metadata + DDL).
    fn create_collection(
        &self,
        schema: &CollectionSchemaDto,
    ) -> std::result::Result<(), SchemaRepoError>;

    /// Update an existing collection (metadata + DDL).
    fn update_collection(
        &self,
        name: &str,
        schema: &CollectionSchemaDto,
    ) -> std::result::Result<(), SchemaRepoError>;

    /// Delete a collection (metadata + DDL).
    fn delete_collection(&self, name: &str) -> std::result::Result<(), SchemaRepoError>;

    /// Check whether a collection exists by name.
    fn collection_exists(&self, name: &str) -> std::result::Result<bool, SchemaRepoError>;
}

/// Lightweight DTO for schema persistence — mirrors `zerobase_db::CollectionSchema`.
#[derive(Debug, Clone)]
pub struct CollectionSchemaDto {
    pub name: String,
    pub collection_type: String,
    pub columns: Vec<ColumnDto>,
    pub indexes: Vec<IndexDto>,
    /// Field names that should be included in full-text search indexes.
    pub searchable_fields: Vec<String>,
    /// For View collections: the SQL query that defines the view.
    pub view_query: Option<String>,
}

/// A column definition DTO.
#[derive(Debug, Clone)]
pub struct ColumnDto {
    pub name: String,
    pub sql_type: String,
    pub not_null: bool,
    pub default: Option<String>,
    pub unique: bool,
}

/// Sort direction for an index column in the DTO layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexColumnSortDto {
    Asc,
    Desc,
}

/// A column with sort direction in an index DTO.
#[derive(Debug, Clone)]
pub struct IndexColumnDto {
    pub name: String,
    pub sort: IndexColumnSortDto,
}

/// An index definition DTO.
#[derive(Debug, Clone)]
pub struct IndexDto {
    pub name: String,
    pub columns: Vec<String>,
    /// Rich column definitions with sort directions.
    /// If non-empty, takes precedence over `columns` for DDL.
    pub index_columns: Vec<IndexColumnDto>,
    pub unique: bool,
}

/// Errors that a schema repository can produce.
#[derive(Debug, thiserror::Error)]
pub enum SchemaRepoError {
    #[error("{resource_type} not found")]
    NotFound {
        resource_type: String,
        resource_id: Option<String>,
    },
    #[error("conflict: {message}")]
    Conflict { message: String },
    #[error("schema error: {message}")]
    Schema { message: String },
    #[error("database error: {message}")]
    Database { message: String },
}

impl From<SchemaRepoError> for ZerobaseError {
    fn from(err: SchemaRepoError) -> Self {
        match err {
            SchemaRepoError::NotFound {
                resource_type,
                resource_id,
            } => match resource_id {
                Some(id) => ZerobaseError::not_found_with_id(resource_type, id),
                None => ZerobaseError::not_found(resource_type),
            },
            SchemaRepoError::Conflict { message } => ZerobaseError::conflict(message),
            SchemaRepoError::Schema { message } => {
                ZerobaseError::database(format!("schema: {message}"))
            }
            SchemaRepoError::Database { message } => ZerobaseError::database(message),
        }
    }
}

// ── CollectionService ─────────────────────────────────────────────────────────

/// Service for CRUD operations on collections.
///
/// Generic over `R: SchemaRepository` so tests can inject mocks.
pub struct CollectionService<R: SchemaRepository> {
    repo: R,
}

impl<R: SchemaRepository> CollectionService<R> {
    /// Create a new service wrapping the given repository.
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    /// Create a new collection.
    ///
    /// Validates the domain model, converts to the DB schema DTO,
    /// and delegates persistence.
    pub fn create_collection(&self, collection: &Collection) -> Result<Collection> {
        // 1. Domain validation.
        collection.validate()?;

        // 2. Convert to DB schema.
        let schema_dto = collection_to_schema(collection);

        // 3. Persist.
        self.repo.create_collection(&schema_dto)?;

        // Return the collection as-is (ID was already assigned).
        Ok(collection.clone())
    }

    /// Get a collection by name.
    ///
    /// Returns the full domain-level `Collection` reconstructed from metadata.
    pub fn get_collection(&self, name: &str) -> Result<Collection> {
        let dto = self.repo.get_collection(name)?;
        Ok(schema_to_collection(&dto))
    }

    /// List all collections.
    pub fn list_collections(&self) -> Result<Vec<Collection>> {
        let dtos = self.repo.list_collections()?;
        Ok(dtos.iter().map(schema_to_collection).collect())
    }

    /// Update an existing collection.
    ///
    /// Validates the new definition, then applies changes.
    /// System collections (names starting with `_`) cannot be renamed,
    /// and their system fields cannot be modified.
    pub fn update_collection(&self, name: &str, collection: &Collection) -> Result<Collection> {
        // Prevent renaming system collections.
        if is_system_collection(name) && collection.name != name {
            return Err(ZerobaseError::forbidden(format!(
                "system collection '{}' cannot be renamed",
                name
            )));
        }

        // Prevent modifying system fields in system collections.
        if is_system_collection(name) {
            validate_no_system_field_changes(name, collection)?;
        }

        // Validate the new definition.
        // For system collections, skip the underscore-prefix check since
        // `validate_name` rejects names starting with `_`. Instead, we
        // only validate fields and type-specific constraints.
        if is_system_collection(name) {
            collection.validate_fields_and_type()?;
        } else {
            collection.validate()?;
        }

        let schema_dto = collection_to_schema(collection);
        self.repo.update_collection(name, &schema_dto)?;

        Ok(collection.clone())
    }

    /// Delete a collection by name.
    ///
    /// System collections (names starting with `_`) cannot be deleted.
    pub fn delete_collection(&self, name: &str) -> Result<()> {
        if is_system_collection(name) {
            return Err(ZerobaseError::forbidden(format!(
                "system collection '{}' cannot be deleted",
                name
            )));
        }

        self.repo.delete_collection(name)?;
        Ok(())
    }

    /// Check if a collection exists.
    pub fn collection_exists(&self, name: &str) -> Result<bool> {
        Ok(self.repo.collection_exists(name)?)
    }

    /// Export all non-system collections as a list of `Collection` objects.
    ///
    /// System collections (names starting with `_`) are excluded from export
    /// since they are managed internally by the system.
    pub fn export_collections(&self) -> Result<Vec<Collection>> {
        let all = self.list_collections()?;
        Ok(all
            .into_iter()
            .filter(|c| !is_system_collection(&c.name))
            .collect())
    }

    /// Import a list of collections, creating new ones or updating existing ones.
    ///
    /// Each collection in the input is validated before being applied.
    /// If a collection with the same name already exists, it is updated;
    /// otherwise, a new collection is created.
    ///
    /// System collections (names starting with `_`) are rejected.
    ///
    /// Returns the list of collections that were successfully imported
    /// (with their final state). If any collection fails validation,
    /// the entire import is aborted and an error is returned.
    pub fn import_collections(&self, collections: &[Collection]) -> Result<Vec<Collection>> {
        // Phase 1: Validate all collections upfront before applying any changes.
        for collection in collections {
            if is_system_collection(&collection.name) {
                return Err(ZerobaseError::validation(format!(
                    "cannot import system collection '{}'",
                    collection.name
                )));
            }
            collection.validate()?;
        }

        // Phase 2: Apply — create or update each collection.
        let mut results = Vec::with_capacity(collections.len());
        for collection in collections {
            let exists = self.repo.collection_exists(&collection.name)?;

            let result = if exists {
                let schema_dto = collection_to_schema(collection);
                self.repo.update_collection(&collection.name, &schema_dto)?;
                collection.clone()
            } else {
                let mut c = collection.clone();
                if c.id.is_empty() {
                    c.id = uuid::Uuid::new_v4().to_string().replace('-', "")[..15].to_string();
                }
                let schema_dto = collection_to_schema(&c);
                self.repo.create_collection(&schema_dto)?;
                c
            };

            results.push(result);
        }

        Ok(results)
    }
}

impl<R: SchemaRepository> SchemaLookup for CollectionService<R> {
    fn get_collection(&self, name: &str) -> Result<Collection> {
        self.get_collection(name)
    }

    fn get_collection_by_id(&self, id: &str) -> Result<Collection> {
        self.list_collections()?
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| ZerobaseError::not_found_with_id("Collection", id))
    }

    fn list_all_collections(&self) -> Result<Vec<Collection>> {
        self.list_collections()
    }
}

// ── System collection protection ──────────────────────────────────────────────

/// Check that the proposed collection update does not include user-defined fields
/// that collide with system field names.
///
/// System fields (id, created, updated, and auth-specific fields) are implicit
/// and must not appear in the user-defined field list.
fn validate_no_system_field_changes(name: &str, collection: &Collection) -> Result<()> {
    let system_fields: Vec<&str> = BASE_SYSTEM_FIELDS.to_vec();

    // Check if any user-defined field tries to redefine a system field.
    for field in &collection.fields {
        if system_fields.contains(&field.name.as_str()) {
            return Err(ZerobaseError::forbidden(format!(
                "cannot modify system field '{}' on system collection '{}'",
                field.name, name
            )));
        }

        // Also check auth system fields for auth-type system collections.
        if collection.collection_type == CollectionType::Auth
            && AUTH_SYSTEM_FIELDS.contains(&field.name.as_str())
        {
            return Err(ZerobaseError::forbidden(format!(
                "cannot modify system field '{}' on system collection '{}'",
                field.name, name
            )));
        }
    }

    Ok(())
}

// ── Domain ↔ Schema conversion ────────────────────────────────────────────────

/// Convert a domain `Collection` to a persistence DTO.
fn collection_to_schema(c: &Collection) -> CollectionSchemaDto {
    let columns: Vec<ColumnDto> = c
        .fields
        .iter()
        .map(|f| ColumnDto {
            name: f.name.clone(),
            sql_type: f.sql_type().to_string(),
            not_null: f.required,
            default: None,
            unique: f.unique,
        })
        .collect();

    let indexes: Vec<IndexDto> = c
        .indexes
        .iter()
        .map(|idx| {
            let col_names: Vec<String> = idx
                .effective_column_names()
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            let index_columns: Vec<IndexColumnDto> = idx
                .effective_index_columns()
                .into_iter()
                .map(|ic| IndexColumnDto {
                    name: ic.name,
                    sort: match ic.direction {
                        IndexSortDirection::Asc => IndexColumnSortDto::Asc,
                        IndexSortDirection::Desc => IndexColumnSortDto::Desc,
                    },
                })
                .collect();
            IndexDto {
                name: idx.generate_name(&c.name),
                columns: col_names,
                index_columns,
                unique: idx.unique,
            }
        })
        .collect();

    // Auto-generate indexes for fields referenced in access rules.
    let mut indexes = indexes;
    let rule_fields = c.rules.referenced_fields();
    let field_names: std::collections::HashSet<&str> =
        c.fields.iter().map(|f| f.name.as_str()).collect();
    let existing_indexed: std::collections::HashSet<String> =
        indexes.iter().flat_map(|idx| idx.columns.clone()).collect();

    for field_name in &rule_fields {
        // Only auto-index if the field exists in the collection and isn't already indexed.
        if field_names.contains(field_name.as_str()) && !existing_indexed.contains(field_name) {
            indexes.push(IndexDto {
                name: format!("idx_{}_{}", c.name, field_name),
                columns: vec![field_name.clone()],
                index_columns: vec![],
                unique: false,
            });
        }
    }

    let searchable_fields: Vec<String> = c
        .searchable_field_names()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    CollectionSchemaDto {
        name: c.name.clone(),
        collection_type: c.collection_type.as_str().to_string(),
        columns,
        indexes,
        searchable_fields,
        view_query: c.view_query.clone(),
    }
}

/// Reconstruct a domain `Collection` from a persistence DTO.
///
/// This produces a best-effort reconstruction. Field types default to `Text`
/// because the DTO only carries SQL types, not the richer domain type info.
/// In a full implementation, the service would also read `_fields.options`
/// to restore the exact `FieldType`.
fn schema_to_collection(dto: &CollectionSchemaDto) -> Collection {
    let collection_type = dto
        .collection_type
        .parse::<CollectionType>()
        .unwrap_or(CollectionType::Base);

    let fields: Vec<Field> = dto
        .columns
        .iter()
        .map(|col| {
            let field_type = sql_type_to_field_type(&col.sql_type);
            let mut field = Field::new(&col.name, field_type);
            field.required = col.not_null;
            field.unique = col.unique;
            field
        })
        .collect();

    let indexes: Vec<IndexSpec> = dto
        .indexes
        .iter()
        .map(|idx| {
            if idx.index_columns.is_empty() {
                if idx.unique {
                    IndexSpec::unique(idx.columns.clone())
                } else {
                    IndexSpec::new(idx.columns.clone())
                }
            } else {
                use crate::schema::IndexColumn;
                let cols: Vec<IndexColumn> = idx
                    .index_columns
                    .iter()
                    .map(|ic| match ic.sort {
                        IndexColumnSortDto::Desc => IndexColumn::desc(&ic.name),
                        IndexColumnSortDto::Asc => IndexColumn::asc(&ic.name),
                    })
                    .collect();
                IndexSpec::with_columns(cols, idx.unique)
            }
        })
        .collect();

    let auth_options = if collection_type == CollectionType::Auth {
        Some(AuthOptions::default())
    } else {
        None
    };

    Collection {
        id: String::new(), // Will be populated when reading from full metadata.
        name: dto.name.clone(),
        collection_type,
        fields,
        rules: Default::default(),
        indexes,
        view_query: dto.view_query.clone(),
        auth_options,
    }
}

/// Map a SQL type string back to a default `FieldType`.
fn sql_type_to_field_type(sql_type: &str) -> FieldType {
    match sql_type.to_uppercase().as_str() {
        "REAL" => FieldType::Number(NumberOptions::default()),
        "INTEGER" => FieldType::Bool(BoolOptions::default()),
        _ => FieldType::Text(TextOptions::default()),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{BoolOptions, NumberOptions, SelectOptions, TextOptions};
    use std::collections::HashMap;
    use std::sync::RwLock;

    // ── Mock repository ───────────────────────────────────────────────────

    /// A simple in-memory mock of SchemaRepository for unit testing.
    struct MockRepo {
        collections: RwLock<HashMap<String, CollectionSchemaDto>>,
    }

    impl MockRepo {
        fn new() -> Self {
            Self {
                collections: RwLock::new(HashMap::new()),
            }
        }
    }

    impl SchemaRepository for MockRepo {
        fn list_collections(
            &self,
        ) -> std::result::Result<Vec<CollectionSchemaDto>, SchemaRepoError> {
            let map = self.collections.read().unwrap();
            let mut list: Vec<CollectionSchemaDto> = map.values().cloned().collect();
            list.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(list)
        }

        fn get_collection(
            &self,
            name: &str,
        ) -> std::result::Result<CollectionSchemaDto, SchemaRepoError> {
            self.collections
                .read()
                .unwrap()
                .get(name)
                .cloned()
                .ok_or(SchemaRepoError::NotFound {
                    resource_type: "Collection".to_string(),
                    resource_id: Some(name.to_string()),
                })
        }

        fn create_collection(
            &self,
            schema: &CollectionSchemaDto,
        ) -> std::result::Result<(), SchemaRepoError> {
            let mut map = self.collections.write().unwrap();
            if map.contains_key(&schema.name) {
                return Err(SchemaRepoError::Conflict {
                    message: format!("collection '{}' already exists", schema.name),
                });
            }
            map.insert(schema.name.clone(), schema.clone());
            Ok(())
        }

        fn update_collection(
            &self,
            name: &str,
            schema: &CollectionSchemaDto,
        ) -> std::result::Result<(), SchemaRepoError> {
            let mut map = self.collections.write().unwrap();
            if !map.contains_key(name) {
                return Err(SchemaRepoError::NotFound {
                    resource_type: "Collection".to_string(),
                    resource_id: Some(name.to_string()),
                });
            }
            // If renaming, check new name isn't taken.
            if schema.name != name && map.contains_key(&schema.name) {
                return Err(SchemaRepoError::Conflict {
                    message: format!("collection '{}' already exists", schema.name),
                });
            }
            map.remove(name);
            map.insert(schema.name.clone(), schema.clone());
            Ok(())
        }

        fn delete_collection(&self, name: &str) -> std::result::Result<(), SchemaRepoError> {
            let mut map = self.collections.write().unwrap();
            if map.remove(name).is_none() {
                return Err(SchemaRepoError::NotFound {
                    resource_type: "Collection".to_string(),
                    resource_id: Some(name.to_string()),
                });
            }
            Ok(())
        }

        fn collection_exists(&self, name: &str) -> std::result::Result<bool, SchemaRepoError> {
            Ok(self.collections.read().unwrap().contains_key(name))
        }
    }

    fn make_service() -> CollectionService<MockRepo> {
        CollectionService::new(MockRepo::new())
    }

    // ── create_collection ─────────────────────────────────────────────────

    #[test]
    fn create_valid_base_collection() {
        let svc = make_service();
        let c = Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())).required(true),
                Field::new("body", FieldType::Text(TextOptions::default())),
            ],
        );

        let result = svc.create_collection(&c);
        assert!(result.is_ok());
        let created = result.unwrap();
        assert_eq!(created.name, "posts");
        assert_eq!(created.fields.len(), 2);
    }

    #[test]
    fn create_collection_rejects_invalid_name() {
        let svc = make_service();
        let c = Collection::base(
            "1invalid",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );

        let result = svc.create_collection(&c);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn create_collection_rejects_reserved_name() {
        let svc = make_service();
        let c = Collection::base("id", vec![]);
        let result = svc.create_collection(&c);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn create_collection_rejects_duplicate_fields() {
        let svc = make_service();
        let c = Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())),
                Field::new("title", FieldType::Text(TextOptions::default())),
            ],
        );

        let result = svc.create_collection(&c);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn create_collection_rejects_reserved_field_name() {
        let svc = make_service();
        let c = Collection::base(
            "posts",
            vec![Field::new("id", FieldType::Text(TextOptions::default()))],
        );

        let result = svc.create_collection(&c);
        assert!(result.is_err());
    }

    #[test]
    fn create_duplicate_collection_returns_conflict() {
        let svc = make_service();
        let c = Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );

        svc.create_collection(&c).unwrap();
        let result = svc.create_collection(&c);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 409);
    }

    #[test]
    fn create_auth_collection() {
        let svc = make_service();
        let c = Collection::auth(
            "users",
            vec![Field::new("name", FieldType::Text(TextOptions::default()))],
        );

        let result = svc.create_collection(&c);
        assert!(result.is_ok());
    }

    #[test]
    fn create_auth_collection_without_options_fails() {
        let svc = make_service();
        let mut c = Collection::auth("users", vec![]);
        c.auth_options = None;

        let result = svc.create_collection(&c);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn create_view_collection() {
        let svc = make_service();
        let c = Collection::view("stats", "SELECT 1");
        let result = svc.create_collection(&c);
        assert!(result.is_ok());
    }

    #[test]
    fn create_view_collection_without_query_fails() {
        let svc = make_service();
        let mut c = Collection::view("stats", "SELECT 1");
        c.view_query = None;

        let result = svc.create_collection(&c);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn create_collection_with_various_field_types() {
        let svc = make_service();
        let c = Collection::base(
            "products",
            vec![
                Field::new(
                    "name",
                    FieldType::Text(TextOptions {
                        min_length: 1,
                        max_length: 200,
                        pattern: None,
                        searchable: false,
                    }),
                )
                .required(true),
                Field::new(
                    "price",
                    FieldType::Number(NumberOptions {
                        min: Some(0.0),
                        max: None,
                        only_int: false,
                    }),
                ),
                Field::new("active", FieldType::Bool(BoolOptions::default())),
                Field::new(
                    "category",
                    FieldType::Select(SelectOptions {
                        values: vec!["electronics".into(), "clothing".into()],
                    }),
                ),
            ],
        );

        let result = svc.create_collection(&c);
        assert!(result.is_ok());
    }

    #[test]
    fn create_collection_with_index() {
        let svc = make_service();
        let mut c = Collection::base(
            "articles",
            vec![
                Field::new("slug", FieldType::Text(TextOptions::default())).unique(true),
                Field::new("title", FieldType::Text(TextOptions::default())),
            ],
        );
        c.indexes = vec![IndexSpec::unique(vec!["slug".to_string()])];

        let result = svc.create_collection(&c);
        assert!(result.is_ok());
    }

    #[test]
    fn create_collection_rejects_index_with_unknown_column() {
        let svc = make_service();
        let mut c = Collection::base(
            "articles",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        c.indexes = vec![IndexSpec::new(vec!["nonexistent".to_string()])];

        let result = svc.create_collection(&c);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    // ── get_collection ────────────────────────────────────────────────────

    #[test]
    fn get_existing_collection() {
        let svc = make_service();
        let c = Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default())).required(true)],
        );
        svc.create_collection(&c).unwrap();

        let retrieved = svc.get_collection("posts").unwrap();
        assert_eq!(retrieved.name, "posts");
        assert_eq!(retrieved.collection_type, CollectionType::Base);
        assert_eq!(retrieved.fields.len(), 1);
        assert_eq!(retrieved.fields[0].name, "title");
        assert!(retrieved.fields[0].required);
    }

    #[test]
    fn get_nonexistent_collection_returns_not_found() {
        let svc = make_service();
        let result = svc.get_collection("nonexistent");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 404);
    }

    // ── list_collections ──────────────────────────────────────────────────

    #[test]
    fn list_empty_collections() {
        let svc = make_service();
        let result = svc.list_collections().unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn list_multiple_collections() {
        let svc = make_service();
        svc.create_collection(&Collection::base(
            "alpha",
            vec![Field::new("x", FieldType::Text(TextOptions::default()))],
        ))
        .unwrap();
        svc.create_collection(&Collection::base(
            "beta",
            vec![Field::new("y", FieldType::Text(TextOptions::default()))],
        ))
        .unwrap();
        svc.create_collection(&Collection::base(
            "gamma",
            vec![Field::new("z", FieldType::Text(TextOptions::default()))],
        ))
        .unwrap();

        let list = svc.list_collections().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "alpha");
        assert_eq!(list[1].name, "beta");
        assert_eq!(list[2].name, "gamma");
    }

    // ── update_collection ─────────────────────────────────────────────────

    #[test]
    fn update_collection_changes_fields() {
        let svc = make_service();
        let c = Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        svc.create_collection(&c).unwrap();

        let updated = Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())),
                Field::new("body", FieldType::Text(TextOptions::default())),
            ],
        );
        let result = svc.update_collection("posts", &updated);
        assert!(result.is_ok());

        let retrieved = svc.get_collection("posts").unwrap();
        assert_eq!(retrieved.fields.len(), 2);
    }

    #[test]
    fn update_collection_validates_new_definition() {
        let svc = make_service();
        let c = Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        svc.create_collection(&c).unwrap();

        // Invalid: duplicate field names.
        let invalid = Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())),
                Field::new("title", FieldType::Text(TextOptions::default())),
            ],
        );
        let result = svc.update_collection("posts", &invalid);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn update_nonexistent_collection_fails() {
        let svc = make_service();
        let c = Collection::base(
            "ghost",
            vec![Field::new("x", FieldType::Text(TextOptions::default()))],
        );
        let result = svc.update_collection("ghost", &c);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 404);
    }

    #[test]
    fn update_collection_rename() {
        let svc = make_service();
        let c = Collection::base(
            "old_name",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        svc.create_collection(&c).unwrap();

        let renamed = Collection::base(
            "new_name",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        svc.update_collection("old_name", &renamed).unwrap();

        assert!(!svc.collection_exists("old_name").unwrap());
        assert!(svc.collection_exists("new_name").unwrap());
    }

    // ── delete_collection ─────────────────────────────────────────────────

    #[test]
    fn delete_existing_collection() {
        let svc = make_service();
        let c = Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        svc.create_collection(&c).unwrap();

        svc.delete_collection("posts").unwrap();
        assert!(!svc.collection_exists("posts").unwrap());
    }

    #[test]
    fn delete_nonexistent_collection_fails() {
        let svc = make_service();
        let result = svc.delete_collection("nonexistent");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 404);
    }

    // ── collection_exists ─────────────────────────────────────────────────

    #[test]
    fn collection_exists_reflects_state() {
        let svc = make_service();
        assert!(!svc.collection_exists("posts").unwrap());

        svc.create_collection(&Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        ))
        .unwrap();
        assert!(svc.collection_exists("posts").unwrap());

        svc.delete_collection("posts").unwrap();
        assert!(!svc.collection_exists("posts").unwrap());
    }

    // ── Conversion helpers ────────────────────────────────────────────────

    #[test]
    fn collection_to_schema_maps_fields_correctly() {
        let c = Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())).required(true),
                Field::new("views", FieldType::Number(NumberOptions::default())),
                Field::new("active", FieldType::Bool(BoolOptions::default())).unique(true),
            ],
        );

        let dto = collection_to_schema(&c);
        assert_eq!(dto.name, "posts");
        assert_eq!(dto.collection_type, "base");
        assert_eq!(dto.columns.len(), 3);

        assert_eq!(dto.columns[0].name, "title");
        assert_eq!(dto.columns[0].sql_type, "TEXT");
        assert!(dto.columns[0].not_null);

        assert_eq!(dto.columns[1].name, "views");
        assert_eq!(dto.columns[1].sql_type, "REAL");
        assert!(!dto.columns[1].not_null);

        assert_eq!(dto.columns[2].name, "active");
        assert_eq!(dto.columns[2].sql_type, "INTEGER");
        assert!(dto.columns[2].unique);
    }

    #[test]
    fn collection_to_schema_generates_index_names() {
        let mut c = Collection::base(
            "articles",
            vec![
                Field::new("slug", FieldType::Text(TextOptions::default())),
                Field::new("title", FieldType::Text(TextOptions::default())),
            ],
        );
        c.indexes = vec![
            IndexSpec::unique(vec!["slug".to_string()]),
            IndexSpec::new(vec!["slug".to_string(), "title".to_string()]),
        ];

        let dto = collection_to_schema(&c);
        assert_eq!(dto.indexes[0].name, "idx_articles_slug");
        assert!(dto.indexes[0].unique);
        assert_eq!(dto.indexes[1].name, "idx_articles_slug_title");
        assert!(!dto.indexes[1].unique);
    }

    #[test]
    fn schema_to_collection_round_trip() {
        let dto = CollectionSchemaDto {
            name: "posts".to_string(),
            collection_type: "base".to_string(),
            columns: vec![
                ColumnDto {
                    name: "title".to_string(),
                    sql_type: "TEXT".to_string(),
                    not_null: true,
                    default: None,
                    unique: false,
                },
                ColumnDto {
                    name: "views".to_string(),
                    sql_type: "REAL".to_string(),
                    not_null: false,
                    default: None,
                    unique: false,
                },
            ],
            indexes: vec![],
            searchable_fields: vec![],
            view_query: None,
        };

        let c = schema_to_collection(&dto);
        assert_eq!(c.name, "posts");
        assert_eq!(c.collection_type, CollectionType::Base);
        assert_eq!(c.fields.len(), 2);
        assert_eq!(c.fields[0].name, "title");
        assert!(c.fields[0].required);
        assert_eq!(c.fields[1].name, "views");
        assert!(!c.fields[1].required);
    }

    #[test]
    fn schema_to_collection_auth_type_gets_default_options() {
        let dto = CollectionSchemaDto {
            name: "users".to_string(),
            collection_type: "auth".to_string(),
            columns: vec![],
            indexes: vec![],
            searchable_fields: vec![],
            view_query: None,
        };

        let c = schema_to_collection(&dto);
        assert_eq!(c.collection_type, CollectionType::Auth);
        assert!(c.auth_options.is_some());
    }

    // ── System collection protection ───────────────────────────────────────

    /// Helper: create a service with a pre-seeded system collection.
    fn make_service_with_system_collection(
        name: &str,
        collection_type: &str,
    ) -> CollectionService<MockRepo> {
        let repo = MockRepo::new();
        {
            let mut map = repo.collections.write().unwrap();
            map.insert(
                name.to_string(),
                CollectionSchemaDto {
                    name: name.to_string(),
                    collection_type: collection_type.to_string(),
                    columns: vec![],
                    indexes: vec![],
                    searchable_fields: vec![],
                    view_query: None,
                },
            );
        }
        CollectionService::new(repo)
    }

    #[test]
    fn delete_system_collection_is_forbidden() {
        let svc = make_service_with_system_collection("_superusers", "auth");

        let result = svc.delete_collection("_superusers");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 403);
        assert!(
            err.to_string().contains("cannot be deleted"),
            "error message should mention deletion: {}",
            err
        );
    }

    #[test]
    fn delete_system_collection_collections_is_forbidden() {
        let svc = make_service_with_system_collection("_collections", "base");

        let result = svc.delete_collection("_collections");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 403);
    }

    #[test]
    fn delete_system_collection_fields_is_forbidden() {
        let svc = make_service_with_system_collection("_fields", "base");

        let result = svc.delete_collection("_fields");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 403);
    }

    #[test]
    fn delete_system_collection_settings_is_forbidden() {
        let svc = make_service_with_system_collection("_settings", "base");

        let result = svc.delete_collection("_settings");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 403);
    }

    #[test]
    fn delete_system_collection_migrations_is_forbidden() {
        let svc = make_service_with_system_collection("_migrations", "base");

        let result = svc.delete_collection("_migrations");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 403);
    }

    #[test]
    fn delete_any_underscore_prefixed_collection_is_forbidden() {
        let svc = make_service_with_system_collection("_custom_system", "base");

        let result = svc.delete_collection("_custom_system");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 403);
    }

    #[test]
    fn delete_regular_collection_still_works() {
        let svc = make_service();
        let c = Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        svc.create_collection(&c).unwrap();

        let result = svc.delete_collection("posts");
        assert!(result.is_ok());
    }

    #[test]
    fn rename_system_collection_is_forbidden() {
        let svc = make_service_with_system_collection("_superusers", "auth");

        let mut renamed = Collection::auth("_renamed", vec![]);
        renamed.name = "_renamed".to_string();

        let result = svc.update_collection("_superusers", &renamed);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 403);
        assert!(
            err.to_string().contains("cannot be renamed"),
            "error message should mention renaming: {}",
            err
        );
    }

    #[test]
    fn rename_system_collection_to_non_system_name_is_forbidden() {
        let svc = make_service_with_system_collection("_superusers", "auth");

        let renamed = Collection::auth("regular_name", vec![]);

        let result = svc.update_collection("_superusers", &renamed);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 403);
    }

    #[test]
    fn update_system_collection_with_system_field_id_is_forbidden() {
        let svc = make_service_with_system_collection("_superusers", "auth");

        let mut updated = Collection::auth(
            "_superusers",
            vec![Field::new("id", FieldType::Text(TextOptions::default()))],
        );
        updated.name = "_superusers".to_string();

        let result = svc.update_collection("_superusers", &updated);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 403);
        assert!(
            err.to_string().contains("system field 'id'"),
            "error message should mention the field: {}",
            err
        );
    }

    #[test]
    fn update_system_collection_with_system_field_created_is_forbidden() {
        let svc = make_service_with_system_collection("_settings", "base");

        let mut updated = Collection::base(
            "_settings",
            vec![Field::new(
                "created",
                FieldType::Text(TextOptions::default()),
            )],
        );
        updated.name = "_settings".to_string();

        let result = svc.update_collection("_settings", &updated);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 403);
    }

    #[test]
    fn update_system_collection_with_system_field_updated_is_forbidden() {
        let svc = make_service_with_system_collection("_settings", "base");

        let mut updated = Collection::base(
            "_settings",
            vec![Field::new(
                "updated",
                FieldType::Text(TextOptions::default()),
            )],
        );
        updated.name = "_settings".to_string();

        let result = svc.update_collection("_settings", &updated);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 403);
    }

    #[test]
    fn update_system_auth_collection_with_auth_system_field_email_is_forbidden() {
        let svc = make_service_with_system_collection("_superusers", "auth");

        let mut updated = Collection::auth(
            "_superusers",
            vec![Field::new("email", FieldType::Text(TextOptions::default()))],
        );
        updated.name = "_superusers".to_string();

        let result = svc.update_collection("_superusers", &updated);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 403);
        assert!(err.to_string().contains("system field 'email'"));
    }

    #[test]
    fn update_system_auth_collection_with_auth_system_field_password_is_forbidden() {
        let svc = make_service_with_system_collection("_superusers", "auth");

        let mut updated = Collection::auth(
            "_superusers",
            vec![Field::new(
                "password",
                FieldType::Text(TextOptions::default()),
            )],
        );
        updated.name = "_superusers".to_string();

        let result = svc.update_collection("_superusers", &updated);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 403);
    }

    #[test]
    fn update_system_auth_collection_with_auth_system_field_verified_is_forbidden() {
        let svc = make_service_with_system_collection("_superusers", "auth");

        let mut updated = Collection::auth(
            "_superusers",
            vec![Field::new(
                "verified",
                FieldType::Bool(BoolOptions::default()),
            )],
        );
        updated.name = "_superusers".to_string();

        let result = svc.update_collection("_superusers", &updated);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 403);
    }

    #[test]
    fn update_system_auth_collection_with_auth_system_field_token_key_is_forbidden() {
        let svc = make_service_with_system_collection("_superusers", "auth");

        let mut updated = Collection::auth(
            "_superusers",
            vec![Field::new(
                "tokenKey",
                FieldType::Text(TextOptions::default()),
            )],
        );
        updated.name = "_superusers".to_string();

        let result = svc.update_collection("_superusers", &updated);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 403);
    }

    #[test]
    fn update_system_collection_with_non_system_field_is_allowed() {
        let svc = make_service_with_system_collection("_settings", "base");

        let mut updated = Collection::base(
            "_settings",
            vec![Field::new(
                "custom_field",
                FieldType::Text(TextOptions::default()),
            )],
        );
        updated.name = "_settings".to_string();

        let result = svc.update_collection("_settings", &updated);
        assert!(result.is_ok());
    }

    #[test]
    fn update_system_collection_same_name_is_allowed() {
        let svc = make_service_with_system_collection("_settings", "base");

        let mut updated = Collection::base("_settings", vec![]);
        updated.name = "_settings".to_string();

        let result = svc.update_collection("_settings", &updated);
        assert!(
            result.is_ok(),
            "updating without rename should succeed: {:?}",
            result
        );
    }

    #[test]
    fn user_cannot_create_underscore_prefixed_collection() {
        let svc = make_service();
        let c = Collection::base(
            "_sneaky",
            vec![Field::new("data", FieldType::Text(TextOptions::default()))],
        );

        let result = svc.create_collection(&c);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn is_system_collection_detects_underscore_prefix() {
        use crate::schema::is_system_collection;
        assert!(is_system_collection("_superusers"));
        assert!(is_system_collection("_collections"));
        assert!(is_system_collection("_anything"));
        assert!(!is_system_collection("users"));
        assert!(!is_system_collection("posts"));
        assert!(!is_system_collection(""));
    }

    // ── export_collections ────────────────────────────────────────────────

    #[test]
    fn export_empty_returns_empty_list() {
        let svc = make_service();
        let exported = svc.export_collections().unwrap();
        assert!(exported.is_empty());
    }

    #[test]
    fn export_returns_only_non_system_collections() {
        let svc = make_service_with_system_collection("_superusers", "auth");

        // Add a regular collection.
        svc.create_collection(&Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        ))
        .unwrap();

        let exported = svc.export_collections().unwrap();
        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].name, "posts");
    }

    #[test]
    fn export_returns_all_user_collections() {
        let svc = make_service();
        svc.create_collection(&Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        ))
        .unwrap();
        svc.create_collection(&Collection::base(
            "comments",
            vec![Field::new("body", FieldType::Text(TextOptions::default()))],
        ))
        .unwrap();

        let exported = svc.export_collections().unwrap();
        assert_eq!(exported.len(), 2);
        let names: Vec<&str> = exported.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"posts"));
        assert!(names.contains(&"comments"));
    }

    // ── import_collections ────────────────────────────────────────────────

    #[test]
    fn import_creates_new_collections() {
        let svc = make_service();
        let collections = vec![
            Collection::base(
                "posts",
                vec![Field::new("title", FieldType::Text(TextOptions::default()))],
            ),
            Collection::base(
                "comments",
                vec![Field::new("body", FieldType::Text(TextOptions::default()))],
            ),
        ];

        let result = svc.import_collections(&collections).unwrap();
        assert_eq!(result.len(), 2);

        // Verify they exist in the repo.
        assert!(svc.collection_exists("posts").unwrap());
        assert!(svc.collection_exists("comments").unwrap());
    }

    #[test]
    fn import_updates_existing_collections() {
        let svc = make_service();
        svc.create_collection(&Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        ))
        .unwrap();

        // Import with updated fields.
        let updated = vec![Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())),
                Field::new("body", FieldType::Text(TextOptions::default())),
            ],
        )];

        let result = svc.import_collections(&updated).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].fields.len(), 2);
    }

    #[test]
    fn import_mixed_create_and_update() {
        let svc = make_service();
        svc.create_collection(&Collection::base(
            "existing",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        ))
        .unwrap();

        let collections = vec![
            Collection::base(
                "existing",
                vec![
                    Field::new("title", FieldType::Text(TextOptions::default())),
                    Field::new("body", FieldType::Text(TextOptions::default())),
                ],
            ),
            Collection::base(
                "brand_new",
                vec![Field::new("name", FieldType::Text(TextOptions::default()))],
            ),
        ];

        let result = svc.import_collections(&collections).unwrap();
        assert_eq!(result.len(), 2);
        assert!(svc.collection_exists("existing").unwrap());
        assert!(svc.collection_exists("brand_new").unwrap());
    }

    #[test]
    fn import_rejects_system_collections() {
        let svc = make_service();
        let collections = vec![Collection::base(
            "_sneaky",
            vec![Field::new("x", FieldType::Text(TextOptions::default()))],
        )];

        let result = svc.import_collections(&collections);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert!(err.to_string().contains("system collection"));
    }

    #[test]
    fn import_rejects_invalid_collection_name() {
        let svc = make_service();
        let collections = vec![Collection::base(
            "1invalid",
            vec![Field::new("x", FieldType::Text(TextOptions::default()))],
        )];

        let result = svc.import_collections(&collections);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn import_rejects_duplicate_fields() {
        let svc = make_service();
        let collections = vec![Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())),
                Field::new("title", FieldType::Text(TextOptions::default())),
            ],
        )];

        let result = svc.import_collections(&collections);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 400);
    }

    #[test]
    fn import_aborts_entirely_if_any_collection_invalid() {
        let svc = make_service();
        let collections = vec![
            Collection::base(
                "valid_one",
                vec![Field::new("x", FieldType::Text(TextOptions::default()))],
            ),
            Collection::base(
                "1invalid",
                vec![Field::new("y", FieldType::Text(TextOptions::default()))],
            ),
        ];

        let result = svc.import_collections(&collections);
        assert!(result.is_err());

        // The valid one should NOT have been created because validation
        // happens upfront before any changes.
        assert!(!svc.collection_exists("valid_one").unwrap());
    }

    #[test]
    fn import_empty_list_succeeds() {
        let svc = make_service();
        let result = svc.import_collections(&[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn import_assigns_id_when_empty() {
        let svc = make_service();
        let mut c = Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        c.id = String::new();

        let result = svc.import_collections(&[c]).unwrap();
        assert_eq!(result.len(), 1);
        assert!(!result[0].id.is_empty());
    }

    #[test]
    fn import_preserves_existing_id() {
        let svc = make_service();
        let mut c = Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        c.id = "custom_id_12345".to_string();

        let result = svc.import_collections(&[c]).unwrap();
        assert_eq!(result[0].id, "custom_id_12345");
    }

    #[test]
    fn export_then_import_round_trip() {
        let svc = make_service();
        svc.create_collection(&Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())).required(true),
                Field::new("views", FieldType::Number(NumberOptions::default())),
            ],
        ))
        .unwrap();

        let exported = svc.export_collections().unwrap();
        assert_eq!(exported.len(), 1);

        // Delete and re-import.
        svc.delete_collection("posts").unwrap();
        assert!(!svc.collection_exists("posts").unwrap());

        let imported = svc.import_collections(&exported).unwrap();
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].name, "posts");
        assert_eq!(imported[0].fields.len(), 2);
        assert!(svc.collection_exists("posts").unwrap());
    }

    #[test]
    fn system_field_names_are_comprehensive() {
        use crate::schema::{AUTH_SYSTEM_FIELDS, BASE_SYSTEM_FIELDS};
        assert!(BASE_SYSTEM_FIELDS.contains(&"id"));
        assert!(BASE_SYSTEM_FIELDS.contains(&"created"));
        assert!(BASE_SYSTEM_FIELDS.contains(&"updated"));

        assert!(AUTH_SYSTEM_FIELDS.contains(&"email"));
        assert!(AUTH_SYSTEM_FIELDS.contains(&"emailVisibility"));
        assert!(AUTH_SYSTEM_FIELDS.contains(&"verified"));
        assert!(AUTH_SYSTEM_FIELDS.contains(&"password"));
        assert!(AUTH_SYSTEM_FIELDS.contains(&"tokenKey"));
    }

    // ── Auto-index from rules ────────────────────────────────────────────

    #[test]
    fn auto_index_generated_for_rule_fields() {
        use crate::schema::ApiRules;
        let mut c = Collection::base(
            "posts",
            vec![
                Field::new("status", FieldType::Text(TextOptions::default())),
                Field::new("title", FieldType::Text(TextOptions::default())),
            ],
        );
        c.rules = ApiRules {
            list_rule: Some("status = \"published\"".into()),
            view_rule: None,
            create_rule: None,
            update_rule: None,
            delete_rule: None,
            manage_rule: None,
        };

        let dto = collection_to_schema(&c);
        // Should auto-generate an index on "status" from the rule reference.
        let auto_idx = dto.indexes.iter().find(|i| i.columns == vec!["status"]);
        assert!(auto_idx.is_some(), "auto-index for 'status' should exist");
        assert!(!auto_idx.unwrap().unique);
    }

    #[test]
    fn auto_index_skips_already_indexed_fields() {
        use crate::schema::ApiRules;
        let mut c = Collection::base(
            "posts",
            vec![Field::new(
                "status",
                FieldType::Text(TextOptions::default()),
            )],
        );
        c.indexes = vec![IndexSpec::new(vec!["status".to_string()])];
        c.rules = ApiRules {
            list_rule: Some("status = \"published\"".into()),
            ..Default::default()
        };

        let dto = collection_to_schema(&c);
        // Should have exactly 1 index on status (the explicit one, no duplicate).
        let status_indexes: Vec<_> = dto
            .indexes
            .iter()
            .filter(|i| i.columns.contains(&"status".to_string()))
            .collect();
        assert_eq!(status_indexes.len(), 1);
    }

    #[test]
    fn auto_index_skips_nonexistent_fields() {
        use crate::schema::ApiRules;
        let mut c = Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        c.rules = ApiRules {
            list_rule: Some("nonexistent = \"value\"".into()),
            ..Default::default()
        };

        let dto = collection_to_schema(&c);
        // Should NOT create an index for a field that doesn't exist in the schema.
        assert!(dto.indexes.is_empty());
    }

    #[test]
    fn sort_direction_preserved_in_schema_round_trip() {
        use crate::schema::{IndexColumn, IndexSortDirection};
        let mut c = Collection::base(
            "events",
            vec![
                Field::new("category", FieldType::Text(TextOptions::default())),
                Field::new("date", FieldType::Text(TextOptions::default())),
            ],
        );
        c.indexes = vec![IndexSpec::with_columns(
            vec![IndexColumn::asc("category"), IndexColumn::desc("date")],
            false,
        )];

        let dto = collection_to_schema(&c);
        assert_eq!(dto.indexes.len(), 1);
        assert_eq!(dto.indexes[0].index_columns.len(), 2);
        assert_eq!(
            dto.indexes[0].index_columns[0].sort,
            IndexColumnSortDto::Asc
        );
        assert_eq!(
            dto.indexes[0].index_columns[1].sort,
            IndexColumnSortDto::Desc
        );

        // Round-trip back to Collection.
        let c2 = schema_to_collection(&dto);
        assert_eq!(c2.indexes.len(), 1);
        let cols = c2.indexes[0].effective_index_columns();
        assert_eq!(cols[0].direction, IndexSortDirection::Asc);
        assert_eq!(cols[1].direction, IndexSortDirection::Desc);
    }
}
