//! File storage abstraction.
//!
//! [`FileStorage`] is the trait that all storage backends implement.
//! It lives in `zerobase-core` so that services can depend on it without
//! pulling in concrete implementations (local FS, S3, etc.).
//!
//! # Storage key format
//!
//! Files are addressed by a **storage key** that encodes their location
//! within the system:
//!
//! ```text
//! <collection_id>/<record_id>/<filename>
//! ```
//!
//! This mirrors PocketBase's layout and keeps files organized per-record.
//! The filename itself is generated at upload time as `<random>_<original>`
//! to avoid collisions.
//!
//! # Protected files
//!
//! File fields can be marked `protected: true` in the collection schema.
//! Protected files require a short-lived file token for download. The
//! `generate_url` method accepts an optional token to produce either a
//! direct URL (public files) or a token-bearing URL (protected files).

use std::fmt;

/// Metadata about a stored file.
///
/// Returned alongside data on download, and used to populate HTTP response
/// headers (Content-Type, Content-Length, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    /// Storage key (e.g. `"collections/abc123/records/def456/photo.jpg"`).
    pub key: String,
    /// Original filename as uploaded by the user.
    pub original_name: String,
    /// MIME type (e.g. `"image/jpeg"`).
    pub content_type: String,
    /// Size in bytes.
    pub size: u64,
}

/// Result of a download operation: metadata + raw bytes.
#[derive(Debug)]
pub struct FileDownload {
    pub metadata: FileMetadata,
    pub data: Vec<u8>,
}

/// Options for controlling thumbnail generation.
///
/// Thumbnail specs follow PocketBase's format: `WxH`, `WxHt` (top),
/// `WxHb` (bottom), `WxHf` (fit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbSize {
    pub width: u32,
    pub height: u32,
    pub mode: ThumbMode,
}

/// How a thumbnail should be cropped/resized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbMode {
    /// Center crop (default, no suffix).
    Center,
    /// Top crop (`t` suffix).
    Top,
    /// Bottom crop (`b` suffix).
    Bottom,
    /// Fit within bounds (`f` suffix).
    Fit,
}

impl fmt::Display for ThumbSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)?;
        match self.mode {
            ThumbMode::Center => Ok(()),
            ThumbMode::Top => write!(f, "t"),
            ThumbMode::Bottom => write!(f, "b"),
            ThumbMode::Fit => write!(f, "f"),
        }
    }
}

/// Errors that can occur during file storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// The requested file was not found.
    #[error("file not found: {key}")]
    NotFound { key: String },

    /// An I/O error occurred (local filesystem).
    #[error("I/O error: {message}")]
    Io {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// A remote storage error occurred (S3, etc.).
    #[error("remote storage error: {message}")]
    Remote {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// The file exceeds the configured maximum size.
    #[error("file too large: {size} bytes (max {max_size})")]
    TooLarge { size: u64, max_size: u64 },

    /// The file's MIME type is not allowed.
    #[error("MIME type not allowed: {content_type}")]
    MimeTypeNotAllowed { content_type: String },
}

impl StorageError {
    pub fn io(message: impl Into<String>) -> Self {
        Self::Io {
            message: message.into(),
            source: None,
        }
    }

    pub fn io_with_source(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Io {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    pub fn remote(message: impl Into<String>) -> Self {
        Self::Remote {
            message: message.into(),
            source: None,
        }
    }

    pub fn remote_with_source(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Remote {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }
}

/// Convert [`StorageError`] into the unified [`ZerobaseError`].
impl From<StorageError> for crate::error::ZerobaseError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::NotFound { key } => crate::error::ZerobaseError::NotFound {
                resource_type: "file".to_string(),
                resource_id: Some(key),
            },
            StorageError::TooLarge { size, max_size } => crate::error::ZerobaseError::validation(
                format!("file too large: {size} bytes exceeds maximum of {max_size} bytes"),
            ),
            StorageError::MimeTypeNotAllowed { content_type } => {
                crate::error::ZerobaseError::validation(format!(
                    "file type not allowed: {content_type}"
                ))
            }
            StorageError::Io { message, source } => crate::error::ZerobaseError::Internal {
                message: format!("file storage I/O error: {message}"),
                source,
            },
            StorageError::Remote { message, source } => crate::error::ZerobaseError::Internal {
                message: format!("remote storage error: {message}"),
                source,
            },
        }
    }
}

// ── Core trait ──────────────────────────────────────────────────────────────

/// Async file storage backend.
///
/// Implementations handle the actual storage of file bytes — on the local
/// filesystem, in S3-compatible object storage, or any other backend.
///
/// # Key format
///
/// All keys follow the pattern `<collection_id>/<record_id>/<filename>`.
/// Implementations must preserve this hierarchy (as directories on local FS,
/// as object key prefixes in S3).
///
/// # Thread safety
///
/// Implementations must be `Send + Sync` so they can be shared across
/// axum handlers via `Arc<dyn FileStorage>`.
#[async_trait::async_trait]
pub trait FileStorage: Send + Sync {
    /// Upload a file to storage.
    ///
    /// - `key`: storage path (e.g. `"col_id/rec_id/abc123_photo.jpg"`)
    /// - `data`: raw file bytes
    /// - `content_type`: MIME type (e.g. `"image/jpeg"`)
    ///
    /// Overwrites any existing file at the same key.
    async fn upload(&self, key: &str, data: &[u8], content_type: &str) -> Result<(), StorageError>;

    /// Download a file from storage.
    ///
    /// Returns the file data and metadata. Returns `StorageError::NotFound`
    /// if the key does not exist.
    async fn download(&self, key: &str) -> Result<FileDownload, StorageError>;

    /// Delete a file from storage.
    ///
    /// Returns `Ok(())` even if the file did not exist (idempotent delete).
    async fn delete(&self, key: &str) -> Result<(), StorageError>;

    /// Check whether a file exists at the given key.
    async fn exists(&self, key: &str) -> Result<bool, StorageError>;

    /// Generate a URL for accessing the file.
    ///
    /// - `key`: storage path
    /// - `base_url`: the server's base URL (e.g. `"https://api.example.com"`)
    ///
    /// For local storage this returns a path-based URL served by the API.
    /// For S3 this may return a pre-signed URL or a proxied path, depending
    /// on configuration.
    fn generate_url(&self, key: &str, base_url: &str) -> String;

    /// Delete all files for a given record (all files under `<collection_id>/<record_id>/`).
    ///
    /// Used when a record is deleted to clean up all its attached files.
    async fn delete_prefix(&self, prefix: &str) -> Result<(), StorageError>;
}

// ── Storage key helpers ─────────────────────────────────────────────────────

/// Build a storage key for a file attached to a record.
///
/// # Arguments
/// - `collection_id`: the collection's ID
/// - `record_id`: the record's ID
/// - `filename`: the stored filename (typically `<random>_<original>`)
///
/// # Returns
/// A key like `"abc123def45/xyz789abc12/rnd_photo.jpg"`.
pub fn file_key(collection_id: &str, record_id: &str, filename: &str) -> String {
    format!("{collection_id}/{record_id}/{filename}")
}

/// Build the prefix for all files belonging to a record.
///
/// Used by [`FileStorage::delete_prefix`] when removing all files for a record.
pub fn record_file_prefix(collection_id: &str, record_id: &str) -> String {
    format!("{collection_id}/{record_id}/")
}

/// Generate a unique filename for storage.
///
/// Produces `<10-char-random>_<sanitized_original>` to prevent collisions
/// while preserving the original name for human readability.
pub fn generate_filename(original_name: &str) -> String {
    let random_prefix = crate::id::generate_id();
    // Sanitize: keep only alphanumeric, dots, hyphens, underscores
    let sanitized: String = original_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let sanitized = if sanitized.is_empty() {
        "file".to_string()
    } else {
        sanitized
    };
    format!("{random_prefix}_{sanitized}")
}

// ── File attachment helpers for records ──────────────────────────────────────

/// Describes a file being uploaded as part of a record create/update.
///
/// This is the input type used when the API layer receives a multipart
/// upload and needs to pass file data to the service layer.
#[derive(Debug)]
pub struct FileUpload {
    /// The field name in the collection schema (e.g. `"avatar"`, `"documents"`).
    pub field_name: String,
    /// Original filename from the upload (e.g. `"photo.jpg"`).
    pub original_name: String,
    /// MIME type detected from the upload.
    pub content_type: String,
    /// Raw file bytes.
    pub data: Vec<u8>,
}

impl FileUpload {
    /// Size of the file in bytes.
    pub fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── file_key ────────────────────────────────────────────────────────

    #[test]
    fn file_key_builds_correct_path() {
        let key = file_key("col123", "rec456", "abc_photo.jpg");
        assert_eq!(key, "col123/rec456/abc_photo.jpg");
    }

    #[test]
    fn record_file_prefix_ends_with_slash() {
        let prefix = record_file_prefix("col123", "rec456");
        assert_eq!(prefix, "col123/rec456/");
    }

    // ── generate_filename ───────────────────────────────────────────────

    #[test]
    fn generate_filename_preserves_extension() {
        let name = generate_filename("photo.jpg");
        assert!(name.ends_with("_photo.jpg"));
    }

    #[test]
    fn generate_filename_sanitizes_special_chars() {
        let name = generate_filename("my file (1).jpg");
        // Spaces and parens should become underscores
        assert!(name.contains("my_file__1_.jpg"));
    }

    #[test]
    fn generate_filename_handles_empty_name() {
        let name = generate_filename("");
        assert!(name.ends_with("_file"));
    }

    #[test]
    fn generate_filename_produces_unique_names() {
        let name1 = generate_filename("test.txt");
        let name2 = generate_filename("test.txt");
        assert_ne!(name1, name2, "names should differ due to random prefix");
    }

    // ── ThumbSize ───────────────────────────────────────────────────────

    #[test]
    fn thumb_size_display_center() {
        let t = ThumbSize {
            width: 100,
            height: 200,
            mode: ThumbMode::Center,
        };
        assert_eq!(t.to_string(), "100x200");
    }

    #[test]
    fn thumb_size_display_top() {
        let t = ThumbSize {
            width: 100,
            height: 200,
            mode: ThumbMode::Top,
        };
        assert_eq!(t.to_string(), "100x200t");
    }

    #[test]
    fn thumb_size_display_bottom() {
        let t = ThumbSize {
            width: 100,
            height: 200,
            mode: ThumbMode::Bottom,
        };
        assert_eq!(t.to_string(), "100x200b");
    }

    #[test]
    fn thumb_size_display_fit() {
        let t = ThumbSize {
            width: 100,
            height: 200,
            mode: ThumbMode::Fit,
        };
        assert_eq!(t.to_string(), "100x200f");
    }

    // ── StorageError → ZerobaseError ────────────────────────────────────

    #[test]
    fn storage_not_found_converts_to_404() {
        let err = StorageError::NotFound {
            key: "col/rec/file.jpg".into(),
        };
        let zb_err: crate::error::ZerobaseError = err.into();
        assert_eq!(zb_err.status_code(), 404);
    }

    #[test]
    fn storage_too_large_converts_to_400() {
        let err = StorageError::TooLarge {
            size: 20_000_000,
            max_size: 10_000_000,
        };
        let zb_err: crate::error::ZerobaseError = err.into();
        assert_eq!(zb_err.status_code(), 400);
    }

    #[test]
    fn storage_mime_not_allowed_converts_to_400() {
        let err = StorageError::MimeTypeNotAllowed {
            content_type: "application/exe".into(),
        };
        let zb_err: crate::error::ZerobaseError = err.into();
        assert_eq!(zb_err.status_code(), 400);
    }

    #[test]
    fn storage_io_error_converts_to_500() {
        let err = StorageError::io("disk full");
        let zb_err: crate::error::ZerobaseError = err.into();
        assert_eq!(zb_err.status_code(), 500);
    }

    #[test]
    fn storage_remote_error_converts_to_500() {
        let err = StorageError::remote("S3 timeout");
        let zb_err: crate::error::ZerobaseError = err.into();
        assert_eq!(zb_err.status_code(), 500);
    }

    // ── FileUpload ──────────────────────────────────────────────────────

    #[test]
    fn file_upload_size_returns_data_length() {
        let upload = FileUpload {
            field_name: "avatar".into(),
            original_name: "photo.jpg".into(),
            content_type: "image/jpeg".into(),
            data: vec![0u8; 1024],
        };
        assert_eq!(upload.size(), 1024);
    }
}
