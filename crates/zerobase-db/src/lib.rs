//! Zerobase DB — SQLite persistence layer.
//!
//! Manages the embedded SQLite database, connection pooling,
//! migrations, and query execution.
//!
//! # Architecture
//!
//! - [`pool::Database`] — Connection pool (r2d2 read pool + mutex-guarded write connection).
//! - [`migrations`] — Forward-only migration runner with `_migrations` tracking table.
//! - [`query_builder`] — Lightweight parameterized SQL builder for dynamic queries.
//! - [`error::DbError`] — Database-layer error type, converts into `ZerobaseError`.
//!
//! # Interface Traits
//!
//! Two repository traits define the contract for database operations:
//!
//! - [`RecordRepository`] — CRUD on records within collections.
//! - [`SchemaRepository`] — DDL operations for managing collections.
//!
//! Concrete implementations live in `record_repo.rs` and `schema_repo.rs`
//! (to be implemented). Tests can mock these traits or use an in-memory
//! [`Database`].

pub mod backup_repo;
pub mod error;
pub mod external_auth_repo;
pub mod filter;
pub mod fts;
pub mod log_repo;
pub mod migrations;
pub mod pool;
pub mod query_builder;
pub mod record_repo;
pub mod schema_repo;
pub mod settings_repo;
pub mod superuser_repo;
pub mod unique;
pub mod webauthn_credential_repo;

// Re-export key types at crate root.
pub use error::{DbError, Result};
pub use pool::{
    Database, HealthDiagnostics, HealthStatus, PoolConfig, PoolStats,
    DEFAULT_SLOW_QUERY_THRESHOLD,
};

use serde_json::Value as JsonValue;
use std::collections::HashMap;

// ── Domain types for the DB layer ───────────────────────────────────────────

/// A single record as a map of field names to JSON values.
///
/// Records are schema-flexible: the concrete fields depend on the collection
/// definition. The DB layer treats them as `HashMap<String, JsonValue>` — field
/// validation is the responsibility of the core/application layer.
pub type RecordData = HashMap<String, JsonValue>;

/// Sort direction for query ordering.
pub use query_builder::SortDirection;

/// Parameters for listing/filtering records.
#[derive(Debug, Clone, Default)]
pub struct RecordQuery {
    /// Filter expression (Pocketbase-style, to be parsed by the filter module).
    pub filter: Option<String>,
    /// Sort instructions: `(column, direction)` pairs.
    pub sort: Vec<(String, SortDirection)>,
    /// Page number (1-based).
    pub page: u32,
    /// Items per page.
    pub per_page: u32,
    /// Relations to expand.
    pub expand: Vec<String>,
    /// Subset of fields to return.
    pub fields: Vec<String>,
}

/// A paginated list of records.
#[derive(Debug, Clone)]
pub struct RecordList {
    /// The records for the current page.
    pub items: Vec<RecordData>,
    /// Total number of records matching the filter.
    pub total_items: u64,
    /// Current page number.
    pub page: u32,
    /// Items per page.
    pub per_page: u32,
    /// Total number of pages.
    pub total_pages: u32,
}

/// Metadata for a collection (table) in the schema.
///
/// This is the DB layer's view of a collection. The full domain-level
/// `Collection` type (with field types, rules, etc.) lives in `zerobase-core`.
/// The DB layer works with this simpler representation for DDL operations.
#[derive(Debug, Clone)]
pub struct CollectionSchema {
    /// Collection name (maps to table name).
    pub name: String,
    /// Collection type: "base", "auth", or "view".
    pub collection_type: String,
    /// Column definitions.
    pub columns: Vec<ColumnDef>,
    /// Indexes to create.
    pub indexes: Vec<IndexDef>,
    /// Field names included in full-text search indexes.
    pub searchable_fields: Vec<String>,
    /// For View collections: the SQL query that defines the view.
    pub view_query: Option<String>,
}

/// A column definition for DDL operations.
#[derive(Debug, Clone)]
pub struct ColumnDef {
    /// Column name.
    pub name: String,
    /// SQLite type affinity (TEXT, INTEGER, REAL, BLOB, NUMERIC).
    pub sql_type: String,
    /// Whether the column has a NOT NULL constraint.
    pub not_null: bool,
    /// Default value expression (raw SQL), if any.
    pub default: Option<String>,
    /// Whether this column has a UNIQUE constraint.
    pub unique: bool,
}

/// Sort direction for an index column at the DB layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexColumnSort {
    /// Ascending order (default).
    Asc,
    /// Descending order.
    Desc,
}

impl Default for IndexColumnSort {
    fn default() -> Self {
        Self::Asc
    }
}

impl IndexColumnSort {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

/// A column within an index, with sort direction.
#[derive(Debug, Clone)]
pub struct IndexColumnDef {
    /// Column name.
    pub name: String,
    /// Sort direction.
    pub sort: IndexColumnSort,
}

/// An index definition for DDL operations.
#[derive(Debug, Clone)]
pub struct IndexDef {
    /// Index name.
    pub name: String,
    /// Columns in the index (simple form, without sort directions).
    pub columns: Vec<String>,
    /// Rich column definitions with sort directions.
    /// If non-empty, takes precedence over `columns`.
    pub index_columns: Vec<IndexColumnDef>,
    /// Whether this is a unique index.
    pub unique: bool,
}

impl IndexDef {
    /// Returns the effective column expressions for SQL generation.
    /// Each entry is `"col_name" ASC` or `"col_name" DESC`.
    pub fn column_exprs(&self) -> Vec<String> {
        if !self.index_columns.is_empty() {
            self.index_columns
                .iter()
                .map(|c| format!("\"{}\" {}", c.name, c.sort.as_sql()))
                .collect()
        } else {
            self.columns.iter().map(|c| format!("\"{}\"", c)).collect()
        }
    }

    /// Returns just the column names.
    pub fn column_names(&self) -> Vec<&str> {
        if !self.index_columns.is_empty() {
            self.index_columns.iter().map(|c| c.name.as_str()).collect()
        } else {
            self.columns.iter().map(|s| s.as_str()).collect()
        }
    }
}

/// Describes explicit field-level alterations for a collection update.
///
/// When updating a collection, SQLite cannot distinguish a rename from a
/// remove+add. This struct carries explicit rename mappings and type change
/// information so the rebuild can preserve data correctly.
#[derive(Debug, Clone)]
pub struct SchemaAlteration {
    /// The desired final schema for the collection.
    pub schema: CollectionSchema,
    /// Field rename mappings: `(old_name, new_name)`.
    ///
    /// Each pair indicates that the field previously named `old_name` should
    /// be renamed to `new_name` while preserving its data.
    pub renames: Vec<(String, String)>,
}

// ── Repository Traits ───────────────────────────────────────────────────────

/// CRUD operations on records within a collection.
///
/// Implementations receive a collection name (table name) and operate on
/// [`RecordData`] — a flexible map of field names to JSON values.
///
/// # Testability
///
/// This trait is `Send + Sync` so it can be stored in `Arc<AppState>`.
/// Tests can provide mock implementations or use an in-memory [`Database`].
pub trait RecordRepository: Send + Sync {
    /// Retrieve a single record by its ID.
    fn find_one(&self, collection: &str, id: &str) -> Result<RecordData>;

    /// List records matching the given query parameters.
    fn find_many(&self, collection: &str, query: &RecordQuery) -> Result<RecordList>;

    /// Insert a new record and return it with generated fields (id, created, updated).
    fn create(&self, collection: &str, data: RecordData) -> Result<RecordData>;

    /// Update an existing record. Returns the updated record.
    fn update(&self, collection: &str, id: &str, data: RecordData) -> Result<RecordData>;

    /// Delete a record by its ID.
    fn delete(&self, collection: &str, id: &str) -> Result<()>;

    /// Count records matching an optional filter.
    fn count(&self, collection: &str, filter: Option<&str>) -> Result<u64>;
}

/// Schema (DDL) operations for managing collections.
///
/// Collections map to SQLite tables. This trait covers creating, altering,
/// and dropping tables as well as querying the current schema.
pub trait SchemaRepository: Send + Sync {
    /// List all collections (tables managed by Zerobase).
    fn list_collections(&self) -> Result<Vec<CollectionSchema>>;

    /// Get a single collection's schema by name.
    fn get_collection(&self, name: &str) -> Result<CollectionSchema>;

    /// Create a new collection (table + indexes).
    fn create_collection(&self, schema: &CollectionSchema) -> Result<()>;

    /// Update a collection's schema (add/remove/alter columns, indexes).
    ///
    /// This method infers changes by comparing old and new column sets.
    /// It cannot distinguish renames from remove+add. For explicit renames
    /// or type changes with data migration, use [`alter_collection`].
    fn update_collection(&self, name: &str, schema: &CollectionSchema) -> Result<()>;

    /// Alter a collection with explicit field-level changes.
    ///
    /// Supports:
    /// - Adding new fields
    /// - Removing fields
    /// - Renaming fields (via `alteration.renames`)
    /// - Changing field types with automatic data migration
    /// - Changing field constraints (NOT NULL, UNIQUE, DEFAULT)
    ///
    /// Data is preserved across alterations:
    /// - Renamed fields keep their data under the new name.
    /// - Type changes attempt best-effort conversion (e.g. TEXT→REAL
    ///   applies CAST, incompatible values become NULL or the column default).
    /// - Removed fields lose their data.
    fn alter_collection(&self, name: &str, alteration: &SchemaAlteration) -> Result<()>;

    /// Drop a collection (table + indexes).
    fn delete_collection(&self, name: &str) -> Result<()>;

    /// Check if a collection exists.
    fn collection_exists(&self, name: &str) -> Result<bool>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_query_defaults() {
        let q = RecordQuery::default();
        assert!(q.filter.is_none());
        assert!(q.sort.is_empty());
        assert_eq!(q.page, 0);
        assert_eq!(q.per_page, 0);
        assert!(q.expand.is_empty());
        assert!(q.fields.is_empty());
    }

    #[test]
    fn collection_schema_construction() {
        let schema = CollectionSchema {
            name: "posts".to_string(),
            collection_type: "base".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".to_string(),
                    sql_type: "TEXT".to_string(),
                    not_null: true,
                    default: None,
                    unique: true,
                },
                ColumnDef {
                    name: "title".to_string(),
                    sql_type: "TEXT".to_string(),
                    not_null: true,
                    default: None,
                    unique: false,
                },
            ],
            indexes: vec![IndexDef {
                name: "idx_posts_title".to_string(),
                columns: vec!["title".to_string()],
                index_columns: vec![],
                unique: false,
            }],
            searchable_fields: vec![],
            view_query: None,
        };
        assert_eq!(schema.name, "posts");
        assert_eq!(schema.columns.len(), 2);
        assert_eq!(schema.indexes.len(), 1);
    }

    #[test]
    fn record_list_structure() {
        let list = RecordList {
            items: vec![],
            total_items: 100,
            page: 2,
            per_page: 20,
            total_pages: 5,
        };
        assert_eq!(list.total_items, 100);
        assert_eq!(list.total_pages, 5);
    }
}
