//! Auto-generated OpenAPI documentation endpoint.
//!
//! Dynamically generates an OpenAPI 3.1.0 specification based on registered
//! collections and their schemas. Also serves a Swagger UI at `/_/api/docs`.

use std::sync::Arc;

use axum::extract::State;
use axum::response::{Html, IntoResponse};
use axum::Json;
use serde_json::{json, Map, Value};

use zerobase_core::{Collection, CollectionType, FieldType};
use zerobase_core::services::collection_service::SchemaRepository;
use zerobase_core::CollectionService;

/// Shared state for the OpenAPI handler.
pub type OpenApiState<R> = Arc<CollectionService<R>>;

/// Serve the OpenAPI JSON specification.
///
/// The spec is generated dynamically from the current collection schemas,
/// so it always reflects the latest state.
pub async fn openapi_spec<R: SchemaRepository + 'static>(
    State(service): State<OpenApiState<R>>,
) -> Json<Value> {
    let collections = service
        .list_collections()
        .unwrap_or_default();

    Json(generate_openapi_spec(&collections))
}

/// Serve the Swagger UI HTML page.
///
/// The page loads the OpenAPI spec from `/_/api/docs/openapi.json`.
pub async fn swagger_ui() -> impl IntoResponse {
    Html(SWAGGER_UI_HTML)
}

// ── OpenAPI spec generation ─────────────────────────────────────────────────

/// Generate a complete OpenAPI 3.1.0 specification from the given collections.
pub fn generate_openapi_spec(collections: &[Collection]) -> Value {
    let mut paths = Map::new();
    let mut schemas = Map::new();

    // Add system schemas
    add_system_schemas(&mut schemas);

    // Add health endpoint
    paths.insert("/api/health".to_string(), health_path());

    // Add collection management endpoints
    add_collection_management_paths(&mut paths);

    // Add per-collection record endpoints
    for collection in collections {
        add_collection_record_paths(&mut paths, collection);
        add_collection_schema(&mut schemas, collection);

        // Auth-specific endpoints
        if collection.collection_type == CollectionType::Auth {
            add_auth_paths(&mut paths, &collection.name);
        }
    }

    // Add superuser endpoints
    add_superuser_paths(&mut paths);

    // Add file endpoints
    add_file_paths(&mut paths);

    // Add settings endpoints
    add_settings_paths(&mut paths);

    // Add realtime endpoints
    add_realtime_paths(&mut paths);

    // Add batch endpoint
    add_batch_paths(&mut paths);

    // Add backup endpoints
    add_backup_paths(&mut paths);

    // Add log endpoints
    add_log_paths(&mut paths);

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Zerobase API",
            "description": "Auto-generated API documentation for the Zerobase backend. Endpoints are dynamically created based on registered collections.",
            "version": env!("CARGO_PKG_VERSION"),
            "license": {
                "name": "MIT"
            }
        },
        "servers": [
            {
                "url": "/",
                "description": "Current server"
            }
        ],
        "paths": Value::Object(paths),
        "components": {
            "schemas": Value::Object(schemas),
            "securitySchemes": {
                "bearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "JWT",
                    "description": "JWT token obtained from auth-with-password or OAuth2 endpoints"
                }
            }
        },
        "tags": build_tags(collections)
    })
}

fn build_tags(collections: &[Collection]) -> Vec<Value> {
    let mut tags = vec![
        json!({"name": "Health", "description": "Health check"}),
        json!({"name": "Collections", "description": "Collection schema management (superuser only)"}),
        json!({"name": "Auth", "description": "Authentication endpoints"}),
        json!({"name": "Files", "description": "File serving and tokens"}),
        json!({"name": "Settings", "description": "Server settings (superuser only)"}),
        json!({"name": "Realtime", "description": "Server-Sent Events for live updates"}),
        json!({"name": "Batch", "description": "Atomic batch operations"}),
        json!({"name": "Backups", "description": "Backup management (superuser only)"}),
        json!({"name": "Logs", "description": "Activity logs (superuser only)"}),
        json!({"name": "Superusers", "description": "Superuser authentication"}),
    ];

    for collection in collections {
        tags.push(json!({
            "name": format!("Records: {}", collection.name),
            "description": format!(
                "CRUD operations for the '{}' {} collection",
                collection.name,
                collection.collection_type.as_str()
            )
        }));
    }

    tags
}

// ── System schemas ──────────────────────────────────────────────────────────

fn add_system_schemas(schemas: &mut Map<String, Value>) {
    schemas.insert(
        "ListResponse".to_string(),
        json!({
            "type": "object",
            "properties": {
                "page": {"type": "integer", "example": 1},
                "perPage": {"type": "integer", "example": 30},
                "totalPages": {"type": "integer", "example": 1},
                "totalItems": {"type": "integer", "example": 0},
                "items": {"type": "array", "items": {}}
            }
        }),
    );

    schemas.insert(
        "ErrorResponse".to_string(),
        json!({
            "type": "object",
            "properties": {
                "code": {"type": "integer", "example": 400},
                "message": {"type": "string", "example": "Something went wrong."},
                "data": {"type": "object"}
            }
        }),
    );

    schemas.insert(
        "AuthResponse".to_string(),
        json!({
            "type": "object",
            "properties": {
                "token": {"type": "string"},
                "record": {"type": "object"}
            }
        }),
    );

    schemas.insert(
        "Collection".to_string(),
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "name": {"type": "string"},
                "type": {"type": "string", "enum": ["base", "auth", "view"]},
                "fields": {"type": "array", "items": {"$ref": "#/components/schemas/Field"}},
                "rules": {"type": "object"},
                "indexes": {"type": "array", "items": {"type": "object"}}
            }
        }),
    );

    schemas.insert(
        "Field".to_string(),
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "name": {"type": "string"},
                "type": {"type": "string"},
                "required": {"type": "boolean"},
                "unique": {"type": "boolean"},
                "options": {"type": "object"}
            }
        }),
    );

    schemas.insert(
        "HealthResponse".to_string(),
        json!({
            "type": "object",
            "properties": {
                "status": {"type": "string", "example": "healthy"}
            }
        }),
    );
}

// ── Path builders ───────────────────────────────────────────────────────────

fn health_path() -> Value {
    json!({
        "get": {
            "tags": ["Health"],
            "summary": "Health check",
            "operationId": "healthCheck",
            "responses": {
                "200": {
                    "description": "Server is healthy",
                    "content": {
                        "application/json": {
                            "schema": {"$ref": "#/components/schemas/HealthResponse"}
                        }
                    }
                }
            }
        }
    })
}

fn add_collection_management_paths(paths: &mut Map<String, Value>) {
    paths.insert(
        "/api/collections".to_string(),
        json!({
            "get": {
                "tags": ["Collections"],
                "summary": "List all collections",
                "operationId": "listCollections",
                "security": [{"bearerAuth": []}],
                "responses": {
                    "200": {
                        "description": "List of collections",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "array",
                                    "items": {"$ref": "#/components/schemas/Collection"}
                                }
                            }
                        }
                    },
                    "403": {"description": "Forbidden — superuser required"}
                }
            },
            "post": {
                "tags": ["Collections"],
                "summary": "Create a new collection",
                "operationId": "createCollection",
                "security": [{"bearerAuth": []}],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {"$ref": "#/components/schemas/Collection"}
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Collection created",
                        "content": {
                            "application/json": {
                                "schema": {"$ref": "#/components/schemas/Collection"}
                            }
                        }
                    },
                    "400": {"description": "Validation error"},
                    "403": {"description": "Forbidden — superuser required"}
                }
            }
        }),
    );

    paths.insert(
        "/api/collections/{id_or_name}".to_string(),
        json!({
            "get": {
                "tags": ["Collections"],
                "summary": "View a collection",
                "operationId": "viewCollection",
                "security": [{"bearerAuth": []}],
                "parameters": [id_or_name_param("collection")],
                "responses": {
                    "200": {
                        "description": "Collection details",
                        "content": {
                            "application/json": {
                                "schema": {"$ref": "#/components/schemas/Collection"}
                            }
                        }
                    },
                    "404": {"description": "Collection not found"}
                }
            },
            "patch": {
                "tags": ["Collections"],
                "summary": "Update a collection",
                "operationId": "updateCollection",
                "security": [{"bearerAuth": []}],
                "parameters": [id_or_name_param("collection")],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {"$ref": "#/components/schemas/Collection"}
                        }
                    }
                },
                "responses": {
                    "200": {"description": "Collection updated"},
                    "400": {"description": "Validation error"},
                    "404": {"description": "Collection not found"}
                }
            },
            "delete": {
                "tags": ["Collections"],
                "summary": "Delete a collection",
                "operationId": "deleteCollection",
                "security": [{"bearerAuth": []}],
                "parameters": [id_or_name_param("collection")],
                "responses": {
                    "204": {"description": "Collection deleted"},
                    "404": {"description": "Collection not found"}
                }
            }
        }),
    );

    paths.insert(
        "/api/collections/export".to_string(),
        json!({
            "get": {
                "tags": ["Collections"],
                "summary": "Export all collection schemas",
                "operationId": "exportCollections",
                "security": [{"bearerAuth": []}],
                "responses": {
                    "200": {"description": "Collection schemas exported"}
                }
            }
        }),
    );

    paths.insert(
        "/api/collections/import".to_string(),
        json!({
            "put": {
                "tags": ["Collections"],
                "summary": "Import collection schemas",
                "operationId": "importCollections",
                "security": [{"bearerAuth": []}],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "array",
                                "items": {"$ref": "#/components/schemas/Collection"}
                            }
                        }
                    }
                },
                "responses": {
                    "200": {"description": "Collections imported"}
                }
            }
        }),
    );
}

fn add_collection_record_paths(paths: &mut Map<String, Value> , collection: &Collection) {
    let name = &collection.name;
    let tag = format!("Records: {name}");
    let record_schema_ref = format!("#/components/schemas/{name}_record");

    // List + Create
    paths.insert(
        format!("/api/collections/{name}/records"),
        json!({
            "get": {
                "tags": [&tag],
                "summary": format!("List {name} records"),
                "operationId": format!("list_{name}_records"),
                "parameters": [
                    {"name": "page", "in": "query", "schema": {"type": "integer", "default": 1}},
                    {"name": "perPage", "in": "query", "schema": {"type": "integer", "default": 30}},
                    {"name": "sort", "in": "query", "schema": {"type": "string"}, "description": "Sort expression (e.g. '-created')"},
                    {"name": "filter", "in": "query", "schema": {"type": "string"}, "description": "Filter expression"},
                    {"name": "expand", "in": "query", "schema": {"type": "string"}, "description": "Comma-separated relation fields to expand"},
                    {"name": "fields", "in": "query", "schema": {"type": "string"}, "description": "Comma-separated fields to return"},
                    {"name": "skipTotal", "in": "query", "schema": {"type": "boolean"}, "description": "Skip COUNT query for performance"}
                ],
                "responses": {
                    "200": {
                        "description": format!("Paginated list of {name} records"),
                        "content": {
                            "application/json": {
                                "schema": {
                                    "allOf": [
                                        {"$ref": "#/components/schemas/ListResponse"},
                                        {
                                            "type": "object",
                                            "properties": {
                                                "items": {
                                                    "type": "array",
                                                    "items": {"$ref": &record_schema_ref}
                                                }
                                            }
                                        }
                                    ]
                                }
                            }
                        }
                    }
                }
            },
            "post": {
                "tags": [&tag],
                "summary": format!("Create a {name} record"),
                "operationId": format!("create_{name}_record"),
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {"$ref": &record_schema_ref}
                        },
                        "multipart/form-data": {
                            "schema": {"$ref": &record_schema_ref}
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Record created",
                        "content": {
                            "application/json": {
                                "schema": {"$ref": &record_schema_ref}
                            }
                        }
                    },
                    "400": {"description": "Validation error"},
                    "403": {"description": "Forbidden"}
                }
            }
        }),
    );

    // Count
    paths.insert(
        format!("/api/collections/{name}/records/count"),
        json!({
            "get": {
                "tags": [&tag],
                "summary": format!("Count {name} records"),
                "operationId": format!("count_{name}_records"),
                "parameters": [
                    {"name": "filter", "in": "query", "schema": {"type": "string"}, "description": "Filter expression"}
                ],
                "responses": {
                    "200": {
                        "description": "Record count",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "totalItems": {"type": "integer"}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }),
    );

    // View + Update + Delete
    paths.insert(
        format!("/api/collections/{name}/records/{{id}}"),
        json!({
            "get": {
                "tags": [&tag],
                "summary": format!("View a {name} record"),
                "operationId": format!("view_{name}_record"),
                "parameters": [
                    record_id_param(),
                    {"name": "expand", "in": "query", "schema": {"type": "string"}, "description": "Comma-separated relation fields to expand"},
                    {"name": "fields", "in": "query", "schema": {"type": "string"}, "description": "Comma-separated fields to return"}
                ],
                "responses": {
                    "200": {
                        "description": "Record details",
                        "content": {
                            "application/json": {
                                "schema": {"$ref": &record_schema_ref}
                            }
                        }
                    },
                    "404": {"description": "Record not found"}
                }
            },
            "patch": {
                "tags": [&tag],
                "summary": format!("Update a {name} record"),
                "operationId": format!("update_{name}_record"),
                "parameters": [record_id_param()],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {"$ref": &record_schema_ref}
                        },
                        "multipart/form-data": {
                            "schema": {"$ref": &record_schema_ref}
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Record updated",
                        "content": {
                            "application/json": {
                                "schema": {"$ref": &record_schema_ref}
                            }
                        }
                    },
                    "400": {"description": "Validation error"},
                    "404": {"description": "Record not found"}
                }
            },
            "delete": {
                "tags": [&tag],
                "summary": format!("Delete a {name} record"),
                "operationId": format!("delete_{name}_record"),
                "parameters": [record_id_param()],
                "responses": {
                    "204": {"description": "Record deleted"},
                    "404": {"description": "Record not found"}
                }
            }
        }),
    );
}

fn add_collection_schema(schemas: &mut Map<String, Value>, collection: &Collection) {
    let mut properties = Map::new();
    let mut required_fields = Vec::new();

    // System fields
    properties.insert(
        "id".to_string(),
        json!({"type": "string", "description": "Record ID (15-char alphanumeric)", "readOnly": true}),
    );
    properties.insert(
        "collectionId".to_string(),
        json!({"type": "string", "description": "Collection ID", "readOnly": true}),
    );
    properties.insert(
        "collectionName".to_string(),
        json!({"type": "string", "description": "Collection name", "readOnly": true}),
    );
    properties.insert(
        "created".to_string(),
        json!({"type": "string", "format": "date-time", "description": "Record creation timestamp", "readOnly": true}),
    );
    properties.insert(
        "updated".to_string(),
        json!({"type": "string", "format": "date-time", "description": "Record last update timestamp", "readOnly": true}),
    );

    // Auth-specific system fields
    if collection.collection_type == CollectionType::Auth {
        properties.insert(
            "email".to_string(),
            json!({"type": "string", "format": "email"}),
        );
        properties.insert(
            "emailVisibility".to_string(),
            json!({"type": "boolean"}),
        );
        properties.insert(
            "verified".to_string(),
            json!({"type": "boolean", "readOnly": true}),
        );
        properties.insert(
            "username".to_string(),
            json!({"type": "string"}),
        );
    }

    // User-defined fields
    for field in &collection.fields {
        let field_schema = field_type_to_openapi_schema(&field.field_type);
        properties.insert(field.name.clone(), field_schema);
        if field.required {
            required_fields.push(Value::String(field.name.clone()));
        }
    }

    let mut schema = json!({
        "type": "object",
        "properties": Value::Object(properties)
    });

    if !required_fields.is_empty() {
        schema["required"] = Value::Array(required_fields);
    }

    schemas.insert(format!("{}_record", collection.name), schema);
}

fn field_type_to_openapi_schema(field_type: &FieldType) -> Value {
    match field_type {
        FieldType::Text(_) => json!({"type": "string"}),
        FieldType::Number(_) => json!({"type": "number"}),
        FieldType::Bool(_) => json!({"type": "boolean"}),
        FieldType::Email(_) => json!({"type": "string", "format": "email"}),
        FieldType::Url(_) => json!({"type": "string", "format": "uri"}),
        FieldType::DateTime(_) => json!({"type": "string", "format": "date-time"}),
        FieldType::AutoDate(_) => json!({"type": "string", "format": "date-time", "readOnly": true}),
        FieldType::Select(opts) => {
            let mut schema = json!({"type": "string"});
            if !opts.values.is_empty() {
                schema["enum"] = json!(opts.values);
            }
            schema
        }
        FieldType::MultiSelect(opts) => {
            let mut items = json!({"type": "string"});
            if !opts.values.is_empty() {
                items["enum"] = json!(opts.values);
            }
            json!({"type": "array", "items": items})
        }
        FieldType::File(_) => json!({"type": "string", "description": "File name or upload"}),
        FieldType::Relation(_) => json!({"type": "string", "description": "Related record ID (or array of IDs for multi-relations)"}),
        FieldType::Json(_) => json!({"description": "Arbitrary JSON value"}),
        FieldType::Editor(_) => json!({"type": "string", "description": "Rich text HTML content"}),
        FieldType::Password(_) => json!({"type": "string", "format": "password", "writeOnly": true}),
    }
}

fn add_auth_paths(paths: &mut Map<String, Value>, collection_name: &str) {
    paths.insert(
        format!("/api/collections/{collection_name}/auth-with-password"),
        json!({
            "post": {
                "tags": ["Auth", format!("Records: {collection_name}")],
                "summary": format!("Authenticate with password ({collection_name})"),
                "operationId": format!("auth_with_password_{collection_name}"),
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "required": ["identity", "password"],
                                "properties": {
                                    "identity": {"type": "string", "description": "Email or username"},
                                    "password": {"type": "string", "format": "password"}
                                }
                            }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Authentication successful",
                        "content": {
                            "application/json": {
                                "schema": {"$ref": "#/components/schemas/AuthResponse"}
                            }
                        }
                    },
                    "400": {"description": "Invalid credentials"}
                }
            }
        }),
    );

    paths.insert(
        format!("/api/collections/{collection_name}/auth-refresh"),
        json!({
            "post": {
                "tags": ["Auth", format!("Records: {collection_name}")],
                "summary": format!("Refresh auth token ({collection_name})"),
                "operationId": format!("auth_refresh_{collection_name}"),
                "security": [{"bearerAuth": []}],
                "responses": {
                    "200": {
                        "description": "Token refreshed",
                        "content": {
                            "application/json": {
                                "schema": {"$ref": "#/components/schemas/AuthResponse"}
                            }
                        }
                    },
                    "401": {"description": "Invalid or expired token"}
                }
            }
        }),
    );

    paths.insert(
        format!("/api/collections/{collection_name}/request-verification"),
        json!({
            "post": {
                "tags": ["Auth", format!("Records: {collection_name}")],
                "summary": format!("Request email verification ({collection_name})"),
                "operationId": format!("request_verification_{collection_name}"),
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "required": ["email"],
                                "properties": {
                                    "email": {"type": "string", "format": "email"}
                                }
                            }
                        }
                    }
                },
                "responses": {
                    "204": {"description": "Verification email sent"}
                }
            }
        }),
    );

    paths.insert(
        format!("/api/collections/{collection_name}/request-password-reset"),
        json!({
            "post": {
                "tags": ["Auth", format!("Records: {collection_name}")],
                "summary": format!("Request password reset ({collection_name})"),
                "operationId": format!("request_password_reset_{collection_name}"),
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "required": ["email"],
                                "properties": {
                                    "email": {"type": "string", "format": "email"}
                                }
                            }
                        }
                    }
                },
                "responses": {
                    "204": {"description": "Password reset email sent"}
                }
            }
        }),
    );

    paths.insert(
        format!("/api/collections/{collection_name}/request-otp"),
        json!({
            "post": {
                "tags": ["Auth", format!("Records: {collection_name}")],
                "summary": format!("Request OTP code ({collection_name})"),
                "operationId": format!("request_otp_{collection_name}"),
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "required": ["email"],
                                "properties": {
                                    "email": {"type": "string", "format": "email"}
                                }
                            }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "OTP sent",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "otpId": {"type": "string"}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }),
    );

    paths.insert(
        format!("/api/collections/{collection_name}/auth-methods"),
        json!({
            "get": {
                "tags": ["Auth", format!("Records: {collection_name}")],
                "summary": format!("List enabled auth methods ({collection_name})"),
                "operationId": format!("list_auth_methods_{collection_name}"),
                "responses": {
                    "200": {
                        "description": "Available authentication methods",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "password": {"type": "object"},
                                        "oauth2": {"type": "object"},
                                        "otp": {"type": "object"},
                                        "mfa": {"type": "object"}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

fn add_superuser_paths(paths: &mut Map<String, Value>) {
    paths.insert(
        "/_/api/admins/auth-with-password".to_string(),
        json!({
            "post": {
                "tags": ["Superusers"],
                "summary": "Superuser login",
                "operationId": "superuserAuthWithPassword",
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "required": ["identity", "password"],
                                "properties": {
                                    "identity": {"type": "string"},
                                    "password": {"type": "string", "format": "password"}
                                }
                            }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Admin authenticated",
                        "content": {
                            "application/json": {
                                "schema": {"$ref": "#/components/schemas/AuthResponse"}
                            }
                        }
                    },
                    "400": {"description": "Invalid credentials"}
                }
            }
        }),
    );
}

fn add_file_paths(paths: &mut Map<String, Value>) {
    paths.insert(
        "/api/files/token".to_string(),
        json!({
            "get": {
                "tags": ["Files"],
                "summary": "Generate a file access token",
                "operationId": "requestFileToken",
                "security": [{"bearerAuth": []}],
                "responses": {
                    "200": {
                        "description": "File access token",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "token": {"type": "string"}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }),
    );

    paths.insert(
        "/api/files/{collectionId}/{recordId}/{filename}".to_string(),
        json!({
            "get": {
                "tags": ["Files"],
                "summary": "Serve a file",
                "operationId": "serveFile",
                "parameters": [
                    {"name": "collectionId", "in": "path", "required": true, "schema": {"type": "string"}},
                    {"name": "recordId", "in": "path", "required": true, "schema": {"type": "string"}},
                    {"name": "filename", "in": "path", "required": true, "schema": {"type": "string"}},
                    {"name": "token", "in": "query", "schema": {"type": "string"}, "description": "File access token for protected files"},
                    {"name": "thumb", "in": "query", "schema": {"type": "string"}, "description": "Thumbnail dimensions (e.g. '100x100')"}
                ],
                "responses": {
                    "200": {
                        "description": "File content",
                        "content": {
                            "application/octet-stream": {
                                "schema": {"type": "string", "format": "binary"}
                            }
                        }
                    },
                    "404": {"description": "File not found"}
                }
            }
        }),
    );
}

fn add_settings_paths(paths: &mut Map<String, Value>) {
    paths.insert(
        "/api/settings".to_string(),
        json!({
            "get": {
                "tags": ["Settings"],
                "summary": "Get all settings",
                "operationId": "getAllSettings",
                "security": [{"bearerAuth": []}],
                "responses": {
                    "200": {"description": "All settings"},
                    "403": {"description": "Forbidden — superuser required"}
                }
            },
            "patch": {
                "tags": ["Settings"],
                "summary": "Update settings",
                "operationId": "updateSettings",
                "security": [{"bearerAuth": []}],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {"type": "object"}
                        }
                    }
                },
                "responses": {
                    "200": {"description": "Settings updated"},
                    "403": {"description": "Forbidden — superuser required"}
                }
            }
        }),
    );

    paths.insert(
        "/api/settings/{key}".to_string(),
        json!({
            "get": {
                "tags": ["Settings"],
                "summary": "Get a single setting",
                "operationId": "getSetting",
                "security": [{"bearerAuth": []}],
                "parameters": [
                    {"name": "key", "in": "path", "required": true, "schema": {"type": "string"}}
                ],
                "responses": {
                    "200": {"description": "Setting value"},
                    "404": {"description": "Setting not found"}
                }
            },
            "delete": {
                "tags": ["Settings"],
                "summary": "Reset a setting to default",
                "operationId": "resetSetting",
                "security": [{"bearerAuth": []}],
                "parameters": [
                    {"name": "key", "in": "path", "required": true, "schema": {"type": "string"}}
                ],
                "responses": {
                    "204": {"description": "Setting reset"},
                    "404": {"description": "Setting not found"}
                }
            }
        }),
    );
}

fn add_realtime_paths(paths: &mut Map<String, Value>) {
    paths.insert(
        "/api/realtime".to_string(),
        json!({
            "get": {
                "tags": ["Realtime"],
                "summary": "Establish SSE connection",
                "operationId": "sseConnect",
                "description": "Opens a Server-Sent Events connection for receiving real-time record changes.",
                "responses": {
                    "200": {
                        "description": "SSE stream established",
                        "content": {
                            "text/event-stream": {
                                "schema": {"type": "string"}
                            }
                        }
                    }
                }
            },
            "post": {
                "tags": ["Realtime"],
                "summary": "Set subscriptions",
                "operationId": "setSubscriptions",
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "required": ["clientId", "subscriptions"],
                                "properties": {
                                    "clientId": {"type": "string"},
                                    "subscriptions": {
                                        "type": "array",
                                        "items": {"type": "string"}
                                    }
                                }
                            }
                        }
                    }
                },
                "responses": {
                    "204": {"description": "Subscriptions updated"}
                }
            }
        }),
    );
}

fn add_batch_paths(paths: &mut Map<String, Value>) {
    paths.insert(
        "/api/batch".to_string(),
        json!({
            "post": {
                "tags": ["Batch"],
                "summary": "Execute batch operations",
                "operationId": "executeBatch",
                "description": "Execute multiple record operations atomically in a single transaction.",
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "required": ["requests"],
                                "properties": {
                                    "requests": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "method": {"type": "string", "enum": ["GET", "POST", "PATCH", "DELETE"]},
                                                "url": {"type": "string"},
                                                "body": {"type": "object"}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Batch results",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "status": {"type": "integer"},
                                            "body": {"type": "object"}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

fn add_backup_paths(paths: &mut Map<String, Value>) {
    paths.insert(
        "/_/api/backups".to_string(),
        json!({
            "get": {
                "tags": ["Backups"],
                "summary": "List all backups",
                "operationId": "listBackups",
                "security": [{"bearerAuth": []}],
                "responses": {
                    "200": {"description": "List of backups"},
                    "403": {"description": "Forbidden — superuser required"}
                }
            },
            "post": {
                "tags": ["Backups"],
                "summary": "Create a new backup",
                "operationId": "createBackup",
                "security": [{"bearerAuth": []}],
                "responses": {
                    "200": {"description": "Backup created"},
                    "403": {"description": "Forbidden — superuser required"}
                }
            }
        }),
    );

    paths.insert(
        "/_/api/backups/{name}".to_string(),
        json!({
            "get": {
                "tags": ["Backups"],
                "summary": "Download a backup",
                "operationId": "downloadBackup",
                "security": [{"bearerAuth": []}],
                "parameters": [
                    {"name": "name", "in": "path", "required": true, "schema": {"type": "string"}}
                ],
                "responses": {
                    "200": {
                        "description": "Backup file",
                        "content": {
                            "application/octet-stream": {
                                "schema": {"type": "string", "format": "binary"}
                            }
                        }
                    },
                    "404": {"description": "Backup not found"}
                }
            },
            "delete": {
                "tags": ["Backups"],
                "summary": "Delete a backup",
                "operationId": "deleteBackup",
                "security": [{"bearerAuth": []}],
                "parameters": [
                    {"name": "name", "in": "path", "required": true, "schema": {"type": "string"}}
                ],
                "responses": {
                    "204": {"description": "Backup deleted"},
                    "404": {"description": "Backup not found"}
                }
            }
        }),
    );
}

fn add_log_paths(paths: &mut Map<String, Value>) {
    paths.insert(
        "/_/api/logs".to_string(),
        json!({
            "get": {
                "tags": ["Logs"],
                "summary": "List activity logs",
                "operationId": "listLogs",
                "security": [{"bearerAuth": []}],
                "parameters": [
                    {"name": "page", "in": "query", "schema": {"type": "integer", "default": 1}},
                    {"name": "perPage", "in": "query", "schema": {"type": "integer", "default": 30}},
                    {"name": "filter", "in": "query", "schema": {"type": "string"}}
                ],
                "responses": {
                    "200": {"description": "List of log entries"},
                    "403": {"description": "Forbidden — superuser required"}
                }
            }
        }),
    );

    paths.insert(
        "/_/api/logs/stats".to_string(),
        json!({
            "get": {
                "tags": ["Logs"],
                "summary": "Log statistics",
                "operationId": "logStats",
                "security": [{"bearerAuth": []}],
                "responses": {
                    "200": {"description": "Aggregated log statistics"},
                    "403": {"description": "Forbidden — superuser required"}
                }
            }
        }),
    );

    paths.insert(
        "/_/api/logs/{id}".to_string(),
        json!({
            "get": {
                "tags": ["Logs"],
                "summary": "View a log entry",
                "operationId": "getLog",
                "security": [{"bearerAuth": []}],
                "parameters": [
                    {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}
                ],
                "responses": {
                    "200": {"description": "Log entry details"},
                    "404": {"description": "Log not found"}
                }
            }
        }),
    );
}

// ── Helper functions ────────────────────────────────────────────────────────

fn id_or_name_param(resource: &str) -> Value {
    json!({
        "name": "id_or_name",
        "in": "path",
        "required": true,
        "schema": {"type": "string"},
        "description": format!("The {resource} ID or name")
    })
}

fn record_id_param() -> Value {
    json!({
        "name": "id",
        "in": "path",
        "required": true,
        "schema": {"type": "string"},
        "description": "Record ID"
    })
}

// ── Swagger UI HTML ─────────────────────────────────────────────────────────

const SWAGGER_UI_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Zerobase API Documentation</title>
    <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
    <style>
        html { box-sizing: border-box; overflow-y: scroll; }
        *, *:before, *:after { box-sizing: inherit; }
        body { margin: 0; background: #fafafa; }
        .topbar { display: none !important; }
    </style>
</head>
<body>
    <div id="swagger-ui"></div>
    <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
    <script>
        SwaggerUIBundle({
            url: '/_/api/docs/openapi.json',
            dom_id: '#swagger-ui',
            deepLinking: true,
            presets: [
                SwaggerUIBundle.presets.apis,
                SwaggerUIBundle.SwaggerUIStandalonePreset
            ],
            layout: 'BaseLayout',
            defaultModelsExpandDepth: 1,
            docExpansion: 'list',
            filter: true,
            tryItOutEnabled: true
        });
    </script>
</body>
</html>"#;

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use zerobase_core::schema::{
        AuthOptions, BoolOptions, EmailOptions, NumberOptions, SelectOptions, TextOptions,
    };
    use zerobase_core::{ApiRules, Collection, CollectionType, Field, FieldType};

    fn make_base_collection() -> Collection {
        Collection {
            id: "test123".to_string(),
            name: "posts".to_string(),
            collection_type: CollectionType::Base,
            fields: vec![
                Field::new("title", FieldType::Text(TextOptions::default())).required(true),
                Field::new("body", FieldType::Text(TextOptions::default())),
                Field::new("views", FieldType::Number(NumberOptions::default())),
                Field::new("published", FieldType::Bool(BoolOptions::default())),
            ],
            rules: ApiRules::default(),
            indexes: vec![],
            view_query: None,
            auth_options: None,
        }
    }

    fn make_auth_collection() -> Collection {
        Collection {
            id: "auth123".to_string(),
            name: "users".to_string(),
            collection_type: CollectionType::Auth,
            fields: vec![
                Field::new("name", FieldType::Text(TextOptions::default())),
                Field::new("avatar", FieldType::Text(TextOptions::default())),
            ],
            rules: ApiRules::default(),
            indexes: vec![],
            view_query: None,
            auth_options: Some(AuthOptions::default()),
        }
    }

    #[test]
    fn generates_valid_openapi_spec() {
        let collections = vec![make_base_collection()];
        let spec = generate_openapi_spec(&collections);

        assert_eq!(spec["openapi"], "3.1.0");
        assert_eq!(spec["info"]["title"], "Zerobase API");
        assert!(spec["paths"].is_object());
        assert!(spec["components"]["schemas"].is_object());
        assert!(spec["components"]["securitySchemes"]["bearerAuth"].is_object());
    }

    #[test]
    fn includes_health_endpoint() {
        let spec = generate_openapi_spec(&[]);
        assert!(spec["paths"]["/api/health"]["get"].is_object());
    }

    #[test]
    fn includes_collection_management_paths() {
        let spec = generate_openapi_spec(&[]);
        assert!(spec["paths"]["/api/collections"]["get"].is_object());
        assert!(spec["paths"]["/api/collections"]["post"].is_object());
        assert!(spec["paths"]["/api/collections/{id_or_name}"]["get"].is_object());
        assert!(spec["paths"]["/api/collections/{id_or_name}"]["patch"].is_object());
        assert!(spec["paths"]["/api/collections/{id_or_name}"]["delete"].is_object());
    }

    #[test]
    fn generates_record_paths_for_base_collection() {
        let collections = vec![make_base_collection()];
        let spec = generate_openapi_spec(&collections);

        let list_path = &spec["paths"]["/api/collections/posts/records"];
        assert!(list_path["get"].is_object());
        assert!(list_path["post"].is_object());

        let single_path = &spec["paths"]["/api/collections/posts/records/{id}"];
        assert!(single_path["get"].is_object());
        assert!(single_path["patch"].is_object());
        assert!(single_path["delete"].is_object());

        let count_path = &spec["paths"]["/api/collections/posts/records/count"];
        assert!(count_path["get"].is_object());
    }

    #[test]
    fn generates_auth_paths_for_auth_collection() {
        let collections = vec![make_auth_collection()];
        let spec = generate_openapi_spec(&collections);

        assert!(spec["paths"]["/api/collections/users/auth-with-password"]["post"].is_object());
        assert!(spec["paths"]["/api/collections/users/auth-refresh"]["post"].is_object());
        assert!(spec["paths"]["/api/collections/users/request-verification"]["post"].is_object());
        assert!(spec["paths"]["/api/collections/users/auth-methods"]["get"].is_object());
    }

    #[test]
    fn no_auth_paths_for_base_collection() {
        let collections = vec![make_base_collection()];
        let spec = generate_openapi_spec(&collections);

        assert!(spec["paths"]["/api/collections/posts/auth-with-password"].is_null());
    }

    #[test]
    fn generates_record_schema_with_fields() {
        let collections = vec![make_base_collection()];
        let spec = generate_openapi_spec(&collections);

        let schema = &spec["components"]["schemas"]["posts_record"];
        assert_eq!(schema["type"], "object");

        // System fields
        assert!(schema["properties"]["id"].is_object());
        assert!(schema["properties"]["created"].is_object());
        assert!(schema["properties"]["updated"].is_object());

        // User-defined fields
        assert_eq!(schema["properties"]["title"]["type"], "string");
        assert_eq!(schema["properties"]["views"]["type"], "number");
        assert_eq!(schema["properties"]["published"]["type"], "boolean");

        // Required fields
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("title")));
        assert!(!required.contains(&json!("body")));
    }

    #[test]
    fn auth_collection_schema_includes_auth_fields() {
        let collections = vec![make_auth_collection()];
        let spec = generate_openapi_spec(&collections);

        let schema = &spec["components"]["schemas"]["users_record"];
        assert!(schema["properties"]["email"].is_object());
        assert!(schema["properties"]["emailVisibility"].is_object());
        assert!(schema["properties"]["verified"].is_object());
        assert!(schema["properties"]["username"].is_object());
    }

    #[test]
    fn includes_superuser_paths() {
        let spec = generate_openapi_spec(&[]);
        assert!(spec["paths"]["/_/api/admins/auth-with-password"]["post"].is_object());
    }

    #[test]
    fn includes_file_paths() {
        let spec = generate_openapi_spec(&[]);
        assert!(spec["paths"]["/api/files/token"]["get"].is_object());
        assert!(spec["paths"]["/api/files/{collectionId}/{recordId}/{filename}"]["get"].is_object());
    }

    #[test]
    fn includes_settings_paths() {
        let spec = generate_openapi_spec(&[]);
        assert!(spec["paths"]["/api/settings"]["get"].is_object());
        assert!(spec["paths"]["/api/settings"]["patch"].is_object());
    }

    #[test]
    fn includes_realtime_paths() {
        let spec = generate_openapi_spec(&[]);
        assert!(spec["paths"]["/api/realtime"]["get"].is_object());
        assert!(spec["paths"]["/api/realtime"]["post"].is_object());
    }

    #[test]
    fn includes_batch_path() {
        let spec = generate_openapi_spec(&[]);
        assert!(spec["paths"]["/api/batch"]["post"].is_object());
    }

    #[test]
    fn includes_backup_paths() {
        let spec = generate_openapi_spec(&[]);
        assert!(spec["paths"]["/_/api/backups"]["get"].is_object());
        assert!(spec["paths"]["/_/api/backups"]["post"].is_object());
    }

    #[test]
    fn includes_log_paths() {
        let spec = generate_openapi_spec(&[]);
        assert!(spec["paths"]["/_/api/logs"]["get"].is_object());
        assert!(spec["paths"]["/_/api/logs/stats"]["get"].is_object());
    }

    #[test]
    fn spec_updates_with_multiple_collections() {
        let collections = vec![make_base_collection(), make_auth_collection()];
        let spec = generate_openapi_spec(&collections);

        // Both collections should have record endpoints
        assert!(spec["paths"]["/api/collections/posts/records"]["get"].is_object());
        assert!(spec["paths"]["/api/collections/users/records"]["get"].is_object());

        // Both should have schemas
        assert!(spec["components"]["schemas"]["posts_record"].is_object());
        assert!(spec["components"]["schemas"]["users_record"].is_object());

        // Only users (auth) should have auth paths
        assert!(spec["paths"]["/api/collections/users/auth-with-password"]["post"].is_object());
        assert!(spec["paths"]["/api/collections/posts/auth-with-password"].is_null());
    }

    #[test]
    fn field_type_to_openapi_mapping() {
        assert_eq!(
            field_type_to_openapi_schema(&FieldType::Text(TextOptions::default()))["type"],
            "string"
        );
        assert_eq!(
            field_type_to_openapi_schema(&FieldType::Number(NumberOptions::default()))["type"],
            "number"
        );
        assert_eq!(
            field_type_to_openapi_schema(&FieldType::Bool(BoolOptions::default()))["type"],
            "boolean"
        );
        assert_eq!(
            field_type_to_openapi_schema(&FieldType::Email(EmailOptions::default()))["format"],
            "email"
        );
    }

    #[test]
    fn select_field_includes_enum_values() {
        let select = FieldType::Select(SelectOptions {
            values: vec!["draft".into(), "published".into(), "archived".into()],
            ..Default::default()
        });
        let schema = field_type_to_openapi_schema(&select);
        assert_eq!(schema["type"], "string");
        let values = schema["enum"].as_array().unwrap();
        assert_eq!(values.len(), 3);
        assert!(values.contains(&json!("draft")));
    }

    #[test]
    fn empty_collections_produces_valid_spec() {
        let spec = generate_openapi_spec(&[]);
        assert_eq!(spec["openapi"], "3.1.0");
        assert!(spec["paths"].is_object());
        // Should still have system paths
        assert!(spec["paths"]["/api/health"]["get"].is_object());
    }

    #[test]
    fn tags_include_collection_names() {
        let collections = vec![make_base_collection(), make_auth_collection()];
        let spec = generate_openapi_spec(&collections);
        let tags = spec["tags"].as_array().unwrap();
        let tag_names: Vec<&str> = tags.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(tag_names.contains(&"Records: posts"));
        assert!(tag_names.contains(&"Records: users"));
        assert!(tag_names.contains(&"Health"));
        assert!(tag_names.contains(&"Collections"));
    }
}
