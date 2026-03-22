//! Integration tests for the collection management REST API.
//!
//! Tests exercise the full HTTP stack (router → middleware → handler → service)
//! using an in-memory [`SchemaRepository`] mock. Each test spawns an isolated
//! server on a random port.

mod common;

use std::sync::Arc;

use reqwest::StatusCode;
use serde_json::json;
use tokio::net::TcpListener;

use zerobase_core::services::collection_service::{
    CollectionSchemaDto, ColumnDto, IndexDto, SchemaRepoError, SchemaRepository,
};
use zerobase_core::CollectionService;

// ── In-memory mock repository ───────────────────────────────────────────────

struct InMemorySchemaRepo {
    collections: std::sync::Mutex<Vec<CollectionSchemaDto>>,
}

impl InMemorySchemaRepo {
    fn new() -> Self {
        Self {
            collections: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn with(dtos: Vec<CollectionSchemaDto>) -> Self {
        Self {
            collections: std::sync::Mutex::new(dtos),
        }
    }
}

impl SchemaRepository for InMemorySchemaRepo {
    fn list_collections(&self) -> std::result::Result<Vec<CollectionSchemaDto>, SchemaRepoError> {
        Ok(self.collections.lock().unwrap().clone())
    }

    fn get_collection(
        &self,
        name: &str,
    ) -> std::result::Result<CollectionSchemaDto, SchemaRepoError> {
        self.collections
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.name == name)
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
        let mut cols = self.collections.lock().unwrap();
        if cols.iter().any(|c| c.name == schema.name) {
            return Err(SchemaRepoError::Conflict {
                message: format!("collection '{}' already exists", schema.name),
            });
        }
        cols.push(schema.clone());
        Ok(())
    }

    fn update_collection(
        &self,
        name: &str,
        schema: &CollectionSchemaDto,
    ) -> std::result::Result<(), SchemaRepoError> {
        let mut cols = self.collections.lock().unwrap();
        let pos = cols
            .iter()
            .position(|c| c.name == name)
            .ok_or(SchemaRepoError::NotFound {
                resource_type: "Collection".to_string(),
                resource_id: Some(name.to_string()),
            })?;
        cols[pos] = schema.clone();
        Ok(())
    }

    fn delete_collection(&self, name: &str) -> std::result::Result<(), SchemaRepoError> {
        let mut cols = self.collections.lock().unwrap();
        let pos = cols
            .iter()
            .position(|c| c.name == name)
            .ok_or(SchemaRepoError::NotFound {
                resource_type: "Collection".to_string(),
                resource_id: Some(name.to_string()),
            })?;
        cols.remove(pos);
        Ok(())
    }

    fn collection_exists(&self, name: &str) -> std::result::Result<bool, SchemaRepoError> {
        Ok(self
            .collections
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.name == name))
    }
}

// ── Test infrastructure ─────────────────────────────────────────────────────

/// Spawn a test server with collection routes and an in-memory repo.
async fn spawn_app(repo: InMemorySchemaRepo) -> (String, u16, tokio::task::JoinHandle<()>) {
    let service = Arc::new(CollectionService::new(repo));

    let app = zerobase_api::api_router().merge(zerobase_api::collection_routes(service));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let port = addr.port();
    let address = format!("http://127.0.0.1:{port}");

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (address, port, handle)
}

fn make_dto(name: &str, ctype: &str) -> CollectionSchemaDto {
    CollectionSchemaDto {
        name: name.to_string(),
        collection_type: ctype.to_string(),
        columns: vec![ColumnDto {
            name: "title".to_string(),
            sql_type: "TEXT".to_string(),
            not_null: false,
            default: None,
            unique: false,
        }],
        indexes: vec![],
        searchable_fields: vec![],
        view_query: None,
    }
}

fn auth_header() -> (&'static str, &'static str) {
    ("authorization", "Bearer test-superuser-token")
}

// ── List ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_collections_returns_200_with_empty_list() {
    let (addr, _, _handle) = spawn_app(InMemorySchemaRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 0);
    assert_eq!(body["page"], 1);
    assert_eq!(body["perPage"], 100);
    assert_eq!(body["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn list_collections_returns_all_collections() {
    let repo = InMemorySchemaRepo::with(vec![make_dto("posts", "base"), make_dto("users", "auth")]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 2);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn list_collections_without_auth_returns_401() {
    let (addr, _, _handle) = spawn_app(InMemorySchemaRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Create ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_collection_returns_200_with_created_body() {
    let (addr, _, _handle) = spawn_app(InMemorySchemaRepo::new()).await;
    let client = reqwest::Client::new();

    let body = json!({
        "name": "articles",
        "type": "base",
        "fields": [
            {
                "name": "title",
                "type": { "type": "text", "options": { "minLength": 0, "maxLength": 0 } }
            }
        ]
    });

    let resp = client
        .post(format!("{addr}/api/collections"))
        .header(auth_header().0, auth_header().1)
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let created: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(created["name"], "articles");
    assert_eq!(created["type"], "base");
    assert!(created["id"].as_str().is_some_and(|id| !id.is_empty()));
}

#[tokio::test]
async fn create_collection_without_name_returns_400() {
    let (addr, _, _handle) = spawn_app(InMemorySchemaRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/collections"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({ "type": "base" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], 400);
}

#[tokio::test]
async fn create_duplicate_collection_returns_409() {
    let repo = InMemorySchemaRepo::with(vec![make_dto("posts", "base")]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/collections"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({ "name": "posts", "type": "base" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn create_collection_without_auth_returns_401() {
    let (addr, _, _handle) = spawn_app(InMemorySchemaRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/collections"))
        .json(&json!({ "name": "posts", "type": "base" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── View ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn view_collection_returns_200() {
    let repo = InMemorySchemaRepo::with(vec![make_dto("posts", "base")]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "posts");
    assert_eq!(body["type"], "base");
}

#[tokio::test]
async fn view_nonexistent_collection_returns_404() {
    let (addr, _, _handle) = spawn_app(InMemorySchemaRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/nonexistent"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], 404);
}

#[tokio::test]
async fn view_collection_without_auth_returns_401() {
    let repo = InMemorySchemaRepo::with(vec![make_dto("posts", "base")]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/posts"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Update ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn update_collection_returns_200() {
    let repo = InMemorySchemaRepo::with(vec![make_dto("posts", "base")]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let body = json!({
        "name": "articles"
    });

    let resp = client
        .patch(format!("{addr}/api/collections/posts"))
        .header(auth_header().0, auth_header().1)
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let updated: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(updated["name"], "articles");
}

#[tokio::test]
async fn update_nonexistent_collection_returns_404() {
    let (addr, _, _handle) = spawn_app(InMemorySchemaRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!("{addr}/api/collections/nonexistent"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({ "name": "renamed" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn update_collection_without_auth_returns_401() {
    let repo = InMemorySchemaRepo::with(vec![make_dto("posts", "base")]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .patch(format!("{addr}/api/collections/posts"))
        .json(&json!({ "name": "renamed" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Delete ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn delete_collection_returns_204() {
    let repo = InMemorySchemaRepo::with(vec![make_dto("posts", "base")]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{addr}/api/collections/posts"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn delete_nonexistent_collection_returns_404() {
    let (addr, _, _handle) = spawn_app(InMemorySchemaRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{addr}/api/collections/nonexistent"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_system_collection_returns_403() {
    let repo = InMemorySchemaRepo::with(vec![make_dto("_superusers", "auth")]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{addr}/api/collections/_superusers"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], 403);
}

#[tokio::test]
async fn delete_collection_without_auth_returns_401() {
    let repo = InMemorySchemaRepo::with(vec![make_dto("posts", "base")]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{addr}/api/collections/posts"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Create then verify via list ─────────────────────────────────────────────

#[tokio::test]
async fn created_collection_appears_in_list() {
    let (addr, _, _handle) = spawn_app(InMemorySchemaRepo::new()).await;
    let client = reqwest::Client::new();

    // Create
    client
        .post(format!("{addr}/api/collections"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({
            "name": "posts",
            "type": "base"
        }))
        .send()
        .await
        .unwrap();

    // List
    let resp = client
        .get(format!("{addr}/api/collections"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 1);
    assert_eq!(body["items"][0]["name"], "posts");
}

// ── Delete then verify via list ─────────────────────────────────────────────

#[tokio::test]
async fn deleted_collection_disappears_from_list() {
    let repo = InMemorySchemaRepo::with(vec![make_dto("posts", "base")]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    // Delete
    client
        .delete(format!("{addr}/api/collections/posts"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    // List
    let resp = client
        .get(format!("{addr}/api/collections"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 0);
}

// ── Response format ─────────────────────────────────────────────────────────

#[tokio::test]
async fn error_responses_include_code_and_message() {
    let (addr, _, _handle) = spawn_app(InMemorySchemaRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections/nonexistent"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["code"].is_number(),
        "error response should have 'code'"
    );
    assert!(
        body["message"].is_string(),
        "error response should have 'message'"
    );
}

#[tokio::test]
async fn list_response_has_pocketbase_pagination_shape() {
    let (addr, _, _handle) = spawn_app(InMemorySchemaRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/collections"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("page").is_some(), "response must have 'page'");
    assert!(
        body.get("perPage").is_some(),
        "response must have 'perPage'"
    );
    assert!(
        body.get("totalItems").is_some(),
        "response must have 'totalItems'"
    );
    assert!(
        body.get("totalPages").is_some(),
        "response must have 'totalPages'"
    );
    assert!(body.get("items").is_some(), "response must have 'items'");
}
