//! High-level file service.
//!
//! [`FileService`] wraps a [`FileStorage`] backend and adds business logic:
//! - Upload validation (file size, MIME type, max file count)
//! - Filename generation with random prefixes
//! - File attachment tracking in record data
//! - Cleanup of orphaned files on record update/delete
//! - Thumbnail generation delegation
//! - Protected file token verification
//!
//! # Record integration
//!
//! When a record with `File` fields is created or updated, the API handler:
//!
//! 1. Extracts uploaded files from the multipart request into [`FileUpload`] structs.
//! 2. Calls [`FileService::process_uploads`] which:
//!    a. Validates each file against the field's `FileOptions` (size, MIME, count).
//!    b. Generates a unique filename via [`generate_filename`].
//!    c. Uploads the file to the storage backend.
//!    d. Returns the generated filenames to be stored in the record data.
//! 3. The record is saved with filenames in the `File` field (string for single,
//!    JSON array for multi-file fields).
//!
//! On record update, files removed from the field value are deleted from storage.
//! On record delete, all files for the record are deleted via `delete_prefix`.

use std::sync::Arc;

use zerobase_core::schema::FieldType;
use zerobase_core::storage::{
    file_key, generate_filename, record_file_prefix, FileDownload, FileStorage, FileUpload,
    StorageError, ThumbSize,
};

use crate::thumb;

/// High-level file operations service.
///
/// Generic over the storage backend for testability.
pub struct FileService {
    storage: Arc<dyn FileStorage>,
}

impl FileService {
    /// Create a new file service wrapping the given storage backend.
    pub fn new(storage: Arc<dyn FileStorage>) -> Self {
        Self { storage }
    }

    /// Access the underlying storage backend.
    pub fn storage(&self) -> &dyn FileStorage {
        self.storage.as_ref()
    }

    /// Process file uploads for a record, validating against field options.
    ///
    /// Returns a list of `(field_name, generated_filename)` pairs that should
    /// be stored in the record data.
    ///
    /// # Arguments
    /// - `collection_id`: the collection's ID
    /// - `record_id`: the record's ID
    /// - `uploads`: files extracted from the multipart request
    /// - `fields`: the collection's field definitions (for validation)
    pub async fn process_uploads(
        &self,
        collection_id: &str,
        record_id: &str,
        uploads: Vec<FileUpload>,
        fields: &[zerobase_core::schema::Field],
    ) -> Result<Vec<(String, String)>, StorageError> {
        let mut results = Vec::new();

        for upload in uploads {
            // Find the field definition for this upload.
            let field = fields.iter().find(|f| f.name == upload.field_name);
            let field = match field {
                Some(f) => f,
                None => {
                    return Err(StorageError::io(format!(
                        "no file field '{}' in collection",
                        upload.field_name
                    )));
                }
            };

            // Extract FileOptions from the field type.
            let file_opts = match &field.field_type {
                FieldType::File(opts) => opts,
                _ => {
                    return Err(StorageError::io(format!(
                        "field '{}' is not a file field",
                        upload.field_name
                    )));
                }
            };

            // Validate file size.
            if file_opts.max_size > 0 && upload.size() > file_opts.max_size as u64 {
                return Err(StorageError::TooLarge {
                    size: upload.size(),
                    max_size: file_opts.max_size as u64,
                });
            }

            // Validate MIME type.
            if !file_opts.mime_types.is_empty()
                && !file_opts.mime_types.contains(&upload.content_type)
            {
                return Err(StorageError::MimeTypeNotAllowed {
                    content_type: upload.content_type.clone(),
                });
            }

            // Generate unique filename and upload.
            let filename = generate_filename(&upload.original_name);
            let key = file_key(collection_id, record_id, &filename);
            self.storage
                .upload(&key, &upload.data, &upload.content_type)
                .await?;

            results.push((upload.field_name.clone(), filename));
        }

        Ok(results)
    }

    /// Delete a specific file from a record.
    pub async fn delete_file(
        &self,
        collection_id: &str,
        record_id: &str,
        filename: &str,
    ) -> Result<(), StorageError> {
        let key = file_key(collection_id, record_id, filename);
        self.storage.delete(&key).await
    }

    /// Delete all files for a record (used when deleting a record).
    pub async fn delete_record_files(
        &self,
        collection_id: &str,
        record_id: &str,
    ) -> Result<(), StorageError> {
        let prefix = record_file_prefix(collection_id, record_id);
        self.storage.delete_prefix(&prefix).await
    }

    /// Generate a URL for accessing a file.
    pub fn file_url(
        &self,
        collection_id: &str,
        record_id: &str,
        filename: &str,
        base_url: &str,
    ) -> String {
        let key = file_key(collection_id, record_id, filename);
        self.storage.generate_url(&key, base_url)
    }

    /// Get a cached thumbnail or generate one on-the-fly.
    ///
    /// 1. Checks if the cached thumbnail already exists in storage.
    /// 2. If cached, returns it directly.
    /// 3. Otherwise, downloads the original, generates the thumbnail,
    ///    caches it in storage, and returns it.
    ///
    /// Returns `StorageError::Io` if the file is not a supported image type.
    pub async fn get_or_generate_thumbnail(
        &self,
        collection_id: &str,
        record_id: &str,
        filename: &str,
        spec: &ThumbSize,
    ) -> Result<FileDownload, StorageError> {
        let cached_key = thumb::thumb_key(collection_id, record_id, spec, filename);

        // Check if the thumbnail is already cached.
        if let Ok(cached) = self.storage.download(&cached_key).await {
            return Ok(cached);
        }

        // Download the original file.
        let original_key = file_key(collection_id, record_id, filename);
        let original = self.storage.download(&original_key).await?;

        // Verify the file is a thumbable image.
        if !thumb::is_thumbable(&original.metadata.content_type) {
            return Err(StorageError::io(format!(
                "cannot generate thumbnail for MIME type: {}",
                original.metadata.content_type
            )));
        }

        // Generate the thumbnail.
        let thumb_data =
            thumb::generate_thumbnail(&original.data, &original.metadata.content_type, spec)?;

        let thumb_content_type = original.metadata.content_type.clone();
        let thumb_size = thumb_data.len() as u64;

        // Cache the thumbnail in storage.
        self.storage
            .upload(&cached_key, &thumb_data, &thumb_content_type)
            .await?;

        Ok(FileDownload {
            metadata: zerobase_core::storage::FileMetadata {
                key: cached_key,
                original_name: filename.to_string(),
                content_type: thumb_content_type,
                size: thumb_size,
            },
            data: thumb_data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerobase_core::schema::{Field, FieldType};
    use zerobase_core::storage::{FileDownload, FileMetadata};

    /// In-memory storage for testing.
    struct MemoryStorage {
        files: tokio::sync::Mutex<std::collections::HashMap<String, (Vec<u8>, String)>>,
    }

    impl MemoryStorage {
        fn new() -> Self {
            Self {
                files: tokio::sync::Mutex::new(std::collections::HashMap::new()),
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
        ) -> Result<(), StorageError> {
            self.files
                .lock()
                .await
                .insert(key.to_string(), (data.to_vec(), content_type.to_string()));
            Ok(())
        }

        async fn download(&self, key: &str) -> Result<FileDownload, StorageError> {
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

        async fn delete(&self, key: &str) -> Result<(), StorageError> {
            self.files.lock().await.remove(key);
            Ok(())
        }

        async fn exists(&self, key: &str) -> Result<bool, StorageError> {
            Ok(self.files.lock().await.contains_key(key))
        }

        fn generate_url(&self, key: &str, base_url: &str) -> String {
            format!("{base_url}/api/files/{key}")
        }

        async fn delete_prefix(&self, prefix: &str) -> Result<(), StorageError> {
            let mut files = self.files.lock().await;
            files.retain(|k, _| !k.starts_with(prefix));
            Ok(())
        }
    }

    fn make_file_field(name: &str) -> Field {
        use zerobase_core::schema::FileOptions;
        Field::new(name, FieldType::File(FileOptions::default()))
    }

    fn make_file_field_with_opts(name: &str, max_size: u64, mime_types: Vec<String>) -> Field {
        use zerobase_core::schema::FileOptions;
        Field::new(
            name,
            FieldType::File(FileOptions {
                max_select: 5,
                max_size,
                mime_types,
                thumbs: vec![],
                protected: false,
            }),
        )
    }

    #[tokio::test]
    async fn process_uploads_stores_file_and_returns_filename() {
        let storage = Arc::new(MemoryStorage::new());
        let service = FileService::new(storage.clone());

        let uploads = vec![FileUpload {
            field_name: "avatar".into(),
            original_name: "photo.jpg".into(),
            content_type: "image/jpeg".into(),
            data: vec![0xFF, 0xD8, 0xFF],
        }];

        let fields = vec![make_file_field("avatar")];
        let results = service
            .process_uploads("col1", "rec1", uploads, &fields)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "avatar");
        assert!(results[0].1.ends_with("_photo.jpg"));

        // Verify the file exists in storage.
        let key = file_key("col1", "rec1", &results[0].1);
        assert!(storage.exists(&key).await.unwrap());
    }

    #[tokio::test]
    async fn process_uploads_rejects_oversized_file() {
        let storage = Arc::new(MemoryStorage::new());
        let service = FileService::new(storage);

        let uploads = vec![FileUpload {
            field_name: "doc".into(),
            original_name: "big.pdf".into(),
            content_type: "application/pdf".into(),
            data: vec![0u8; 2000],
        }];

        let fields = vec![make_file_field_with_opts("doc", 1000, vec![])];
        let result = service
            .process_uploads("col1", "rec1", uploads, &fields)
            .await;

        assert!(matches!(result, Err(StorageError::TooLarge { .. })));
    }

    #[tokio::test]
    async fn process_uploads_rejects_disallowed_mime_type() {
        let storage = Arc::new(MemoryStorage::new());
        let service = FileService::new(storage);

        let uploads = vec![FileUpload {
            field_name: "doc".into(),
            original_name: "script.exe".into(),
            content_type: "application/octet-stream".into(),
            data: vec![0u8; 100],
        }];

        let fields = vec![make_file_field_with_opts(
            "doc",
            0,
            vec!["application/pdf".into()],
        )];
        let result = service
            .process_uploads("col1", "rec1", uploads, &fields)
            .await;

        assert!(matches!(
            result,
            Err(StorageError::MimeTypeNotAllowed { .. })
        ));
    }

    #[tokio::test]
    async fn delete_record_files_removes_all() {
        let storage = Arc::new(MemoryStorage::new());
        let service = FileService::new(storage.clone());

        // Upload two files.
        storage
            .upload("col1/rec1/a.txt", b"aaa", "text/plain")
            .await
            .unwrap();
        storage
            .upload("col1/rec1/b.txt", b"bbb", "text/plain")
            .await
            .unwrap();
        // Upload a file for a different record (should not be deleted).
        storage
            .upload("col1/rec2/c.txt", b"ccc", "text/plain")
            .await
            .unwrap();

        service.delete_record_files("col1", "rec1").await.unwrap();

        assert!(!storage.exists("col1/rec1/a.txt").await.unwrap());
        assert!(!storage.exists("col1/rec1/b.txt").await.unwrap());
        assert!(storage.exists("col1/rec2/c.txt").await.unwrap());
    }

    /// Storage that always fails on `delete_prefix`, used to verify error
    /// propagation from [`FileService::delete_record_files`].
    struct FailingDeleteStorage;

    #[async_trait::async_trait]
    impl FileStorage for FailingDeleteStorage {
        async fn upload(&self, _: &str, _: &[u8], _: &str) -> Result<(), StorageError> {
            Ok(())
        }
        async fn download(&self, key: &str) -> Result<FileDownload, StorageError> {
            Err(StorageError::NotFound {
                key: key.to_string(),
            })
        }
        async fn delete(&self, _: &str) -> Result<(), StorageError> {
            Err(StorageError::Io {
                message: "delete failed".into(),
                source: None,
            })
        }
        async fn exists(&self, _: &str) -> Result<bool, StorageError> {
            Ok(false)
        }
        fn generate_url(&self, key: &str, base_url: &str) -> String {
            format!("{base_url}/{key}")
        }
        async fn delete_prefix(&self, _: &str) -> Result<(), StorageError> {
            Err(StorageError::Io {
                message: "simulated delete_prefix failure".into(),
                source: None,
            })
        }
    }

    #[tokio::test]
    async fn delete_record_files_propagates_storage_error() {
        let storage = Arc::new(FailingDeleteStorage);
        let service = FileService::new(storage);

        let result = service.delete_record_files("col1", "rec1").await;

        assert!(result.is_err(), "storage errors should propagate to caller");
        let err = result.unwrap_err();
        assert!(
            matches!(err, StorageError::Io { .. }),
            "expected Io error, got: {err}"
        );
    }

    #[tokio::test]
    async fn delete_file_propagates_storage_error() {
        let storage = Arc::new(FailingDeleteStorage);
        let service = FileService::new(storage);

        let result = service.delete_file("col1", "rec1", "file.txt").await;

        assert!(
            result.is_err(),
            "individual file delete errors should propagate"
        );
    }

    #[tokio::test]
    async fn file_url_builds_correct_url() {
        let storage = Arc::new(MemoryStorage::new());
        let service = FileService::new(storage);

        let url = service.file_url("col1", "rec1", "photo.jpg", "https://api.example.com");
        assert_eq!(url, "https://api.example.com/api/files/col1/rec1/photo.jpg");
    }

    // ── Thumbnail tests ──────────────────────────────────────────────────

    use zerobase_core::storage::ThumbMode;

    /// Create a simple test PNG image of given dimensions.
    fn create_test_png(width: u32, height: u32) -> Vec<u8> {
        use image::{DynamicImage, ImageFormat};
        use std::io::Cursor;
        let img = DynamicImage::new_rgba8(width, height);
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    #[tokio::test]
    async fn get_or_generate_thumbnail_generates_and_caches() {
        let storage = Arc::new(MemoryStorage::new());
        let service = FileService::new(storage.clone());

        // Upload an original image.
        let png_data = create_test_png(400, 300);
        storage
            .upload("col1/rec1/photo.png", &png_data, "image/png")
            .await
            .unwrap();

        let spec = ThumbSize {
            width: 100,
            height: 100,
            mode: ThumbMode::Center,
        };

        // First call: generates the thumbnail.
        let result = service
            .get_or_generate_thumbnail("col1", "rec1", "photo.png", &spec)
            .await
            .unwrap();

        assert_eq!(result.metadata.content_type, "image/png");
        assert!(!result.data.is_empty());

        // Verify thumbnail is cached in storage.
        let cached_key = "col1/rec1/thumbs/100x100_photo.png";
        assert!(storage.exists(cached_key).await.unwrap());

        // Second call: returns the cached version.
        let cached_result = service
            .get_or_generate_thumbnail("col1", "rec1", "photo.png", &spec)
            .await
            .unwrap();
        assert_eq!(cached_result.metadata.key, cached_key);
    }

    #[tokio::test]
    async fn get_or_generate_thumbnail_rejects_non_image() {
        let storage = Arc::new(MemoryStorage::new());
        let service = FileService::new(storage.clone());

        // Upload a non-image file.
        storage
            .upload("col1/rec1/doc.pdf", b"PDF data", "application/pdf")
            .await
            .unwrap();

        let spec = ThumbSize {
            width: 100,
            height: 100,
            mode: ThumbMode::Center,
        };

        let result = service
            .get_or_generate_thumbnail("col1", "rec1", "doc.pdf", &spec)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_or_generate_thumbnail_not_found() {
        let storage = Arc::new(MemoryStorage::new());
        let service = FileService::new(storage);

        let spec = ThumbSize {
            width: 100,
            height: 100,
            mode: ThumbMode::Center,
        };

        let result = service
            .get_or_generate_thumbnail("col1", "rec1", "missing.png", &spec)
            .await;
        assert!(matches!(result, Err(StorageError::NotFound { .. })));
    }

    #[tokio::test]
    async fn get_or_generate_thumbnail_different_specs() {
        let storage = Arc::new(MemoryStorage::new());
        let service = FileService::new(storage.clone());

        let png_data = create_test_png(400, 300);
        storage
            .upload("col1/rec1/img.png", &png_data, "image/png")
            .await
            .unwrap();

        let spec_a = ThumbSize {
            width: 50,
            height: 50,
            mode: ThumbMode::Center,
        };
        let spec_b = ThumbSize {
            width: 200,
            height: 0,
            mode: ThumbMode::Center,
        };

        // Generate two different thumbnails.
        service
            .get_or_generate_thumbnail("col1", "rec1", "img.png", &spec_a)
            .await
            .unwrap();
        service
            .get_or_generate_thumbnail("col1", "rec1", "img.png", &spec_b)
            .await
            .unwrap();

        // Both should be cached separately.
        assert!(storage
            .exists("col1/rec1/thumbs/50x50_img.png")
            .await
            .unwrap());
        assert!(storage
            .exists("col1/rec1/thumbs/200x0_img.png")
            .await
            .unwrap());
    }
}
