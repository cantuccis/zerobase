# File Storage Abstraction Design

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Provide a trait-based file storage system supporting local filesystem and S3-compatible backends, with file attachment to records via collection schema.

**Architecture:** The `FileStorage` trait is defined in `zerobase-core` (no I/O, following existing patterns). Concrete implementations (`LocalFileStorage`, `S3FileStorage`) and the high-level `FileService` live in `zerobase-files`. The API layer handles multipart uploads and delegates to `FileService`.

**Tech Stack:** `tokio::fs` (local), `s3` crate (S3), `image` crate (thumbnails), `async-trait` (async traits)

---

## 1. Trait Design

### FileStorage trait (`zerobase-core/src/storage.rs`)

```rust
#[async_trait]
pub trait FileStorage: Send + Sync {
    async fn upload(&self, key: &str, data: &[u8], content_type: &str) -> Result<(), StorageError>;
    async fn download(&self, key: &str) -> Result<FileDownload, StorageError>;
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
    async fn exists(&self, key: &str) -> Result<bool, StorageError>;
    fn generate_url(&self, key: &str, base_url: &str) -> String;
    async fn delete_prefix(&self, prefix: &str) -> Result<(), StorageError>;
}
```

### Storage Key Format

All files are addressed by a key: `<collection_id>/<record_id>/<filename>`

- `collection_id`: 15-char alphanumeric ID
- `record_id`: 15-char alphanumeric ID
- `filename`: `<15-char-random>_<sanitized_original_name>`

### Supporting Types

| Type | Purpose |
|------|---------|
| `FileMetadata` | Key, original name, content type, size |
| `FileDownload` | Metadata + raw bytes |
| `FileUpload` | Input from multipart: field name, original name, content type, data |
| `StorageError` | NotFound, Io, Remote, TooLarge, MimeTypeNotAllowed |
| `ThumbSize` | Width, height, mode (Center/Top/Bottom/Fit) |

### Error Mapping

| StorageError | ZerobaseError | HTTP |
|-------------|---------------|------|
| NotFound | NotFound | 404 |
| TooLarge | Validation | 400 |
| MimeTypeNotAllowed | Validation | 400 |
| Io | Internal | 500 |
| Remote | Internal | 500 |

---

## 2. Local Filesystem Implementation

**File:** `zerobase-files/src/local.rs`
**Status:** Implemented

### Storage Layout

```
<root>/
  <collection_id>/
    <record_id>/
      <filename>
      thumbs/
        <spec>_<filename>
```

### Key Behaviors

- `upload`: Create parent dirs via `tokio::fs::create_dir_all`, then `tokio::fs::write`
- `download`: `tokio::fs::read`, infer MIME from extension, extract original name from filename
- `delete`: `tokio::fs::remove_file`, idempotent (ignore NotFound)
- `exists`: `path.exists()`
- `generate_url`: Returns `/api/files/<key>` (served by axum route)
- `delete_prefix`: `tokio::fs::remove_dir_all` on the record directory

### Configuration

Uses `StorageSettings.local_path` as root directory (default: `zerobase_data/storage`).

---

## 3. S3-Compatible Implementation (Planned)

**File:** `zerobase-files/src/s3.rs`
**Status:** Stub with trait implementation returning "not yet implemented"

### Planned Implementation

**Crate:** `rust-s3` (`s3 = "0.35"`) — well-maintained, 4M+ downloads, supports async

### Key Behaviors

- `upload`: `bucket.put_object_with_content_type(key, data, content_type)`
- `download`: `bucket.get_object(key)` — returns bytes + response headers
- `delete`: `bucket.delete_object(key)` — S3 returns 204 for non-existent keys (idempotent)
- `exists`: `bucket.head_object(key)` — 200 = exists, 404 = not
- `generate_url`: Two strategies:
  - **Pre-signed URL**: `bucket.presign_get(key, expiry_secs)` — direct S3 access
  - **Proxy URL**: `/api/files/<key>` — API server proxies the download (needed for protected files)
- `delete_prefix`: `bucket.list(prefix)` → `bucket.delete_object` for each (or batch delete)

### Configuration

```toml
[storage]
backend = "s3"

[storage.s3]
bucket = "my-bucket"
region = "us-east-1"
endpoint = "https://s3.example.com"  # optional, for MinIO/R2
access_key = "AKID..."
secret_key = "SKEY..."
force_path_style = true              # optional, for MinIO
```

### Implementation Tasks

1. Add `s3 = "0.35"` to `zerobase-files/Cargo.toml`
2. Build `S3FileStorage::new(settings: &S3Settings)` constructor
3. Implement all `FileStorage` trait methods
4. Add integration tests using MinIO (docker)

---

## 4. File Attachment to Records

### Schema Definition

File fields are defined in the collection schema using `FieldType::File(FileOptions)`:

```rust
pub struct FileOptions {
    pub max_select: u32,       // Max file count (1 = single file)
    pub max_size: u64,         // Max bytes per file (0 = no limit)
    pub mime_types: Vec<String>, // Allowed MIME types (empty = all)
    pub thumbs: Vec<String>,   // Thumbnail specs (e.g. "100x100", "200x200f")
    pub protected: bool,       // Require file token for download
}
```

### Record Data Format

File fields are stored in the record's JSON data as:

- **Single file** (`max_select = 1`): `"avatar": "abc123def456g_photo.jpg"`
- **Multiple files** (`max_select > 1`): `"documents": ["abc_doc1.pdf", "def_doc2.pdf"]`

The stored values are **filenames only** — the full storage key is reconstructed at runtime using `collection_id/record_id/filename`.

### Upload Flow (Record Create/Update)

```
1. API handler receives multipart/form-data
2. Extract text fields → record data
3. Extract file parts → Vec<FileUpload>
4. Call FileService::process_uploads(collection_id, record_id, uploads, fields)
   a. Validate each file against FileOptions (size, MIME, count)
   b. Generate unique filename: generate_filename(original_name)
   c. Upload to storage: storage.upload(key, data, content_type)
   d. Return Vec<(field_name, filename)>
5. Merge filenames into record data
6. Save record via RecordService
```

### Download Flow

```
1. GET /api/files/:collection_id/:record_id/:filename
2. If field is protected:
   a. Verify file token (short-lived JWT with file scope)
   b. Reject if missing/invalid/expired
3. If thumbnail requested (?thumb=100x100):
   a. Check if thumb exists in storage
   b. If not, generate and cache it
   c. Return thumb data
4. Retrieve file: storage.download(key)
5. Return with proper Content-Type and Content-Disposition headers
```

### Record Update (File Changes)

```
1. Compare old filenames vs new filenames in the file field
2. Removed files → storage.delete(key)
3. New uploads → process_uploads (as above)
4. Update record data with new filename list
```

### Record Delete (Cleanup)

```
1. Delete the record from the database
2. storage.delete_prefix(collection_id/record_id/)
   → Removes all files + thumbnails for this record
```

### Protected Files

Files marked `protected: true` in the field options:
- Require a file token for download
- File tokens are short-lived JWTs (5 min default) generated via the existing `TokenService`
- Token type: `TokenType::File`
- API handler verifies the token before serving the file

---

## 5. Thumbnail Strategy

**Module:** `zerobase-files/src/thumb.rs`

### Supported Formats

Only image types: JPEG, PNG, GIF, WebP

### Thumb Specs

Follow PocketBase format: `WxH[mode]`
- `100x100` → center crop
- `100x100t` → top crop
- `100x100b` → bottom crop
- `100x100f` → fit within bounds

### Storage

Thumbnails stored at: `<collection_id>/<record_id>/thumbs/<spec>_<filename>`

### Generation Strategy

- **Lazy**: Generated on first request, then cached in storage
- **Pre-generation**: Optionally generated at upload time if thumbs are configured in the field options

### Implementation (Future Task)

- Add `image = "0.25"` to dependencies
- Implement resize/crop based on `ThumbMode`
- Cache in storage backend (same key pattern for local and S3)

---

## 6. Module Structure

```
zerobase-core/src/
  storage.rs          ← FileStorage trait, StorageError, FileMetadata,
                        FileDownload, FileUpload, ThumbSize, ThumbMode,
                        file_key(), record_file_prefix(), generate_filename()

zerobase-files/src/
  lib.rs              ← Module declarations, re-exports
  local.rs            ← LocalFileStorage (implemented)
  s3.rs               ← S3FileStorage (stub, planned)
  service.rs          ← FileService (upload validation, record integration)
  thumb.rs            ← Thumbnail helpers (is_thumbable, thumb_key)
```

---

## 7. Integration Points

### With zerobase-api

- File upload endpoint: `POST /api/collections/:collection/records` (multipart)
- File download endpoint: `GET /api/files/:collection_id/:record_id/:filename`
- File token endpoint: `POST /api/files/token` (for protected files)

### With zerobase-server (Composition Root)

```rust
// In server startup:
let file_storage: Arc<dyn FileStorage> = match settings.storage.backend {
    StorageBackend::Local => Arc::new(LocalFileStorage::new(&settings.storage.local_path).await?),
    StorageBackend::S3 => Arc::new(S3FileStorage::new(settings.storage.s3.as_ref().unwrap()).await?),
};
let file_service = FileService::new(file_storage);
```

### With RecordService

- On create: `FileService::process_uploads` → merge filenames → save record
- On update: detect removed files → delete from storage → process new uploads → save
- On delete: `FileService::delete_record_files` → remove record

---

## 8. Test Strategy

| Layer | Type | Description |
|-------|------|-------------|
| `storage.rs` | Unit | Key helpers, filename generation, error conversion |
| `local.rs` | Integration | Upload/download/delete round trip using tempdir |
| `s3.rs` | Integration | Same tests against MinIO (docker) |
| `service.rs` | Unit | Upload validation, file cleanup (using in-memory mock) |
| `thumb.rs` | Unit | MIME checks, key generation |
| API handlers | Integration | Full HTTP multipart upload/download flow |
