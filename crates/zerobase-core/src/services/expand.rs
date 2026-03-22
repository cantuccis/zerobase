//! Relation expansion for API responses.
//!
//! Implements PocketBase-compatible `?expand=field1,field2.nested` support.
//! When a record has relation fields, the expand mechanism fetches the
//! referenced records and nests them in an `expand` map in the response.
//!
//! # Features
//!
//! - Single relation expansion (`?expand=author`)
//! - Multi-relation expansion (`?expand=tags`)
//! - Nested expansion via dot notation (`?expand=author.profile`)
//! - Back-relation expansion (`?expand=comments_via_post`)
//! - Configurable depth limit to prevent abuse
//! - Circular reference detection

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::error::{Result, ZerobaseError};
use crate::schema::{Collection, Field, FieldType, RelationOptions};

use super::record_service::{RecordRepository, SchemaLookup};

/// Maximum expansion depth to prevent abuse and runaway queries.
pub const MAX_EXPAND_DEPTH: usize = 6;

/// Maximum number of records returned for a single back-relation expansion.
///
/// PocketBase caps back-relation expansion to prevent unbounded result sets.
/// If a back-relation has more records than this limit, only the first N are
/// returned (ordered by the database's natural row order).
pub const MAX_BACK_RELATION_EXPAND: usize = 100;

/// A parsed expand path segment.
///
/// Each segment represents one level of relation traversal. For example,
/// `author.profile` produces two segments: `author` and `profile`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExpandSegment {
    /// The field name (or back-relation pattern like `comments_via_post`).
    pub field: String,
    /// Nested expand paths from this segment.
    pub children: Vec<ExpandPath>,
}

/// A full expand path, potentially with nested segments.
///
/// `author.profile.avatar` → `ExpandPath { segments: ["author", "profile", "avatar"] }`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExpandPath {
    pub segments: Vec<String>,
}

impl ExpandPath {
    /// The top-level field name.
    pub fn root(&self) -> &str {
        &self.segments[0]
    }

    /// Whether this path has nested segments beyond the root.
    pub fn has_children(&self) -> bool {
        self.segments.len() > 1
    }

    /// Get the child path (everything after the first segment).
    pub fn child_path(&self) -> Option<ExpandPath> {
        if self.segments.len() > 1 {
            Some(ExpandPath {
                segments: self.segments[1..].to_vec(),
            })
        } else {
            None
        }
    }
}

/// Parse a comma-separated expand string into expand paths.
///
/// Examples:
/// - `"author"` → `[ExpandPath(["author"])]`
/// - `"author,tags"` → `[ExpandPath(["author"]), ExpandPath(["tags"])]`
/// - `"author.profile"` → `[ExpandPath(["author", "profile"])]`
/// - `"author.profile,tags"` → two paths
pub fn parse_expand(expand: &str) -> Result<Vec<ExpandPath>> {
    let trimmed = expand.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();

    for part in trimmed.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let segments: Vec<String> = part.split('.').map(|s| s.trim().to_string()).collect();

        // Validate: no empty segments
        for seg in &segments {
            if seg.is_empty() {
                return Err(ZerobaseError::validation(format!(
                    "invalid expand path: '{part}' contains empty segment"
                )));
            }
        }

        // Validate depth limit
        if segments.len() > MAX_EXPAND_DEPTH {
            return Err(ZerobaseError::validation(format!(
                "expand depth exceeds maximum of {MAX_EXPAND_DEPTH}: '{part}'"
            )));
        }

        paths.push(ExpandPath { segments });
    }

    Ok(paths)
}

/// Group expand paths by their root field name.
///
/// Returns a map of root field name → list of child paths for that root.
fn group_by_root(paths: &[ExpandPath]) -> HashMap<String, Vec<ExpandPath>> {
    let mut groups: HashMap<String, Vec<ExpandPath>> = HashMap::new();

    for path in paths {
        let root = path.root().to_string();
        if let Some(child) = path.child_path() {
            groups.entry(root).or_default().push(child);
        } else {
            // Leaf expansion — ensure entry exists even with no children
            groups.entry(root).or_default();
        }
    }

    groups
}

/// Describes how a field should be expanded.
#[derive(Debug)]
enum ExpandKind<'a> {
    /// A forward relation field on the current collection.
    Forward {
        field: &'a Field,
        opts: &'a RelationOptions,
    },
    /// A back-relation: records in another collection referencing this one.
    BackRelation {
        /// The collection containing the referencing records.
        source_collection: String,
        /// The relation field name in the source collection.
        source_field: String,
    },
}

/// Resolve which kind of expansion a field name refers to.
fn resolve_expand_kind<'a, S: SchemaLookup>(
    field_name: &str,
    collection: &'a Collection,
    schema: &S,
) -> Option<ExpandKind<'a>> {
    // 1. Check forward relations: field exists on this collection
    for field in &collection.fields {
        if field.name == field_name {
            if let FieldType::Relation(ref opts) = field.field_type {
                return Some(ExpandKind::Forward { field, opts });
            }
            // Field exists but is not a relation
            return None;
        }
    }

    // 2. Check back-relation pattern: `<collection>_via_<field>`
    if let Some((source_collection_name, source_field_name)) = parse_back_relation(field_name) {
        // Verify the source collection and field exist
        if let Ok(source_col) = schema.get_collection(&source_collection_name) {
            for field in &source_col.fields {
                if field.name == source_field_name {
                    if let FieldType::Relation(ref opts) = field.field_type {
                        // Verify it points to our collection
                        if opts.collection_id == collection.id
                            || opts.collection_id == collection.name
                        {
                            return Some(ExpandKind::BackRelation {
                                source_collection: source_collection_name,
                                source_field: source_field_name,
                            });
                        }
                    }
                }
            }
        }
    }

    None
}

/// Parse a back-relation field name like `comments_via_post` into
/// `("comments", "post")`.
fn parse_back_relation(field_name: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = field_name.splitn(3, '_').collect();
    // Pattern: <collection>_via_<field>
    // We need at least 3 parts and the middle one must be "via"
    if let Some(pos) = field_name.find("_via_") {
        let collection = &field_name[..pos];
        let field = &field_name[pos + 5..];
        if !collection.is_empty() && !field.is_empty() {
            return Some((collection.to_string(), field.to_string()));
        }
    }
    let _ = parts; // suppress unused warning
    None
}

/// Expand relations for a single record, returning the expand map.
///
/// The expand map has the same structure as PocketBase: each key is the
/// relation field name, and the value is either a single record object
/// (for single-relation fields) or an array of record objects (for
/// multi-relation fields and back-relations).
pub fn expand_record<R: RecordRepository, S: SchemaLookup>(
    record: &HashMap<String, Value>,
    collection: &Collection,
    expand_paths: &[ExpandPath],
    repo: &R,
    schema: &S,
    visited: &mut HashSet<(String, String)>,
    depth: usize,
) -> Result<HashMap<String, Value>> {
    if expand_paths.is_empty() || depth > MAX_EXPAND_DEPTH {
        return Ok(HashMap::new());
    }

    let grouped = group_by_root(expand_paths);
    let mut expand_map: HashMap<String, Value> = HashMap::new();

    let record_id = record
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    for (field_name, child_paths) in &grouped {
        // Circular reference detection per record+field
        let record_visit_key = (
            format!("{}:{}", record_id, collection.name),
            field_name.clone(),
        );
        if visited.contains(&record_visit_key) {
            continue;
        }
        visited.insert(record_visit_key.clone());

        let kind = match resolve_expand_kind(field_name, collection, schema) {
            Some(k) => k,
            None => continue, // Unknown field or not a relation — skip silently
        };

        match kind {
            ExpandKind::Forward { field, opts } => {
                let target_collection_id = &opts.collection_id;

                // Resolve target collection (could be ID or name)
                let target_collection = match schema.get_collection(target_collection_id) {
                    Ok(c) => c,
                    Err(_) => match schema.get_collection_by_id(target_collection_id) {
                        Ok(c) => c,
                        Err(_) => continue,
                    },
                };

                // Extract referenced IDs from the record's field value
                let field_value = record.get(&field.name).cloned().unwrap_or(Value::Null);
                let ref_ids = RelationOptions::extract_ids(&field_value);

                if ref_ids.is_empty() {
                    continue;
                }

                // Fetch referenced records
                let mut expanded_records = Vec::new();
                for ref_id in &ref_ids {
                    match repo.find_one(&target_collection.name, ref_id) {
                        Ok(mut related) => {
                            // Recursively expand nested paths
                            if !child_paths.is_empty() {
                                let nested_expand = expand_record(
                                    &related,
                                    &target_collection,
                                    child_paths,
                                    repo,
                                    schema,
                                    visited,
                                    depth + 1,
                                )?;
                                if !nested_expand.is_empty() {
                                    related.insert(
                                        "expand".to_string(),
                                        serde_json::to_value(&nested_expand).unwrap_or(Value::Null),
                                    );
                                }
                            }

                            // Add collection metadata
                            related.insert(
                                "collectionId".to_string(),
                                Value::String(target_collection.id.clone()),
                            );
                            related.insert(
                                "collectionName".to_string(),
                                Value::String(target_collection.name.clone()),
                            );

                            expanded_records.push(Value::Object(related.into_iter().collect()));
                        }
                        Err(_) => continue, // Referenced record not found — skip
                    }
                }

                if expanded_records.is_empty() {
                    continue;
                }

                // Single relation → object, multi → array
                if opts.max_select == 1 {
                    expand_map.insert(
                        field_name.clone(),
                        expanded_records.into_iter().next().unwrap(),
                    );
                } else {
                    expand_map.insert(field_name.clone(), Value::Array(expanded_records));
                }
            }

            ExpandKind::BackRelation {
                source_collection,
                source_field,
            } => {
                let source_col = match schema.get_collection(&source_collection) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                // Find records in source_collection where source_field references this record,
                // capped at MAX_BACK_RELATION_EXPAND for safety.
                let referencing = match repo.find_referencing_records_limited(
                    &source_collection,
                    &source_field,
                    &record_id,
                    MAX_BACK_RELATION_EXPAND,
                ) {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                if referencing.is_empty() {
                    continue;
                }

                let mut expanded_records = Vec::new();
                for mut related in referencing {
                    // Recursively expand nested paths
                    if !child_paths.is_empty() {
                        let nested_expand = expand_record(
                            &related,
                            &source_col,
                            child_paths,
                            repo,
                            schema,
                            visited,
                            depth + 1,
                        )?;
                        if !nested_expand.is_empty() {
                            related.insert(
                                "expand".to_string(),
                                serde_json::to_value(&nested_expand).unwrap_or(Value::Null),
                            );
                        }
                    }

                    // Add collection metadata
                    related.insert(
                        "collectionId".to_string(),
                        Value::String(source_col.id.clone()),
                    );
                    related.insert(
                        "collectionName".to_string(),
                        Value::String(source_col.name.clone()),
                    );

                    expanded_records.push(Value::Object(related.into_iter().collect()));
                }

                // Back-relations always produce an array
                expand_map.insert(field_name.clone(), Value::Array(expanded_records));
            }
        }

        // Clean up visit tracking for this record+field to allow other records
        // to expand the same field
        visited.remove(&record_visit_key);
    }

    Ok(expand_map)
}

/// Expand relations for a list of records.
///
/// Applies [`expand_record`] to each record in the list.
pub fn expand_records<R: RecordRepository, S: SchemaLookup>(
    records: &mut [HashMap<String, Value>],
    collection: &Collection,
    expand_paths: &[ExpandPath],
    repo: &R,
    schema: &S,
) -> Result<()> {
    if expand_paths.is_empty() {
        return Ok(());
    }

    for record in records.iter_mut() {
        let mut visited = HashSet::new();
        let expand_map = expand_record(
            record,
            collection,
            expand_paths,
            repo,
            schema,
            &mut visited,
            0,
        )?;
        if !expand_map.is_empty() {
            record.insert(
                "expand".to_string(),
                serde_json::to_value(&expand_map).unwrap_or(Value::Null),
            );
        }
    }

    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Collection, Field, FieldType, RelationOptions, TextOptions};
    use crate::services::record_service::{
        RecordList, RecordQuery, RecordRepoError, RecordRepository, SchemaLookup,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Mutex;

    // ── Mock Repository ──────────────────────────────────────────────────

    struct MockRepo {
        records: Mutex<HashMap<String, Vec<HashMap<String, Value>>>>,
    }

    impl MockRepo {
        fn new() -> Self {
            Self {
                records: Mutex::new(HashMap::new()),
            }
        }

        fn insert_record(&self, collection: &str, record: HashMap<String, Value>) {
            self.records
                .lock()
                .unwrap()
                .entry(collection.to_string())
                .or_default()
                .push(record);
        }
    }

    impl RecordRepository for MockRepo {
        fn find_one(
            &self,
            collection: &str,
            id: &str,
        ) -> std::result::Result<HashMap<String, Value>, RecordRepoError> {
            let store = self.records.lock().unwrap();
            store
                .get(collection)
                .and_then(|rows| {
                    rows.iter()
                        .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
                })
                .cloned()
                .ok_or_else(|| RecordRepoError::NotFound {
                    resource_type: collection.to_string(),
                    resource_id: Some(id.to_string()),
                })
        }

        fn find_many(
            &self,
            _collection: &str,
            _query: &RecordQuery,
        ) -> std::result::Result<RecordList, RecordRepoError> {
            Ok(RecordList {
                page: 1,
                per_page: 30,
                total_pages: 1,
                total_items: 0,
                items: vec![],
            })
        }

        fn insert(
            &self,
            _collection: &str,
            _data: &HashMap<String, Value>,
        ) -> std::result::Result<(), RecordRepoError> {
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
            _collection: &str,
            _filter: Option<&str>,
        ) -> std::result::Result<u64, RecordRepoError> {
            Ok(0)
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

    // ── Mock Schema ──────────────────────────────────────────────────────

    struct MockSchema {
        collections: HashMap<String, Collection>,
    }

    impl MockSchema {
        fn new() -> Self {
            Self {
                collections: HashMap::new(),
            }
        }

        fn add_collection(&mut self, collection: Collection) {
            self.collections.insert(collection.name.clone(), collection);
        }
    }

    impl SchemaLookup for MockSchema {
        fn get_collection(&self, name: &str) -> Result<Collection> {
            self.collections
                .get(name)
                .cloned()
                .ok_or_else(|| ZerobaseError::not_found_with_id("Collection", name))
        }

        fn get_collection_by_id(&self, id: &str) -> Result<Collection> {
            self.collections
                .values()
                .find(|c| c.id == id)
                .cloned()
                .ok_or_else(|| ZerobaseError::not_found_with_id("Collection", id))
        }

        fn list_all_collections(&self) -> Result<Vec<Collection>> {
            Ok(self.collections.values().cloned().collect())
        }
    }

    // ── Parse Tests ──────────────────────────────────────────────────────

    #[test]
    fn parse_expand_empty_string() {
        let paths = parse_expand("").unwrap();
        assert!(paths.is_empty());
    }

    #[test]
    fn parse_expand_single_field() {
        let paths = parse_expand("author").unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].segments, vec!["author"]);
    }

    #[test]
    fn parse_expand_multiple_fields() {
        let paths = parse_expand("author,tags").unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].root(), "author");
        assert_eq!(paths[1].root(), "tags");
    }

    #[test]
    fn parse_expand_nested() {
        let paths = parse_expand("author.profile").unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].segments, vec!["author", "profile"]);
        assert!(paths[0].has_children());
    }

    #[test]
    fn parse_expand_mixed() {
        let paths = parse_expand("author.profile,tags,comments_via_post").unwrap();
        assert_eq!(paths.len(), 3);
        assert_eq!(paths[0].segments, vec!["author", "profile"]);
        assert_eq!(paths[1].segments, vec!["tags"]);
        assert_eq!(paths[2].segments, vec!["comments_via_post"]);
    }

    #[test]
    fn parse_expand_trims_whitespace() {
        let paths = parse_expand(" author , tags ").unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].root(), "author");
        assert_eq!(paths[1].root(), "tags");
    }

    #[test]
    fn parse_expand_depth_limit() {
        let deep = "a.b.c.d.e.f.g"; // 7 levels
        let err = parse_expand(deep).unwrap_err();
        assert!(err.to_string().contains("depth exceeds maximum"));
    }

    #[test]
    fn parse_expand_empty_segment_error() {
        let err = parse_expand("author..profile").unwrap_err();
        assert!(err.to_string().contains("empty segment"));
    }

    #[test]
    fn parse_expand_trailing_comma_ok() {
        let paths = parse_expand("author,").unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].root(), "author");
    }

    // ── Back-relation parsing ────────────────────────────────────────────

    #[test]
    fn parse_back_relation_valid() {
        let (col, field) = parse_back_relation("comments_via_post").unwrap();
        assert_eq!(col, "comments");
        assert_eq!(field, "post");
    }

    #[test]
    fn parse_back_relation_with_underscores() {
        let (col, field) = parse_back_relation("order_items_via_order_id").unwrap();
        assert_eq!(col, "order_items");
        assert_eq!(field, "order_id");
    }

    #[test]
    fn parse_back_relation_invalid() {
        assert!(parse_back_relation("author").is_none());
        assert!(parse_back_relation("_via_field").is_none());
        assert!(parse_back_relation("collection_via_").is_none());
    }

    // ── Expand path methods ──────────────────────────────────────────────

    #[test]
    fn expand_path_root() {
        let p = ExpandPath {
            segments: vec!["author".to_string(), "profile".to_string()],
        };
        assert_eq!(p.root(), "author");
        assert!(p.has_children());
        let child = p.child_path().unwrap();
        assert_eq!(child.segments, vec!["profile"]);
        assert!(!child.has_children());
    }

    // ── Expand record tests ──────────────────────────────────────────────

    fn make_test_collections() -> (MockSchema, MockRepo) {
        let mut schema = MockSchema::new();

        // Users collection
        let mut users = Collection::base(
            "users",
            vec![Field::new("name", FieldType::Text(TextOptions::default()))],
        );
        users.id = "col_users".to_string();

        // Profiles collection with relation to users
        let mut profiles = Collection::base(
            "profiles",
            vec![
                Field::new("bio", FieldType::Text(TextOptions::default())),
                Field::new(
                    "user",
                    FieldType::Relation(RelationOptions {
                        collection_id: "users".to_string(),
                        max_select: 1,
                        ..Default::default()
                    }),
                ),
            ],
        );
        profiles.id = "col_profiles".to_string();

        // Posts collection with single-relation to users and multi-relation to tags
        let mut posts = Collection::base(
            "posts",
            vec![
                Field::new("title", FieldType::Text(TextOptions::default())),
                Field::new(
                    "author",
                    FieldType::Relation(RelationOptions {
                        collection_id: "users".to_string(),
                        max_select: 1,
                        ..Default::default()
                    }),
                ),
                Field::new(
                    "tags",
                    FieldType::Relation(RelationOptions {
                        collection_id: "tags".to_string(),
                        max_select: 0, // unlimited
                        ..Default::default()
                    }),
                ),
            ],
        );
        posts.id = "col_posts".to_string();

        // Tags collection
        let mut tags = Collection::base(
            "tags",
            vec![Field::new("label", FieldType::Text(TextOptions::default()))],
        );
        tags.id = "col_tags".to_string();

        // Comments collection with relation to posts and users
        let mut comments = Collection::base(
            "comments",
            vec![
                Field::new("text", FieldType::Text(TextOptions::default())),
                Field::new(
                    "post",
                    FieldType::Relation(RelationOptions {
                        collection_id: "col_posts".to_string(),
                        max_select: 1,
                        ..Default::default()
                    }),
                ),
                Field::new(
                    "author",
                    FieldType::Relation(RelationOptions {
                        collection_id: "users".to_string(),
                        max_select: 1,
                        ..Default::default()
                    }),
                ),
            ],
        );
        comments.id = "col_comments".to_string();

        schema.add_collection(users);
        schema.add_collection(profiles);
        schema.add_collection(posts);
        schema.add_collection(tags);
        schema.add_collection(comments);

        // Seed data
        let repo = MockRepo::new();

        // Users
        let mut user1 = HashMap::new();
        user1.insert("id".to_string(), json!("user1"));
        user1.insert("name".to_string(), json!("Alice"));
        repo.insert_record("users", user1);

        let mut user2 = HashMap::new();
        user2.insert("id".to_string(), json!("user2"));
        user2.insert("name".to_string(), json!("Bob"));
        repo.insert_record("users", user2);

        // Profiles
        let mut profile1 = HashMap::new();
        profile1.insert("id".to_string(), json!("prof1"));
        profile1.insert("bio".to_string(), json!("Alice's bio"));
        profile1.insert("user".to_string(), json!("user1"));
        repo.insert_record("profiles", profile1);

        // Tags
        let mut tag1 = HashMap::new();
        tag1.insert("id".to_string(), json!("tag1"));
        tag1.insert("label".to_string(), json!("rust"));
        repo.insert_record("tags", tag1);

        let mut tag2 = HashMap::new();
        tag2.insert("id".to_string(), json!("tag2"));
        tag2.insert("label".to_string(), json!("web"));
        repo.insert_record("tags", tag2);

        // Posts
        let mut post1 = HashMap::new();
        post1.insert("id".to_string(), json!("post1"));
        post1.insert("title".to_string(), json!("Hello World"));
        post1.insert("author".to_string(), json!("user1"));
        post1.insert("tags".to_string(), json!(["tag1", "tag2"]));
        repo.insert_record("posts", post1);

        // Comments
        let mut comment1 = HashMap::new();
        comment1.insert("id".to_string(), json!("cmt1"));
        comment1.insert("text".to_string(), json!("Great post!"));
        comment1.insert("post".to_string(), json!("post1"));
        comment1.insert("author".to_string(), json!("user2"));
        repo.insert_record("comments", comment1);

        let mut comment2 = HashMap::new();
        comment2.insert("id".to_string(), json!("cmt2"));
        comment2.insert("text".to_string(), json!("Thanks!"));
        comment2.insert("post".to_string(), json!("post1"));
        comment2.insert("author".to_string(), json!("user1"));
        repo.insert_record("comments", comment2);

        (schema, repo)
    }

    #[test]
    fn expand_single_relation() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("post1"));
        record.insert("title".to_string(), json!("Hello World"));
        record.insert("author".to_string(), json!("user1"));
        record.insert("tags".to_string(), json!(["tag1", "tag2"]));

        let paths = parse_expand("author").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &posts_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        assert!(expand.contains_key("author"));
        let author = &expand["author"];
        assert_eq!(author["name"], "Alice");
        assert_eq!(author["id"], "user1");
        assert_eq!(author["collectionName"], "users");
    }

    #[test]
    fn expand_multi_relation() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("post1"));
        record.insert("title".to_string(), json!("Hello World"));
        record.insert("author".to_string(), json!("user1"));
        record.insert("tags".to_string(), json!(["tag1", "tag2"]));

        let paths = parse_expand("tags").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &posts_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        assert!(expand.contains_key("tags"));
        let tags = expand["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0]["label"], "rust");
        assert_eq!(tags[1]["label"], "web");
    }

    #[test]
    fn expand_multiple_fields() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("post1"));
        record.insert("title".to_string(), json!("Hello World"));
        record.insert("author".to_string(), json!("user1"));
        record.insert("tags".to_string(), json!(["tag1", "tag2"]));

        let paths = parse_expand("author,tags").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &posts_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        assert!(expand.contains_key("author"));
        assert!(expand.contains_key("tags"));
    }

    #[test]
    fn expand_nested_relation() {
        let (schema, repo) = make_test_collections();
        let comments_col = schema.get_collection("comments").unwrap();

        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("cmt1"));
        record.insert("text".to_string(), json!("Great post!"));
        record.insert("post".to_string(), json!("post1"));
        record.insert("author".to_string(), json!("user2"));

        // Expand comment → post → author (nested)
        let paths = parse_expand("post.author").unwrap();
        let mut visited = HashSet::new();
        let expand = expand_record(
            &record,
            &comments_col,
            &paths,
            &repo,
            &schema,
            &mut visited,
            0,
        )
        .unwrap();

        let post = &expand["post"];
        assert_eq!(post["title"], "Hello World");

        // The post should have its own expand with author
        let post_expand = &post["expand"];
        assert_eq!(post_expand["author"]["name"], "Alice");
    }

    #[test]
    fn expand_back_relation() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("post1"));
        record.insert("title".to_string(), json!("Hello World"));
        record.insert("author".to_string(), json!("user1"));
        record.insert("tags".to_string(), json!(["tag1", "tag2"]));

        let paths = parse_expand("comments_via_post").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &posts_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        assert!(expand.contains_key("comments_via_post"));
        let comments = expand["comments_via_post"].as_array().unwrap();
        assert_eq!(comments.len(), 2);
    }

    #[test]
    fn expand_back_relation_with_nested() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("post1"));
        record.insert("title".to_string(), json!("Hello World"));
        record.insert("author".to_string(), json!("user1"));
        record.insert("tags".to_string(), json!(["tag1", "tag2"]));

        // Expand back-relation comments, then each comment's author
        let paths = parse_expand("comments_via_post.author").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &posts_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        let comments = expand["comments_via_post"].as_array().unwrap();
        assert_eq!(comments.len(), 2);

        // Each comment should have expanded author
        for comment in comments {
            assert!(comment["expand"]["author"].is_object());
        }
    }

    #[test]
    fn expand_nonexistent_field_ignored() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("post1"));
        record.insert("title".to_string(), json!("Hello World"));
        record.insert("author".to_string(), json!("user1"));

        let paths = parse_expand("nonexistent").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &posts_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        assert!(expand.is_empty());
    }

    #[test]
    fn expand_null_relation_skipped() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("post1"));
        record.insert("title".to_string(), json!("Hello World"));
        record.insert("author".to_string(), Value::Null);

        let paths = parse_expand("author").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &posts_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        assert!(expand.is_empty());
    }

    #[test]
    fn expand_missing_referenced_record_skipped() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("post1"));
        record.insert("title".to_string(), json!("Hello World"));
        record.insert("author".to_string(), json!("nonexistent_user"));

        let paths = parse_expand("author").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &posts_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        assert!(expand.is_empty());
    }

    #[test]
    fn expand_records_batch() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        let mut record1 = HashMap::new();
        record1.insert("id".to_string(), json!("post1"));
        record1.insert("title".to_string(), json!("Hello World"));
        record1.insert("author".to_string(), json!("user1"));
        record1.insert("tags".to_string(), json!(["tag1"]));

        let mut records = vec![record1];
        let paths = parse_expand("author").unwrap();
        expand_records(&mut records, &posts_col, &paths, &repo, &schema).unwrap();

        assert!(records[0].contains_key("expand"));
        let expand: HashMap<String, Value> =
            serde_json::from_value(records[0]["expand"].clone()).unwrap();
        assert!(expand.contains_key("author"));
    }

    #[test]
    fn expand_depth_limit_enforced_at_runtime() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("post1"));
        record.insert("author".to_string(), json!("user1"));

        let paths = parse_expand("author").unwrap();
        let mut visited = HashSet::new();
        // Call at max depth — should return empty
        let expand = expand_record(
            &record,
            &posts_col,
            &paths,
            &repo,
            &schema,
            &mut visited,
            MAX_EXPAND_DEPTH + 1,
        )
        .unwrap();

        assert!(expand.is_empty());
    }

    #[test]
    fn expand_back_relation_no_matches_excluded() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        // user2 has no posts — back-relation should produce nothing
        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("user2"));
        record.insert("name".to_string(), json!("Bob"));

        let users_col = schema.get_collection("users").unwrap();
        let paths = parse_expand("posts_via_author").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &users_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        // No matching posts → key should not appear in expand map
        assert!(
            !expand.contains_key("posts_via_author"),
            "empty back-relation should not appear in expand map"
        );
    }

    #[test]
    fn expand_back_relation_via_multi_relation_field() {
        // Test that back-relations work when the source field is a multi-relation
        // (JSON array of IDs).
        let mut schema = MockSchema::new();

        let mut items = Collection::base(
            "items",
            vec![Field::new("name", FieldType::Text(TextOptions::default()))],
        );
        items.id = "col_items".to_string();

        let mut bundles = Collection::base(
            "bundles",
            vec![
                Field::new("label", FieldType::Text(TextOptions::default())),
                Field::new(
                    "items",
                    FieldType::Relation(RelationOptions {
                        collection_id: "items".to_string(),
                        max_select: 0, // unlimited (multi-relation)
                        ..Default::default()
                    }),
                ),
            ],
        );
        bundles.id = "col_bundles".to_string();

        schema.add_collection(items);
        schema.add_collection(bundles);

        let repo = MockRepo::new();

        let mut item1 = HashMap::new();
        item1.insert("id".to_string(), json!("item1"));
        item1.insert("name".to_string(), json!("Widget"));
        repo.insert_record("items", item1);

        let mut bundle1 = HashMap::new();
        bundle1.insert("id".to_string(), json!("bundle1"));
        bundle1.insert("label".to_string(), json!("Starter Pack"));
        bundle1.insert("items".to_string(), json!(["item1", "item2"]));
        repo.insert_record("bundles", bundle1);

        let mut bundle2 = HashMap::new();
        bundle2.insert("id".to_string(), json!("bundle2"));
        bundle2.insert("label".to_string(), json!("Pro Pack"));
        bundle2.insert("items".to_string(), json!(["item1"]));
        repo.insert_record("bundles", bundle2);

        // Expand bundles that reference item1 via the "items" multi-relation field
        let items_col = schema.get_collection("items").unwrap();
        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("item1"));
        record.insert("name".to_string(), json!("Widget"));

        let paths = parse_expand("bundles_via_items").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &items_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        let bundles_arr = expand["bundles_via_items"].as_array().unwrap();
        assert_eq!(bundles_arr.len(), 2, "both bundles reference item1");
        let labels: Vec<&str> = bundles_arr
            .iter()
            .map(|b| b["label"].as_str().unwrap())
            .collect();
        assert!(labels.contains(&"Starter Pack"));
        assert!(labels.contains(&"Pro Pack"));
    }

    #[test]
    fn expand_back_relation_wrong_collection_ignored() {
        let (schema, repo) = make_test_collections();

        // "comments" collection's "post" field points to "posts", not "users".
        // Expanding comments_via_post on a *user* record should be silently ignored.
        let users_col = schema.get_collection("users").unwrap();
        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("user1"));
        record.insert("name".to_string(), json!("Alice"));

        let paths = parse_expand("comments_via_post").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &users_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        assert!(
            expand.is_empty(),
            "back-relation pointing to wrong collection should be ignored"
        );
    }

    #[test]
    fn expand_combined_forward_and_back_relation() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("post1"));
        record.insert("title".to_string(), json!("Hello World"));
        record.insert("author".to_string(), json!("user1"));
        record.insert("tags".to_string(), json!(["tag1", "tag2"]));

        // Combine forward relation (author) with back-relation (comments_via_post)
        let paths = parse_expand("author,comments_via_post").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &posts_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        // Forward relation
        assert!(expand.contains_key("author"));
        assert_eq!(expand["author"]["name"], "Alice");

        // Back-relation
        assert!(expand.contains_key("comments_via_post"));
        let comments = expand["comments_via_post"].as_array().unwrap();
        assert_eq!(comments.len(), 2);
    }

    #[test]
    fn expand_back_relation_limit_enforced() {
        let mut schema = MockSchema::new();

        let mut parent = Collection::base(
            "parent",
            vec![Field::new("name", FieldType::Text(TextOptions::default()))],
        );
        parent.id = "col_parent".to_string();

        let mut children = Collection::base(
            "children",
            vec![
                Field::new("label", FieldType::Text(TextOptions::default())),
                Field::new(
                    "parent",
                    FieldType::Relation(RelationOptions {
                        collection_id: "parent".to_string(),
                        max_select: 1,
                        ..Default::default()
                    }),
                ),
            ],
        );
        children.id = "col_children".to_string();

        schema.add_collection(parent);
        schema.add_collection(children);

        let repo = MockRepo::new();

        // Insert more children than MAX_BACK_RELATION_EXPAND
        for i in 0..(MAX_BACK_RELATION_EXPAND + 20) {
            let mut child = HashMap::new();
            child.insert("id".to_string(), json!(format!("child_{i}")));
            child.insert("label".to_string(), json!(format!("Child {i}")));
            child.insert("parent".to_string(), json!("parent1"));
            repo.insert_record("children", child);
        }

        let parent_col = schema.get_collection("parent").unwrap();
        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("parent1"));
        record.insert("name".to_string(), json!("Parent"));

        let paths = parse_expand("children_via_parent").unwrap();
        let mut visited = HashSet::new();
        let expand = expand_record(
            &record,
            &parent_col,
            &paths,
            &repo,
            &schema,
            &mut visited,
            0,
        )
        .unwrap();

        let children_arr = expand["children_via_parent"].as_array().unwrap();
        assert_eq!(
            children_arr.len(),
            MAX_BACK_RELATION_EXPAND,
            "back-relation should be capped at MAX_BACK_RELATION_EXPAND"
        );
    }

    #[test]
    fn expand_back_relation_collection_metadata_present() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("post1"));
        record.insert("title".to_string(), json!("Hello World"));
        record.insert("author".to_string(), json!("user1"));
        record.insert("tags".to_string(), json!(["tag1", "tag2"]));

        let paths = parse_expand("comments_via_post").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &posts_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        let comments = expand["comments_via_post"].as_array().unwrap();
        for comment in comments {
            assert_eq!(
                comment["collectionName"], "comments",
                "back-relation records should have collectionName"
            );
            assert_eq!(
                comment["collectionId"], "col_comments",
                "back-relation records should have collectionId"
            );
        }
    }

    #[test]
    fn expand_back_relation_always_returns_array() {
        let (schema, repo) = make_test_collections();
        let users_col = schema.get_collection("users").unwrap();

        // user1 authored only one post, but back-relations always return arrays
        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("user1"));
        record.insert("name".to_string(), json!("Alice"));

        let paths = parse_expand("posts_via_author").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &users_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        let posts = &expand["posts_via_author"];
        assert!(
            posts.is_array(),
            "back-relations must always produce an array, even for a single result"
        );
        assert_eq!(posts.as_array().unwrap().len(), 1);
    }

    #[test]
    fn expand_records_batch_with_back_relations() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        let mut record1 = HashMap::new();
        record1.insert("id".to_string(), json!("post1"));
        record1.insert("title".to_string(), json!("Hello World"));
        record1.insert("author".to_string(), json!("user1"));
        record1.insert("tags".to_string(), json!(["tag1"]));

        let mut records = vec![record1];
        let paths = parse_expand("comments_via_post").unwrap();
        expand_records(&mut records, &posts_col, &paths, &repo, &schema).unwrap();

        assert!(records[0].contains_key("expand"));
        let expand: HashMap<String, Value> =
            serde_json::from_value(records[0]["expand"].clone()).unwrap();
        assert!(expand.contains_key("comments_via_post"));
        let comments = expand["comments_via_post"].as_array().unwrap();
        assert_eq!(comments.len(), 2);
    }

    #[test]
    fn parse_back_relation_multiple_underscores_in_collection() {
        let (col, field) = parse_back_relation("my_cool_items_via_parent_ref").unwrap();
        assert_eq!(col, "my_cool_items");
        assert_eq!(field, "parent_ref");
    }

    #[test]
    fn expand_non_relation_field_ignored() {
        let (schema, repo) = make_test_collections();
        let posts_col = schema.get_collection("posts").unwrap();

        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("post1"));
        record.insert("title".to_string(), json!("Hello World"));

        // "title" is a text field, not a relation
        let paths = parse_expand("title").unwrap();
        let mut visited = HashSet::new();
        let expand =
            expand_record(&record, &posts_col, &paths, &repo, &schema, &mut visited, 0).unwrap();

        assert!(expand.is_empty());
    }
}
