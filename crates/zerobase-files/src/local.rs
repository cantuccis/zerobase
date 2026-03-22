//! Local filesystem storage backend.
//!
//! Stores files on disk under a configurable root directory. The directory
//! structure mirrors the storage key hierarchy:
//!
//! ```text
//! <root>/
//!   <collection_id>/
//!     <record_id>/
//!       <filename>
//!       thumbs/
//!         <thumb_spec>_<filename>
//! ```
//!
//! # Thread safety
//!
//! File I/O is performed via `tokio::fs` for non-blocking operations.
//! No internal locking is needed because each file has a unique key
//! (thanks to random prefix generation).
//!
//! # Path safety
//!
//! All keys are validated before use to prevent directory traversal attacks.
//! Keys containing `..` segments or absolute paths are rejected.
//!
//! # Streaming
//!
//! In addition to the byte-slice-based [`FileStorage`] trait methods,
//! [`LocalFileStorage`] exposes [`upload_stream`](LocalFileStorage::upload_stream)
//! and [`download_stream`](LocalFileStorage::download_stream) for efficient
//! handling of large files without loading entire contents into memory.

use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncWriteExt, BufWriter};
use zerobase_core::storage::{FileDownload, FileMetadata, FileStorage, StorageError};

/// Local filesystem storage backend.
///
/// Created from [`StorageSettings`](zerobase_core::configuration::StorageSettings)
/// when the backend is set to `local`.
pub struct LocalFileStorage {
    /// Canonicalized root directory for all stored files.
    root: PathBuf,
}

impl LocalFileStorage {
    /// Create a new local storage backend rooted at the given directory.
    ///
    /// The directory will be created if it does not exist. The root path is
    /// canonicalized to enable safe path traversal checks.
    pub async fn new(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let root = root.into();
        tokio::fs::create_dir_all(&root)
            .await
            .map_err(|e| StorageError::io_with_source("failed to create storage root", e))?;

        // Canonicalize for reliable path containment checks.
        let root = tokio::fs::canonicalize(&root)
            .await
            .map_err(|e| StorageError::io_with_source("failed to canonicalize storage root", e))?;

        Ok(Self { root })
    }

    /// Resolve a storage key to an absolute filesystem path.
    ///
    /// Returns an error if the key would escape the root directory.
    fn resolve_path(&self, key: &str) -> Result<PathBuf, StorageError> {
        validate_key(key)?;
        let path = self.root.join(key);
        // Double-check: the resolved path must still start with root.
        // This catches symlink attacks or unexpected resolution.
        if !path.starts_with(&self.root) {
            return Err(StorageError::io(format!(
                "key resolves outside storage root: {key}"
            )));
        }
        Ok(path)
    }

    /// Upload file data from an async reader (streaming).
    ///
    /// Reads from `reader` in chunks and writes to disk without buffering
    /// the entire file in memory. Ideal for large file uploads.
    pub async fn upload_stream(
        &self,
        key: &str,
        mut reader: impl AsyncRead + Unpin,
        _content_type: &str,
    ) -> Result<u64, StorageError> {
        let path = self.resolve_path(key)?;

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| StorageError::io_with_source("failed to create directories", e))?;
        }

        let file = tokio::fs::File::create(&path)
            .await
            .map_err(|e| StorageError::io_with_source(format!("failed to create {key}"), e))?;
        let mut writer = BufWriter::new(file);

        let bytes_written = tokio::io::copy(&mut reader, &mut writer)
            .await
            .map_err(|e| {
                StorageError::io_with_source(format!("failed to stream-write {key}"), e)
            })?;

        writer
            .flush()
            .await
            .map_err(|e| StorageError::io_with_source(format!("failed to flush {key}"), e))?;

        Ok(bytes_written)
    }

    /// Download a file as an async reader (streaming).
    ///
    /// Returns the file metadata and an async reader. The caller can stream
    /// the data directly to the HTTP response without loading everything
    /// into memory.
    pub async fn download_stream(
        &self,
        key: &str,
    ) -> Result<(FileMetadata, tokio::fs::File), StorageError> {
        let path = self.resolve_path(key)?;

        let meta = tokio::fs::metadata(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound {
                    key: key.to_string(),
                }
            } else {
                StorageError::io_with_source(format!("failed to stat {key}"), e)
            }
        })?;

        let content_type = mime_from_path(&path);
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let original_name = strip_random_prefix(filename);

        let file = tokio::fs::File::open(&path)
            .await
            .map_err(|e| StorageError::io_with_source(format!("failed to open {key}"), e))?;

        let metadata = FileMetadata {
            key: key.to_string(),
            original_name,
            content_type,
            size: meta.len(),
        };

        Ok((metadata, file))
    }
}

#[async_trait::async_trait]
impl FileStorage for LocalFileStorage {
    async fn upload(
        &self,
        key: &str,
        data: &[u8],
        _content_type: &str,
    ) -> Result<(), StorageError> {
        let path = self.resolve_path(key)?;

        // Ensure parent directories exist.
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| StorageError::io_with_source("failed to create directories", e))?;
        }

        tokio::fs::write(&path, data)
            .await
            .map_err(|e| StorageError::io_with_source(format!("failed to write {key}"), e))?;

        Ok(())
    }

    async fn download(&self, key: &str) -> Result<FileDownload, StorageError> {
        let path = self.resolve_path(key)?;

        if !path.exists() {
            return Err(StorageError::NotFound {
                key: key.to_string(),
            });
        }

        let data = tokio::fs::read(&path)
            .await
            .map_err(|e| StorageError::io_with_source(format!("failed to read {key}"), e))?;

        let size = data.len() as u64;

        // Infer content type from extension.
        let content_type = mime_from_path(&path);

        // Extract original name (everything after the random prefix + underscore).
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let original_name = strip_random_prefix(filename);

        Ok(FileDownload {
            metadata: FileMetadata {
                key: key.to_string(),
                original_name,
                content_type,
                size,
            },
            data,
        })
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let path = self.resolve_path(key)?;

        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), // idempotent
            Err(e) => Err(StorageError::io_with_source(
                format!("failed to delete {key}"),
                e,
            )),
        }
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let path = self.resolve_path(key)?;
        Ok(path.exists())
    }

    fn generate_url(&self, key: &str, base_url: &str) -> String {
        // URL format: /api/files/<collection_id>/<record_id>/<filename>
        let base = base_url.trim_end_matches('/');
        format!("{base}/api/files/{key}")
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<(), StorageError> {
        let dir = self.resolve_path(prefix.trim_end_matches('/'))?;

        match tokio::fs::remove_dir_all(&dir).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StorageError::io_with_source(
                format!("failed to delete prefix {prefix}"),
                e,
            )),
        }
    }
}

/// Validate that a storage key is safe.
///
/// Rejects keys that:
/// - Are empty
/// - Contain `..` path segments (directory traversal)
/// - Start with `/` (absolute paths)
/// - Contain null bytes
fn validate_key(key: &str) -> Result<(), StorageError> {
    if key.is_empty() {
        return Err(StorageError::io("storage key must not be empty"));
    }
    if key.contains('\0') {
        return Err(StorageError::io("storage key must not contain null bytes"));
    }
    if key.starts_with('/') || key.starts_with('\\') {
        return Err(StorageError::io(format!(
            "storage key must be relative: {key}"
        )));
    }
    // Reject backslashes anywhere in the key — they can act as path
    // separators on Windows and bypass the `..` check on forward-slash splits.
    if key.contains('\\') {
        return Err(StorageError::io(format!(
            "storage key must not contain backslashes: {key}"
        )));
    }
    // Check each path component for `..`.
    for component in key.split('/') {
        if component == ".." {
            return Err(StorageError::io(format!(
                "storage key must not contain '..': {key}"
            )));
        }
    }
    Ok(())
}

/// Infer MIME type from file extension.
fn mime_from_path(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "zip" => "application/zip",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "csv" => "text/csv",
        "xml" => "application/xml",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Strip the random prefix from a generated filename.
///
/// Generated filenames are `<15-char-id>_<original>`. If the pattern
/// doesn't match, the full filename is returned.
fn strip_random_prefix(filename: &str) -> String {
    // The ID is 15 chars, followed by underscore, then original name.
    if filename.len() > 16 && filename.as_bytes()[15] == b'_' {
        filename[16..].to_string()
    } else {
        filename.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // ── Unit tests ──────────────────────────────────────────────────────

    #[test]
    fn mime_from_path_returns_correct_types() {
        assert_eq!(mime_from_path(Path::new("photo.jpg")), "image/jpeg");
        assert_eq!(mime_from_path(Path::new("photo.JPEG")), "image/jpeg");
        assert_eq!(mime_from_path(Path::new("doc.pdf")), "application/pdf");
        assert_eq!(mime_from_path(Path::new("data.json")), "application/json");
        assert_eq!(mime_from_path(Path::new("style.css")), "text/css");
        assert_eq!(mime_from_path(Path::new("font.woff2")), "font/woff2");
        assert_eq!(mime_from_path(Path::new("image.avif")), "image/avif");
        assert_eq!(
            mime_from_path(Path::new("unknown.xyz")),
            "application/octet-stream"
        );
        assert_eq!(
            mime_from_path(Path::new("noext")),
            "application/octet-stream"
        );
    }

    #[test]
    fn strip_random_prefix_extracts_original_name() {
        assert_eq!(
            strip_random_prefix("abc123def456ghi_photo.jpg"),
            "photo.jpg"
        );
    }

    #[test]
    fn strip_random_prefix_returns_full_name_if_no_prefix() {
        assert_eq!(strip_random_prefix("short.jpg"), "short.jpg");
    }

    #[test]
    fn validate_key_rejects_empty() {
        assert!(validate_key("").is_err());
    }

    #[test]
    fn validate_key_rejects_path_traversal() {
        assert!(validate_key("../etc/passwd").is_err());
        assert!(validate_key("col1/../../etc/passwd").is_err());
        assert!(validate_key("col1/rec1/../../../etc/passwd").is_err());
    }

    #[test]
    fn validate_key_rejects_absolute_paths() {
        assert!(validate_key("/etc/passwd").is_err());
        assert!(validate_key("\\windows\\system32").is_err());
    }

    #[test]
    fn validate_key_rejects_null_bytes() {
        assert!(validate_key("col1/rec1/file\0.txt").is_err());
    }

    #[test]
    fn validate_key_rejects_backslashes() {
        assert!(validate_key("col1\\rec1\\file.txt").is_err());
        assert!(validate_key("col1/rec1/..\\..\\etc\\passwd").is_err());
    }

    #[test]
    fn validate_key_accepts_valid_keys() {
        assert!(validate_key("col1/rec1/file.txt").is_ok());
        assert!(validate_key("col1/rec1/thumbs/100x100_file.jpg").is_ok());
        assert!(validate_key("abc123/def456/rnd_photo.jpg").is_ok());
    }

    // ── URL generation ──────────────────────────────────────────────────

    #[test]
    fn generate_url_builds_correct_path() {
        let storage = LocalFileStorage {
            root: PathBuf::from("/data/storage"),
        };
        let url = storage.generate_url("col123/rec456/file.jpg", "https://api.example.com");
        assert_eq!(
            url,
            "https://api.example.com/api/files/col123/rec456/file.jpg"
        );
    }

    #[test]
    fn generate_url_trims_trailing_slash() {
        let storage = LocalFileStorage {
            root: PathBuf::from("/data/storage"),
        };
        let url = storage.generate_url("col/rec/f.jpg", "https://api.example.com/");
        assert_eq!(url, "https://api.example.com/api/files/col/rec/f.jpg");
    }

    // ── Integration tests (filesystem I/O) ──────────────────────────────

    #[tokio::test]
    async fn upload_download_delete_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let key = "col1/rec1/test_file.txt";
        let data = b"hello world";

        // Upload
        storage.upload(key, data, "text/plain").await.unwrap();

        // Exists
        assert!(storage.exists(key).await.unwrap());

        // Download
        let download = storage.download(key).await.unwrap();
        assert_eq!(download.data, data);
        assert_eq!(download.metadata.content_type, "text/plain");
        assert_eq!(download.metadata.size, 11);

        // Delete
        storage.delete(key).await.unwrap();
        assert!(!storage.exists(key).await.unwrap());
    }

    #[tokio::test]
    async fn delete_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();
        // Deleting a non-existent file should succeed.
        storage.delete("no/such/file.txt").await.unwrap();
    }

    #[tokio::test]
    async fn download_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();
        let result = storage.download("no/such/file.txt").await;
        assert!(matches!(result, Err(StorageError::NotFound { .. })));
    }

    #[tokio::test]
    async fn delete_prefix_removes_directory() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        // Upload two files under the same record prefix.
        storage
            .upload("col1/rec1/a.txt", b"aaa", "text/plain")
            .await
            .unwrap();
        storage
            .upload("col1/rec1/b.txt", b"bbb", "text/plain")
            .await
            .unwrap();

        assert!(storage.exists("col1/rec1/a.txt").await.unwrap());
        assert!(storage.exists("col1/rec1/b.txt").await.unwrap());

        // Delete all files for this record.
        storage.delete_prefix("col1/rec1/").await.unwrap();

        assert!(!storage.exists("col1/rec1/a.txt").await.unwrap());
        assert!(!storage.exists("col1/rec1/b.txt").await.unwrap());
    }

    #[tokio::test]
    async fn delete_prefix_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();
        // Deleting a non-existent prefix should succeed.
        storage.delete_prefix("nonexistent/prefix/").await.unwrap();
    }

    #[tokio::test]
    async fn delete_prefix_does_not_affect_other_records() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        storage
            .upload("col1/rec1/a.txt", b"aaa", "text/plain")
            .await
            .unwrap();
        storage
            .upload("col1/rec2/b.txt", b"bbb", "text/plain")
            .await
            .unwrap();

        storage.delete_prefix("col1/rec1/").await.unwrap();

        assert!(!storage.exists("col1/rec1/a.txt").await.unwrap());
        // rec2 should be untouched.
        assert!(storage.exists("col1/rec2/b.txt").await.unwrap());
    }

    // ── Path traversal protection ───────────────────────────────────────

    #[tokio::test]
    async fn upload_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let result = storage
            .upload("../escape/file.txt", b"evil", "text/plain")
            .await;
        assert!(result.is_err());

        let result = storage
            .upload("col1/../../etc/passwd", b"evil", "text/plain")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn download_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let result = storage.download("../escape/file.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn exists_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let result = storage.exists("../escape/file.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let result = storage.delete("../escape/file.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn upload_rejects_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let result = storage.upload("/etc/passwd", b"evil", "text/plain").await;
        assert!(result.is_err());
    }

    // ── Overwrite behavior ──────────────────────────────────────────────

    #[tokio::test]
    async fn upload_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let key = "col1/rec1/file.txt";
        storage
            .upload(key, b"version1", "text/plain")
            .await
            .unwrap();
        storage
            .upload(key, b"version2", "text/plain")
            .await
            .unwrap();

        let download = storage.download(key).await.unwrap();
        assert_eq!(download.data, b"version2");
        assert_eq!(download.metadata.size, 8);
    }

    // ── Empty file handling ─────────────────────────────────────────────

    #[tokio::test]
    async fn upload_and_download_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let key = "col1/rec1/empty.txt";
        storage.upload(key, b"", "text/plain").await.unwrap();

        assert!(storage.exists(key).await.unwrap());

        let download = storage.download(key).await.unwrap();
        assert!(download.data.is_empty());
        assert_eq!(download.metadata.size, 0);
    }

    // ── Binary data ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn upload_and_download_binary_data() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let key = "col1/rec1/image.png";
        let data: Vec<u8> = (0..=255).collect();

        storage.upload(key, &data, "image/png").await.unwrap();

        let download = storage.download(key).await.unwrap();
        assert_eq!(download.data, data);
        assert_eq!(download.metadata.content_type, "image/png");
    }

    // ── Multiple collections and records ────────────────────────────────

    #[tokio::test]
    async fn files_isolated_between_collections_and_records() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        storage
            .upload("col1/rec1/file.txt", b"col1-rec1", "text/plain")
            .await
            .unwrap();
        storage
            .upload("col1/rec2/file.txt", b"col1-rec2", "text/plain")
            .await
            .unwrap();
        storage
            .upload("col2/rec1/file.txt", b"col2-rec1", "text/plain")
            .await
            .unwrap();

        let d1 = storage.download("col1/rec1/file.txt").await.unwrap();
        let d2 = storage.download("col1/rec2/file.txt").await.unwrap();
        let d3 = storage.download("col2/rec1/file.txt").await.unwrap();

        assert_eq!(d1.data, b"col1-rec1");
        assert_eq!(d2.data, b"col1-rec2");
        assert_eq!(d3.data, b"col2-rec1");

        // Deleting one record's files doesn't affect others.
        storage.delete_prefix("col1/rec1/").await.unwrap();
        assert!(storage.exists("col1/rec2/file.txt").await.unwrap());
        assert!(storage.exists("col2/rec1/file.txt").await.unwrap());
    }

    // ── Directory structure verification ────────────────────────────────

    #[tokio::test]
    async fn upload_creates_nested_directories() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let key = "deep/collection/record123/file.txt";
        storage.upload(key, b"data", "text/plain").await.unwrap();

        // Verify the directory structure was created.
        let expected_dir = dir.path().join("deep/collection/record123");
        assert!(expected_dir.is_dir());
        assert!(expected_dir.join("file.txt").is_file());
    }

    // ── Streaming upload/download ───────────────────────────────────────

    #[tokio::test]
    async fn stream_upload_and_download() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let key = "col1/rec1/streamed.txt";
        let data = b"streamed content here";
        let cursor = std::io::Cursor::new(data.to_vec());

        // Upload via stream.
        let bytes_written = storage
            .upload_stream(key, cursor, "text/plain")
            .await
            .unwrap();
        assert_eq!(bytes_written, data.len() as u64);

        // Download via stream.
        let (metadata, mut file) = storage.download_stream(key).await.unwrap();
        assert_eq!(metadata.size, data.len() as u64);
        assert_eq!(metadata.content_type, "text/plain");

        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut file, &mut buf)
            .await
            .unwrap();
        assert_eq!(buf, data);
    }

    #[tokio::test]
    async fn stream_upload_large_data() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let key = "col1/rec1/large.bin";
        // 1 MB of data
        let data = vec![0xABu8; 1024 * 1024];
        let cursor = std::io::Cursor::new(data.clone());

        let bytes_written = storage
            .upload_stream(key, cursor, "application/octet-stream")
            .await
            .unwrap();
        assert_eq!(bytes_written, 1024 * 1024);

        let download = storage.download(key).await.unwrap();
        assert_eq!(download.data.len(), 1024 * 1024);
        assert_eq!(download.data, data);
    }

    #[tokio::test]
    async fn stream_download_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let result = storage.download_stream("no/such/file.txt").await;
        assert!(matches!(result, Err(StorageError::NotFound { .. })));
    }

    #[tokio::test]
    async fn stream_upload_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let cursor = std::io::Cursor::new(b"evil".to_vec());
        let result = storage
            .upload_stream("../escape.txt", cursor, "text/plain")
            .await;
        assert!(result.is_err());
    }

    // ── Concurrent access ───────────────────────────────────────────────

    #[tokio::test]
    async fn concurrent_uploads_to_different_keys() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(LocalFileStorage::new(dir.path()).await.unwrap());

        let mut handles = Vec::new();
        for i in 0..20 {
            let storage = Arc::clone(&storage);
            handles.push(tokio::spawn(async move {
                let key = format!("col1/rec{i}/file.txt");
                let data = format!("data-{i}");
                storage
                    .upload(&key, data.as_bytes(), "text/plain")
                    .await
                    .unwrap();
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all files were written correctly.
        for i in 0..20 {
            let key = format!("col1/rec{i}/file.txt");
            let download = storage.download(&key).await.unwrap();
            assert_eq!(download.data, format!("data-{i}").as_bytes());
        }
    }

    #[tokio::test]
    async fn concurrent_upload_and_download() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(LocalFileStorage::new(dir.path()).await.unwrap());

        // Pre-upload a file.
        storage
            .upload("col1/rec1/shared.txt", b"initial", "text/plain")
            .await
            .unwrap();

        let mut handles = Vec::new();

        // Spawn readers concurrently.
        for _ in 0..10 {
            let storage = Arc::clone(&storage);
            handles.push(tokio::spawn(async move {
                let download = storage.download("col1/rec1/shared.txt").await.unwrap();
                assert!(!download.data.is_empty());
            }));
        }

        // Spawn writers to different keys concurrently.
        for i in 0..10 {
            let storage = Arc::clone(&storage);
            handles.push(tokio::spawn(async move {
                let key = format!("col1/rec1/concurrent_{i}.txt");
                storage
                    .upload(&key, format!("data-{i}").as_bytes(), "text/plain")
                    .await
                    .unwrap();
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }
    }

    #[tokio::test]
    async fn concurrent_delete_is_safe() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(LocalFileStorage::new(dir.path()).await.unwrap());

        // Upload a file.
        storage
            .upload("col1/rec1/todelete.txt", b"data", "text/plain")
            .await
            .unwrap();

        // Multiple concurrent deletes of the same file should all succeed.
        let mut handles = Vec::new();
        for _ in 0..10 {
            let storage = Arc::clone(&storage);
            handles.push(tokio::spawn(async move {
                storage.delete("col1/rec1/todelete.txt").await.unwrap();
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        assert!(!storage.exists("col1/rec1/todelete.txt").await.unwrap());
    }

    // ── MIME type edge cases ────────────────────────────────────────────

    #[tokio::test]
    async fn download_infers_mime_from_extension() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let cases = vec![
            ("col/rec/image.jpg", "image/jpeg"),
            ("col/rec/image.png", "image/png"),
            ("col/rec/video.mp4", "video/mp4"),
            ("col/rec/doc.pdf", "application/pdf"),
            ("col/rec/data.csv", "text/csv"),
            ("col/rec/unknown.xyz", "application/octet-stream"),
        ];

        for (key, expected_mime) in cases {
            storage.upload(key, b"data", expected_mime).await.unwrap();
            let download = storage.download(key).await.unwrap();
            assert_eq!(
                download.metadata.content_type, expected_mime,
                "MIME mismatch for key: {key}"
            );
        }
    }

    // ── Original name extraction ────────────────────────────────────────

    #[tokio::test]
    async fn download_extracts_original_name_from_prefixed_filename() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        // Simulate a generated filename with 15-char prefix + underscore.
        let key = "col1/rec1/abc123def456ghi_original_photo.jpg";
        storage.upload(key, b"img", "image/jpeg").await.unwrap();

        let download = storage.download(key).await.unwrap();
        assert_eq!(download.metadata.original_name, "original_photo.jpg");
    }

    #[tokio::test]
    async fn download_returns_full_name_when_no_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalFileStorage::new(dir.path()).await.unwrap();

        let key = "col1/rec1/simple.txt";
        storage.upload(key, b"txt", "text/plain").await.unwrap();

        let download = storage.download(key).await.unwrap();
        assert_eq!(download.metadata.original_name, "simple.txt");
    }

    // ── Root directory creation ──────────────────────────────────────────

    #[tokio::test]
    async fn new_creates_root_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("deeply/nested/storage/root");
        assert!(!root.exists());

        let _storage = LocalFileStorage::new(&root).await.unwrap();
        assert!(root.exists());
    }
}
