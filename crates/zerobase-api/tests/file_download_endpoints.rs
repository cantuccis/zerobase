//! Integration tests for file download and serving endpoints.
//!
//! Tests exercise the full HTTP stack for:
//! - `GET /api/files/token` — generating short-lived file access tokens
//! - `GET /api/files/:collectionId/:recordId/:filename` — serving files
//!
//! Covers: public files, protected files, MIME types, missing files,
//! Content-Disposition, and token validation.

use std::collections::HashMap;
use std::sync::Arc;

use reqwest::StatusCode;
use serde_json::Value;
use tokio::net::TcpListener;

use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

use zerobase_core::auth::{TokenClaims, TokenService, TokenType, ValidatedToken};
use zerobase_core::error::Result;
use zerobase_core::schema::{Collection, Field, FieldType, FileOptions, TextOptions};
use zerobase_core::services::record_service::SchemaLookup;
use zerobase_core::storage::{FileDownload, FileMetadata, FileStorage, StorageError};
use zerobase_core::ZerobaseError;
use zerobase_files::FileService;

use image::{DynamicImage, ImageFormat};

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
        // Encode token info so we can verify it in tests.
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
        // Accept tokens starting with "valid-file-token".
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

// ── Test collections ────────────────────────────────────────────────────────

const PUBLIC_COL_ID: &str = "col_public_123";
const PROTECTED_COL_ID: &str = "col_protect_12";

fn make_public_collection() -> Collection {
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
            "document",
            FieldType::File(FileOptions {
                max_select: 1,
                max_size: 0,
                mime_types: vec![],
                thumbs: vec![],
                protected: false,
            }),
        ),
    ];
    let mut col = Collection::base("public_docs", fields);
    col.id = PUBLIC_COL_ID.to_string();
    col
}

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
            "secret_file",
            FieldType::File(FileOptions {
                max_select: 1,
                max_size: 0,
                mime_types: vec![],
                thumbs: vec![],
                protected: true,
            }),
        ),
    ];
    let mut col = Collection::base("private_docs", fields);
    col.id = PROTECTED_COL_ID.to_string();
    col
}

// ── Spawn test app ──────────────────────────────────────────────────────────

async fn spawn_app(
    storage: Arc<MemoryStorage>,
    collections: Vec<Collection>,
) -> (String, tokio::task::JoinHandle<()>) {
    let schema: Arc<MockSchemaLookup> = Arc::new(MockSchemaLookup::with(collections));
    let token_service: Arc<dyn TokenService> = Arc::new(MockTokenService);
    let file_service = Arc::new(FileService::new(storage as Arc<dyn FileStorage>));

    let app = zerobase_api::api_router()
        .merge(zerobase_api::file_routes(
            file_service,
            token_service,
            schema,
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

// ── Tests: GET /api/files/token ─────────────────────────────────────────────

#[tokio::test]
async fn request_file_token_requires_auth() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    // No Authorization header → 401
    let resp = client
        .get(format!("{addr}/api/files/token"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn request_file_token_returns_token_for_authenticated_user() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/files/token"))
        .header("Authorization", "Bearer user123")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let token = body["token"].as_str().unwrap();

    // Our mock encodes: file-tok:<user_id>:<collection_id>:<type>:<key>:<dur>
    assert!(token.starts_with("file-tok:user123:users:file:user_key:120"));
}

#[tokio::test]
async fn request_file_token_works_for_superuser() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/files/token"))
        .header("Authorization", "Bearer SUPERUSER")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let token = body["token"].as_str().unwrap();
    assert!(token.starts_with("file-tok:admin1:_superusers:file:admin_key:120"));
}

// ── Tests: GET /api/files/:collectionId/:recordId/:filename ─────────────────

#[tokio::test]
async fn serve_public_file_without_token() {
    let storage = Arc::new(MemoryStorage::new());

    // Pre-populate storage with a file.
    let file_data = b"Hello, World!";
    storage
        .upload(
            &format!("{PUBLIC_COL_ID}/rec1/abc_hello.txt"),
            file_data,
            "text/plain",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/files/{PUBLIC_COL_ID}/rec1/abc_hello.txt"
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
    assert_eq!(
        resp.headers()
            .get("content-length")
            .unwrap()
            .to_str()
            .unwrap(),
        "13"
    );

    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), file_data);
}

#[tokio::test]
async fn serve_file_with_correct_mime_type() {
    let storage = Arc::new(MemoryStorage::new());

    // Upload a JPEG.
    let jpeg_data = vec![0xFF, 0xD8, 0xFF, 0xE0]; // JPEG magic bytes
    storage
        .upload(
            &format!("{PUBLIC_COL_ID}/rec2/photo.jpg"),
            &jpeg_data,
            "image/jpeg",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/files/{PUBLIC_COL_ID}/rec2/photo.jpg"))
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
        "image/jpeg"
    );
}

#[tokio::test]
async fn serve_file_returns_404_for_missing_file() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/files/{PUBLIC_COL_ID}/rec1/nonexistent.txt"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn serve_protected_file_requires_token() {
    let storage = Arc::new(MemoryStorage::new());

    // Upload a protected file.
    storage
        .upload(
            &format!("{PROTECTED_COL_ID}/rec1/secret.pdf"),
            b"secret data",
            "application/pdf",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(
        storage,
        vec![make_public_collection(), make_protected_collection()],
    )
    .await;
    let client = reqwest::Client::new();

    // No token → 401
    let resp = client
        .get(format!(
            "{addr}/api/files/{PROTECTED_COL_ID}/rec1/secret.pdf"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn serve_protected_file_with_invalid_token() {
    let storage = Arc::new(MemoryStorage::new());

    storage
        .upload(
            &format!("{PROTECTED_COL_ID}/rec1/secret.pdf"),
            b"secret data",
            "application/pdf",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(
        storage,
        vec![make_public_collection(), make_protected_collection()],
    )
    .await;
    let client = reqwest::Client::new();

    // Invalid token → 401
    let resp = client
        .get(format!(
            "{addr}/api/files/{PROTECTED_COL_ID}/rec1/secret.pdf?token=bad-token"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn serve_protected_file_with_valid_token() {
    let storage = Arc::new(MemoryStorage::new());

    let file_data = b"secret data";
    storage
        .upload(
            &format!("{PROTECTED_COL_ID}/rec1/secret.pdf"),
            file_data,
            "application/pdf",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(
        storage,
        vec![make_public_collection(), make_protected_collection()],
    )
    .await;
    let client = reqwest::Client::new();

    // Valid token → 200
    let resp = client
        .get(format!(
            "{addr}/api/files/{PROTECTED_COL_ID}/rec1/secret.pdf?token=valid-file-token"
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
        "application/pdf"
    );
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), file_data);
}

#[tokio::test]
async fn serve_file_with_download_flag() {
    let storage = Arc::new(MemoryStorage::new());

    storage
        .upload(
            &format!("{PUBLIC_COL_ID}/rec1/report.pdf"),
            b"pdf data",
            "application/pdf",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    // ?download=true → Content-Disposition: attachment
    let resp = client
        .get(format!(
            "{addr}/api/files/{PUBLIC_COL_ID}/rec1/report.pdf?download=true"
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
        .unwrap();
    assert!(
        disposition.contains("attachment"),
        "expected attachment disposition, got: {disposition}"
    );
    assert!(disposition.contains("report.pdf"));
}

#[tokio::test]
async fn serve_file_inline_by_default() {
    let storage = Arc::new(MemoryStorage::new());

    storage
        .upload(
            &format!("{PUBLIC_COL_ID}/rec1/image.png"),
            b"png data",
            "image/png",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/files/{PUBLIC_COL_ID}/rec1/image.png"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let disposition = resp
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        disposition.contains("inline"),
        "expected inline disposition, got: {disposition}"
    );
}

#[tokio::test]
async fn serve_file_includes_cache_control() {
    let storage = Arc::new(MemoryStorage::new());

    storage
        .upload(
            &format!("{PUBLIC_COL_ID}/rec1/style.css"),
            b"body {}",
            "text/css",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/files/{PUBLIC_COL_ID}/rec1/style.css"))
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
    assert!(cache_control.contains("max-age=604800"));
}

#[tokio::test]
async fn serve_file_from_unknown_collection_treated_as_public() {
    let storage = Arc::new(MemoryStorage::new());

    // File exists in storage under a collection ID that has no schema.
    storage
        .upload(
            "unknown_col/rec1/data.bin",
            b"binary",
            "application/octet-stream",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    // Should serve the file (treated as public since collection not found).
    let resp = client
        .get(format!("{addr}/api/files/unknown_col/rec1/data.bin"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), b"binary");
}

#[tokio::test]
async fn serve_public_file_no_auth_required() {
    let storage = Arc::new(MemoryStorage::new());

    storage
        .upload(
            &format!("{PUBLIC_COL_ID}/rec1/readme.txt"),
            b"public readme",
            "text/plain",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    // No Authorization header at all — public files don't need auth.
    let resp = client
        .get(format!("{addr}/api/files/{PUBLIC_COL_ID}/rec1/readme.txt"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), b"public readme");
}

#[tokio::test]
async fn serve_file_empty_content_type_falls_back_to_octet_stream() {
    let storage = Arc::new(MemoryStorage::new());

    // Upload with empty content type.
    storage
        .upload(&format!("{PUBLIC_COL_ID}/rec1/data.bin"), b"raw bytes", "")
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/api/files/{PUBLIC_COL_ID}/rec1/data.bin"))
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
        "application/octet-stream"
    );
}

// ── Tests: Thumbnail generation via ?thumb= ──────────────────────────────

/// Create a simple test PNG image of given dimensions.
fn create_test_png(width: u32, height: u32) -> Vec<u8> {
    let img = DynamicImage::new_rgba8(width, height);
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png).unwrap();
    buf.into_inner()
}

#[tokio::test]
async fn serve_thumbnail_generates_and_returns_resized_image() {
    let storage = Arc::new(MemoryStorage::new());

    // Upload a 400x300 PNG image.
    let png_data = create_test_png(400, 300);
    storage
        .upload(
            &format!("{PUBLIC_COL_ID}/rec1/photo.png"),
            &png_data,
            "image/png",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/files/{PUBLIC_COL_ID}/rec1/photo.png?thumb=100x100"
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

    let body = resp.bytes().await.unwrap();
    let thumb = image::load_from_memory(&body).unwrap();
    assert_eq!(thumb.width(), 100);
    assert_eq!(thumb.height(), 100);
}

#[tokio::test]
async fn serve_thumbnail_auto_height() {
    let storage = Arc::new(MemoryStorage::new());

    let png_data = create_test_png(400, 200);
    storage
        .upload(
            &format!("{PUBLIC_COL_ID}/rec1/wide.png"),
            &png_data,
            "image/png",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    // ?thumb=200x0 should resize to width 200, auto height (100).
    let resp = client
        .get(format!(
            "{addr}/api/files/{PUBLIC_COL_ID}/rec1/wide.png?thumb=200x0"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.bytes().await.unwrap();
    let thumb = image::load_from_memory(&body).unwrap();
    assert_eq!(thumb.width(), 200);
    assert_eq!(thumb.height(), 100);
}

#[tokio::test]
async fn serve_thumbnail_fit_mode() {
    let storage = Arc::new(MemoryStorage::new());

    let png_data = create_test_png(400, 200);
    storage
        .upload(
            &format!("{PUBLIC_COL_ID}/rec1/wide.png"),
            &png_data,
            "image/png",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    // ?thumb=100x100f should fit within 100x100 → 100x50.
    let resp = client
        .get(format!(
            "{addr}/api/files/{PUBLIC_COL_ID}/rec1/wide.png?thumb=100x100f"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.bytes().await.unwrap();
    let thumb = image::load_from_memory(&body).unwrap();
    assert_eq!(thumb.width(), 100);
    assert_eq!(thumb.height(), 50);
}

#[tokio::test]
async fn serve_thumbnail_caches_result() {
    let storage = Arc::new(MemoryStorage::new());

    let png_data = create_test_png(400, 300);
    storage
        .upload(
            &format!("{PUBLIC_COL_ID}/rec1/cached.png"),
            &png_data,
            "image/png",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage.clone(), vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    // First request generates the thumbnail.
    let resp1 = client
        .get(format!(
            "{addr}/api/files/{PUBLIC_COL_ID}/rec1/cached.png?thumb=50x50"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    // Verify the thumbnail is now cached in storage.
    let cached_key = format!("{PUBLIC_COL_ID}/rec1/thumbs/50x50_cached.png");
    assert!(
        storage.exists(&cached_key).await.unwrap(),
        "thumbnail should be cached in storage"
    );

    // Second request should also succeed (from cache).
    let resp2 = client
        .get(format!(
            "{addr}/api/files/{PUBLIC_COL_ID}/rec1/cached.png?thumb=50x50"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);

    let body = resp2.bytes().await.unwrap();
    let thumb = image::load_from_memory(&body).unwrap();
    assert_eq!(thumb.width(), 50);
    assert_eq!(thumb.height(), 50);
}

#[tokio::test]
async fn serve_thumbnail_returns_400_for_non_image() {
    let storage = Arc::new(MemoryStorage::new());

    // Upload a non-image file.
    storage
        .upload(
            &format!("{PUBLIC_COL_ID}/rec1/document.pdf"),
            b"PDF content",
            "application/pdf",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/files/{PUBLIC_COL_ID}/rec1/document.pdf?thumb=100x100"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn serve_thumbnail_returns_400_for_invalid_spec() {
    let storage = Arc::new(MemoryStorage::new());

    let png_data = create_test_png(100, 100);
    storage
        .upload(
            &format!("{PUBLIC_COL_ID}/rec1/img.png"),
            &png_data,
            "image/png",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    // Invalid thumb spec.
    let resp = client
        .get(format!(
            "{addr}/api/files/{PUBLIC_COL_ID}/rec1/img.png?thumb=invalid"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn serve_thumbnail_returns_400_for_zero_dimensions() {
    let storage = Arc::new(MemoryStorage::new());

    let png_data = create_test_png(100, 100);
    storage
        .upload(
            &format!("{PUBLIC_COL_ID}/rec1/img.png"),
            &png_data,
            "image/png",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    // 0x0 is invalid.
    let resp = client
        .get(format!(
            "{addr}/api/files/{PUBLIC_COL_ID}/rec1/img.png?thumb=0x0"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn serve_thumbnail_returns_404_for_missing_original() {
    let storage = Arc::new(MemoryStorage::new());

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{addr}/api/files/{PUBLIC_COL_ID}/rec1/missing.png?thumb=100x100"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn serve_file_without_thumb_param_returns_original() {
    let storage = Arc::new(MemoryStorage::new());

    let png_data = create_test_png(400, 300);
    storage
        .upload(
            &format!("{PUBLIC_COL_ID}/rec1/original.png"),
            &png_data,
            "image/png",
        )
        .await
        .unwrap();

    let (addr, _handle) = spawn_app(storage, vec![make_public_collection()]).await;
    let client = reqwest::Client::new();

    // No thumb param → original file.
    let resp = client
        .get(format!(
            "{addr}/api/files/{PUBLIC_COL_ID}/rec1/original.png"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.bytes().await.unwrap();
    let img = image::load_from_memory(&body).unwrap();
    assert_eq!(img.width(), 400);
    assert_eq!(img.height(), 300);
}
