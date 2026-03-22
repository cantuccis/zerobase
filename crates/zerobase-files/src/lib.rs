//! Zerobase Files — file storage abstraction.
//!
//! Supports local filesystem and S3-compatible storage backends
//! via the [`FileStorage`](zerobase_core::storage::FileStorage) trait defined
//! in `zerobase-core`.
//!
//! # Implementations
//!
//! - [`LocalFileStorage`] — stores files on the local filesystem under a
//!   configurable root directory. File layout mirrors the storage key
//!   hierarchy: `<root>/<collection_id>/<record_id>/<filename>`.
//!
//! - [`S3FileStorage`] — stores files in an S3-compatible object store.
//!   The storage key is used directly as the S3 object key within the
//!   configured bucket.
//!
//! # File service
//!
//! [`FileService`] sits above the storage backend and provides higher-level
//! operations: upload validation (size, MIME type), filename generation,
//! thumbnail creation, and file token verification for protected files.

pub mod local;
pub mod s3;
pub mod service;
pub mod thumb;

pub use local::LocalFileStorage;
pub use s3::S3FileStorage;
pub use service::FileService;
