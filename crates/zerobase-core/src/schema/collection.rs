//! Collection definitions.
//!
//! A [`Collection`] represents a table in the database with its fields,
//! API rules, and type-specific configuration.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::field::Field;
use super::rule_parser::validate_rule;
use super::rules::ApiRules;
use super::validation::validate_name;
use crate::error::{Result, ZerobaseError};

// ── CollectionType ─────────────────────────────────────────────────────────────

/// The kind of collection.
///
/// Mirrors PocketBase's three collection types:
/// - **Base** — general-purpose data collection.
/// - **Auth** — extends Base with built-in authentication fields and capabilities.
/// - **View** — read-only collection backed by a SQL `SELECT` query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CollectionType {
    /// General-purpose data collection.
    Base,
    /// Authentication-enabled collection with built-in user fields.
    Auth,
    /// Read-only collection backed by a SQL query.
    View,
}

impl CollectionType {
    /// String representation matching the DB/API format.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Auth => "auth",
            Self::View => "view",
        }
    }
}

impl std::fmt::Display for CollectionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for CollectionType {
    type Err = ZerobaseError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "base" => Ok(Self::Base),
            "auth" => Ok(Self::Auth),
            "view" => Ok(Self::View),
            _ => Err(ZerobaseError::validation(format!(
                "unknown collection type: {s}"
            ))),
        }
    }
}

// ── Collection ─────────────────────────────────────────────────────────────────

/// A complete collection definition.
///
/// This is the domain-level representation used by the application layer.
/// The DB layer converts this to/from its own [`zerobase_db::CollectionSchema`]
/// for DDL operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    /// Unique identifier (system-assigned).
    #[serde(default = "default_collection_id")]
    pub id: String,
    /// Collection name (used as table name and API path segment).
    pub name: String,
    /// The collection type.
    #[serde(rename = "type")]
    pub collection_type: CollectionType,
    /// User-defined fields. System fields (id, created, updated) are implicit.
    #[serde(default)]
    pub fields: Vec<Field>,
    /// Per-operation API access rules.
    #[serde(default)]
    pub rules: ApiRules,
    /// Index definitions: each entry is a list of field names.
    /// `true` in the tuple means UNIQUE index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexSpec>,
    /// For View collections: the SQL query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_query: Option<String>,
    /// Auth collection options (only meaningful for Auth type).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_options: Option<AuthOptions>,
}

fn default_collection_id() -> String {
    uuid::Uuid::new_v4().to_string().replace('-', "")[..15].to_string()
}

/// Sort direction for an index column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexSortDirection {
    /// Ascending order (default).
    Asc,
    /// Descending order.
    Desc,
}

impl Default for IndexSortDirection {
    fn default() -> Self {
        Self::Asc
    }
}

impl IndexSortDirection {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

/// A single column within an index, with optional sort direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexColumn {
    /// Column name.
    pub name: String,
    /// Sort direction (ASC or DESC).
    #[serde(default)]
    pub direction: IndexSortDirection,
}

impl IndexColumn {
    /// Create an ascending index column.
    pub fn asc(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            direction: IndexSortDirection::Asc,
        }
    }

    /// Create a descending index column.
    pub fn desc(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            direction: IndexSortDirection::Desc,
        }
    }
}

/// Index specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexSpec {
    /// Column names in the index (simple form, for backwards compatibility).
    /// If `index_columns` is non-empty, `columns` is ignored in favor of it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    /// Rich column definitions with sort direction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub index_columns: Vec<IndexColumn>,
    /// Whether this is a unique index.
    #[serde(default)]
    pub unique: bool,
}

impl IndexSpec {
    /// Create a simple non-unique index on the given columns.
    pub fn new(columns: Vec<String>) -> Self {
        Self {
            columns,
            index_columns: Vec::new(),
            unique: false,
        }
    }

    /// Create a unique index on the given columns.
    pub fn unique(columns: Vec<String>) -> Self {
        Self {
            columns,
            index_columns: Vec::new(),
            unique: true,
        }
    }

    /// Create an index with rich column definitions (sort directions).
    pub fn with_columns(index_columns: Vec<IndexColumn>, unique: bool) -> Self {
        Self {
            columns: Vec::new(),
            index_columns,
            unique,
        }
    }

    /// Returns the effective column names for this index.
    ///
    /// If `index_columns` is populated, extracts names from there;
    /// otherwise falls back to `columns`.
    pub fn effective_column_names(&self) -> Vec<&str> {
        if !self.index_columns.is_empty() {
            self.index_columns.iter().map(|c| c.name.as_str()).collect()
        } else {
            self.columns.iter().map(|s| s.as_str()).collect()
        }
    }

    /// Returns the effective IndexColumn list (with directions).
    ///
    /// If `index_columns` is populated, returns those;
    /// otherwise builds from `columns` with default ASC direction.
    pub fn effective_index_columns(&self) -> Vec<IndexColumn> {
        if !self.index_columns.is_empty() {
            self.index_columns.clone()
        } else {
            self.columns
                .iter()
                .map(|name| IndexColumn::asc(name.clone()))
                .collect()
        }
    }

    /// Generate a deterministic index name for this spec on a given table.
    pub fn generate_name(&self, table_name: &str) -> String {
        let col_names: Vec<&str> = self.effective_column_names();
        format!("idx_{}_{}", table_name, col_names.join("_"))
    }
}

/// Authentication options for Auth collections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthOptions {
    /// Allow email/password authentication.
    #[serde(default = "default_true")]
    pub allow_email_auth: bool,
    /// Allow OAuth2 authentication.
    #[serde(default)]
    pub allow_oauth2_auth: bool,
    /// Allow OTP (one-time password) authentication.
    #[serde(default)]
    pub allow_otp_auth: bool,
    /// Require email verification before allowing authentication.
    #[serde(default)]
    pub require_email: bool,
    /// Whether MFA is enabled for this collection.
    #[serde(default)]
    pub mfa_enabled: bool,
    /// Duration in seconds for MFA session validity (0 = use system default).
    #[serde(default)]
    pub mfa_duration: u64,
    /// Minimum password length.
    #[serde(default = "default_min_password")]
    pub min_password_length: u32,
    /// Fields that can be used as identity for login (e.g., ["email", "username"]).
    #[serde(default = "default_identity_fields")]
    pub identity_fields: Vec<String>,
    /// Additional rule for who can manage (CRUD) other users in this collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manage_rule: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_min_password() -> u32 {
    8
}

fn default_identity_fields() -> Vec<String> {
    vec!["email".to_string()]
}

impl Default for AuthOptions {
    fn default() -> Self {
        Self {
            allow_email_auth: true,
            allow_oauth2_auth: false,
            allow_otp_auth: false,
            require_email: true,
            mfa_enabled: false,
            mfa_duration: 0,
            min_password_length: 8,
            identity_fields: default_identity_fields(),
            manage_rule: None,
        }
    }
}

// ── System collection constants ───────────────────────────────────────────────

/// Names of system collections (prefixed with `_`).
///
/// These are created by system migrations and cannot be deleted, renamed,
/// or have their schema modified by users.
pub const SYSTEM_COLLECTION_NAMES: &[&str] = &[
    "_migrations",
    "_collections",
    "_fields",
    "_settings",
    "_superusers",
];

/// System fields that are implicit for every collection and cannot be modified.
pub const BASE_SYSTEM_FIELDS: &[&str] = &["id", "created", "updated"];

/// Additional system fields for Auth collections that cannot be modified.
pub const AUTH_SYSTEM_FIELDS: &[&str] = &[
    "email",
    "emailVisibility",
    "verified",
    "password",
    "tokenKey",
];

/// Returns `true` if the given name is a system collection (starts with `_`).
pub fn is_system_collection(name: &str) -> bool {
    name.starts_with('_')
}

// ── Collection methods ─────────────────────────────────────────────────────────

/// Reserved names that cannot be used as collection names.
const RESERVED_NAMES: &[&str] = &[
    "id",
    "created",
    "updated",
    "expand",
    "collectionId",
    "collectionName",
];

impl Collection {
    /// Create a new Base collection with the given name and fields.
    pub fn base(name: impl Into<String>, fields: Vec<Field>) -> Self {
        Self {
            id: default_collection_id(),
            name: name.into(),
            collection_type: CollectionType::Base,
            fields,
            rules: ApiRules::default(),
            indexes: Vec::new(),
            view_query: None,
            auth_options: None,
        }
    }

    /// Create a new Auth collection with the given name and additional fields.
    pub fn auth(name: impl Into<String>, fields: Vec<Field>) -> Self {
        Self {
            id: default_collection_id(),
            name: name.into(),
            collection_type: CollectionType::Auth,
            fields,
            rules: ApiRules::default(),
            indexes: Vec::new(),
            view_query: None,
            auth_options: Some(AuthOptions::default()),
        }
    }

    /// Create a new View collection with the given name and SQL query.
    pub fn view(name: impl Into<String>, query: impl Into<String>) -> Self {
        Self {
            id: default_collection_id(),
            name: name.into(),
            collection_type: CollectionType::View,
            fields: Vec::new(),
            rules: ApiRules::default(),
            indexes: Vec::new(),
            view_query: Some(query.into()),
            auth_options: None,
        }
    }

    /// Validate the entire collection definition.
    ///
    /// Checks:
    /// - Collection name is valid and not reserved.
    /// - No duplicate field names.
    /// - Each field passes its own validation.
    /// - View collections must have a `view_query`.
    /// - Auth collections must have `auth_options`.
    /// - Index columns reference existing fields.
    pub fn validate(&self) -> Result<()> {
        // Name
        validate_name(&self.name, "collection name")?;

        if RESERVED_NAMES.contains(&self.name.as_str()) {
            return Err(ZerobaseError::validation(format!(
                "'{}' is a reserved name",
                self.name
            )));
        }

        // Type-specific checks
        match self.collection_type {
            CollectionType::View => {
                if self
                    .view_query
                    .as_ref()
                    .map_or(true, |q| q.trim().is_empty())
                {
                    return Err(ZerobaseError::validation(
                        "view collection must have a non-empty view_query",
                    ));
                }
            }
            CollectionType::Auth => {
                if self.auth_options.is_none() {
                    return Err(ZerobaseError::validation(
                        "auth collection must have auth_options",
                    ));
                }
                if let Some(ref opts) = self.auth_options {
                    if opts.min_password_length == 0 {
                        return Err(ZerobaseError::validation(
                            "min_password_length must be at least 1",
                        ));
                    }
                    if opts.identity_fields.is_empty() {
                        return Err(ZerobaseError::validation(
                            "auth collection must have at least one identity field",
                        ));
                    }
                }
            }
            CollectionType::Base => {}
        }

        // Fields — check for duplicates
        let mut seen_names = std::collections::HashSet::new();
        for field in &self.fields {
            if !seen_names.insert(&field.name) {
                return Err(ZerobaseError::validation(format!(
                    "duplicate field name: {}",
                    field.name,
                )));
            }

            // System field name collisions
            if RESERVED_NAMES.contains(&field.name.as_str()) {
                return Err(ZerobaseError::validation(format!(
                    "'{}' is a reserved field name",
                    field.name,
                )));
            }

            field.validate()?;
        }

        self.validate_indexes()?;
        self.validate_rules()?;

        Ok(())
    }

    /// Validate only fields and type-specific constraints (skip name validation).
    ///
    /// Used for system collections whose names start with `_` and would
    /// fail the standard `validate_name` check.
    pub fn validate_fields_and_type(&self) -> Result<()> {
        // Type-specific checks
        match self.collection_type {
            CollectionType::View => {
                if self
                    .view_query
                    .as_ref()
                    .map_or(true, |q| q.trim().is_empty())
                {
                    return Err(ZerobaseError::validation(
                        "view collection must have a non-empty view_query",
                    ));
                }
            }
            CollectionType::Auth => {
                if self.auth_options.is_none() {
                    return Err(ZerobaseError::validation(
                        "auth collection must have auth_options",
                    ));
                }
                if let Some(ref opts) = self.auth_options {
                    if opts.min_password_length == 0 {
                        return Err(ZerobaseError::validation(
                            "min_password_length must be at least 1",
                        ));
                    }
                    if opts.identity_fields.is_empty() {
                        return Err(ZerobaseError::validation(
                            "auth collection must have at least one identity field",
                        ));
                    }
                }
            }
            CollectionType::Base => {}
        }

        // Fields — check for duplicates
        let mut seen_names = std::collections::HashSet::new();
        for field in &self.fields {
            if !seen_names.insert(&field.name) {
                return Err(ZerobaseError::validation(format!(
                    "duplicate field name: {}",
                    field.name,
                )));
            }
            field.validate()?;
        }

        self.validate_indexes()?;
        self.validate_rules()?;

        Ok(())
    }

    /// Validate index definitions: columns must reference known fields.
    fn validate_indexes(&self) -> Result<()> {
        let field_names: std::collections::HashSet<&str> =
            self.fields.iter().map(|f| f.name.as_str()).collect();

        for (i, idx) in self.indexes.iter().enumerate() {
            let col_names = idx.effective_column_names();
            if col_names.is_empty() {
                return Err(ZerobaseError::validation(format!(
                    "index {i} must have at least one column"
                )));
            }
            for col in col_names {
                if !field_names.contains(col) {
                    let mut errors = HashMap::new();
                    errors.insert(
                        format!("indexes[{i}]"),
                        format!("column '{col}' does not reference a known field"),
                    );
                    return Err(ZerobaseError::validation_with_fields(
                        "invalid index",
                        errors,
                    ));
                }
            }
        }

        Ok(())
    }

    /// Validate rule syntax for all non-null rules.
    ///
    /// Returns an error with the rule name and parse error message if any rule
    /// contains invalid syntax. This catches errors at collection save time
    /// rather than at runtime.
    fn validate_rules(&self) -> Result<()> {
        let rule_checks: &[(&str, &Option<String>)] = &[
            ("listRule", &self.rules.list_rule),
            ("viewRule", &self.rules.view_rule),
            ("createRule", &self.rules.create_rule),
            ("updateRule", &self.rules.update_rule),
            ("deleteRule", &self.rules.delete_rule),
            ("manageRule", &self.rules.manage_rule),
        ];

        for (name, rule) in rule_checks {
            if let Some(expr) = rule {
                if let Err(e) = validate_rule(expr) {
                    return Err(ZerobaseError::validation(format!(
                        "invalid {name}: {e}"
                    )));
                }
            }
        }

        Ok(())
    }

    /// Return the list of system fields that are implicit for this collection type.
    ///
    /// System fields (id, created, updated) are always present and don't need to be
    /// defined by the user. Auth collections also have email, password, verified, etc.
    pub fn system_field_names(&self) -> Vec<&'static str> {
        let mut fields = vec!["id", "created", "updated"];
        if self.collection_type == CollectionType::Auth {
            fields.extend_from_slice(&[
                "email",
                "emailVisibility",
                "verified",
                "password",
                "tokenKey",
            ]);
        }
        fields
    }

    /// Check whether a field name is valid for this collection (for sorting, filtering, etc).
    ///
    /// A field is valid if it is a system field (id, created, updated, plus auth-specific
    /// fields for auth collections) or a user-defined field in the collection schema.
    pub fn has_field(&self, name: &str) -> bool {
        self.system_field_names().contains(&name) || self.fields.iter().any(|f| f.name == name)
    }

    /// Return the names of fields marked as searchable for FTS indexing.
    ///
    /// Only user-defined fields with `searchable = true` (currently Text and
    /// Editor field types) are returned. System fields are never searchable.
    pub fn searchable_field_names(&self) -> Vec<&str> {
        self.fields
            .iter()
            .filter(|f| f.is_searchable())
            .map(|f| f.name.as_str())
            .collect()
    }

    /// Whether this collection has any searchable fields.
    pub fn has_searchable_fields(&self) -> bool {
        self.fields.iter().any(|f| f.is_searchable())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::field::{BoolOptions, FieldType, NumberOptions, SelectOptions, TextOptions};

    // ── CollectionType ─────────────────────────────────────────────────────

    #[test]
    fn collection_type_as_str() {
        assert_eq!(CollectionType::Base.as_str(), "base");
        assert_eq!(CollectionType::Auth.as_str(), "auth");
        assert_eq!(CollectionType::View.as_str(), "view");
    }

    #[test]
    fn collection_type_display() {
        assert_eq!(format!("{}", CollectionType::Base), "base");
        assert_eq!(format!("{}", CollectionType::Auth), "auth");
        assert_eq!(format!("{}", CollectionType::View), "view");
    }

    #[test]
    fn collection_type_from_str() {
        assert_eq!(
            "base".parse::<CollectionType>().unwrap(),
            CollectionType::Base
        );
        assert_eq!(
            "auth".parse::<CollectionType>().unwrap(),
            CollectionType::Auth
        );
        assert_eq!(
            "view".parse::<CollectionType>().unwrap(),
            CollectionType::View
        );
        assert!("unknown".parse::<CollectionType>().is_err());
    }

    #[test]
    fn collection_type_serde_round_trip() {
        let json = serde_json::to_string(&CollectionType::Auth).unwrap();
        assert_eq!(json, "\"auth\"");
        let deserialized: CollectionType = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, CollectionType::Auth);
    }

    // ── Collection construction ────────────────────────────────────────────

    #[test]
    fn base_collection_construction() {
        let c = Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())).required(true),
                Field::new("views", FieldType::Number(NumberOptions::default())),
            ],
        );
        assert_eq!(c.collection_type, CollectionType::Base);
        assert_eq!(c.fields.len(), 2);
        assert!(c.view_query.is_none());
        assert!(c.auth_options.is_none());
    }

    #[test]
    fn auth_collection_construction() {
        let c = Collection::auth(
            "users",
            vec![Field::new("name", FieldType::Text(TextOptions::default()))],
        );
        assert_eq!(c.collection_type, CollectionType::Auth);
        assert!(c.auth_options.is_some());
        let opts = c.auth_options.unwrap();
        assert!(opts.allow_email_auth);
        assert_eq!(opts.min_password_length, 8);
    }

    #[test]
    fn view_collection_construction() {
        let c = Collection::view(
            "post_stats",
            "SELECT p.id, COUNT(*) as count FROM posts p GROUP BY p.id",
        );
        assert_eq!(c.collection_type, CollectionType::View);
        assert!(c.view_query.is_some());
    }

    // ── Validation ─────────────────────────────────────────────────────────

    #[test]
    fn valid_base_collection() {
        let c = Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        assert!(c.validate().is_ok());
    }

    #[test]
    fn rejects_reserved_collection_name() {
        let c = Collection::base("id", vec![]);
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_reserved_field_name() {
        let c = Collection::base(
            "posts",
            vec![Field::new("id", FieldType::Text(TextOptions::default()))],
        );
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_field_names() {
        let c = Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())),
                Field::new("title", FieldType::Text(TextOptions::default())),
            ],
        );
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_invalid_collection_name() {
        let c = Collection::base("1invalid", vec![]);
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_field_with_invalid_name() {
        let c = Collection::base(
            "posts",
            vec![Field::new(
                "bad name!",
                FieldType::Text(TextOptions::default()),
            )],
        );
        assert!(c.validate().is_err());
    }

    #[test]
    fn view_collection_requires_query() {
        let mut c = Collection::view("stats", "SELECT 1");
        c.view_query = None;
        assert!(c.validate().is_err());
    }

    #[test]
    fn view_collection_rejects_empty_query() {
        let c = Collection::view("stats", "   ");
        assert!(c.validate().is_err());
    }

    #[test]
    fn auth_collection_requires_options() {
        let mut c = Collection::auth("users", vec![]);
        c.auth_options = None;
        assert!(c.validate().is_err());
    }

    #[test]
    fn auth_collection_requires_identity_fields() {
        let mut c = Collection::auth("users", vec![]);
        c.auth_options.as_mut().unwrap().identity_fields = Vec::new();
        assert!(c.validate().is_err());
    }

    #[test]
    fn auth_collection_requires_nonzero_min_password() {
        let mut c = Collection::auth("users", vec![]);
        c.auth_options.as_mut().unwrap().min_password_length = 0;
        assert!(c.validate().is_err());
    }

    // ── Index validation ───────────────────────────────────────────────────

    #[test]
    fn valid_index_accepted() {
        let mut c = Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())),
                Field::new(
                    "status",
                    FieldType::Select(SelectOptions {
                        values: vec!["draft".into(), "published".into()],
                    }),
                ),
            ],
        );
        c.indexes = vec![IndexSpec::new(vec!["title".into()])];
        assert!(c.validate().is_ok());
    }

    #[test]
    fn index_with_unknown_column_rejected() {
        let mut c = Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        c.indexes = vec![IndexSpec::new(vec!["nonexistent".into()])];
        assert!(c.validate().is_err());
    }

    #[test]
    fn index_with_empty_columns_rejected() {
        let mut c = Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        c.indexes = vec![IndexSpec::new(Vec::new())];
        assert!(c.validate().is_err());
    }

    #[test]
    fn composite_index_accepted() {
        let mut c = Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())),
                Field::new(
                    "status",
                    FieldType::Select(SelectOptions {
                        values: vec!["draft".into(), "published".into()],
                    }),
                ),
            ],
        );
        c.indexes = vec![IndexSpec::new(vec!["title".into(), "status".into()])];
        assert!(c.validate().is_ok());
    }

    #[test]
    fn unique_index_accepted() {
        let mut c = Collection::base(
            "posts",
            vec![Field::new("slug", FieldType::Text(TextOptions::default()))],
        );
        c.indexes = vec![IndexSpec::unique(vec!["slug".into()])];
        assert!(c.validate().is_ok());
        assert!(c.indexes[0].unique);
    }

    #[test]
    fn index_with_sort_directions() {
        let mut c = Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())),
                Field::new("views", FieldType::Number(NumberOptions::default())),
            ],
        );
        c.indexes = vec![IndexSpec::with_columns(
            vec![IndexColumn::asc("title"), IndexColumn::desc("views")],
            false,
        )];
        assert!(c.validate().is_ok());
        let cols = c.indexes[0].effective_index_columns();
        assert_eq!(cols[0].direction, IndexSortDirection::Asc);
        assert_eq!(cols[1].direction, IndexSortDirection::Desc);
    }

    #[test]
    fn index_generate_name() {
        let idx = IndexSpec::new(vec!["title".into(), "status".into()]);
        assert_eq!(idx.generate_name("posts"), "idx_posts_title_status");
    }

    // ── System fields ──────────────────────────────────────────────────────

    #[test]
    fn base_system_fields() {
        let c = Collection::base("posts", vec![]);
        let sys = c.system_field_names();
        assert!(sys.contains(&"id"));
        assert!(sys.contains(&"created"));
        assert!(sys.contains(&"updated"));
        assert!(!sys.contains(&"email"));
    }

    #[test]
    fn auth_system_fields() {
        let c = Collection::auth("users", vec![]);
        let sys = c.system_field_names();
        assert!(sys.contains(&"id"));
        assert!(sys.contains(&"email"));
        assert!(sys.contains(&"password"));
        assert!(sys.contains(&"verified"));
        assert!(sys.contains(&"tokenKey"));
    }

    // ── has_field ─────────────────────────────────────────────────────────

    #[test]
    fn has_field_finds_system_fields() {
        let c = Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        assert!(c.has_field("id"));
        assert!(c.has_field("created"));
        assert!(c.has_field("updated"));
    }

    #[test]
    fn has_field_finds_user_defined_fields() {
        let c = Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())),
                Field::new("views", FieldType::Number(NumberOptions::default())),
            ],
        );
        assert!(c.has_field("title"));
        assert!(c.has_field("views"));
    }

    #[test]
    fn has_field_returns_false_for_unknown() {
        let c = Collection::base(
            "posts",
            vec![Field::new("title", FieldType::Text(TextOptions::default()))],
        );
        assert!(!c.has_field("nonexistent"));
        assert!(!c.has_field("email")); // Not auth collection
    }

    #[test]
    fn has_field_finds_auth_system_fields() {
        let c = Collection::auth(
            "users",
            vec![Field::new("name", FieldType::Text(TextOptions::default()))],
        );
        assert!(c.has_field("email"));
        assert!(c.has_field("verified"));
        assert!(c.has_field("tokenKey"));
        assert!(c.has_field("name")); // user-defined
        assert!(!c.has_field("nonexistent"));
    }

    // ── Serde round-trip ───────────────────────────────────────────────────

    #[test]
    fn collection_serializes_and_deserializes() {
        let c = Collection::base(
            "posts",
            vec![
                Field::new(
                    "title",
                    FieldType::Text(TextOptions {
                        min_length: 1,
                        max_length: 255,
                        pattern: None,
                        searchable: false,
                    }),
                )
                .required(true),
                Field::new("published", FieldType::Bool(BoolOptions::default())),
            ],
        );

        let json = serde_json::to_string_pretty(&c).unwrap();
        let deserialized: Collection = serde_json::from_str(&json).unwrap();

        assert_eq!(c.name, deserialized.name);
        assert_eq!(c.collection_type, deserialized.collection_type);
        assert_eq!(c.fields.len(), deserialized.fields.len());
    }

    #[test]
    fn auth_collection_serializes_with_options() {
        let c = Collection::auth(
            "users",
            vec![Field::new("name", FieldType::Text(TextOptions::default()))],
        );

        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json["type"], "auth");
        assert!(json["authOptions"].is_object());
        assert_eq!(json["authOptions"]["allowEmailAuth"], true);
        assert_eq!(json["authOptions"]["minPasswordLength"], 8);

        let deserialized: Collection = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.auth_options.unwrap().min_password_length, 8);
    }

    #[test]
    fn view_collection_serializes_with_query() {
        let c = Collection::view("stats", "SELECT id, COUNT(*) FROM posts GROUP BY id");
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json["type"], "view");
        assert!(json["viewQuery"].is_string());
    }

    // ── System collection helpers ─────────────────────────────────────────

    #[test]
    fn is_system_collection_with_underscore_prefix() {
        assert!(is_system_collection("_superusers"));
        assert!(is_system_collection("_collections"));
        assert!(is_system_collection("_fields"));
        assert!(is_system_collection("_settings"));
        assert!(is_system_collection("_migrations"));
        assert!(is_system_collection("_anything_custom"));
    }

    #[test]
    fn is_system_collection_without_underscore_prefix() {
        assert!(!is_system_collection("users"));
        assert!(!is_system_collection("posts"));
        assert!(!is_system_collection("my_collection"));
        assert!(!is_system_collection(""));
    }

    #[test]
    fn system_collection_names_constant_covers_all_known() {
        assert!(SYSTEM_COLLECTION_NAMES.contains(&"_migrations"));
        assert!(SYSTEM_COLLECTION_NAMES.contains(&"_collections"));
        assert!(SYSTEM_COLLECTION_NAMES.contains(&"_fields"));
        assert!(SYSTEM_COLLECTION_NAMES.contains(&"_settings"));
        assert!(SYSTEM_COLLECTION_NAMES.contains(&"_superusers"));
    }

    #[test]
    fn base_system_fields_constant_correct() {
        assert_eq!(BASE_SYSTEM_FIELDS, &["id", "created", "updated"]);
    }

    #[test]
    fn auth_system_fields_constant_correct() {
        assert!(AUTH_SYSTEM_FIELDS.contains(&"email"));
        assert!(AUTH_SYSTEM_FIELDS.contains(&"emailVisibility"));
        assert!(AUTH_SYSTEM_FIELDS.contains(&"verified"));
        assert!(AUTH_SYSTEM_FIELDS.contains(&"password"));
        assert!(AUTH_SYSTEM_FIELDS.contains(&"tokenKey"));
    }

    // ── validate_fields_and_type ──────────────────────────────────────────

    #[test]
    fn validate_fields_and_type_passes_for_valid_base() {
        let c = Collection::base(
            "_settings",
            vec![Field::new(
                "custom",
                FieldType::Text(TextOptions::default()),
            )],
        );
        assert!(c.validate_fields_and_type().is_ok());
    }

    #[test]
    fn validate_fields_and_type_passes_for_valid_auth() {
        let c = Collection::auth("_superusers", vec![]);
        assert!(c.validate_fields_and_type().is_ok());
    }

    #[test]
    fn validate_fields_and_type_rejects_duplicate_fields() {
        let c = Collection::base(
            "_system",
            vec![
                Field::new("x", FieldType::Text(TextOptions::default())),
                Field::new("x", FieldType::Text(TextOptions::default())),
            ],
        );
        assert!(c.validate_fields_and_type().is_err());
    }

    #[test]
    fn validate_fields_and_type_does_not_reject_underscore_name() {
        let c = Collection::base("_system", vec![]);
        // validate() would reject this; validate_fields_and_type() should not
        assert!(c.validate_fields_and_type().is_ok());
    }
}
