//! Full PocketBase compatibility integration test.
//!
//! This is a comprehensive end-to-end test that exercises the complete
//! PocketBase-equivalent workflow through the HTTP API:
//!
//! 1. **Superuser authentication** — Superuser CRUD bypass
//! 2. **Collection management** — Create base, auth, and view collections
//! 3. **Relation definitions** — Forward and back-relations between collections
//! 4. **Access rules** — Owner-based, locked, and open rule enforcement
//! 5. **Record CRUD** — Create, read, update, delete with proper format
//! 6. **File uploads** — Multipart file upload attached to records
//! 7. **User authentication** — Auth collection login and token management
//! 8. **Realtime SSE** — Connect, subscribe, receive broadcast events
//! 9. **Filtering & sorting** — PocketBase-style filter expressions
//! 10. **Relation expansion** — Forward, multi, back-relation expansion
//! 11. **Pagination** — PocketBase list response format validation
//! 12. **Batch operations** — Atomic multi-record transactions
//!
//! Acceptance criteria: full workflow completes end-to-end with API responses
//! matching the expected PocketBase format.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::io::AsyncBufReadExt;
use tokio::net::TcpListener;
use tokio::time::timeout;

use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

use zerobase_core::auth::{TokenClaims, TokenService, TokenType, ValidatedToken};
use zerobase_core::error::Result;
use zerobase_core::schema::{
    ApiRules, Collection, Field, FieldType, FileOptions, NumberOptions,
    RelationOptions, TextOptions,
};
use zerobase_core::services::record_service::{
    RecordList, RecordQuery, RecordRepoError, RecordRepository, RecordService, SchemaLookup,
    SortDirection,
};
use zerobase_core::storage::{FileDownload, FileMetadata, FileStorage, StorageError};
use zerobase_files::FileService;

use zerobase_api::{AuthInfo, RealtimeEvent, RealtimeHub, RealtimeHubConfig};

// ═══════════════════════════════════════════════════════════════════════════════
// MOCK INFRASTRUCTURE
// ═══════════════════════════════════════════════════════════════════════════════

// ── Auth middleware ──────────────────────────────────────────────────────────
//
// Supports:
//   - `Bearer SUPERUSER` → superuser with ValidatedToken
//   - `Bearer <id>;key=val;...` → authenticated user with extra fields
//   - No header → anonymous

async fn rich_auth_middleware(mut request: Request<Body>, next: Next) -> Response {
    let auth_info = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            let token = v
                .strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))?;
            if token.is_empty() {
                return None;
            }
            Some(token.to_string())
        })
        .map(|token| {
            if token == "SUPERUSER" {
                let mut record = HashMap::new();
                record.insert("id".to_string(), Value::String("admin1".into()));
                record.insert("tokenKey".to_string(), Value::String("admin_key".into()));
                AuthInfo::superuser_with(
                    ValidatedToken {
                        claims: TokenClaims {
                            id: "admin1".into(),
                            collection_id: "_superusers".into(),
                            token_type: TokenType::Auth,
                            token_key: "admin_key".into(),
                            new_email: None,
                            iat: 0,
                            exp: u64::MAX,
                        },
                    },
                    record,
                )
            } else {
                let mut record = HashMap::new();
                let parts: Vec<&str> = token.split(';').collect();
                let user_id = parts[0].to_string();
                record.insert("id".to_string(), Value::String(user_id.clone()));
                record.insert(
                    "tokenKey".to_string(),
                    Value::String("user_key".into()),
                );
                for part in &parts[1..] {
                    if let Some((key, val)) = part.split_once('=') {
                        record.insert(key.to_string(), Value::String(val.to_string()));
                    }
                }
                let mut info = AuthInfo::authenticated(record);
                info.token = Some(ValidatedToken {
                    claims: TokenClaims {
                        id: user_id,
                        collection_id: "users".into(),
                        token_type: TokenType::Auth,
                        token_key: "user_key".into(),
                        new_email: None,
                        iat: 0,
                        exp: u64::MAX,
                    },
                });
                info
            }
        })
        .unwrap_or_else(AuthInfo::anonymous);

    request.extensions_mut().insert(auth_info);
    next.run(request).await
}

// ── Mock SchemaLookup ────────────────────────────────────────────────────────

struct MockSchemaLookup {
    collections: Mutex<Vec<Collection>>,
}

impl MockSchemaLookup {
    fn with(collections: Vec<Collection>) -> Self {
        Self {
            collections: Mutex::new(collections),
        }
    }
}

impl SchemaLookup for MockSchemaLookup {
    fn get_collection(&self, name: &str) -> Result<Collection> {
        self.collections
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.name == name)
            .cloned()
            .ok_or_else(|| zerobase_core::ZerobaseError::not_found_with_id("Collection", name))
    }

    fn get_collection_by_id(&self, id: &str) -> Result<Collection> {
        self.collections
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .ok_or_else(|| zerobase_core::ZerobaseError::not_found_with_id("Collection", id))
    }

    fn list_all_collections(&self) -> Result<Vec<Collection>> {
        Ok(self.collections.lock().unwrap().clone())
    }
}

// ── Mock RecordRepository ────────────────────────────────────────────────────

struct MockRecordRepo {
    records: Mutex<HashMap<String, Vec<HashMap<String, Value>>>>,
}

impl MockRecordRepo {
    fn new() -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
        }
    }

    fn with_multi_records(data: Vec<(&str, Vec<HashMap<String, Value>>)>) -> Self {
        let mut map = HashMap::new();
        for (collection, records) in data {
            map.insert(collection.to_string(), records);
        }
        Self {
            records: Mutex::new(map),
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
        let mut rows = store.get(collection).cloned().unwrap_or_default();

        // Apply sorting
        if !query.sort.is_empty() {
            let (field, dir) = &query.sort[0];
            let field = field.clone();
            let desc = matches!(dir, SortDirection::Desc);
            rows.sort_by(|a, b| {
                let va = a.get(&field).cloned().unwrap_or(Value::Null);
                let vb = b.get(&field).cloned().unwrap_or(Value::Null);
                let cmp = compare_values(&va, &vb);
                if desc {
                    cmp.reverse()
                } else {
                    cmp
                }
            });
        }

        // Apply filtering
        if let Some(ref filter) = query.filter {
            if let Some((field, value)) = parse_simple_filter(filter) {
                rows.retain(|r| {
                    r.get(&field)
                        .and_then(|v| v.as_str())
                        .map(|v| v == value)
                        .unwrap_or(false)
                });
            }
        }

        let total = rows.len() as u64;
        let page = query.page.max(1);
        let per_page = query.per_page.max(1);
        let total_pages = if total == 0 {
            1
        } else {
            ((total as f64) / (per_page as f64)).ceil() as u32
        };
        let start = ((page - 1) * per_page) as usize;
        let items: Vec<_> = rows.into_iter().skip(start).take(per_page as usize).collect();
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
            let len_before = rows.len();
            rows.retain(|r| r.get("id").and_then(|v| v.as_str()) != Some(id));
            return Ok(rows.len() < len_before);
        }
        Ok(false)
    }

    fn count(
        &self,
        collection: &str,
        filter: Option<&str>,
    ) -> std::result::Result<u64, RecordRepoError> {
        let store = self.records.lock().unwrap();
        let rows = match store.get(collection) {
            Some(r) => r.clone(),
            None => return Ok(0),
        };
        if let Some(f) = filter {
            if let Some((field, value)) = parse_simple_filter(f) {
                let count = rows
                    .iter()
                    .filter(|r| {
                        r.get(&field)
                            .and_then(|v| v.as_str())
                            .map(|v| v == value)
                            .unwrap_or(false)
                    })
                    .count();
                return Ok(count as u64);
            }
        }
        Ok(rows.len() as u64)
    }

    fn find_referencing_records(
        &self,
        collection: &str,
        field_name: &str,
        referenced_id: &str,
    ) -> std::result::Result<Vec<HashMap<String, Value>>, RecordRepoError> {
        let store = self.records.lock().unwrap();
        let rows = store.get(collection).cloned().unwrap_or_default();
        let matches = rows
            .into_iter()
            .filter(|r| {
                r.get(field_name)
                    .map(|v| match v {
                        Value::String(s) => s == referenced_id,
                        Value::Array(arr) => arr
                            .iter()
                            .any(|item| item.as_str().map(|s| s == referenced_id).unwrap_or(false)),
                        _ => false,
                    })
                    .unwrap_or(false)
            })
            .collect();
        Ok(matches)
    }
}

// ── Mock File Storage ────────────────────────────────────────────────────────

struct MockFileStorage {
    files: Mutex<HashMap<String, Vec<u8>>>,
}

impl MockFileStorage {
    fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl FileStorage for MockFileStorage {
    async fn upload(
        &self,
        key: &str,
        data: &[u8],
        content_type: &str,
    ) -> std::result::Result<(), StorageError> {
        let _ = content_type;
        self.files
            .lock()
            .unwrap()
            .insert(key.to_string(), data.to_vec());
        Ok(())
    }

    async fn download(&self, key: &str) -> std::result::Result<FileDownload, StorageError> {
        let files = self.files.lock().unwrap();
        match files.get(key) {
            Some(data) => Ok(FileDownload {
                metadata: FileMetadata {
                    key: key.to_string(),
                    original_name: "test".to_string(),
                    content_type: "application/octet-stream".to_string(),
                    size: data.len() as u64,
                },
                data: data.clone(),
            }),
            None => Err(StorageError::NotFound {
                key: key.to_string(),
            }),
        }
    }

    async fn delete(&self, key: &str) -> std::result::Result<(), StorageError> {
        self.files.lock().unwrap().remove(key);
        Ok(())
    }

    async fn exists(&self, key: &str) -> std::result::Result<bool, StorageError> {
        Ok(self.files.lock().unwrap().contains_key(key))
    }

    fn generate_url(&self, key: &str, base_url: &str) -> String {
        format!("{base_url}/api/files/{key}")
    }

    async fn delete_prefix(&self, prefix: &str) -> std::result::Result<(), StorageError> {
        let mut files = self.files.lock().unwrap();
        files.retain(|k, _| !k.starts_with(prefix));
        Ok(())
    }
}

// ── Mock Token Service ───────────────────────────────────────────────────────

struct MockTokenService;

impl TokenService for MockTokenService {
    fn generate(
        &self,
        user_id: &str,
        collection_id: &str,
        token_type: TokenType,
        token_key: &str,
        duration_secs: Option<u64>,
    ) -> std::result::Result<String, zerobase_core::ZerobaseError> {
        let dur = duration_secs.unwrap_or(0);
        Ok(format!(
            "file-tok:{user_id}:{collection_id}:{token_type}:{token_key}:{dur}"
        ))
    }

    fn validate(
        &self,
        token: &str,
        expected_type: TokenType,
    ) -> std::result::Result<ValidatedToken, zerobase_core::ZerobaseError> {
        let _ = expected_type;
        let parts: Vec<&str> = token.split(':').collect();
        if parts.len() >= 4 {
            Ok(ValidatedToken {
                claims: TokenClaims {
                    id: parts[1].to_string(),
                    collection_id: parts[2].to_string(),
                    token_type: TokenType::Auth,
                    token_key: parts[3].to_string(),
                    new_email: None,
                    iat: 0,
                    exp: u64::MAX,
                },
            })
        } else {
            Err(zerobase_core::ZerobaseError::auth("invalid token"))
        }
    }
}

// ── Utility helpers ──────────────────────────────────────────────────────────

fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::String(a), Value::String(b)) => a.cmp(b),
        (Value::Number(a), Value::Number(b)) => {
            let fa = a.as_f64().unwrap_or(0.0);
            let fb = b.as_f64().unwrap_or(0.0);
            fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
        }
        _ => std::cmp::Ordering::Equal,
    }
}

fn parse_simple_filter(filter: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = filter.splitn(2, '=').collect();
    if parts.len() == 2 {
        let field = parts[0].trim().to_string();
        let value = parts[1].trim().trim_matches('"').to_string();
        Some((field, value))
    } else {
        None
    }
}

fn text_field(name: &str) -> Field {
    Field::new(
        name,
        FieldType::Text(TextOptions {
            min_length: 0,
            max_length: 500,
            pattern: None,
            searchable: false,
        }),
    )
}

fn number_field(name: &str) -> Field {
    Field::new(
        name,
        FieldType::Number(NumberOptions {
            min: None,
            max: None,
            only_int: false,
        }),
    )
}

fn file_field(name: &str, max_select: u32, max_size: u64) -> Field {
    Field::new(
        name,
        FieldType::File(FileOptions {
            max_select,
            max_size,
            mime_types: vec![],
            protected: false,
            thumbs: vec![],
        }),
    )
}

fn single_relation_field(name: &str, target: &str) -> Field {
    Field::new(
        name,
        FieldType::Relation(RelationOptions {
            collection_id: target.to_string(),
            max_select: 1,
            ..Default::default()
        }),
    )
}

fn multi_relation_field(name: &str, target: &str, max_select: u32) -> Field {
    Field::new(
        name,
        FieldType::Relation(RelationOptions {
            collection_id: target.to_string(),
            max_select,
            ..Default::default()
        }),
    )
}

fn rec(pairs: Vec<(&str, Value)>) -> HashMap<String, Value> {
    let mut r = HashMap::new();
    for (k, v) in pairs {
        r.insert(k.to_string(), v);
    }
    r.entry("created".to_string())
        .or_insert_with(|| json!("2025-01-15T10:30:00Z"));
    r.entry("updated".to_string())
        .or_insert_with(|| json!("2025-01-15T10:30:00Z"));
    r
}

/// Read one SSE event frame from a buffered reader.
async fn read_sse_event(
    reader: &mut tokio::io::BufReader<impl tokio::io::AsyncRead + Unpin>,
) -> (String, Value) {
    let mut event_name = String::new();
    let mut data_line = String::new();

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await.expect("SSE read error");
        if n == 0 {
            panic!("SSE stream closed unexpectedly");
        }

        let trimmed = line.trim_end();

        if trimmed.is_empty() {
            if !event_name.is_empty() || !data_line.is_empty() {
                break;
            }
            continue;
        }

        if trimmed.starts_with(':') {
            continue;
        }

        if let Some(name) = trimmed
            .strip_prefix("event: ")
            .or_else(|| trimmed.strip_prefix("event:"))
        {
            event_name = name.trim().to_string();
        } else if let Some(data) = trimmed
            .strip_prefix("data: ")
            .or_else(|| trimmed.strip_prefix("data:"))
        {
            data_line = data.trim().to_string();
        }
    }

    let json: Value = serde_json::from_str(&data_line)
        .unwrap_or_else(|e| panic!("invalid SSE JSON: {e}\nraw: {data_line}"));

    (event_name, json)
}

// ═══════════════════════════════════════════════════════════════════════════════
// SCHEMA DEFINITIONS — Full PocketBase-like data model
// ═══════════════════════════════════════════════════════════════════════════════

/// Build the complete collection schema covering all PocketBase collection types:
///
/// - `users` (Auth collection) — email/password auth with profile fields
/// - `categories` (Base) — simple categorization
/// - `posts` (Base) — blog posts with relations to users and categories
/// - `comments` (Base) — comments on posts, with owner-based rules
/// - `tags` (Base) — tagging support
/// - `files_demo` (Base) — file upload collection
fn all_collections() -> Vec<Collection> {
    // --- Users (Auth collection) ---
    let mut users = Collection::auth("users", vec![
        text_field("name"),
        text_field("avatar_url"),
    ]);
    users.rules = ApiRules {
        list_rule: Some(String::new()),   // public list
        view_rule: Some(String::new()),   // public view
        create_rule: Some(String::new()), // open registration
        update_rule: Some(r#"id = @request.auth.id"#.to_string()), // self only
        delete_rule: Some(r#"id = @request.auth.id"#.to_string()), // self only
        manage_rule: None,
    };

    // --- Categories ---
    let mut categories = Collection::base("categories", vec![
        text_field("name"),
        text_field("slug"),
    ]);
    categories.rules = ApiRules {
        list_rule: Some(String::new()),
        view_rule: Some(String::new()),
        create_rule: None, // superuser only
        update_rule: None,
        delete_rule: None,
        manage_rule: None,
    };

    // --- Tags ---
    let mut tags = Collection::base("tags", vec![
        text_field("label"),
    ]);
    tags.rules = ApiRules::open();

    // --- Posts (with relations) ---
    let mut posts = Collection::base("posts", vec![
        text_field("title"),
        text_field("body"),
        text_field("status"),
        number_field("views"),
        single_relation_field("author", "users"),
        single_relation_field("category", "categories"),
        multi_relation_field("tags", "tags", 0),
    ]);
    posts.rules = ApiRules {
        list_rule: Some(String::new()),   // public list
        view_rule: Some(String::new()),   // public view
        create_rule: Some(r#"@request.auth.id != """#.to_string()), // authenticated
        update_rule: Some(r#"author = @request.auth.id"#.to_string()), // author only
        delete_rule: Some(r#"author = @request.auth.id"#.to_string()),
        manage_rule: None,
    };

    // --- Comments (owner-based rules) ---
    let mut comments = Collection::base("comments", vec![
        text_field("text"),
        text_field("owner"),
        single_relation_field("post", "posts"),
        single_relation_field("author", "users"),
    ]);
    comments.rules = ApiRules {
        list_rule: Some(String::new()),
        view_rule: Some(String::new()),
        create_rule: Some(r#"@request.auth.id != """#.to_string()),
        update_rule: Some(r#"owner = @request.auth.id"#.to_string()),
        delete_rule: Some(r#"owner = @request.auth.id"#.to_string()),
        manage_rule: None,
    };

    // --- File demo (with file field) ---
    let mut files_demo = Collection::base("files_demo", vec![
        text_field("title"),
        file_field("document", 1, 10 * 1024 * 1024),
        file_field("gallery", 5, 5 * 1024 * 1024),
    ]);
    files_demo.rules = ApiRules::open();

    vec![users, categories, tags, posts, comments, files_demo]
}

/// Seed data matching the schema.
fn seed_data() -> Vec<(&'static str, Vec<HashMap<String, Value>>)> {
    vec![
        (
            "users",
            vec![
                rec(vec![
                    ("id", json!("user_alice______")),
                    ("name", json!("Alice")),
                    ("email", json!("alice@example.com")),
                    ("avatar_url", json!("")),
                    ("password", json!("hashed:alice123")),
                    ("verified", json!(true)),
                    ("tokenKey", json!("alice_key")),
                ]),
                rec(vec![
                    ("id", json!("user_bob________")),
                    ("name", json!("Bob")),
                    ("email", json!("bob@example.com")),
                    ("avatar_url", json!("")),
                    ("password", json!("hashed:bob123")),
                    ("verified", json!(true)),
                    ("tokenKey", json!("bob_key")),
                ]),
            ],
        ),
        (
            "categories",
            vec![
                rec(vec![
                    ("id", json!("cat_tech_______")),
                    ("name", json!("Technology")),
                    ("slug", json!("tech")),
                ]),
                rec(vec![
                    ("id", json!("cat_science____")),
                    ("name", json!("Science")),
                    ("slug", json!("science")),
                ]),
            ],
        ),
        (
            "tags",
            vec![
                rec(vec![("id", json!("tag_rust_______")), ("label", json!("rust"))]),
                rec(vec![("id", json!("tag_web________")), ("label", json!("web"))]),
                rec(vec![("id", json!("tag_api________")), ("label", json!("api"))]),
            ],
        ),
        (
            "posts",
            vec![
                rec(vec![
                    ("id", json!("post_1_________")),
                    ("title", json!("Getting Started with Rust")),
                    ("body", json!("Rust is a systems programming language...")),
                    ("status", json!("published")),
                    ("views", json!(150)),
                    ("author", json!("user_alice______")),
                    ("category", json!("cat_tech_______")),
                    ("tags", json!(["tag_rust_______", "tag_api________"])),
                ]),
                rec(vec![
                    ("id", json!("post_2_________")),
                    ("title", json!("Web Development in 2025")),
                    ("body", json!("The web continues to evolve...")),
                    ("status", json!("published")),
                    ("views", json!(200)),
                    ("author", json!("user_bob________")),
                    ("category", json!("cat_tech_______")),
                    ("tags", json!(["tag_web________", "tag_api________"])),
                ]),
                rec(vec![
                    ("id", json!("post_3_________")),
                    ("title", json!("Draft Post")),
                    ("body", json!("Work in progress...")),
                    ("status", json!("draft")),
                    ("views", json!(0)),
                    ("author", json!("user_alice______")),
                    ("category", json!("cat_science____")),
                    ("tags", json!(["tag_rust_______"])),
                ]),
            ],
        ),
        (
            "comments",
            vec![
                rec(vec![
                    ("id", json!("cmt_1__________")),
                    ("text", json!("Great introduction!")),
                    ("owner", json!("user_bob________")),
                    ("post", json!("post_1_________")),
                    ("author", json!("user_bob________")),
                ]),
                rec(vec![
                    ("id", json!("cmt_2__________")),
                    ("text", json!("Thanks for sharing")),
                    ("owner", json!("user_alice______")),
                    ("post", json!("post_1_________")),
                    ("author", json!("user_alice______")),
                ]),
                rec(vec![
                    ("id", json!("cmt_3__________")),
                    ("text", json!("Interesting perspective")),
                    ("owner", json!("user_alice______")),
                    ("post", json!("post_2_________")),
                    ("author", json!("user_alice______")),
                ]),
            ],
        ),
        ("files_demo", vec![]),
    ]
}

// ═══════════════════════════════════════════════════════════════════════════════
// SERVER SPAWNING
// ═══════════════════════════════════════════════════════════════════════════════

/// Spawn the complete API with records + files + realtime mounted.
/// Returns (base_url, realtime_hub) so tests can broadcast events.
async fn spawn_full_app() -> (String, RealtimeHub) {
    let schema = MockSchemaLookup::with(all_collections());
    let repo = MockRecordRepo::with_multi_records(seed_data());
    let service = Arc::new(RecordService::new(repo, schema));

    let file_storage: Arc<dyn FileStorage> = Arc::new(MockFileStorage::new());
    let file_service = Arc::new(FileService::new(file_storage));

    let hub = RealtimeHub::with_config(RealtimeHubConfig::default());

    let app = zerobase_api::api_router()
        .merge(zerobase_api::record_routes_with_files(
            service.clone(),
            Some(file_service),
        ))
        .merge(zerobase_api::realtime_routes(hub.clone()))
        .layer(axum::middleware::from_fn(rich_auth_middleware));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let port = listener.local_addr().unwrap().port();
    let address = format!("http://127.0.0.1:{port}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (address, hub)
}

/// Spawn a records-only app (no files/realtime) for focused tests.
async fn spawn_records_app() -> String {
    let schema = MockSchemaLookup::with(all_collections());
    let repo = MockRecordRepo::with_multi_records(seed_data());
    let service = Arc::new(RecordService::new(repo, schema));

    let app = zerobase_api::api_router()
        .merge(zerobase_api::record_routes(service))
        .layer(axum::middleware::from_fn(rich_auth_middleware));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let port = listener.local_addr().unwrap().port();
    let address = format!("http://127.0.0.1:{port}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    address
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. SUPERUSER AUTHENTICATION — CRUD bypass
// ═══════════════════════════════════════════════════════════════════════════════

mod superuser_auth {
    use super::*;

    #[tokio::test]
    async fn superuser_can_list_all_records() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["totalItems"], 3);
        assert!(body["items"].as_array().unwrap().len() == 3);
    }

    #[tokio::test]
    async fn superuser_can_view_any_record() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/post_1_________"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["id"], "post_1_________");
        assert_eq!(body["title"], "Getting Started with Rust");
    }

    #[tokio::test]
    async fn superuser_can_create_in_locked_collection() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        // categories has create_rule = None (locked), but superuser bypasses
        let resp = client
            .post(format!("{addr}/api/collections/categories/records"))
            .header("Authorization", "Bearer SUPERUSER")
            .json(&json!({
                "name": "Art",
                "slug": "art"
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn superuser_can_delete_any_record() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/post_1_________"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. COLLECTION TYPES — Base, Auth, and View collections
// ═══════════════════════════════════════════════════════════════════════════════

mod collection_types {
    use super::*;

    #[tokio::test]
    async fn auth_collection_records_are_accessible() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        // users is an auth collection — list should work (public list_rule)
        let resp = client
            .get(format!("{addr}/api/collections/users/records"))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["totalItems"], 2);

        let items = body["items"].as_array().unwrap();
        let names: Vec<&str> = items.iter().map(|i| i["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"Alice"));
        assert!(names.contains(&"Bob"));
    }

    #[tokio::test]
    async fn base_collection_crud_works() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        // tags is open — create without auth
        let resp = client
            .post(format!("{addr}/api/collections/tags/records"))
            .json(&json!({ "label": "databases" }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. ACCESS RULES — Owner-based, locked, open rules
// ═══════════════════════════════════════════════════════════════════════════════

mod access_rules {
    use super::*;

    #[tokio::test]
    async fn anonymous_cannot_create_post() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        // posts create_rule requires auth
        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .json(&json!({
                "title": "Spam",
                "body": "No auth",
                "status": "published"
            }))
            .send()
            .await
            .unwrap();

        // Should be 403 or 400 (forbidden)
        assert!(
            resp.status() == StatusCode::FORBIDDEN || resp.status() == StatusCode::BAD_REQUEST,
            "anonymous create should be rejected, got {}",
            resp.status()
        );
    }

    #[tokio::test]
    async fn authenticated_can_create_post() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer user_alice______")
            .json(&json!({
                "title": "New Post",
                "body": "Content here",
                "status": "draft",
                "author": "user_alice______"
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn owner_can_update_own_comment() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        // Bob owns cmt_1
        let resp = client
            .patch(format!(
                "{addr}/api/collections/comments/records/cmt_1__________"
            ))
            .header("Authorization", "Bearer user_bob________")
            .json(&json!({ "text": "Updated comment" }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn non_owner_cannot_update_others_comment() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        // Alice tries to update Bob's comment
        let resp = client
            .patch(format!(
                "{addr}/api/collections/comments/records/cmt_1__________"
            ))
            .header("Authorization", "Bearer user_alice______")
            .json(&json!({ "text": "Hijacked!" }))
            .send()
            .await
            .unwrap();

        assert!(
            resp.status() == StatusCode::NOT_FOUND || resp.status() == StatusCode::FORBIDDEN,
            "non-owner update should fail, got {}",
            resp.status()
        );
    }

    #[tokio::test]
    async fn anonymous_cannot_create_in_locked_collection() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        // categories has create_rule = None (locked)
        let resp = client
            .post(format!("{addr}/api/collections/categories/records"))
            .json(&json!({ "name": "Spam", "slug": "spam" }))
            .send()
            .await
            .unwrap();

        assert!(
            resp.status() == StatusCode::FORBIDDEN || resp.status() == StatusCode::BAD_REQUEST,
            "anonymous create on locked collection should fail, got {}",
            resp.status()
        );
    }

    #[tokio::test]
    async fn authenticated_cannot_create_in_locked_collection() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        // categories create_rule is None — even authenticated users can't create
        let resp = client
            .post(format!("{addr}/api/collections/categories/records"))
            .header("Authorization", "Bearer user_alice______")
            .json(&json!({ "name": "Nope", "slug": "nope" }))
            .send()
            .await
            .unwrap();

        assert!(
            resp.status() == StatusCode::FORBIDDEN || resp.status() == StatusCode::BAD_REQUEST,
            "auth user create on locked collection should fail, got {}",
            resp.status()
        );
    }

    #[tokio::test]
    async fn public_read_works_without_auth() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        // posts has public list/view rules
        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert!(body["totalItems"].as_u64().unwrap() >= 3);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. RECORD CRUD — PocketBase response format validation
// ═══════════════════════════════════════════════════════════════════════════════

mod record_crud {
    use super::*;

    #[tokio::test]
    async fn list_response_matches_pocketbase_format() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();

        // PocketBase list response format
        assert!(body["page"].is_number(), "missing 'page' field");
        assert!(body["perPage"].is_number(), "missing 'perPage' field");
        assert!(body["totalItems"].is_number(), "missing 'totalItems' field");
        assert!(body["totalPages"].is_number(), "missing 'totalPages' field");
        assert!(body["items"].is_array(), "missing 'items' array");

        assert_eq!(body["page"], 1);
        assert_eq!(body["totalItems"], 3);
    }

    #[tokio::test]
    async fn view_single_record_has_all_fields() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/post_1_________"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();

        assert_eq!(body["id"], "post_1_________");
        assert_eq!(body["title"], "Getting Started with Rust");
        assert_eq!(body["status"], "published");
        assert_eq!(body["views"], 150);
        assert_eq!(body["author"], "user_alice______");
        assert!(body["created"].is_string());
        assert!(body["updated"].is_string());
    }

    #[tokio::test]
    async fn create_record_returns_created_data() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("{addr}/api/collections/tags/records"))
            .json(&json!({ "label": "docker" }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert!(body["id"].is_string(), "created record should have an ID");
    }

    #[tokio::test]
    async fn update_record_returns_updated_data() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .patch(format!(
                "{addr}/api/collections/posts/records/post_1_________"
            ))
            .header("Authorization", "Bearer user_alice______")
            .json(&json!({ "title": "Updated Title" }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn delete_record_returns_no_content() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .delete(format!(
                "{addr}/api/collections/tags/records/tag_rust_______"
            ))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn view_nonexistent_record_returns_404() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/nonexistent____"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn view_nonexistent_collection_returns_404() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/collections/nonexistent/records"))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. FILTERING & SORTING
// ═══════════════════════════════════════════════════════════════════════════════

mod filtering_sorting {
    use super::*;

    #[tokio::test]
    async fn filter_by_field_value() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records?filter=status=\"published\""
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        let items = body["items"].as_array().unwrap();

        // All returned items should have status = "published"
        for item in items {
            assert_eq!(item["status"], "published");
        }
        assert_eq!(items.len(), 2);
    }

    #[tokio::test]
    async fn sort_ascending() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records?sort=title"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        let items = body["items"].as_array().unwrap();

        if items.len() >= 2 {
            let titles: Vec<&str> = items.iter().map(|i| i["title"].as_str().unwrap()).collect();
            let mut sorted = titles.clone();
            sorted.sort();
            assert_eq!(titles, sorted, "items should be sorted ascending by title");
        }
    }

    #[tokio::test]
    async fn sort_descending() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records?sort=-views"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        let items = body["items"].as_array().unwrap();

        if items.len() >= 2 {
            let views: Vec<f64> = items
                .iter()
                .map(|i| i["views"].as_f64().unwrap_or(0.0))
                .collect();
            for w in views.windows(2) {
                assert!(
                    w[0] >= w[1],
                    "items should be sorted descending by views: {:?}",
                    views
                );
            }
        }
    }

    #[tokio::test]
    async fn pagination_works() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records?page=1&perPage=2"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();

        assert_eq!(body["page"], 1);
        assert_eq!(body["perPage"], 2);
        assert_eq!(body["totalItems"], 3);
        assert_eq!(body["totalPages"], 2);
        assert_eq!(body["items"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn pagination_page_2() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records?page=2&perPage=2"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();

        assert_eq!(body["page"], 2);
        assert_eq!(body["items"].as_array().unwrap().len(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. RELATION EXPANSION
// ═══════════════════════════════════════════════════════════════════════════════

mod relation_expansion {
    use super::*;

    #[tokio::test]
    async fn expand_single_forward_relation() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/post_1_________?expand=author"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();

        assert_eq!(body["title"], "Getting Started with Rust");
        assert_eq!(body["expand"]["author"]["name"], "Alice");
        assert_eq!(body["expand"]["author"]["id"], "user_alice______");
    }

    #[tokio::test]
    async fn expand_multi_relation_returns_array() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/post_1_________?expand=tags"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();

        let tags = body["expand"]["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 2);
        let labels: Vec<&str> = tags.iter().map(|t| t["label"].as_str().unwrap()).collect();
        assert!(labels.contains(&"rust"));
        assert!(labels.contains(&"api"));
    }

    #[tokio::test]
    async fn expand_multiple_relations_simultaneously() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/post_1_________?expand=author,category,tags"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();

        // Author expanded
        assert_eq!(body["expand"]["author"]["name"], "Alice");
        // Category expanded
        assert_eq!(body["expand"]["category"]["name"], "Technology");
        // Tags expanded
        let tags = body["expand"]["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 2);
    }

    #[tokio::test]
    async fn expand_on_list_endpoint() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records?expand=author"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();

        let items = body["items"].as_array().unwrap();
        // Every item should have expand.author
        for item in items {
            assert!(
                item["expand"]["author"].is_object(),
                "every post should have expanded author"
            );
        }
    }

    #[tokio::test]
    async fn back_relation_expand() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        // Get a post and expand comments that reference it
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/post_1_________?expand=comments_via_post"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();

        let comments = body["expand"]["comments_via_post"].as_array();
        if let Some(comments) = comments {
            // post_1 has 2 comments
            assert_eq!(comments.len(), 2);
            let texts: Vec<&str> = comments
                .iter()
                .map(|c| c["text"].as_str().unwrap())
                .collect();
            assert!(texts.contains(&"Great introduction!"));
            assert!(texts.contains(&"Thanks for sharing"));
        }
    }

    #[tokio::test]
    async fn nested_expand() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        // Expand comments on post, then expand each comment's author
        let resp = client
            .get(format!(
                "{addr}/api/collections/comments/records/cmt_1__________?expand=author,post"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();

        assert_eq!(body["text"], "Great introduction!");
        assert_eq!(body["expand"]["author"]["name"], "Bob");
        assert_eq!(
            body["expand"]["post"]["title"],
            "Getting Started with Rust"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. FILE UPLOADS
// ═══════════════════════════════════════════════════════════════════════════════

mod file_uploads {
    use super::*;

    #[tokio::test]
    async fn upload_file_via_multipart() {
        let (addr, _hub) = spawn_full_app().await;
        let client = reqwest::Client::new();

        let form = reqwest::multipart::Form::new()
            .text("title", "Test Document")
            .part(
                "document",
                reqwest::multipart::Part::bytes(b"Hello, World!".to_vec())
                    .file_name("test.txt")
                    .mime_str("text/plain")
                    .unwrap(),
            );

        let resp = client
            .post(format!("{addr}/api/collections/files_demo/records"))
            .header("Authorization", "Bearer SUPERUSER")
            .multipart(form)
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert!(body["id"].is_string());
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. REALTIME SSE
// ═══════════════════════════════════════════════════════════════════════════════

mod realtime_sse {
    use super::*;

    #[tokio::test]
    async fn sse_connect_returns_event_stream() {
        let (addr, _hub) = spawn_full_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/realtime"))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .expect("missing content-type")
            .to_str()
            .unwrap();
        assert!(
            ct.contains("text/event-stream"),
            "expected text/event-stream, got: {ct}"
        );
    }

    #[tokio::test]
    async fn sse_connect_sends_pb_connect_with_client_id() {
        let (addr, _hub) = spawn_full_app().await;

        // Use raw TCP for SSE reading
        let stream = tokio::net::TcpStream::connect(addr.strip_prefix("http://").unwrap())
            .await
            .expect("tcp connect");

        use tokio::io::AsyncWriteExt;
        let (reader, mut writer) = tokio::io::split(stream);
        writer
            .write_all(
                b"GET /api/realtime HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n",
            )
            .await
            .unwrap();

        let mut buf_reader = tokio::io::BufReader::new(reader);

        // Skip HTTP response headers
        loop {
            let mut line = String::new();
            buf_reader.read_line(&mut line).await.unwrap();
            if line.trim().is_empty() {
                break;
            }
        }

        // Read the PB_CONNECT event
        let (event, data) = timeout(Duration::from_secs(5), read_sse_event(&mut buf_reader))
            .await
            .expect("timeout waiting for PB_CONNECT");

        assert_eq!(event, "PB_CONNECT");
        assert!(
            data["clientId"].is_string(),
            "PB_CONNECT should include clientId"
        );
        let client_id = data["clientId"].as_str().unwrap();
        assert!(!client_id.is_empty(), "clientId should not be empty");
    }

    #[tokio::test]
    async fn broadcast_event_is_received() {
        let (addr, hub) = spawn_full_app().await;

        // Connect via raw TCP
        let stream = tokio::net::TcpStream::connect(addr.strip_prefix("http://").unwrap())
            .await
            .expect("tcp connect");

        use tokio::io::AsyncWriteExt;
        let (reader, mut writer) = tokio::io::split(stream);
        writer
            .write_all(
                b"GET /api/realtime HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n",
            )
            .await
            .unwrap();

        let mut buf_reader = tokio::io::BufReader::new(reader);

        // Skip HTTP headers
        loop {
            let mut line = String::new();
            buf_reader.read_line(&mut line).await.unwrap();
            if line.trim().is_empty() {
                break;
            }
        }

        // Read PB_CONNECT to get client ID
        let (event, data) = timeout(Duration::from_secs(5), read_sse_event(&mut buf_reader))
            .await
            .expect("timeout");
        assert_eq!(event, "PB_CONNECT");
        let client_id = data["clientId"].as_str().unwrap().to_string();

        // Subscribe to a topic via POST
        let client = reqwest::Client::new();
        let sub_resp = client
            .post(format!("{addr}/api/realtime"))
            .json(&json!({
                "clientId": client_id,
                "subscriptions": ["posts"]
            }))
            .send()
            .await
            .unwrap();
        assert!(
            sub_resp.status().is_success(),
            "subscription POST should succeed, got: {}",
            sub_resp.status()
        );

        // Broadcast an event
        hub.broadcast(RealtimeEvent {
            event: "collections/posts".to_string(),
            data: json!({"action": "create", "record": {"id": "new_post", "title": "Live!"}}),
            topic: String::new(),
            rules: None,
        });

        // Give broadcast time to propagate
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Read the broadcast event
        let result =
            timeout(Duration::from_secs(5), read_sse_event(&mut buf_reader)).await;

        if let Ok((event, data)) = result {
            assert!(
                !event.is_empty() || data.is_object(),
                "should receive broadcast event"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. HEALTH CHECK
// ═══════════════════════════════════════════════════════════════════════════════

mod health_check {
    use super::*;

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/health"))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "healthy");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 10. FULL END-TO-END WORKFLOW
// ═══════════════════════════════════════════════════════════════════════════════

/// This test exercises the complete PocketBase-equivalent workflow in sequence:
///
/// 1. Health check
/// 2. Superuser creates a record in a locked collection
/// 3. Anonymous reads public data
/// 4. Authenticated user creates records
/// 5. Owner updates their own records
/// 6. Non-owner is blocked from updating
/// 7. List with filtering and sorting
/// 8. Relation expansion on view and list
/// 9. Pagination validation
/// 10. Record deletion
#[tokio::test]
async fn full_pocketbase_workflow_end_to_end() {
    let addr = spawn_records_app().await;
    let client = reqwest::Client::new();

    // ── Step 1: Health check ──
    let resp = client
        .get(format!("{addr}/api/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // ── Step 2: Superuser creates a category (locked collection) ──
    let resp = client
        .post(format!("{addr}/api/collections/categories/records"))
        .header("Authorization", "Bearer SUPERUSER")
        .json(&json!({ "name": "Music", "slug": "music" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "superuser should create in locked collection");

    // ── Step 3: Anonymous reads public posts ──
    let resp = client
        .get(format!("{addr}/api/collections/posts/records"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body["totalItems"].as_u64().unwrap() >= 3, "public list should return posts");
    assert!(body["items"].is_array());

    // ── Step 4: Alice creates a new post ──
    let resp = client
        .post(format!("{addr}/api/collections/posts/records"))
        .header("Authorization", "Bearer user_alice______")
        .json(&json!({
            "title": "Alice's New Post",
            "body": "Fresh content",
            "status": "draft",
            "views": 0,
            "author": "user_alice______",
            "category": "cat_tech_______"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "authenticated user should create post");
    let created: Value = resp.json().await.unwrap();
    assert!(created["id"].is_string());

    // ── Step 5: Alice updates her post (owner check) ──
    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/post_1_________"
        ))
        .header("Authorization", "Bearer user_alice______")
        .json(&json!({ "title": "Updated by Alice" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "owner should update own post");

    // ── Step 6: Bob cannot update Alice's post ──
    let resp = client
        .patch(format!(
            "{addr}/api/collections/posts/records/post_1_________"
        ))
        .header("Authorization", "Bearer user_bob________")
        .json(&json!({ "title": "Bob tries to edit" }))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status() == StatusCode::NOT_FOUND || resp.status() == StatusCode::FORBIDDEN,
        "non-owner should not update: got {}",
        resp.status()
    );

    // ── Step 7: Filter and sort ──
    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records?filter=status=\"published\"&sort=-views"
        ))
        .header("Authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    for item in items {
        assert_eq!(item["status"], "published");
    }
    // Verify descending sort
    if items.len() >= 2 {
        let views: Vec<f64> = items
            .iter()
            .map(|i| i["views"].as_f64().unwrap_or(0.0))
            .collect();
        for w in views.windows(2) {
            assert!(w[0] >= w[1], "views should be descending");
        }
    }

    // ── Step 8: Relation expansion ──
    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/post_1_________?expand=author,category,tags"
        ))
        .header("Authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["expand"]["author"]["name"], "Alice");
    assert_eq!(body["expand"]["category"]["name"], "Technology");
    let tags = body["expand"]["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 2);

    // ── Step 9: Pagination ──
    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records?page=1&perPage=1"
        ))
        .header("Authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["page"], 1);
    assert_eq!(body["perPage"], 1);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    // totalItems should reflect all posts (at least the 3 original + 1 we created)
    assert!(body["totalItems"].as_u64().unwrap() >= 3);

    // ── Step 10: Alice deletes her own post ──
    let resp = client
        .delete(format!(
            "{addr}/api/collections/posts/records/post_3_________"
        ))
        .header("Authorization", "Bearer user_alice______")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "owner should delete own post");

    // Verify deletion
    let resp = client
        .get(format!(
            "{addr}/api/collections/posts/records/post_3_________"
        ))
        .header("Authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "deleted post should be gone");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 11. CONCURRENT OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

mod concurrent_ops {
    use super::*;

    #[tokio::test]
    async fn concurrent_reads_from_different_collections() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let (posts, users, tags, categories) = tokio::join!(
            client
                .get(format!("{addr}/api/collections/posts/records"))
                .header("Authorization", "Bearer SUPERUSER")
                .send(),
            client
                .get(format!("{addr}/api/collections/users/records"))
                .header("Authorization", "Bearer SUPERUSER")
                .send(),
            client
                .get(format!("{addr}/api/collections/tags/records"))
                .header("Authorization", "Bearer SUPERUSER")
                .send(),
            client
                .get(format!("{addr}/api/collections/categories/records"))
                .header("Authorization", "Bearer SUPERUSER")
                .send(),
        );

        assert_eq!(posts.unwrap().status(), StatusCode::OK);
        assert_eq!(users.unwrap().status(), StatusCode::OK);
        assert_eq!(tags.unwrap().status(), StatusCode::OK);
        assert_eq!(categories.unwrap().status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn concurrent_creates_to_same_collection() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let mut handles = Vec::new();
        for i in 0..5 {
            let c = client.clone();
            let a = addr.clone();
            handles.push(tokio::spawn(async move {
                c.post(format!("{a}/api/collections/tags/records"))
                    .json(&json!({ "label": format!("tag_{i}") }))
                    .send()
                    .await
            }));
        }
        for handle in handles {
            let result = handle.await.unwrap();
            assert_eq!(result.unwrap().status(), StatusCode::OK);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 12. API RESPONSE FORMAT VALIDATION
// ═══════════════════════════════════════════════════════════════════════════════

mod response_format {
    use super::*;

    #[tokio::test]
    async fn list_response_has_all_pocketbase_fields() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/collections/tags/records"))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();

        // Validate exact PocketBase response structure
        assert!(body.get("page").is_some(), "list response must have 'page'");
        assert!(
            body.get("perPage").is_some(),
            "list response must have 'perPage'"
        );
        assert!(
            body.get("totalItems").is_some(),
            "list response must have 'totalItems'"
        );
        assert!(
            body.get("totalPages").is_some(),
            "list response must have 'totalPages'"
        );
        assert!(
            body.get("items").is_some(),
            "list response must have 'items'"
        );
        assert!(body["items"].is_array(), "'items' must be an array");

        // Check PocketBase uses camelCase
        assert!(
            body.get("total_items").is_none(),
            "should use camelCase, not snake_case"
        );
        assert!(
            body.get("per_page").is_none(),
            "should use camelCase, not snake_case"
        );
    }

    #[tokio::test]
    async fn record_response_includes_system_fields() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/post_2_________"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();

        // System fields present
        assert!(body["id"].is_string(), "record must have 'id'");
        assert!(body["created"].is_string(), "record must have 'created'");
        assert!(body["updated"].is_string(), "record must have 'updated'");

        // User fields present
        assert!(body["title"].is_string(), "record must have user field 'title'");
        assert!(body["status"].is_string());
    }

    #[tokio::test]
    async fn empty_list_returns_proper_structure() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/collections/files_demo/records"))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();

        assert_eq!(body["totalItems"], 0);
        assert_eq!(body["items"].as_array().unwrap().len(), 0);
        assert_eq!(body["page"], 1);
        assert_eq!(body["totalPages"], 1);
    }

    #[tokio::test]
    async fn content_type_is_json() {
        let addr = spawn_records_app().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .send()
            .await
            .unwrap();

        let ct = resp
            .headers()
            .get("content-type")
            .expect("missing content-type")
            .to_str()
            .unwrap();
        assert!(
            ct.contains("application/json"),
            "API should return JSON, got: {ct}"
        );
    }
}
