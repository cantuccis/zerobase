//! Comprehensive integration tests for record CRUD with access rules.
//!
//! These tests exercise the full HTTP stack (router → handler → rule engine → mock repo)
//! and verify that access rules are correctly enforced for every operation type.
//!
//! Test categories:
//! - Owner-based rules (record field matching against `@request.auth.id`)
//! - Record field matching in rules (e.g. `status = "published"`)
//! - `@request.auth.*` context variable resolution
//! - Rule-as-filter behaviour on list endpoints
//! - `manage_rule` bypass
//! - Per-operation asymmetric rules
//! - Compound rule expressions (`&&`, `||`)
//! - Superuser bypass of all expression rules

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

use zerobase_core::schema::{ApiRules, Collection, Field, FieldType, NumberOptions, TextOptions};
use zerobase_core::services::record_service::{
    RecordList, RecordQuery, RecordRepoError, RecordRepository, RecordService, SchemaLookup,
    SortDirection,
};

use zerobase_api::AuthInfo;

// ── Enhanced fake auth middleware ────────────────────────────────────────────
//
// Supports richer auth records by encoding fields in the Bearer token:
//
// - `Bearer SUPERUSER`              → superuser
// - `Bearer <id>`                   → authenticated with `id` = `<id>`
// - `Bearer <id>;role=admin`        → authenticated with `id` + extra fields
// - `Bearer <id>;role=editor;org=x` → authenticated with multiple extra fields
// - No header                       → anonymous
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
                AuthInfo::superuser()
            } else {
                let mut record = HashMap::new();
                // Parse "id;key=val;key2=val2" format
                let parts: Vec<&str> = token.split(';').collect();
                record.insert(
                    "id".to_string(),
                    serde_json::Value::String(parts[0].to_string()),
                );
                for part in &parts[1..] {
                    if let Some((key, val)) = part.split_once('=') {
                        record.insert(
                            key.to_string(),
                            serde_json::Value::String(val.to_string()),
                        );
                    }
                }
                AuthInfo::authenticated(record)
            }
        })
        .unwrap_or_else(AuthInfo::anonymous);

    request.extensions_mut().insert(auth_info);
    next.run(request).await
}

// ── In-memory mock: SchemaLookup ────────────────────────────────────────────

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
    fn get_collection(&self, name: &str) -> zerobase_core::error::Result<Collection> {
        self.collections
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.name == name)
            .cloned()
            .ok_or_else(|| zerobase_core::ZerobaseError::not_found_with_id("Collection", name))
    }
}

// ── In-memory mock: RecordRepository ────────────────────────────────────────

struct MockRecordRepo {
    records: Mutex<HashMap<String, Vec<HashMap<String, Value>>>>,
}

impl MockRecordRepo {
    fn new() -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
        }
    }

    fn with_records(collection: &str, records: Vec<HashMap<String, Value>>) -> Self {
        let mut map = HashMap::new();
        map.insert(collection.to_string(), records);
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

// ── Test helpers ────────────────────────────────────────────────────────────

fn posts_fields() -> Vec<Field> {
    vec![
        Field::new(
            "title",
            FieldType::Text(TextOptions {
                min_length: 0,
                max_length: 200,
                pattern: None,
                searchable: false,
            }),
        ),
        Field::new(
            "status",
            FieldType::Text(TextOptions {
                min_length: 0,
                max_length: 50,
                pattern: None,
                searchable: false,
            }),
        ),
        Field::new(
            "owner",
            FieldType::Text(TextOptions {
                min_length: 0,
                max_length: 50,
                pattern: None,
                searchable: false,
            }),
        ),
        Field::new(
            "views",
            FieldType::Number(NumberOptions {
                min: None,
                max: None,
                only_int: false,
            }),
        ),
    ]
}

fn make_collection_with_rules(rules: ApiRules) -> Collection {
    let mut col = Collection::base("posts", posts_fields());
    col.rules = rules;
    col
}

fn make_record(
    id: &str,
    title: &str,
    status: &str,
    owner: &str,
    views: i64,
) -> HashMap<String, Value> {
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!(id));
    record.insert("title".to_string(), json!(title));
    record.insert("status".to_string(), json!(status));
    record.insert("owner".to_string(), json!(owner));
    record.insert("views".to_string(), json!(views));
    record.insert("created".to_string(), json!("2025-01-01T00:00:00Z"));
    record.insert("updated".to_string(), json!("2025-01-01T00:00:00Z"));
    record
}

async fn spawn_app(
    rules: ApiRules,
    records: Vec<HashMap<String, Value>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let schema = MockSchemaLookup::with(vec![make_collection_with_rules(rules)]);
    let repo = if records.is_empty() {
        MockRecordRepo::new()
    } else {
        MockRecordRepo::with_records("posts", records)
    };

    let service = Arc::new(RecordService::new(repo, schema));
    let app = zerobase_api::api_router()
        .merge(zerobase_api::record_routes(service))
        .layer(axum::middleware::from_fn(rich_auth_middleware));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let address = format!("http://127.0.0.1:{}", addr.port());

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (address, handle)
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Owner-based rules: `owner = @request.auth.id`
// ═══════════════════════════════════════════════════════════════════════════

mod owner_rules {
    use super::*;

    fn owner_rules() -> ApiRules {
        let owner_check = r#"owner = @request.auth.id"#.to_string();
        ApiRules {
            list_rule: Some(owner_check.clone()),
            view_rule: Some(owner_check.clone()),
            create_rule: Some(r#"@request.auth.id != """#.to_string()),
            update_rule: Some(owner_check.clone()),
            delete_rule: Some(owner_check),
            manage_rule: None,
        }
    }

    fn seed_records() -> Vec<HashMap<String, Value>> {
        vec![
            make_record("post_alice_1___", "Alice Post 1", "published", "alice", 10),
            make_record("post_alice_2___", "Alice Post 2", "draft", "alice", 5),
            make_record("post_bob_1_____", "Bob Post 1", "published", "bob", 20),
        ]
    }

    // ── View ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn owner_can_view_own_record() {
        let (addr, _h) = spawn_app(owner_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/post_alice_1___"
            ))
            .header("Authorization", "Bearer alice")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["id"], "post_alice_1___");
        assert_eq!(body["owner"], "alice");
    }

    #[tokio::test]
    async fn non_owner_cannot_view_others_record() {
        let (addr, _h) = spawn_app(owner_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        // Bob tries to view Alice's record → 404 (hides existence)
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/post_alice_1___"
            ))
            .header("Authorization", "Bearer bob")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn anonymous_cannot_view_owned_record() {
        let (addr, _h) = spawn_app(owner_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/post_alice_1___"
            ))
            .send()
            .await
            .unwrap();

        // Anonymous user → @request.auth.id resolves to "" → doesn't match owner
        assert!(
            resp.status() == StatusCode::NOT_FOUND
                || resp.status() == StatusCode::FORBIDDEN
        );
    }

    // ── Update ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn owner_can_update_own_record() {
        let (addr, _h) = spawn_app(owner_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .patch(format!(
                "{addr}/api/collections/posts/records/post_alice_1___"
            ))
            .header("Authorization", "Bearer alice")
            .json(&json!({"title": "Updated by Alice"}))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["title"], "Updated by Alice");
    }

    #[tokio::test]
    async fn non_owner_cannot_update_others_record() {
        let (addr, _h) = spawn_app(owner_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .patch(format!(
                "{addr}/api/collections/posts/records/post_alice_1___"
            ))
            .header("Authorization", "Bearer bob")
            .json(&json!({"title": "Hacked by Bob"}))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── Delete ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn owner_can_delete_own_record() {
        let (addr, _h) = spawn_app(owner_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/post_alice_1___"
            ))
            .header("Authorization", "Bearer alice")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn non_owner_cannot_delete_others_record() {
        let (addr, _h) = spawn_app(owner_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/post_alice_1___"
            ))
            .header("Authorization", "Bearer bob")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── Create ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn authenticated_user_can_create() {
        let (addr, _h) = spawn_app(owner_rules(), vec![]).await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer alice")
            .json(&json!({
                "title": "New Post",
                "status": "draft",
                "owner": "alice",
                "views": 0
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn anonymous_cannot_create() {
        let (addr, _h) = spawn_app(owner_rules(), vec![]).await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .json(&json!({
                "title": "Anon Post",
                "status": "draft",
                "owner": "nobody",
                "views": 0
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // ── List (rule-as-filter) ───────────────────────────────────────────

    #[tokio::test]
    async fn list_returns_empty_for_anonymous_with_owner_rule() {
        let (addr, _h) = spawn_app(owner_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        // Anonymous → @request.auth.id = "" → no records match owner = ""
        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["totalItems"], 0);
    }

    #[tokio::test]
    async fn superuser_bypasses_owner_rules_on_all_operations() {
        let (addr, _h) = spawn_app(owner_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        // Superuser can list all
        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["totalItems"], 3);

        // Superuser can view any
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/post_alice_1___"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Superuser can update any
        let resp = client
            .patch(format!(
                "{addr}/api/collections/posts/records/post_bob_1_____"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .json(&json!({"title": "SU Updated"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Superuser can delete any
        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/post_bob_1_____"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Record field matching in rules (e.g. `status = "published"`)
// ═══════════════════════════════════════════════════════════════════════════

mod record_field_rules {
    use super::*;

    /// Only published records can be viewed by non-superusers.
    fn published_only_view_rules() -> ApiRules {
        ApiRules {
            list_rule: Some(String::new()),
            view_rule: Some(r#"status = "published""#.to_string()),
            create_rule: Some(String::new()),
            update_rule: Some(String::new()),
            delete_rule: Some(String::new()),
            manage_rule: None,
        }
    }

    fn seed_records() -> Vec<HashMap<String, Value>> {
        vec![
            make_record("published_1____", "Public Post", "published", "alice", 10),
            make_record("draft_1________", "Draft Post", "draft", "alice", 5),
        ]
    }

    #[tokio::test]
    async fn can_view_published_record() {
        let (addr, _h) = spawn_app(published_only_view_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/published_1____"
            ))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "published");
    }

    #[tokio::test]
    async fn cannot_view_draft_record() {
        let (addr, _h) = spawn_app(published_only_view_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/draft_1________"
            ))
            .send()
            .await
            .unwrap();

        // Should return 404 to hide existence (PocketBase behaviour for view)
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn superuser_can_view_draft_record() {
        let (addr, _h) = spawn_app(published_only_view_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/draft_1________"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "draft");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. @request.auth.* context variable resolution
// ═══════════════════════════════════════════════════════════════════════════

mod auth_context_variables {
    use super::*;

    /// Rule that checks a custom auth field: `@request.auth.role = "admin"`
    fn admin_only_rules() -> ApiRules {
        let admin_check = r#"@request.auth.role = "admin""#.to_string();
        ApiRules {
            list_rule: Some(admin_check.clone()),
            view_rule: Some(admin_check.clone()),
            create_rule: Some(admin_check.clone()),
            update_rule: Some(admin_check.clone()),
            delete_rule: Some(admin_check),
            manage_rule: None,
        }
    }

    fn seed_records() -> Vec<HashMap<String, Value>> {
        vec![make_record(
            "rec_admin_only_",
            "Admin Only Post",
            "published",
            "someone",
            1,
        )]
    }

    #[tokio::test]
    async fn admin_role_user_can_access() {
        let (addr, _h) = spawn_app(admin_only_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        // User with role=admin
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/rec_admin_only_"
            ))
            .header("Authorization", "Bearer user1;role=admin")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn non_admin_role_user_denied() {
        let (addr, _h) = spawn_app(admin_only_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        // User with role=editor (not admin)
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/rec_admin_only_"
            ))
            .header("Authorization", "Bearer user2;role=editor")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn user_without_role_field_denied() {
        let (addr, _h) = spawn_app(admin_only_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        // User without role field
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/rec_admin_only_"
            ))
            .header("Authorization", "Bearer user3")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn admin_user_can_list() {
        let (addr, _h) = spawn_app(admin_only_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer user1;role=admin")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["totalItems"], 1);
    }

    #[tokio::test]
    async fn non_admin_list_returns_empty() {
        let (addr, _h) = spawn_app(admin_only_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer user2;role=editor")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["totalItems"], 0);
    }

    #[tokio::test]
    async fn admin_can_create() {
        let (addr, _h) = spawn_app(admin_only_rules(), vec![]).await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer user1;role=admin")
            .json(&json!({
                "title": "Admin Created",
                "status": "published",
                "owner": "user1",
                "views": 0
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn non_admin_cannot_create() {
        let (addr, _h) = spawn_app(admin_only_rules(), vec![]).await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer user2;role=editor")
            .json(&json!({
                "title": "Should Fail",
                "status": "draft",
                "owner": "user2",
                "views": 0
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_can_delete() {
        let (addr, _h) = spawn_app(admin_only_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/rec_admin_only_"
            ))
            .header("Authorization", "Bearer user1;role=admin")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn non_admin_cannot_delete() {
        let (addr, _h) = spawn_app(admin_only_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/rec_admin_only_"
            ))
            .header("Authorization", "Bearer user2;role=editor")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Compound rule expressions (&&, ||)
// ═══════════════════════════════════════════════════════════════════════════

mod compound_rules {
    use super::*;

    /// View requires: owner matches OR status is published.
    /// This is a common pattern: you can see your own or any published post.
    fn owner_or_published_rules() -> ApiRules {
        ApiRules {
            list_rule: Some(String::new()), // open
            view_rule: Some(
                r#"owner = @request.auth.id || status = "published""#.to_string(),
            ),
            create_rule: Some(r#"@request.auth.id != """#.to_string()),
            update_rule: Some(r#"owner = @request.auth.id"#.to_string()),
            delete_rule: Some(
                r#"owner = @request.auth.id && status = "draft""#.to_string(),
            ),
            manage_rule: None,
        }
    }

    fn seed_records() -> Vec<HashMap<String, Value>> {
        vec![
            make_record("pub_post_______", "Public", "published", "alice", 10),
            make_record("draft_alice____", "Alice Draft", "draft", "alice", 5),
            make_record("draft_bob______", "Bob Draft", "draft", "bob", 3),
        ]
    }

    #[tokio::test]
    async fn view_published_by_anyone() {
        let (addr, _h) = spawn_app(owner_or_published_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        // Bob can view published post (not owned by him)
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/pub_post_______"
            ))
            .header("Authorization", "Bearer bob")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn view_own_draft() {
        let (addr, _h) = spawn_app(owner_or_published_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        // Alice can view her own draft
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/draft_alice____"
            ))
            .header("Authorization", "Bearer alice")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn cannot_view_others_draft() {
        let (addr, _h) = spawn_app(owner_or_published_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        // Alice can't view Bob's draft
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/draft_bob______"
            ))
            .header("Authorization", "Bearer alice")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // delete_rule = `owner = @request.auth.id && status = "draft"`
    #[tokio::test]
    async fn owner_can_delete_own_draft() {
        let (addr, _h) = spawn_app(owner_or_published_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/draft_alice____"
            ))
            .header("Authorization", "Bearer alice")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn owner_cannot_delete_own_published() {
        let (addr, _h) = spawn_app(owner_or_published_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        // Alice owns pub_post but it's published → AND condition fails
        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/pub_post_______"
            ))
            .header("Authorization", "Bearer alice")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn non_owner_cannot_delete_others_draft() {
        let (addr, _h) = spawn_app(owner_or_published_rules(), seed_records()).await;
        let client = reqwest::Client::new();

        // Alice tries to delete Bob's draft → owner doesn't match
        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/draft_bob______"
            ))
            .header("Authorization", "Bearer alice")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Per-operation asymmetric rules
// ═══════════════════════════════════════════════════════════════════════════

mod asymmetric_rules {
    use super::*;

    /// Asymmetric rules: list/view open, create requires auth, update/delete owner-only
    fn asymmetric() -> ApiRules {
        ApiRules {
            list_rule: Some(String::new()),   // open
            view_rule: Some(String::new()),   // open
            create_rule: Some(r#"@request.auth.id != """#.to_string()), // auth required
            update_rule: Some(r#"owner = @request.auth.id"#.to_string()), // owner only
            delete_rule: None,                // locked (superusers only)
            manage_rule: None,
        }
    }

    fn seed_records() -> Vec<HashMap<String, Value>> {
        vec![make_record(
            "asym_rec_______",
            "Asymmetric",
            "published",
            "alice",
            10,
        )]
    }

    #[tokio::test]
    async fn anonymous_can_list() {
        let (addr, _h) = spawn_app(asymmetric(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["totalItems"], 1);
    }

    #[tokio::test]
    async fn anonymous_can_view() {
        let (addr, _h) = spawn_app(asymmetric(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/asym_rec_______"
            ))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn anonymous_cannot_create() {
        let (addr, _h) = spawn_app(asymmetric(), vec![]).await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .json(&json!({"title": "Anon", "status": "draft", "owner": "x", "views": 0}))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn authenticated_can_create() {
        let (addr, _h) = spawn_app(asymmetric(), vec![]).await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer alice")
            .json(&json!({"title": "Created", "status": "draft", "owner": "alice", "views": 0}))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn owner_can_update() {
        let (addr, _h) = spawn_app(asymmetric(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .patch(format!(
                "{addr}/api/collections/posts/records/asym_rec_______"
            ))
            .header("Authorization", "Bearer alice")
            .json(&json!({"title": "Updated"}))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn non_owner_cannot_update() {
        let (addr, _h) = spawn_app(asymmetric(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .patch(format!(
                "{addr}/api/collections/posts/records/asym_rec_______"
            ))
            .header("Authorization", "Bearer bob")
            .json(&json!({"title": "Hacked"}))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_locked_for_regular_user() {
        let (addr, _h) = spawn_app(asymmetric(), seed_records()).await;
        let client = reqwest::Client::new();

        // Even the owner can't delete (delete_rule is None = locked)
        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/asym_rec_______"
            ))
            .header("Authorization", "Bearer alice")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn delete_allowed_for_superuser() {
        let (addr, _h) = spawn_app(asymmetric(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/asym_rec_______"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. manage_rule bypass
// ═══════════════════════════════════════════════════════════════════════════

mod manage_rule {
    use super::*;

    /// All individual rules are locked (None), but manage_rule allows "admin" role users.
    fn locked_with_manage() -> ApiRules {
        ApiRules {
            list_rule: None,
            view_rule: None,
            create_rule: None,
            update_rule: None,
            delete_rule: None,
            manage_rule: Some(r#"@request.auth.role = "admin""#.to_string()),
        }
    }

    fn seed_records() -> Vec<HashMap<String, Value>> {
        vec![make_record(
            "managed_rec____",
            "Managed Record",
            "published",
            "someone",
            1,
        )]
    }

    #[tokio::test]
    async fn manage_user_can_list_despite_locked_rule() {
        let (addr, _h) = spawn_app(locked_with_manage(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer admin1;role=admin")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["totalItems"], 1);
    }

    #[tokio::test]
    async fn manage_user_can_view_despite_locked_rule() {
        let (addr, _h) = spawn_app(locked_with_manage(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/managed_rec____"
            ))
            .header("Authorization", "Bearer admin1;role=admin")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn manage_user_can_create_despite_locked_rule() {
        let (addr, _h) = spawn_app(locked_with_manage(), vec![]).await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer admin1;role=admin")
            .json(&json!({
                "title": "Managed Create",
                "status": "draft",
                "owner": "admin1",
                "views": 0
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn manage_user_can_update_despite_locked_rule() {
        let (addr, _h) = spawn_app(locked_with_manage(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .patch(format!(
                "{addr}/api/collections/posts/records/managed_rec____"
            ))
            .header("Authorization", "Bearer admin1;role=admin")
            .json(&json!({"title": "Admin Updated"}))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn manage_user_can_delete_despite_locked_rule() {
        let (addr, _h) = spawn_app(locked_with_manage(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/managed_rec____"
            ))
            .header("Authorization", "Bearer admin1;role=admin")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn non_manage_user_blocked_on_locked_rules() {
        let (addr, _h) = spawn_app(locked_with_manage(), seed_records()).await;
        let client = reqwest::Client::new();

        // User with role=editor doesn't match manage_rule
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/managed_rec____"
            ))
            .header("Authorization", "Bearer user1;role=editor")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn non_manage_user_list_returns_empty() {
        let (addr, _h) = spawn_app(locked_with_manage(), seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer user1;role=editor")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["totalItems"], 0);
    }

    /// Empty manage_rule means any authenticated user gets manage access.
    #[tokio::test]
    async fn empty_manage_rule_allows_any_authenticated() {
        let rules = ApiRules {
            list_rule: None,
            view_rule: None,
            create_rule: None,
            update_rule: None,
            delete_rule: None,
            manage_rule: Some(String::new()), // empty = any authenticated
        };
        let (addr, _h) = spawn_app(rules, seed_records()).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/managed_rec____"
            ))
            .header("Authorization", "Bearer anyuser")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn empty_manage_rule_blocks_anonymous() {
        let rules = ApiRules {
            list_rule: None,
            view_rule: None,
            create_rule: None,
            update_rule: None,
            delete_rule: None,
            manage_rule: Some(String::new()), // empty = any authenticated
        };
        let (addr, _h) = spawn_app(rules, seed_records()).await;
        let client = reqwest::Client::new();

        // Anonymous cannot manage even with empty manage_rule
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/managed_rec____"
            ))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. @request.data.* context (rules referencing request body fields)
// ═══════════════════════════════════════════════════════════════════════════

mod request_data_context {
    use super::*;

    /// Create rule checks that the submitted owner field matches the auth user.
    /// This prevents users from creating records assigned to other users.
    fn create_self_only_rules() -> ApiRules {
        ApiRules {
            list_rule: Some(String::new()),
            view_rule: Some(String::new()),
            create_rule: Some(r#"owner = @request.auth.id"#.to_string()),
            update_rule: Some(String::new()),
            delete_rule: Some(String::new()),
            manage_rule: None,
        }
    }

    #[tokio::test]
    async fn create_allowed_when_owner_matches_auth() {
        let (addr, _h) = spawn_app(create_self_only_rules(), vec![]).await;
        let client = reqwest::Client::new();

        // Alice creates with owner = "alice" → matches @request.auth.id
        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer alice")
            .json(&json!({
                "title": "My Post",
                "status": "draft",
                "owner": "alice",
                "views": 0
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn create_denied_when_owner_mismatches_auth() {
        let (addr, _h) = spawn_app(create_self_only_rules(), vec![]).await;
        let client = reqwest::Client::new();

        // Alice tries to create with owner = "bob" → doesn't match @request.auth.id
        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer alice")
            .json(&json!({
                "title": "Impersonation",
                "status": "draft",
                "owner": "bob",
                "views": 0
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. List rule-as-filter behaviour
// ═══════════════════════════════════════════════════════════════════════════

mod list_rule_as_filter {
    use super::*;

    /// Locked list rule returns 200 with empty items (not 403).
    #[tokio::test]
    async fn locked_list_returns_200_empty_not_403() {
        let rules = ApiRules {
            list_rule: None, // locked
            view_rule: Some(String::new()),
            create_rule: Some(String::new()),
            update_rule: Some(String::new()),
            delete_rule: Some(String::new()),
            manage_rule: None,
        };
        let records = vec![make_record("r1_____________", "Exists", "published", "alice", 1)];
        let (addr, _h) = spawn_app(rules, records).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .send()
            .await
            .unwrap();

        // PocketBase returns 200 with empty items, not 403
        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["totalItems"], 0);
        assert!(body["items"].as_array().unwrap().is_empty());
    }

    /// Expression list rule that evaluates to false returns 200 empty.
    #[tokio::test]
    async fn expression_list_rule_false_returns_200_empty() {
        let rules = ApiRules {
            list_rule: Some(r#"@request.auth.id != """#.to_string()),
            view_rule: Some(String::new()),
            create_rule: Some(String::new()),
            update_rule: Some(String::new()),
            delete_rule: Some(String::new()),
            manage_rule: None,
        };
        let records = vec![make_record("r1_____________", "Exists", "published", "alice", 1)];
        let (addr, _h) = spawn_app(rules, records).await;
        let client = reqwest::Client::new();

        // Anonymous → expression evaluates to false → 200 with empty items
        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["totalItems"], 0);
    }

    /// Open list rule returns all records.
    #[tokio::test]
    async fn open_list_returns_all_records() {
        let rules = ApiRules {
            list_rule: Some(String::new()), // open
            view_rule: Some(String::new()),
            create_rule: Some(String::new()),
            update_rule: Some(String::new()),
            delete_rule: Some(String::new()),
            manage_rule: None,
        };
        let records = vec![
            make_record("r1_____________", "A", "published", "alice", 1),
            make_record("r2_____________", "B", "draft", "bob", 2),
        ];
        let (addr, _h) = spawn_app(rules, records).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["totalItems"], 2);
    }

    /// Superuser can list even with locked list rule.
    #[tokio::test]
    async fn superuser_lists_all_despite_locked_rule() {
        let rules = ApiRules {
            list_rule: None, // locked
            view_rule: None,
            create_rule: None,
            update_rule: None,
            delete_rule: None,
            manage_rule: None,
        };
        let records = vec![
            make_record("r1_____________", "A", "published", "alice", 1),
            make_record("r2_____________", "B", "draft", "bob", 2),
        ];
        let (addr, _h) = spawn_app(rules, records).await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["totalItems"], 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Mixed rule combinations — full lifecycle with rules
// ═══════════════════════════════════════════════════════════════════════════

mod mixed_lifecycle {
    use super::*;

    /// Realistic scenario: blog posts with role-based + owner-based rules.
    /// - List: open (anyone can browse)
    /// - View: anyone can view published; owners can view own drafts
    /// - Create: authenticated only
    /// - Update: owner only
    /// - Delete: admin role only (via manage_rule)
    fn blog_rules() -> ApiRules {
        ApiRules {
            list_rule: Some(String::new()),
            view_rule: Some(
                r#"status = "published" || owner = @request.auth.id"#.to_string(),
            ),
            create_rule: Some(r#"@request.auth.id != """#.to_string()),
            update_rule: Some(r#"owner = @request.auth.id"#.to_string()),
            delete_rule: None, // locked
            manage_rule: Some(r#"@request.auth.role = "admin""#.to_string()),
        }
    }

    #[tokio::test]
    async fn full_blog_lifecycle() {
        let records = vec![
            make_record("blog_pub_______", "Published Blog", "published", "alice", 100),
            make_record("blog_draft_____", "Alice Draft", "draft", "alice", 0),
            make_record("blog_bob_draft_", "Bob Draft", "draft", "bob", 0),
        ];
        let (addr, _h) = spawn_app(blog_rules(), records).await;
        let client = reqwest::Client::new();

        // 1. Anonymous can list all (open list rule)
        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["totalItems"], 3);

        // 2. Anonymous can view published post
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/blog_pub_______"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 3. Anonymous CANNOT view draft
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/blog_draft_____"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // 4. Alice CAN view her own draft
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/blog_draft_____"
            ))
            .header("Authorization", "Bearer alice")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 5. Alice CANNOT view Bob's draft
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/blog_bob_draft_"
            ))
            .header("Authorization", "Bearer alice")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // 6. Alice can update her own post
        let resp = client
            .patch(format!(
                "{addr}/api/collections/posts/records/blog_draft_____"
            ))
            .header("Authorization", "Bearer alice")
            .json(&json!({"title": "Updated Draft"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 7. Alice cannot update Bob's post
        let resp = client
            .patch(format!(
                "{addr}/api/collections/posts/records/blog_bob_draft_"
            ))
            .header("Authorization", "Bearer alice")
            .json(&json!({"title": "Hacked"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // 8. Alice CANNOT delete (delete_rule is locked)
        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/blog_draft_____"
            ))
            .header("Authorization", "Bearer alice")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // 9. Admin role user CAN delete via manage_rule bypass
        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/blog_draft_____"
            ))
            .header("Authorization", "Bearer admin1;role=admin")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // 10. Anonymous cannot create
        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .json(&json!({"title": "Anon", "status": "draft", "owner": "x", "views": 0}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // 11. Authenticated user can create
        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer bob")
            .json(&json!({"title": "Bob's New Post", "status": "draft", "owner": "bob", "views": 0}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Edge cases
// ═══════════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    /// Rule with != operator.
    #[tokio::test]
    async fn not_equal_operator_in_rule() {
        let rules = ApiRules {
            list_rule: Some(String::new()),
            view_rule: Some(r#"status != "archived""#.to_string()),
            create_rule: Some(String::new()),
            update_rule: Some(String::new()),
            delete_rule: Some(String::new()),
            manage_rule: None,
        };
        let records = vec![
            make_record("active_________", "Active", "published", "alice", 1),
            make_record("archived_______", "Archived", "archived", "alice", 1),
        ];
        let (addr, _h) = spawn_app(rules, records).await;
        let client = reqwest::Client::new();

        // Can view non-archived
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/active_________"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Cannot view archived
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/archived_______"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// All rules are expression-based — verify each operation enforces its own rule.
    #[tokio::test]
    async fn each_operation_enforces_its_own_rule() {
        // list: open, view: auth only, create: admin only, update: locked, delete: locked
        let rules = ApiRules {
            list_rule: Some(String::new()),
            view_rule: Some(r#"@request.auth.id != """#.to_string()),
            create_rule: Some(r#"@request.auth.role = "admin""#.to_string()),
            update_rule: None,
            delete_rule: None,
            manage_rule: None,
        };
        let records = vec![make_record("r1_____________", "Rec", "published", "alice", 1)];
        let (addr, _h) = spawn_app(rules, records).await;
        let client = reqwest::Client::new();

        // List: anyone
        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.json::<Value>().await.unwrap()["totalItems"], 1);

        // View: auth required → anonymous denied
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/r1_____________"
            ))
            .send()
            .await
            .unwrap();
        assert!(
            resp.status() == StatusCode::NOT_FOUND
                || resp.status() == StatusCode::FORBIDDEN
        );

        // View: authenticated → allowed
        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/r1_____________"
            ))
            .header("Authorization", "Bearer user1")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Create: requires admin → regular user denied
        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer user1")
            .json(&json!({"title": "T", "status": "d", "owner": "u", "views": 0}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Create: admin allowed
        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer admin1;role=admin")
            .json(&json!({"title": "T", "status": "d", "owner": "admin1", "views": 0}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Update: locked → even admin blocked (no manage_rule)
        let resp = client
            .patch(format!(
                "{addr}/api/collections/posts/records/r1_____________"
            ))
            .header("Authorization", "Bearer admin1;role=admin")
            .json(&json!({"title": "Changed"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Delete: locked → blocked
        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/r1_____________"
            ))
            .header("Authorization", "Bearer admin1;role=admin")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    /// Superuser always bypasses every rule combination.
    #[tokio::test]
    async fn superuser_always_bypasses() {
        let rules = ApiRules {
            list_rule: Some(r#"@request.auth.role = "nobody""#.to_string()),
            view_rule: Some(r#"@request.auth.role = "nobody""#.to_string()),
            create_rule: Some(r#"@request.auth.role = "nobody""#.to_string()),
            update_rule: Some(r#"@request.auth.role = "nobody""#.to_string()),
            delete_rule: Some(r#"@request.auth.role = "nobody""#.to_string()),
            manage_rule: None,
        };
        let records = vec![make_record("r1_____________", "Rec", "published", "x", 1)];
        let (addr, _h) = spawn_app(rules, records).await;
        let client = reqwest::Client::new();

        // All operations succeed for superuser
        let resp = client
            .get(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.json::<Value>().await.unwrap()["totalItems"], 1);

        let resp = client
            .get(format!(
                "{addr}/api/collections/posts/records/r1_____________"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = client
            .post(format!("{addr}/api/collections/posts/records"))
            .header("Authorization", "Bearer SUPERUSER")
            .json(&json!({"title": "SU", "status": "d", "owner": "su", "views": 0}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = client
            .patch(format!(
                "{addr}/api/collections/posts/records/r1_____________"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .json(&json!({"title": "Updated"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = client
            .delete(format!(
                "{addr}/api/collections/posts/records/r1_____________"
            ))
            .header("Authorization", "Bearer SUPERUSER")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}
