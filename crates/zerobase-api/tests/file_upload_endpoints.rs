//! Integration tests for file upload support in record create/update/delete.
//!
//! Tests exercise the full HTTP stack with multipart form data, verifying that
//! files are uploaded via [`FileService`], filenames are stored in records, and
//! files are cleaned up on update/delete.

use std::collections::HashMap;
use std::sync::Arc;

use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

use zerobase_core::error::Result;
use zerobase_core::schema::{ApiRules, Collection, Field, FieldType, FileOptions, TextOptions};
use zerobase_core::services::record_service::{
    RecordList, RecordQuery, RecordRepoError, RecordRepository, RecordService, SchemaLookup,
};
use zerobase_core::storage::{FileDownload, FileMetadata, FileStorage, StorageError};
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
                AuthInfo::superuser()
            } else {
                let mut record = HashMap::new();
                record.insert("id".to_string(), Value::String(token));
                AuthInfo::authenticated(record)
            }
        })
        .unwrap_or_else(AuthInfo::anonymous);

    request.extensions_mut().insert(auth_info);
    next.run(request).await
}

// ── In-memory mock: SchemaLookup ────────────────────────────────────────────

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
    fn get_collection(&self, name: &str) -> Result<Collection> {
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
    records: std::sync::Mutex<HashMap<String, Vec<HashMap<String, Value>>>>,
}

impl MockRecordRepo {
    fn new() -> Self {
        Self {
            records: std::sync::Mutex::new(HashMap::new()),
        }
    }

    fn with_records(collection: &str, records: Vec<HashMap<String, Value>>) -> Self {
        let mut map = HashMap::new();
        map.insert(collection.to_string(), records);
        Self {
            records: std::sync::Mutex::new(map),
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

// ── Test collection with file fields ────────────────────────────────────────

/// Fixed collection ID for deterministic file key construction in tests.
const DOCUMENTS_COLLECTION_ID: &str = "test_docs_col";

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
                max_size: 10_000,
                mime_types: vec![],
                thumbs: vec![],
                protected: false,
            }),
        ),
        Field::new(
            "gallery",
            FieldType::File(FileOptions {
                max_select: 5,
                max_size: 0,
                mime_types: vec![],
                thumbs: vec![],
                protected: false,
            }),
        ),
    ];
    let mut col = Collection::base("documents", fields);
    col.id = DOCUMENTS_COLLECTION_ID.to_string();
    col.rules = ApiRules::open();
    col
}

// ── Spawn test app with file service ────────────────────────────────────────

async fn spawn_file_app(
    records: Vec<HashMap<String, Value>>,
    storage: Arc<MemoryStorage>,
) -> (String, tokio::task::JoinHandle<()>) {
    let schema = MockSchemaLookup::with(vec![make_documents_collection()]);
    let repo = if records.is_empty() {
        MockRecordRepo::new()
    } else {
        MockRecordRepo::with_records("documents", records)
    };

    let record_service = Arc::new(RecordService::new(repo, schema));
    let file_service = Arc::new(FileService::new(storage));

    let app = zerobase_api::api_router()
        .merge(zerobase_api::record_routes_with_files(
            record_service,
            Some(file_service),
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

// ── Tests ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_record_with_json_still_works() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _handle) = spawn_file_app(vec![], storage.clone()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/api/collections/documents/records"))
        .json(&json!({ "title": "My Doc" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "My Doc");
    assert_eq!(storage.file_count().await, 0);
}

#[tokio::test]
async fn create_record_with_multipart_file_upload() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _handle) = spawn_file_app(vec![], storage.clone()).await;
    let client = reqwest::Client::new();

    let form = reqwest::multipart::Form::new()
        .text("title", "Doc with file")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(vec![0xDE, 0xAD, 0xBE, 0xEF])
                .file_name("test.bin")
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
    assert_eq!(body["title"], "Doc with file");

    // The attachment field should have a generated filename ending with _test.bin.
    let attachment = body["attachment"].as_str().unwrap();
    assert!(
        attachment.ends_with("_test.bin"),
        "expected filename ending with _test.bin, got: {attachment}"
    );

    // File should exist in storage.
    assert_eq!(storage.file_count().await, 1);
}

#[tokio::test]
async fn create_record_with_multipart_validates_file_size() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _handle) = spawn_file_app(vec![], storage.clone()).await;
    let client = reqwest::Client::new();

    // attachment field has max_size = 10_000, send a file larger than that.
    let big_data = vec![0u8; 20_000];
    let form = reqwest::multipart::Form::new()
        .text("title", "Big file")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(big_data)
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

    // Should fail with 400 (file too large).
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(storage.file_count().await, 0);
}

#[tokio::test]
async fn create_record_with_multiple_files_for_gallery_field() {
    let storage = Arc::new(MemoryStorage::new());
    let (addr, _handle) = spawn_file_app(vec![], storage.clone()).await;
    let client = reqwest::Client::new();

    let form = reqwest::multipart::Form::new()
        .text("title", "Gallery doc")
        .part(
            "gallery",
            reqwest::multipart::Part::bytes(vec![1, 2, 3])
                .file_name("img1.png")
                .mime_str("image/png")
                .unwrap(),
        )
        .part(
            "gallery",
            reqwest::multipart::Part::bytes(vec![4, 5, 6])
                .file_name("img2.png")
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

    // Gallery should be an array with 2 filenames.
    let gallery = body["gallery"].as_array().unwrap();
    assert_eq!(gallery.len(), 2);
    assert!(gallery[0].as_str().unwrap().ends_with("_img1.png"));
    assert!(gallery[1].as_str().unwrap().ends_with("_img2.png"));

    // 2 files in storage.
    assert_eq!(storage.file_count().await, 2);
}

#[tokio::test]
async fn delete_record_cleans_up_files() {
    let storage = Arc::new(MemoryStorage::new());

    // Seed a record with a file.
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!("rec1_test_id_ab"));
    record.insert("title".to_string(), json!("To delete"));
    record.insert("attachment".to_string(), json!("abc123_test.bin"));
    record.insert("created".to_string(), json!("2025-01-01T00:00:00Z"));
    record.insert("updated".to_string(), json!("2025-01-01T00:00:00Z"));

    // Pre-populate storage with the file.
    let file_key = format!(
        "{}/rec1_test_id_ab/abc123_test.bin",
        DOCUMENTS_COLLECTION_ID
    );
    storage
        .upload(&file_key, b"file data", "application/octet-stream")
        .await
        .unwrap();
    assert_eq!(storage.file_count().await, 1);

    let (addr, _handle) = spawn_file_app(vec![record], storage.clone()).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!(
            "{addr}/api/collections/documents/records/rec1_test_id_ab"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // File should have been cleaned up.
    assert_eq!(storage.file_count().await, 0);
}

#[tokio::test]
async fn update_record_with_new_file_upload() {
    let storage = Arc::new(MemoryStorage::new());

    // Seed a record without files.
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!("rec1_test_id_ab"));
    record.insert("title".to_string(), json!("Original"));
    record.insert("attachment".to_string(), json!(""));
    record.insert("created".to_string(), json!("2025-01-01T00:00:00Z"));
    record.insert("updated".to_string(), json!("2025-01-01T00:00:00Z"));

    let (addr, _handle) = spawn_file_app(vec![record], storage.clone()).await;
    let client = reqwest::Client::new();

    let form = reqwest::multipart::Form::new()
        .text("title", "Updated")
        .part(
            "attachment",
            reqwest::multipart::Part::bytes(vec![0xCA, 0xFE])
                .file_name("new_file.pdf")
                .mime_str("application/pdf")
                .unwrap(),
        );

    let resp = client
        .patch(format!(
            "{addr}/api/collections/documents/records/rec1_test_id_ab"
        ))
        .multipart(form)
        .send()
        .await
        .unwrap();

    let status = resp.status();
    let body: Value = resp.json().await.unwrap();
    if status != StatusCode::OK {
        panic!("expected 200, got {status}: {body:#}");
    }
    assert_eq!(body["title"], "Updated");

    let attachment = body["attachment"].as_str().unwrap();
    assert!(
        attachment.ends_with("_new_file.pdf"),
        "expected filename ending with _new_file.pdf, got: {attachment}"
    );

    assert_eq!(storage.file_count().await, 1);
}

// ── Failing storage for graceful-error tests ──────────────────────────────

/// Storage that fails on deletion operations but succeeds on uploads.
/// Used to verify that file cleanup errors don't block record deletion.
struct FailingDeleteStorage {
    inner: MemoryStorage,
}

impl FailingDeleteStorage {
    fn new() -> Self {
        Self {
            inner: MemoryStorage::new(),
        }
    }
}

#[async_trait::async_trait]
impl FileStorage for FailingDeleteStorage {
    async fn upload(
        &self,
        key: &str,
        data: &[u8],
        content_type: &str,
    ) -> std::result::Result<(), StorageError> {
        self.inner.upload(key, data, content_type).await
    }

    async fn download(&self, key: &str) -> std::result::Result<FileDownload, StorageError> {
        self.inner.download(key).await
    }

    async fn delete(&self, _key: &str) -> std::result::Result<(), StorageError> {
        Err(StorageError::Io {
            message: "simulated storage failure on delete".into(),
            source: None,
        })
    }

    async fn exists(&self, key: &str) -> std::result::Result<bool, StorageError> {
        self.inner.exists(key).await
    }

    fn generate_url(&self, key: &str, base_url: &str) -> String {
        self.inner.generate_url(key, base_url)
    }

    async fn delete_prefix(&self, _prefix: &str) -> std::result::Result<(), StorageError> {
        Err(StorageError::Io {
            message: "simulated storage failure on delete_prefix".into(),
            source: None,
        })
    }
}

/// Spawn the test app with an arbitrary `FileStorage` implementation.
async fn spawn_file_app_dyn(
    records: Vec<HashMap<String, Value>>,
    storage: Arc<dyn FileStorage>,
) -> (String, tokio::task::JoinHandle<()>) {
    let schema = MockSchemaLookup::with(vec![make_documents_collection()]);
    let repo = if records.is_empty() {
        MockRecordRepo::new()
    } else {
        MockRecordRepo::with_records("documents", records)
    };

    let record_service = Arc::new(RecordService::new(repo, schema));
    let file_service = Arc::new(FileService::new(storage));

    let app = zerobase_api::api_router()
        .merge(zerobase_api::record_routes_with_files(
            record_service,
            Some(file_service),
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

// ── File cleanup on record deletion tests ─────────────────────────────────

#[tokio::test]
async fn delete_record_succeeds_even_when_file_cleanup_fails() {
    let storage = Arc::new(FailingDeleteStorage::new());

    // Seed a record with a file.
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!("rec_fail_del_ab"));
    record.insert("title".to_string(), json!("Has file"));
    record.insert("attachment".to_string(), json!("abc123_test.bin"));
    record.insert("created".to_string(), json!("2025-01-01T00:00:00Z"));
    record.insert("updated".to_string(), json!("2025-01-01T00:00:00Z"));

    // Pre-populate storage with the file.
    let file_key = format!(
        "{}/rec_fail_del_ab/abc123_test.bin",
        DOCUMENTS_COLLECTION_ID
    );
    storage
        .upload(&file_key, b"file data", "application/octet-stream")
        .await
        .unwrap();

    let (addr, _handle) =
        spawn_file_app_dyn(vec![record], storage.clone() as Arc<dyn FileStorage>).await;
    let client = reqwest::Client::new();

    // The DELETE should still return 204 even though file cleanup fails.
    let resp = client
        .delete(format!(
            "{addr}/api/collections/documents/records/rec_fail_del_ab"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "record deletion must succeed even when storage cleanup fails"
    );

    // The file still exists because delete_prefix failed, but that's OK.
    assert!(
        storage.exists(&file_key).await.unwrap(),
        "file should still exist since cleanup failed"
    );
}

#[tokio::test]
async fn delete_record_cleans_up_multiple_file_fields() {
    let storage = Arc::new(MemoryStorage::new());

    // Seed a record with files in two different file fields.
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!("rec_multi_file_ab"));
    record.insert("title".to_string(), json!("Multi files"));
    record.insert("attachment".to_string(), json!("abc123_single.pdf"));
    record.insert(
        "gallery".to_string(),
        json!(["def456_img1.png", "ghi789_img2.png"]),
    );
    record.insert("created".to_string(), json!("2025-01-01T00:00:00Z"));
    record.insert("updated".to_string(), json!("2025-01-01T00:00:00Z"));

    // Pre-populate storage with all three files.
    let prefix = format!("{}/rec_multi_file_ab", DOCUMENTS_COLLECTION_ID);
    storage
        .upload(
            &format!("{prefix}/abc123_single.pdf"),
            b"pdf data",
            "application/pdf",
        )
        .await
        .unwrap();
    storage
        .upload(
            &format!("{prefix}/def456_img1.png"),
            b"png data 1",
            "image/png",
        )
        .await
        .unwrap();
    storage
        .upload(
            &format!("{prefix}/ghi789_img2.png"),
            b"png data 2",
            "image/png",
        )
        .await
        .unwrap();
    assert_eq!(storage.file_count().await, 3);

    let (addr, _handle) = spawn_file_app(vec![record], storage.clone()).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!(
            "{addr}/api/collections/documents/records/rec_multi_file_ab"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // All three files should be cleaned up via delete_prefix.
    assert_eq!(
        storage.file_count().await,
        0,
        "all files from all file fields should be removed"
    );
}

#[tokio::test]
async fn delete_record_without_files_succeeds() {
    let storage = Arc::new(MemoryStorage::new());

    // Seed a record with no files attached.
    let mut record = HashMap::new();
    record.insert("id".to_string(), json!("rec_no_files_ab"));
    record.insert("title".to_string(), json!("No files"));
    record.insert("attachment".to_string(), json!(""));
    record.insert("gallery".to_string(), json!([]));
    record.insert("created".to_string(), json!("2025-01-01T00:00:00Z"));
    record.insert("updated".to_string(), json!("2025-01-01T00:00:00Z"));

    // Add a file for a different record to prove it's not deleted.
    let other_key = format!("{}/other_record_ab/keep.txt", DOCUMENTS_COLLECTION_ID);
    storage
        .upload(&other_key, b"keep me", "text/plain")
        .await
        .unwrap();
    assert_eq!(storage.file_count().await, 1);

    let (addr, _handle) = spawn_file_app(vec![record], storage.clone()).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!(
            "{addr}/api/collections/documents/records/rec_no_files_ab"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // The other record's file should still be there.
    assert_eq!(
        storage.file_count().await,
        1,
        "other record's files must not be affected"
    );
    assert!(storage.exists(&other_key).await.unwrap());
}
