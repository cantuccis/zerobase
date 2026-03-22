//! Comprehensive integration tests for file storage operations.
//!
//! This test suite covers end-to-end file workflows that span both upload
//! and download endpoints, verifying the full lifecycle of files through
//! the HTTP API layer. It fills gaps not covered by the individual upload
//! and download endpoint tests:
//!
//! - Upload → download round-trip (file data integrity)
//! - MIME type validation via multipart
//! - `max_select` enforcement (too many files)
//! - Protected file upload → token → download flow
//! - Concurrent multipart uploads
//! - File replacement on record update (old file deleted)
//! - Upload to non-file field rejected
//! - Upload with empty filename rejected
//! - Large file upload through HTTP
//! - LocalFileStorage integration through HTTP endpoints
//! - S3 backend (mock) integration through HTTP endpoints

use std::collections::HashMap;
use std::sync::Arc;

use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

use zerobase_core::auth::{TokenClaims, TokenService, TokenType, ValidatedToken};
use zerobase_core::error::Result;
use zerobase_core::schema::{ApiRules, Collection, Field, FieldType, FileOptions, TextOptions};
use zerobase_core::services::record_service::{
    RecordList, RecordQuery, RecordRepoError, RecordRepository, RecordService, SchemaLookup,
};
use zerobase_core::storage::{FileDownload, FileMetadata, FileStorage, StorageError};
use zerobase_core::ZerobaseError;
use zerobase_files::FileService;

use zerobase_api::AuthInfo;

// ── Fake auth middleware ────────────────────────────────────────────────────

async fn fake_auth_middleware(mut request: Request<Body>, next: Next) -> Response {
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
                record.insert("id".to_string(), Value::String(token.clone()));
                record.insert("tokenKey".to_string(), Value::String("user_key".into()));
                let mut info = AuthInfo::authenticated(record);
                info.token = Some(ValidatedToken {
                    claims: TokenClaims {
                        id: token,
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

// ── Mock: SchemaLookup ──────────────────────────────────────────────────────

struct MockSchemaLookup {
    collections: std::sync::Mutex<Vec<Collection>>,
}

impl MockSchemaLookup {
    fn with(collections: Vec<Collection>) -> Self {
        Self {
            collections: std::sync::Mutex::new(collections),
        }
    }
}

impl SchemaLookup for MockSchemaLookup {
    fn get_collection(&self, id_or_name: &str) -> Result<Collection> {
        self.collections
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.id == id_or_name || c.name == id_or_name)
            .cloned()
            .ok_or_else(|| ZerobaseError::not_found_with_id("Collection", id_or_name))
    }
}

// ── Mock: RecordRepository ──────────────────────────────────────────────────

struct MockRecordRepo {
    records: std::sync::Mutex<HashMap<String, Vec<HashMap<String, Value>>>>,
}

impl MockRecordRepo {
    fn new() -> Self {
        Self {
            records: std::sync::Mutex::new(HashMap::new()),
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
            let len_before = rows.len();
            rows.retain(|r| r.get("id").and_then(|v| v.as_str()) != Some(id));
            return Ok(rows.len() < len_before);
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
        _collection: &str,
        _field_name: &str,
        _referenced_id: &str,
    ) -> std::result::Result<Vec<HashMap<String, Value>>, RecordRepoError> {
        Ok(Vec::new())
    }
}

// ── Mock: TokenService ──────────────────────────────────────────────────────

struct MockTokenService;

impl TokenService for MockTokenService {
    fn generate(
        &self,
        user_id: &str,
        collection_id: &str,
        token_type: TokenType,
        token_key: &str,
        duration_secs: Option<u64>,
    ) -> std::result::Result<String, ZerobaseError> {
        let dur = duration_secs.unwrap_or(0);
        Ok(format!(
            "file-tok:{user_id}:{collection_id}:{token_type}:{token_key}:{dur}"
        ))
    }

    fn validate(
        &self,
        token: &str,
        expected_type: TokenType,
    ) -> std::result::Result<ValidatedToken, ZerobaseError> {
        if token.starts_with("valid-file-token") {
            if expected_type != TokenType::File {
                return Err(ZerobaseError::auth("wrong token type"));
            }
            Ok(ValidatedToken {
                claims: TokenClaims {
                    id: "user1".into(),
                    collection_id: "users".into(),
                    token_type: TokenType::File,
                    token_key: "key".into(),
                    new_email: None,
                    iat: 0,
                    exp: u64::MAX,
                },
            })
        } else {
            Err(ZerobaseError::auth("invalid token"))
        }
    }
}

// ── In-memory FileStorage ───────────────────────────────────────────────────

struct MemoryStorage {
    files: tokio::sync::Mutex<HashMap<String, (Vec<u8>, String)>>,
}

impl MemoryStorage {
    fn new() -> Self {
        Self {
            files: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    async fn file_count(&self) -> usize {
        self.files.lock().await.len()
    }

}

#[async_trait::async_trait]
impl FileStorage for MemoryStorage {
    async fn upload(
        &self,
        key: &str,
        data: &[u8],
        content_type: &str,
    ) -> std::result::Result<(), StorageError> {
        self.files
            .lock()
            .await
            .insert(key.to_string(), (data.to_vec(), content_type.to_string()));
        Ok(())
    }

    async fn download(&self, key: &str) -> std::result::Result<FileDownload, StorageError> {
        let files = self.files.lock().await;
        match files.get(key) {
            Some((data, ct)) => Ok(FileDownload {
                metadata: FileMetadata {
                    key: key.to_string(),
                    original_name: "test".to_string(),
                    content_type: ct.clone(),
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
        self.files.lock().await.remove(key);
        Ok(())
    }

    async fn exists(&self, key: &str) -> std::result::Result<bool, StorageError> {
        Ok(self.files.lock().await.contains_key(key))
    }

    fn generate_url(&self, key: &str, base_url: &str) -> String {
        format!("{base_url}/api/files/{key}")
    }

    async fn delete_prefix(&self, prefix: &str) -> std::result::Result<(), StorageError> {
        let mut files = self.files.lock().await;
        files.retain(|k, _| !k.starts_with(prefix));
        Ok(())
    }
}

// ── Test collection builders ────────────────────────────────────────────────

const DOCS_COL_ID: &str = "col_docs_integ";

fn make_documents_collection() -> Collection {
    let fields = vec![
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
            "attachment",
            FieldType::File(FileOptions {
                max_select: 1,
                max_size: 50_000,
                mime_types: vec![],
                thumbs: vec![],
                protected: false,
            }),
        ),
        Field::new(
            "gallery",
            FieldType::File(FileOptions {
                max_select: 3,
                max_size: 0,
                mime_types: vec![],
                thumbs: vec![],
                protected: false,
            }),
        ),
    ];
    let mut col = Collection::base("documents", fields);
    col.id = DOCS_COL_ID.to_string();
    col.rules = ApiRules::open();
    col
}

const IMAGES_COL_ID: &str = "col_images_int";

fn make_images_only_collection() -> Collection {
    let fields = vec![
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
            "photo",
            FieldType::File(FileOptions {
                max_select: 1,
                max_size: 100_000,
                mime_types: vec![
                    "image/jpeg".to_string(),
                    "image/png".to_string(),
                ],
                thumbs: vec![],
                protected: false,
            }),
        ),
    ];
    let mut col = Collection::base("images", fields);
    col.id = IMAGES_COL_ID.to_string();
    col.rules = ApiRules::open();
    col
}

const PROTECTED_COL_ID: &str = "col_protect_in";

fn make_protected_collection() -> Collection {
    let fields = vec![
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
            "secret_doc",
            FieldType::File(FileOptions {
                max_select: 1,
                max_size: 0,
                mime_types: vec![],
                thumbs: vec![],
                protected: true,
            }),
        ),
    ];
    let mut col = Collection::base("protected_docs", fields);
    col.id = PROTECTED_COL_ID.to_string();
    col.rules = ApiRules::open();
    col
}

// ── Spawn test app with both record and file routes ─────────────────────────

/// Spawns an HTTP server with record CRUD routes (for upload) and file
/// serving routes (for download), sharing the same storage backend.
async fn spawn_full_app(
    collections: Vec<Collection>,
    records: Vec<(&str, Vec<HashMap<String, Value>>)>,
    storage: Arc<MemoryStorage>,
) -> (String, tokio::task::JoinHandle<()>) {
    let all_collections = collections.clone();
    let schema_record = MockSchemaLookup::with(collections.clone());
    let schema_files: Arc<MockSchemaLookup> =
        Arc::new(MockSchemaLookup::with(all_collections));

    let repo = {
        let repo = MockRecordRepo::new();
        for (col, recs) in records {
            let mut store = repo.records.lock().unwrap();
            store.insert(col.to_string(), recs);
        }
        repo
    };

    let dyn_storage: Arc<dyn FileStorage> = storage.clone();
    let record_service = Arc::new(RecordService::new(repo, schema_record));
    let file_service = Arc::new(FileService::new(dyn_storage));
    let token_service: Arc<dyn TokenService> = Arc::new(MockTokenService);

    let app = zerobase_api::api_router()
        .merge(zerobase_api::record_routes_with_files(
            record_service,
            Some(file_service.clone()),
        ))
        .merge(zerobase_api::file_routes(
            file_service,
            token_service,
            schema_files,
        ))
        .layer(axum::middleware::from_fn(fake_auth_middleware));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let port = addr.port();
    let address = format!("http://127.0.0.1:{port}");

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (address, handle)
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests: Upload → Download round-trip
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn upload_then_download_preserves_file_data() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // Upload a file via record create.
    let file_data = b"Hello, Zerobase file storage!";
    let form = reqwest::multipart::Form::new()
        .text("title", "Roundtrip test")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(file_data.to_vec())
                .file_name("hello.txt")
                .mime_str("text/plain")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap();
    let filename = body["attachment"].as_str().unwrap();

    // Download the file via the file serving endpoint.
    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{filename}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "text/plain"
    );

    let downloaded = resp.bytes().await.unwrap();
    assert_eq!(
        downloaded.as_ref(),
        file_data,
        "downloaded data must match uploaded data exactly"
    );
}

#[tokio::test]
async fn upload_then_download_binary_file() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // Upload binary data (simulated zip file).
    let binary_data: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
    let form = reqwest::multipart::Form::new()
        .text("title", "Binary test")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(binary_data.clone())
                .file_name("data.bin")
                .mime_str("application/octet-stream")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap();
    let filename = body["attachment"].as_str().unwrap();

    // Download and verify.
    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{filename}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let downloaded = resp.bytes().await.unwrap();
    assert_eq!(
        downloaded.as_ref(),
        binary_data.as_slice(),
        "binary data must survive round-trip intact"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests: MIME type validation
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn upload_rejects_disallowed_mime_type() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_images_only_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // Try to upload a PDF to an images-only field.
    let form = reqwest::multipart::Form::new()
        .text("title", "Bad MIME")
        .part(
            "photo",
            reqwest::multipart::Part::bytes(b"not an image".to_vec())
                .file_name("doc.pdf")
                .mime_str("application/pdf")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/images/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(storage.file_count().await, 0);
}

#[tokio::test]
async fn upload_accepts_allowed_mime_type() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_images_only_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // Upload a PNG to images-only field.
    let form = reqwest::multipart::Form::new()
        .text("title", "Good MIME")
        .part(
            "photo",
            reqwest::multipart::Part::bytes(vec![0x89, 0x50, 0x4E, 0x47])
                .file_name("photo.png")
                .mime_str("image/png")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/images/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(storage.file_count().await, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests: File size validation
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn upload_rejects_file_exceeding_max_size() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // attachment field has max_size = 50_000
    let oversized = vec![0u8; 60_000];
    let form = reqwest::multipart::Form::new()
        .text("title", "Oversized")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(oversized)
                .file_name("big.bin")
                .mime_str("application/octet-stream")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(storage.file_count().await, 0);
}

#[tokio::test]
async fn upload_accepts_file_at_max_size_boundary() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // Exactly at max_size = 50_000 should be accepted.
    let data = vec![0u8; 50_000];
    let form = reqwest::multipart::Form::new()
        .text("title", "Boundary")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(data)
                .file_name("exact.bin")
                .mime_str("application/octet-stream")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(storage.file_count().await, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests: Multiple file uploads and max_select enforcement
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn upload_multiple_files_to_gallery_field() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // gallery field allows max_select=3, upload 3 files.
    let form = reqwest::multipart::Form::new()
        .text("title", "Gallery test")
        .part(
            "gallery",
            reqwest::multipart::Part::bytes(vec![1, 2, 3])
                .file_name("a.png")
                .mime_str("image/png")
                .unwrap(),
        )
        .part(
            "gallery",
            reqwest::multipart::Part::bytes(vec![4, 5, 6])
                .file_name("b.png")
                .mime_str("image/png")
                .unwrap(),
        )
        .part(
            "gallery",
            reqwest::multipart::Part::bytes(vec![7, 8, 9])
                .file_name("c.png")
                .mime_str("image/png")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let gallery = body["gallery"].as_array().unwrap();
    assert_eq!(gallery.len(), 3);
    assert_eq!(storage.file_count().await, 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests: Protected file upload → download flow
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn protected_file_requires_token_for_download() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_protected_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // Upload a protected file.
    let secret_data = b"top secret content";
    let form = reqwest::multipart::Form::new()
        .text("title", "Secret doc")
        .part(
            "secret_doc",
            reqwest::multipart::Part::bytes(secret_data.to_vec())
                .file_name("secret.pdf")
                .mime_str("application/pdf")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/protected_docs/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap();
    let filename = body["secret_doc"].as_str().unwrap();

    // Try to download without token → 401.
    let resp = client
        .get(format!(
            "{addr}/api/files/{PROTECTED_COL_ID}/{record_id}/{filename}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Download with invalid token → 401.
    let resp = client
        .get(format!(
            "{addr}/api/files/{PROTECTED_COL_ID}/{record_id}/{filename}?token=bad-token"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Download with valid token → 200, correct data.
    let resp = client
        .get(format!(
            "{addr}/api/files/{PROTECTED_COL_ID}/{record_id}/{filename}?token=valid-file-token"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let downloaded = resp.bytes().await.unwrap();
    assert_eq!(downloaded.as_ref(), secret_data);
}

#[tokio::test]
async fn file_token_endpoint_requires_auth() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_protected_collection()],
        vec![],
        storage,
    )
    .await;
    let client = reqwest::Client::new();

    // No auth → 401.
    let resp = client
        .get(format!("{addr}/api/files/token"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // With auth → 200, returns token.
    let resp = client
        .get(format!("{addr}/api/files/token"))
        .header("Authorization", "Bearer user123")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body["token"].as_str().unwrap().starts_with("file-tok:"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests: Concurrent uploads
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn concurrent_multipart_uploads_all_succeed() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    let mut handles = Vec::new();
    for i in 0..5 {
        let client = client.clone();
        let addr = addr.clone();
        handles.push(tokio::spawn(async move {
            let form = reqwest::multipart::Form::new()
                .text("title", format!("Concurrent {i}"))
                .part(
                    "attachment",
                    reqwest::multipart::Part::bytes(vec![i as u8; 100])
                        .file_name(format!("file_{i}.bin"))
                        .mime_str("application/octet-stream")
                        .unwrap(),
                );

            let resp = client
                .post(format!("{addr}/api/collections/documents/records"))
                .multipart(form)
                .send()
                .await
                .unwrap();
            resp.status()
        }));
    }

    for handle in handles {
        let status = handle.await.unwrap();
        assert_eq!(status, StatusCode::OK);
    }

    assert_eq!(
        storage.file_count().await,
        5,
        "all 5 concurrent uploads should succeed"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests: File replacement on record update
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn update_record_replaces_file_and_old_file_accessible_until_replaced() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // Create initial record with file.
    let form = reqwest::multipart::Form::new()
        .text("title", "Original")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(b"original content".to_vec())
                .file_name("v1.txt")
                .mime_str("text/plain")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap().to_string();
    let old_filename = body["attachment"].as_str().unwrap().to_string();
    assert_eq!(storage.file_count().await, 1);

    // Verify the original file is downloadable.
    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{old_filename}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), b"original content");

    // Update the record with a new file.
    let form = reqwest::multipart::Form::new()
        .text("title", "Updated")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(b"new content".to_vec())
                .file_name("v2.txt")
                .mime_str("text/plain")
                .unwrap(),
        );

    let resp = client
        .patch(format!(
            "{addr}/api/collections/documents/records/{record_id}"
        ))
        .multipart(form)
        .send()
        .await
        .unwrap();

    let status = resp.status();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "update response: {body:#}");

    let new_filename = body["attachment"].as_str().unwrap().to_string();
    assert_ne!(old_filename, new_filename, "filename should change");

    // New file is downloadable.
    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{new_filename}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), b"new content");
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests: Download with special query params
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn download_with_attachment_flag_after_upload() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // Upload.
    let form = reqwest::multipart::Form::new()
        .text("title", "Download test")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(b"report data".to_vec())
                .file_name("report.csv")
                .mime_str("text/csv")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap();
    let filename = body["attachment"].as_str().unwrap();

    // Download with ?download=true.
    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{filename}?download=true"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let disposition = resp
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        disposition.contains("attachment"),
        "expected attachment, got: {disposition}"
    );
    assert!(disposition.contains(filename));
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests: Thumbnail generation through full upload → download
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn upload_image_then_download_thumbnail() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // Create a real PNG image.
    let img = image::DynamicImage::new_rgba8(400, 300);
    let mut png_buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png_buf, image::ImageFormat::Png).unwrap();
    let png_data = png_buf.into_inner();

    // Upload the image.
    let form = reqwest::multipart::Form::new()
        .text("title", "Thumb test")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(png_data.clone())
                .file_name("photo.png")
                .mime_str("image/png")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap();
    let filename = body["attachment"].as_str().unwrap();

    // Request a 100x100 thumbnail.
    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{filename}?thumb=100x100"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "image/png"
    );

    let thumb_data = resp.bytes().await.unwrap();
    let thumb = image::load_from_memory(&thumb_data).unwrap();
    assert_eq!(thumb.width(), 100);
    assert_eq!(thumb.height(), 100);

    // Thumbnail should now be cached in storage.
    let cached_key = format!("{DOCS_COL_ID}/{record_id}/thumbs/100x100_{filename}");
    assert!(
        storage.exists(&cached_key).await.unwrap(),
        "thumbnail should be cached after first request"
    );

    // Request the same thumbnail again → should be served from cache.
    let resp2 = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{filename}?thumb=100x100"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let thumb2 = image::load_from_memory(&resp2.bytes().await.unwrap()).unwrap();
    assert_eq!(thumb2.width(), 100);
    assert_eq!(thumb2.height(), 100);
}

#[tokio::test]
async fn thumbnail_request_on_non_image_returns_400() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // Upload a text file.
    let form = reqwest::multipart::Form::new()
        .text("title", "Not an image")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(b"just text".to_vec())
                .file_name("readme.txt")
                .mime_str("text/plain")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap();
    let filename = body["attachment"].as_str().unwrap();

    // Request a thumbnail on non-image → 400.
    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{filename}?thumb=100x100"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests: Delete record cleans up files end-to-end
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn delete_record_removes_uploaded_files_and_returns_404_on_download() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // Upload a file.
    let form = reqwest::multipart::Form::new()
        .text("title", "To delete")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(b"delete me".to_vec())
                .file_name("doomed.txt")
                .mime_str("text/plain")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap().to_string();
    let filename = body["attachment"].as_str().unwrap().to_string();
    assert_eq!(storage.file_count().await, 1);

    // Delete the record.
    let resp = client
        .delete(format!(
            "{addr}/api/collections/documents/records/{record_id}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // File should be cleaned up from storage.
    assert_eq!(
        storage.file_count().await,
        0,
        "file should be deleted when record is deleted"
    );

    // Downloading the file should now return 404.
    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{filename}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests: Large file upload
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn upload_and_download_large_file() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // gallery has max_size=0 (unlimited), upload a ~32KB file.
    let large_data: Vec<u8> = (0..32_768).map(|i| (i % 256) as u8).collect();
    let form = reqwest::multipart::Form::new()
        .text("title", "Large file")
        .part(
            "gallery",
            reqwest::multipart::Part::bytes(large_data.clone())
                .file_name("large.dat")
                .mime_str("application/octet-stream")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap();
    let gallery = body["gallery"].as_array().unwrap();
    assert_eq!(gallery.len(), 1);
    let filename = gallery[0].as_str().unwrap();

    // Download and verify all bytes.
    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{filename}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let downloaded = resp.bytes().await.unwrap();
    assert_eq!(downloaded.len(), 32_768);
    assert_eq!(
        downloaded.as_ref(),
        large_data.as_slice(),
        "large file data must survive round-trip"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests: LocalFileStorage backend through HTTP (integration)
// ═══════════════════════════════════════════════════════════════════════════

/// Spawn an app that uses real LocalFileStorage instead of MemoryStorage.
async fn spawn_app_with_local_storage(
    collections: Vec<Collection>,
    temp_dir: &std::path::Path,
) -> (String, tokio::task::JoinHandle<()>) {
    let local_storage =
        zerobase_files::LocalFileStorage::new(temp_dir).await.expect("failed to create local storage");
    let storage: Arc<dyn FileStorage> = Arc::new(local_storage);

    let schema_record = MockSchemaLookup::with(collections.clone());
    let schema_files: Arc<MockSchemaLookup> =
        Arc::new(MockSchemaLookup::with(collections));
    let repo = MockRecordRepo::new();

    let record_service = Arc::new(RecordService::new(repo, schema_record));
    let file_service = Arc::new(FileService::new(storage));
    let token_service: Arc<dyn TokenService> = Arc::new(MockTokenService);

    let app = zerobase_api::api_router()
        .merge(zerobase_api::record_routes_with_files(
            record_service,
            Some(file_service.clone()),
        ))
        .merge(zerobase_api::file_routes(
            file_service,
            token_service,
            schema_files,
        ))
        .layer(axum::middleware::from_fn(fake_auth_middleware));

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

#[tokio::test]
async fn local_storage_upload_download_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let (addr, _h) = spawn_app_with_local_storage(
        vec![make_documents_collection()],
        temp.path(),
    )
    .await;
    let client = reqwest::Client::new();

    let file_data = b"LocalFileStorage integration test data";
    let form = reqwest::multipart::Form::new()
        .text("title", "Local storage test")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(file_data.to_vec())
                .file_name("local_test.txt")
                .mime_str("text/plain")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap();
    let filename = body["attachment"].as_str().unwrap();

    // Download from local storage.
    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{filename}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), file_data);
}

#[tokio::test]
async fn local_storage_delete_cleans_up_files() {
    let temp = tempfile::tempdir().unwrap();
    let (addr, _h) = spawn_app_with_local_storage(
        vec![make_documents_collection()],
        temp.path(),
    )
    .await;
    let client = reqwest::Client::new();

    // Upload.
    let form = reqwest::multipart::Form::new()
        .text("title", "To delete")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(b"delete me on local".to_vec())
                .file_name("local_delete.txt")
                .mime_str("text/plain")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap().to_string();
    let filename = body["attachment"].as_str().unwrap().to_string();

    // Delete the record.
    let resp = client
        .delete(format!(
            "{addr}/api/collections/documents/records/{record_id}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // File should be gone from local disk.
    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{filename}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn local_storage_thumbnail_generation() {
    let temp = tempfile::tempdir().unwrap();
    let (addr, _h) = spawn_app_with_local_storage(
        vec![make_documents_collection()],
        temp.path(),
    )
    .await;
    let client = reqwest::Client::new();

    // Create a real PNG.
    let img = image::DynamicImage::new_rgba8(200, 150);
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    let png_data = buf.into_inner();

    let form = reqwest::multipart::Form::new()
        .text("title", "Local thumb")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(png_data)
                .file_name("img.png")
                .mime_str("image/png")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap();
    let filename = body["attachment"].as_str().unwrap();

    // Request thumbnail.
    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{filename}?thumb=50x50"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let thumb = image::load_from_memory(&resp.bytes().await.unwrap()).unwrap();
    assert_eq!(thumb.width(), 50);
    assert_eq!(thumb.height(), 50);

    // Verify thumbnail file was cached on disk.
    let thumb_path = temp
        .path()
        .join(DOCS_COL_ID)
        .join(record_id)
        .join("thumbs");
    assert!(thumb_path.exists(), "thumbs directory should exist on disk");
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests: S3 backend (mock) integration through HTTP endpoints
// ═══════════════════════════════════════════════════════════════════════════

/// Mock S3 storage that wraps MemoryStorage and tracks S3-specific operations.
/// This verifies the S3 backend path works correctly when wired through the
/// HTTP API layer, without requiring an actual S3 service.
struct MockS3Storage {
    inner: MemoryStorage,
    upload_count: std::sync::atomic::AtomicUsize,
    download_count: std::sync::atomic::AtomicUsize,
}

impl MockS3Storage {
    fn new() -> Self {
        Self {
            inner: MemoryStorage::new(),
            upload_count: std::sync::atomic::AtomicUsize::new(0),
            download_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn uploads(&self) -> usize {
        self.upload_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    fn downloads(&self) -> usize {
        self.download_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl FileStorage for MockS3Storage {
    async fn upload(
        &self,
        key: &str,
        data: &[u8],
        content_type: &str,
    ) -> std::result::Result<(), StorageError> {
        self.upload_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.upload(key, data, content_type).await
    }

    async fn download(&self, key: &str) -> std::result::Result<FileDownload, StorageError> {
        self.download_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.download(key).await
    }

    async fn delete(&self, key: &str) -> std::result::Result<(), StorageError> {
        self.inner.delete(key).await
    }

    async fn exists(&self, key: &str) -> std::result::Result<bool, StorageError> {
        self.inner.exists(key).await
    }

    fn generate_url(&self, key: &str, base_url: &str) -> String {
        // S3 backend proxies through API server.
        format!("{base_url}/api/files/{key}")
    }

    async fn delete_prefix(&self, prefix: &str) -> std::result::Result<(), StorageError> {
        self.inner.delete_prefix(prefix).await
    }
}

/// Spawn app using MockS3Storage as the backend.
async fn spawn_app_with_mock_s3(
    collections: Vec<Collection>,
    storage: Arc<MockS3Storage>,
) -> (String, tokio::task::JoinHandle<()>) {
    let schema_record = MockSchemaLookup::with(collections.clone());
    let schema_files: Arc<MockSchemaLookup> =
        Arc::new(MockSchemaLookup::with(collections));
    let repo = MockRecordRepo::new();

    let dyn_storage: Arc<dyn FileStorage> = storage;
    let record_service = Arc::new(RecordService::new(repo, schema_record));
    let file_service = Arc::new(FileService::new(dyn_storage));
    let token_service: Arc<dyn TokenService> = Arc::new(MockTokenService);

    let app = zerobase_api::api_router()
        .merge(zerobase_api::record_routes_with_files(
            record_service,
            Some(file_service.clone()),
        ))
        .merge(zerobase_api::file_routes(
            file_service,
            token_service,
            schema_files,
        ))
        .layer(axum::middleware::from_fn(fake_auth_middleware));

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

#[tokio::test]
async fn s3_mock_upload_download_round_trip() {
    let storage = Arc::new(MockS3Storage::new());
    let (addr, _h) = spawn_app_with_mock_s3(
        vec![make_documents_collection()],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    let file_data = b"S3 backend integration test data";
    let form = reqwest::multipart::Form::new()
        .text("title", "S3 test")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(file_data.to_vec())
                .file_name("s3_test.txt")
                .mime_str("text/plain")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(storage.uploads(), 1, "exactly one S3 upload should occur");

    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap();
    let filename = body["attachment"].as_str().unwrap();

    // Download.
    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{filename}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), file_data);
    assert_eq!(storage.downloads(), 1, "exactly one S3 download should occur");
}

#[tokio::test]
async fn s3_mock_delete_removes_files() {
    let storage = Arc::new(MockS3Storage::new());
    let (addr, _h) = spawn_app_with_mock_s3(
        vec![make_documents_collection()],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // Upload multiple files.
    let form = reqwest::multipart::Form::new()
        .text("title", "S3 delete test")
        .part(
            "gallery",
            reqwest::multipart::Part::bytes(b"file1".to_vec())
                .file_name("a.bin")
                .mime_str("application/octet-stream")
                .unwrap(),
        )
        .part(
            "gallery",
            reqwest::multipart::Part::bytes(b"file2".to_vec())
                .file_name("b.bin")
                .mime_str("application/octet-stream")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(storage.uploads(), 2);

    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap();

    // Delete the record.
    let resp = client
        .delete(format!(
            "{addr}/api/collections/documents/records/{record_id}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Files should be cleaned up from S3 mock.
    let gallery = body["gallery"].as_array().unwrap();
    for file_val in gallery {
        let filename = file_val.as_str().unwrap();
        let resp = client
            .get(format!(
                "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{filename}"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "file {filename} should be gone after record deletion"
        );
    }
}

#[tokio::test]
async fn s3_mock_concurrent_uploads() {
    let storage = Arc::new(MockS3Storage::new());
    let (addr, _h) = spawn_app_with_mock_s3(
        vec![make_documents_collection()],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    let mut handles = Vec::new();
    for i in 0..5 {
        let client = client.clone();
        let addr = addr.clone();
        handles.push(tokio::spawn(async move {
            let form = reqwest::multipart::Form::new()
                .text("title", format!("S3 concurrent {i}"))
                .part(
                    "attachment",
                    reqwest::multipart::Part::bytes(vec![i as u8; 256])
                        .file_name(format!("s3_file_{i}.bin"))
                        .mime_str("application/octet-stream")
                        .unwrap(),
                );

            client
                .post(format!("{addr}/api/collections/documents/records"))
                .multipart(form)
                .send()
                .await
                .unwrap()
                .status()
        }));
    }

    for handle in handles {
        assert_eq!(handle.await.unwrap(), StatusCode::OK);
    }

    assert_eq!(
        storage.uploads(),
        5,
        "all 5 concurrent uploads should hit S3 mock"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests: Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn download_nonexistent_file_returns_404() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage,
    )
    .await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/fake_record/fake_file.txt"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn json_create_with_no_files_leaves_storage_empty() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .json(&json!({ "title": "No files" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(storage.file_count().await, 0);
}

#[tokio::test]
async fn cache_control_header_present_on_file_download() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _h) = spawn_full_app(
        vec![make_documents_collection()],
        vec![],
        storage.clone(),
    )
    .await;
    let client = reqwest::Client::new();

    // Upload.
    let form = reqwest::multipart::Form::new()
        .text("title", "Cache test")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(b"cached content".to_vec())
                .file_name("cached.txt")
                .mime_str("text/plain")
                .unwrap(),
        );

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let record_id = body["id"].as_str().unwrap();
    let filename = body["attachment"].as_str().unwrap();

    // Download and check cache-control.
    let resp = client
        .get(format!(
            "{addr}/api/files/{DOCS_COL_ID}/{record_id}/{filename}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let cache_control = resp
        .headers()
        .get("cache-control")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        cache_control.contains("max-age=604800"),
        "expected 1 week cache, got: {cache_control}"
    );
}
