//! Collection management handlers.
//!
//! Provides HTTP handlers for CRUD operations on collections.
//! All endpoints require superuser authentication.
//!
//! # Endpoints
//!
//! - `GET  /api/collections`        — list all collections
//! - `POST /api/collections`        — create a new collection
//! - `GET  /api/collections/:id`    — view a single collection
//! - `PATCH /api/collections/:id`   — update a collection
//! - `DELETE /api/collections/:id`  — delete a collection

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use zerobase_core::services::collection_service::SchemaRepository;
use zerobase_core::{Collection, CollectionService, ZerobaseError};

// ── Response types ──────────────────────────────────────────────────────────

/// Paginated list response matching PocketBase's format.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionListResponse {
    pub page: u32,
    pub per_page: u32,
    pub total_items: usize,
    pub total_pages: u32,
    pub items: Vec<Collection>,
}

// ── Request types ───────────────────────────────────────────────────────────

/// Request body for creating or updating a collection.
///
/// Uses the same shape as the domain `Collection` type. Fields not provided
/// in a PATCH request retain their current values.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionBody {
    /// Collection name (used as table name and API path segment).
    pub name: Option<String>,
    /// The collection type.
    #[serde(rename = "type")]
    pub collection_type: Option<zerobase_core::CollectionType>,
    /// User-defined fields.
    #[serde(default)]
    pub fields: Option<Vec<zerobase_core::Field>>,
    /// Per-operation API access rules.
    #[serde(default)]
    pub rules: Option<zerobase_core::ApiRules>,
    /// Index definitions.
    #[serde(default)]
    pub indexes: Option<Vec<zerobase_core::schema::IndexSpec>>,
    /// For View collections: the SQL query.
    #[serde(default)]
    pub view_query: Option<String>,
    /// Auth collection options (only meaningful for Auth type).
    #[serde(default)]
    pub auth_options: Option<zerobase_core::schema::AuthOptions>,
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// `GET /api/collections`
///
/// List all collections. Returns a paginated response matching PocketBase's
/// format. Since the total number of collections is typically small, all
/// collections are returned in a single page.
///
/// Response (200 OK):
/// ```json
/// {
///   "page": 1,
///   "perPage": 100,
///   "totalItems": 3,
///   "totalPages": 1,
///   "items": [...]
/// }
/// ```
pub async fn list_collections<R: SchemaRepository>(
    State(service): State<Arc<CollectionService<R>>>,
) -> impl IntoResponse {
    match service.list_collections() {
        Ok(collections) => {
            let total = collections.len();
            let response = CollectionListResponse {
                page: 1,
                per_page: 100,
                total_items: total,
                total_pages: if total == 0 { 0 } else { 1 },
                items: collections,
            };
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
                .into_response()
        }
        Err(e) => error_response(e),
    }
}

/// `POST /api/collections`
///
/// Create a new collection. The request body must contain at least a `name`
/// and `type`. Returns the created collection with a system-assigned `id`.
///
/// Response (200 OK): The created collection.
/// Response (400 Bad Request): Validation errors.
/// Response (409 Conflict): Collection with that name already exists.
pub async fn create_collection<R: SchemaRepository>(
    State(service): State<Arc<CollectionService<R>>>,
    Json(body): Json<CollectionBody>,
) -> impl IntoResponse {
    let name = match body.name {
        Some(n) => n,
        None => {
            return error_response(ZerobaseError::validation("name is required"));
        }
    };

    let collection_type = body
        .collection_type
        .unwrap_or(zerobase_core::CollectionType::Base);

    let collection = Collection {
        id: uuid::Uuid::new_v4().to_string().replace('-', "")[..15].to_string(),
        name,
        collection_type,
        fields: body.fields.unwrap_or_default(),
        rules: body.rules.unwrap_or_default(),
        indexes: body.indexes.unwrap_or_default(),
        view_query: body.view_query,
        auth_options: body.auth_options,
    };

    match service.create_collection(&collection) {
        Ok(created) => {
            (StatusCode::OK, Json(serde_json::to_value(created).unwrap())).into_response()
        }
        Err(e) => error_response(e),
    }
}

/// `GET /api/collections/:id_or_name`
///
/// View a single collection by name or ID.
///
/// Response (200 OK): The collection.
/// Response (404 Not Found): No collection with that name/ID.
pub async fn view_collection<R: SchemaRepository>(
    State(service): State<Arc<CollectionService<R>>>,
    Path(id_or_name): Path<String>,
) -> impl IntoResponse {
    match service.get_collection(&id_or_name) {
        Ok(collection) => (
            StatusCode::OK,
            Json(serde_json::to_value(collection).unwrap()),
        )
            .into_response(),
        Err(e) => error_response(e),
    }
}

/// `PATCH /api/collections/:id_or_name`
///
/// Update an existing collection. Only provided fields are updated.
///
/// Response (200 OK): The updated collection.
/// Response (400 Bad Request): Validation errors.
/// Response (403 Forbidden): Attempting to modify a system collection.
/// Response (404 Not Found): No collection with that name/ID.
pub async fn update_collection<R: SchemaRepository>(
    State(service): State<Arc<CollectionService<R>>>,
    Path(id_or_name): Path<String>,
    Json(body): Json<CollectionBody>,
) -> impl IntoResponse {
    // Fetch the existing collection to merge with the update body.
    let existing = match service.get_collection(&id_or_name) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    let updated = Collection {
        id: existing.id,
        name: body.name.unwrap_or(existing.name),
        collection_type: body.collection_type.unwrap_or(existing.collection_type),
        fields: body.fields.unwrap_or(existing.fields),
        rules: body.rules.unwrap_or(existing.rules),
        indexes: body.indexes.unwrap_or(existing.indexes),
        view_query: body.view_query.or(existing.view_query),
        auth_options: body.auth_options.or(existing.auth_options),
    };

    match service.update_collection(&id_or_name, &updated) {
        Ok(result) => (StatusCode::OK, Json(serde_json::to_value(result).unwrap())).into_response(),
        Err(e) => error_response(e),
    }
}

/// `DELETE /api/collections/:id_or_name`
///
/// Delete a collection by name or ID.
///
/// Response (204 No Content): Successfully deleted.
/// Response (403 Forbidden): Cannot delete system collections.
/// Response (404 Not Found): No collection with that name/ID.
pub async fn delete_collection<R: SchemaRepository>(
    State(service): State<Arc<CollectionService<R>>>,
    Path(id_or_name): Path<String>,
) -> impl IntoResponse {
    match service.delete_collection(&id_or_name) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(e),
    }
}

// ── Import/Export types ──────────────────────────────────────────────────────

/// Request body for importing collections.
#[derive(Debug, Deserialize)]
pub struct ImportCollectionsBody {
    /// The list of collections to import (create or update).
    pub collections: Vec<Collection>,
}

/// Response body for collection export.
#[derive(Debug, Serialize)]
pub struct ExportCollectionsResponse {
    /// The exported collections (non-system only).
    pub collections: Vec<Collection>,
}

// ── Import/Export Handlers ───────────────────────────────────────────────────

/// `GET /api/collections/export`
///
/// Export all non-system collection schemas as JSON.
///
/// Response (200 OK):
/// ```json
/// {
///   "collections": [...]
/// }
/// ```
pub async fn export_collections<R: SchemaRepository>(
    State(service): State<Arc<CollectionService<R>>>,
) -> impl IntoResponse {
    match service.export_collections() {
        Ok(collections) => {
            let response = ExportCollectionsResponse { collections };
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
                .into_response()
        }
        Err(e) => error_response(e),
    }
}

/// `PUT /api/collections/import`
///
/// Import collection schemas from JSON. Creates new collections or updates
/// existing ones. All collections are validated before any changes are applied.
///
/// System collections (names starting with `_`) are rejected.
///
/// Response (200 OK):
/// ```json
/// {
///   "collections": [...]
/// }
/// ```
/// Response (400 Bad Request): Validation errors.
pub async fn import_collections<R: SchemaRepository>(
    State(service): State<Arc<CollectionService<R>>>,
    Json(body): Json<ImportCollectionsBody>,
) -> impl IntoResponse {
    match service.import_collections(&body.collections) {
        Ok(imported) => {
            let response = ExportCollectionsResponse {
                collections: imported,
            };
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
                .into_response()
        }
        Err(e) => error_response(e),
    }
}

// ── Index management endpoints ──────────────────────────────────────────────

/// `GET /api/collections/:id_or_name/indexes`
///
/// List all indexes for a collection.
///
/// Response (200 OK):
/// ```json
/// {
///   "indexes": [{ "columns": ["title"], "unique": false }, ...]
/// }
/// ```
pub async fn list_indexes<R: SchemaRepository>(
    State(service): State<Arc<CollectionService<R>>>,
    Path(id_or_name): Path<String>,
) -> impl IntoResponse {
    match service.get_collection(&id_or_name) {
        Ok(collection) => {
            let response = serde_json::json!({ "indexes": collection.indexes });
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => error_response(e),
    }
}

/// Request body for adding an index.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddIndexBody {
    /// The index specification to add.
    #[serde(flatten)]
    pub index: zerobase_core::schema::IndexSpec,
}

/// `POST /api/collections/:id_or_name/indexes`
///
/// Add a new index to an existing collection.
///
/// Response (200 OK): The updated collection.
/// Response (400 Bad Request): Validation errors.
pub async fn add_index<R: SchemaRepository>(
    State(service): State<Arc<CollectionService<R>>>,
    Path(id_or_name): Path<String>,
    Json(body): Json<AddIndexBody>,
) -> impl IntoResponse {
    let collection = match service.get_collection(&id_or_name) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    let mut updated = collection;
    updated.indexes.push(body.index);

    match service.update_collection(&id_or_name, &updated) {
        Ok(result) => (StatusCode::OK, Json(serde_json::to_value(result).unwrap())).into_response(),
        Err(e) => error_response(e),
    }
}

/// `DELETE /api/collections/:id_or_name/indexes/:index_pos`
///
/// Remove an index from a collection by its position (0-based).
///
/// Response (204 No Content): Index removed.
/// Response (400 Bad Request): Invalid index position.
pub async fn remove_index<R: SchemaRepository>(
    State(service): State<Arc<CollectionService<R>>>,
    Path((id_or_name, index_pos)): Path<(String, usize)>,
) -> impl IntoResponse {
    let collection = match service.get_collection(&id_or_name) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    let mut updated = collection;
    if index_pos >= updated.indexes.len() {
        return error_response(ZerobaseError::validation(format!(
            "index position {} is out of range (collection has {} indexes)",
            index_pos,
            updated.indexes.len()
        )));
    }
    updated.indexes.remove(index_pos);

    match service.update_collection(&id_or_name, &updated) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(e),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Convert a [`ZerobaseError`] into an axum HTTP response.
fn error_response(err: ZerobaseError) -> axum::response::Response {
    let status =
        StatusCode::from_u16(err.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = err.error_response_body();
    (status, Json(body)).into_response()
}

// ── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::{delete, get, patch, post};
    use axum::Router;
    use serde_json::json;
    use tower::ServiceExt;
    use zerobase_core::services::collection_service::{
        CollectionSchemaDto, ColumnDto, IndexDto, SchemaRepoError, SchemaRepository,
    };

    // ── Mock repository ─────────────────────────────────────────────────

    /// In-memory mock for SchemaRepository.
    struct MockRepo {
        collections: std::sync::Mutex<Vec<CollectionSchemaDto>>,
    }

    impl MockRepo {
        fn new() -> Self {
            Self {
                collections: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn with_collections(collections: Vec<CollectionSchemaDto>) -> Self {
            Self {
                collections: std::sync::Mutex::new(collections),
            }
        }
    }

    impl SchemaRepository for MockRepo {
        fn list_collections(
            &self,
        ) -> std::result::Result<Vec<CollectionSchemaDto>, SchemaRepoError> {
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
            let mut collections = self.collections.lock().unwrap();
            if collections.iter().any(|c| c.name == schema.name) {
                return Err(SchemaRepoError::Conflict {
                    message: format!("collection '{}' already exists", schema.name),
                });
            }
            collections.push(schema.clone());
            Ok(())
        }

        fn update_collection(
            &self,
            name: &str,
            schema: &CollectionSchemaDto,
        ) -> std::result::Result<(), SchemaRepoError> {
            let mut collections = self.collections.lock().unwrap();
            let pos = collections.iter().position(|c| c.name == name).ok_or(
                SchemaRepoError::NotFound {
                    resource_type: "Collection".to_string(),
                    resource_id: Some(name.to_string()),
                },
            )?;
            collections[pos] = schema.clone();
            Ok(())
        }

        fn delete_collection(&self, name: &str) -> std::result::Result<(), SchemaRepoError> {
            let mut collections = self.collections.lock().unwrap();
            let pos = collections.iter().position(|c| c.name == name).ok_or(
                SchemaRepoError::NotFound {
                    resource_type: "Collection".to_string(),
                    resource_id: Some(name.to_string()),
                },
            )?;
            collections.remove(pos);
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

    // ── Test helpers ────────────────────────────────────────────────────

    fn test_app(repo: MockRepo) -> Router {
        let service = Arc::new(CollectionService::new(repo));
        Router::new()
            .route("/api/collections", get(list_collections::<MockRepo>))
            .route("/api/collections", post(create_collection::<MockRepo>))
            .route(
                "/api/collections/export",
                get(export_collections::<MockRepo>),
            )
            .route(
                "/api/collections/import",
                axum::routing::put(import_collections::<MockRepo>),
            )
            .route(
                "/api/collections/{id_or_name}",
                get(view_collection::<MockRepo>),
            )
            .route(
                "/api/collections/{id_or_name}",
                patch(update_collection::<MockRepo>),
            )
            .route(
                "/api/collections/{id_or_name}",
                delete(delete_collection::<MockRepo>),
            )
            .with_state(service)
    }

    fn make_dto(name: &str, collection_type: &str) -> CollectionSchemaDto {
        CollectionSchemaDto {
            name: name.to_string(),
            collection_type: collection_type.to_string(),
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

    async fn send_request(app: &Router, request: Request<Body>) -> axum::response::Response {
        app.clone().oneshot(request).await.unwrap()
    }

    async fn body_json(response: axum::response::Response) -> serde_json::Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    // ── Tests ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_empty_collections_returns_200() {
        let app = test_app(MockRepo::new());
        let req = Request::get("/api/collections")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert_eq!(json["totalItems"], 0);
        assert_eq!(json["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn list_collections_returns_all() {
        let repo =
            MockRepo::with_collections(vec![make_dto("posts", "base"), make_dto("users", "auth")]);
        let app = test_app(repo);
        let req = Request::get("/api/collections")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert_eq!(json["totalItems"], 2);
        assert_eq!(json["page"], 1);
    }

    #[tokio::test]
    async fn create_collection_returns_200() {
        let app = test_app(MockRepo::new());
        let body = json!({
            "name": "posts",
            "type": "base",
            "fields": [
                {
                    "name": "title",
                    "type": { "type": "text", "options": { "minLength": 0, "maxLength": 0 } }
                }
            ]
        });
        let req = Request::post("/api/collections")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert_eq!(json["name"], "posts");
        assert!(json["id"].as_str().is_some());
    }

    #[tokio::test]
    async fn create_collection_without_name_returns_400() {
        let app = test_app(MockRepo::new());
        let body = json!({ "type": "base" });
        let req = Request::post("/api/collections")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_duplicate_collection_returns_409() {
        let repo = MockRepo::with_collections(vec![make_dto("posts", "base")]);
        let app = test_app(repo);
        let body = json!({ "name": "posts", "type": "base" });
        let req = Request::post("/api/collections")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn view_existing_collection_returns_200() {
        let repo = MockRepo::with_collections(vec![make_dto("posts", "base")]);
        let app = test_app(repo);
        let req = Request::get("/api/collections/posts")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert_eq!(json["name"], "posts");
    }

    #[tokio::test]
    async fn view_nonexistent_collection_returns_404() {
        let app = test_app(MockRepo::new());
        let req = Request::get("/api/collections/nonexistent")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_collection_returns_200() {
        let repo = MockRepo::with_collections(vec![make_dto("posts", "base")]);
        let app = test_app(repo);
        let body = json!({
            "name": "articles",
            "fields": [
                {
                    "name": "title",
                    "type": { "type": "text", "options": { "minLength": 0, "maxLength": 0 } }
                },
                {
                    "name": "body",
                    "type": { "type": "editor", "options": {} }
                }
            ]
        });
        let req = Request::patch("/api/collections/posts")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert_eq!(json["name"], "articles");
    }

    #[tokio::test]
    async fn update_nonexistent_collection_returns_404() {
        let app = test_app(MockRepo::new());
        let body = json!({ "name": "posts" });
        let req = Request::patch("/api/collections/nonexistent")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_collection_returns_204() {
        let repo = MockRepo::with_collections(vec![make_dto("posts", "base")]);
        let app = test_app(repo);
        let req = Request::delete("/api/collections/posts")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn delete_nonexistent_collection_returns_404() {
        let app = test_app(MockRepo::new());
        let req = Request::delete("/api/collections/nonexistent")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_system_collection_returns_403() {
        let repo = MockRepo::with_collections(vec![make_dto("_superusers", "auth")]);
        let app = test_app(repo);
        let req = Request::delete("/api/collections/_superusers")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // ── Export endpoint ──────────────────────────────────────────────

    #[tokio::test]
    async fn export_empty_returns_200_with_empty_list() {
        let app = test_app(MockRepo::new());
        let req = Request::get("/api/collections/export")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert!(json["collections"].is_array());
        assert_eq!(json["collections"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn export_returns_non_system_collections() {
        let repo = MockRepo::with_collections(vec![
            make_dto("posts", "base"),
            make_dto("_superusers", "auth"),
        ]);
        let app = test_app(repo);
        let req = Request::get("/api/collections/export")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        let collections = json["collections"].as_array().unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0]["name"], "posts");
    }

    #[tokio::test]
    async fn export_response_contains_collections_key() {
        let repo = MockRepo::with_collections(vec![make_dto("posts", "base")]);
        let app = test_app(repo);
        let req = Request::get("/api/collections/export")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&app, req).await;
        let json = body_json(resp).await;
        assert!(
            json.get("collections").is_some(),
            "response must have 'collections' key"
        );
    }

    // ── Import endpoint ──────────────────────────────────────────────

    #[tokio::test]
    async fn import_creates_new_collections_returns_200() {
        let app = test_app(MockRepo::new());
        let body = json!({
            "collections": [
                {
                    "name": "posts",
                    "type": "base",
                    "fields": [
                        {
                            "name": "title",
                            "type": { "type": "text", "options": { "minLength": 0, "maxLength": 0 } }
                        }
                    ]
                }
            ]
        });
        let req = Request::put("/api/collections/import")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        let collections = json["collections"].as_array().unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0]["name"], "posts");
    }

    #[tokio::test]
    async fn import_updates_existing_collection() {
        let repo = MockRepo::with_collections(vec![make_dto("posts", "base")]);
        let app = test_app(repo);
        let body = json!({
            "collections": [
                {
                    "name": "posts",
                    "type": "base",
                    "fields": [
                        {
                            "name": "title",
                            "type": { "type": "text", "options": { "minLength": 0, "maxLength": 0 } }
                        },
                        {
                            "name": "body",
                            "type": { "type": "editor", "options": {} }
                        }
                    ]
                }
            ]
        });
        let req = Request::put("/api/collections/import")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        let collections = json["collections"].as_array().unwrap();
        assert_eq!(collections[0]["fields"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn import_rejects_invalid_collection_returns_400() {
        let app = test_app(MockRepo::new());
        let body = json!({
            "collections": [
                {
                    "name": "1invalid",
                    "type": "base",
                    "fields": []
                }
            ]
        });
        let req = Request::put("/api/collections/import")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn import_rejects_system_collection_returns_400() {
        let app = test_app(MockRepo::new());
        let body = json!({
            "collections": [
                {
                    "name": "_sneaky",
                    "type": "base",
                    "fields": []
                }
            ]
        });
        let req = Request::put("/api/collections/import")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn import_empty_list_returns_200() {
        let app = test_app(MockRepo::new());
        let body = json!({ "collections": [] });
        let req = Request::put("/api/collections/import")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert_eq!(json["collections"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn import_invalid_json_returns_error() {
        let app = test_app(MockRepo::new());
        let req = Request::put("/api/collections/import")
            .header("content-type", "application/json")
            .body(Body::from("{ broken json"))
            .unwrap();
        let resp = send_request(&app, req).await;
        // Axum returns 400 for JSON deserialization failures.
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn import_multiple_collections() {
        let app = test_app(MockRepo::new());
        let body = json!({
            "collections": [
                {
                    "name": "posts",
                    "type": "base",
                    "fields": [
                        {
                            "name": "title",
                            "type": { "type": "text", "options": { "minLength": 0, "maxLength": 0 } }
                        }
                    ]
                },
                {
                    "name": "comments",
                    "type": "base",
                    "fields": [
                        {
                            "name": "body",
                            "type": { "type": "text", "options": { "minLength": 0, "maxLength": 0 } }
                        }
                    ]
                }
            ]
        });
        let req = Request::put("/api/collections/import")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert_eq!(json["collections"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn list_response_matches_pocketbase_format() {
        let repo = MockRepo::with_collections(vec![make_dto("posts", "base")]);
        let app = test_app(repo);
        let req = Request::get("/api/collections")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&app, req).await;
        let json = body_json(resp).await;

        // Verify all PocketBase-style fields are present
        assert!(json.get("page").is_some(), "missing 'page'");
        assert!(json.get("perPage").is_some(), "missing 'perPage'");
        assert!(json.get("totalItems").is_some(), "missing 'totalItems'");
        assert!(json.get("totalPages").is_some(), "missing 'totalPages'");
        assert!(json.get("items").is_some(), "missing 'items'");
    }
}
