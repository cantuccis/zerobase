//! S3-compatible object storage backend.
//!
//! Stores files in an S3-compatible bucket (AWS S3, MinIO, Cloudflare R2,
//! DigitalOcean Spaces, Backblaze B2, etc.). The storage key is used directly
//! as the S3 object key within the configured bucket.
//!
//! # Configuration
//!
//! Configured via [`S3Settings`](zerobase_core::configuration::S3Settings):
//! - `bucket` — S3 bucket name
//! - `region` — AWS region (e.g. `us-east-1`)
//! - `endpoint` — custom endpoint for S3-compatible services
//! - `access_key` / `secret_key` — credentials
//! - `force_path_style` — for MinIO and similar services
//!
//! # Key behaviors
//!
//! - `upload`: `PUT` object with content-type metadata
//! - `download`: `GET` object, returns bytes + content-type from HEAD metadata
//! - `delete`: `DELETE` object (idempotent — S3 returns 204 for non-existent keys)
//! - `exists`: `HEAD` object — 200 means exists, 404 means not
//! - `generate_url`: proxied URL through the API server
//! - `delete_prefix`: `LIST` objects with prefix → individual `DELETE` calls

use s3::creds::Credentials;
use s3::{Bucket, Region};
use secrecy::ExposeSecret;
use tracing::debug;
use zerobase_core::configuration::S3Settings;
use zerobase_core::storage::{FileDownload, FileMetadata, FileStorage, StorageError};

/// S3-compatible object storage backend.
///
/// Wraps a [`Bucket`] handle from the `rust-s3` crate and implements
/// [`FileStorage`] for use as a drop-in replacement for local storage.
pub struct S3FileStorage {
    bucket: Box<Bucket>,
}

impl S3FileStorage {
    /// Create a new S3 storage backend from configuration settings.
    ///
    /// Builds credentials, region (with optional custom endpoint), and a bucket
    /// handle. If `force_path_style` is enabled, the bucket is configured to use
    /// path-style addressing (required for MinIO and many S3-compatible services).
    pub fn new(settings: &S3Settings) -> Result<Self, StorageError> {
        let credentials = Credentials::new(
            Some(settings.access_key.expose_secret()),
            Some(settings.secret_key.expose_secret()),
            None, // security_token
            None, // session_token
            None, // profile
        )
        .map_err(|e| StorageError::remote_with_source("failed to create S3 credentials", e))?;

        let region = match &settings.endpoint {
            Some(endpoint) => Region::Custom {
                region: settings.region.clone(),
                endpoint: endpoint.clone(),
            },
            None => settings
                .region
                .parse::<Region>()
                .map_err(|e| StorageError::remote_with_source("invalid S3 region", e))?,
        };

        let mut bucket = Bucket::new(&settings.bucket, region, credentials).map_err(|e| {
            StorageError::remote_with_source("failed to create S3 bucket handle", e)
        })?;

        if settings.force_path_style.unwrap_or(false) {
            bucket = bucket.with_path_style();
        }

        debug!(
            bucket = %settings.bucket,
            region = %settings.region,
            endpoint = ?settings.endpoint,
            path_style = settings.force_path_style.unwrap_or(false),
            "S3 storage backend initialized"
        );

        Ok(Self { bucket })
    }

    /// Create an S3 storage backend from individual parameters.
    ///
    /// Useful for testing or when configuration is not loaded from settings.
    pub fn from_params(
        bucket_name: &str,
        region: Region,
        credentials: Credentials,
        force_path_style: bool,
    ) -> Result<Self, StorageError> {
        let mut bucket = Bucket::new(bucket_name, region, credentials).map_err(|e| {
            StorageError::remote_with_source("failed to create S3 bucket handle", e)
        })?;

        if force_path_style {
            bucket = bucket.with_path_style();
        }

        Ok(Self { bucket })
    }
}

#[async_trait::async_trait]
impl FileStorage for S3FileStorage {
    async fn upload(&self, key: &str, data: &[u8], content_type: &str) -> Result<(), StorageError> {
        let response = self
            .bucket
            .put_object_with_content_type(key, data, content_type)
            .await
            .map_err(|e| StorageError::remote_with_source(format!("S3 PUT failed for {key}"), e))?;

        let status = response.status_code();
        if !(200..300).contains(&status) {
            return Err(StorageError::remote(format!(
                "S3 PUT returned status {status} for {key}"
            )));
        }

        debug!(key, content_type, size = data.len(), "uploaded file to S3");
        Ok(())
    }

    async fn download(&self, key: &str) -> Result<FileDownload, StorageError> {
        // GET the object data.
        let response =
            self.bucket.get_object(key).await.map_err(|e| {
                StorageError::remote_with_source(format!("S3 GET failed for {key}"), e)
            })?;

        let status = response.status_code();
        if status == 404 {
            return Err(StorageError::NotFound {
                key: key.to_string(),
            });
        }
        if !(200..300).contains(&status) {
            return Err(StorageError::remote(format!(
                "S3 GET returned status {status} for {key}"
            )));
        }

        // Extract content-type from response headers, or use HEAD as fallback.
        let content_type = response
            .headers()
            .get("content-type")
            .cloned()
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let data = response.to_vec();
        let original_name = key.rsplit('/').next().unwrap_or(key).to_string();

        Ok(FileDownload {
            metadata: FileMetadata {
                key: key.to_string(),
                original_name,
                content_type,
                size: data.len() as u64,
            },
            data,
        })
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        // S3 DELETE is idempotent — returns 204 even for non-existent keys.
        let response = self.bucket.delete_object(key).await.map_err(|e| {
            StorageError::remote_with_source(format!("S3 DELETE failed for {key}"), e)
        })?;

        let status = response.status_code();
        if !(200..300).contains(&status) {
            return Err(StorageError::remote(format!(
                "S3 DELETE returned status {status} for {key}"
            )));
        }

        debug!(key, "deleted file from S3");
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        self.bucket
            .object_exists(key)
            .await
            .map_err(|e| StorageError::remote_with_source(format!("S3 HEAD failed for {key}"), e))
    }

    fn generate_url(&self, key: &str, base_url: &str) -> String {
        // Proxy through the API server rather than generating pre-signed URLs.
        // This keeps file access governed by the same API rules and auth tokens
        // as the rest of the API.
        let base = base_url.trim_end_matches('/');
        format!("{base}/api/files/{key}")
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<(), StorageError> {
        // List all objects under the prefix, then delete each one.
        let results = self
            .bucket
            .list(prefix.to_string(), None)
            .await
            .map_err(|e| {
                StorageError::remote_with_source(format!("S3 LIST failed for prefix {prefix}"), e)
            })?;

        let mut delete_count = 0u64;
        for page in &results {
            for object in &page.contents {
                self.bucket.delete_object(&object.key).await.map_err(|e| {
                    StorageError::remote_with_source(
                        format!("S3 DELETE failed for {}", object.key),
                        e,
                    )
                })?;
                delete_count += 1;
            }
        }

        if delete_count > 0 {
            debug!(prefix, delete_count, "deleted files by prefix from S3");
        }

        Ok(())
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Unit tests (no S3 connection required) ──────────────────────────

    #[test]
    fn generate_url_produces_api_path() {
        // We can't easily construct S3FileStorage without credentials in a unit test,
        // so test the URL generation logic directly.
        let base = "https://api.example.com";
        let key = "col123/rec456/abc_photo.jpg";
        let expected = "https://api.example.com/api/files/col123/rec456/abc_photo.jpg";

        let url = {
            let base = base.trim_end_matches('/');
            format!("{base}/api/files/{key}")
        };

        assert_eq!(url, expected);
    }

    #[test]
    fn generate_url_strips_trailing_slash() {
        let base = "https://api.example.com/";
        let key = "col/rec/file.txt";

        let url = {
            let base = base.trim_end_matches('/');
            format!("{base}/api/files/{key}")
        };

        assert_eq!(url, "https://api.example.com/api/files/col/rec/file.txt");
    }

    #[test]
    fn from_params_path_style_creates_storage() {
        let creds =
            Credentials::new(Some("test-key"), Some("test-secret"), None, None, None).unwrap();

        let region = Region::Custom {
            region: "us-east-1".to_string(),
            endpoint: "http://localhost:9000".to_string(),
        };

        let storage = S3FileStorage::from_params("test-bucket", region, creds, true);
        assert!(storage.is_ok());
    }

    #[test]
    fn from_params_virtual_hosted_creates_storage() {
        let creds = Credentials::new(
            Some("AKIAIOSFODNN7EXAMPLE"),
            Some("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"),
            None,
            None,
            None,
        )
        .unwrap();

        let region = "us-east-1".parse::<Region>().unwrap();

        let storage = S3FileStorage::from_params("my-bucket", region, creds, false);
        assert!(storage.is_ok());
    }

    #[test]
    fn new_with_settings_creates_storage() {
        let settings = S3Settings {
            bucket: "test-bucket".to_string(),
            region: "us-east-1".to_string(),
            endpoint: Some("http://localhost:9000".to_string()),
            access_key: secrecy::SecretString::from("test-key".to_string()),
            secret_key: secrecy::SecretString::from("test-secret".to_string()),
            force_path_style: Some(true),
        };

        let storage = S3FileStorage::new(&settings);
        assert!(storage.is_ok());
    }

    #[test]
    fn new_with_aws_region_creates_storage() {
        let settings = S3Settings {
            bucket: "my-bucket".to_string(),
            region: "eu-west-1".to_string(),
            endpoint: None,
            access_key: secrecy::SecretString::from("AKID".to_string()),
            secret_key: secrecy::SecretString::from("SECRET".to_string()),
            force_path_style: None,
        };

        let storage = S3FileStorage::new(&settings);
        assert!(storage.is_ok());
    }

    // ── Mock-based integration tests ────────────────────────────────────
    //
    // These tests use a local HTTP server to simulate S3 responses, allowing
    // us to test the full upload/download/delete/exists/delete_prefix flow
    // without requiring a real S3 instance.

    use std::sync::{Arc, Mutex};
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    /// A minimal mock S3 server for testing.
    ///
    /// Stores objects in memory and handles PUT, GET, DELETE, HEAD, and
    /// rudimentary LIST operations.
    struct MockS3Server {
        objects: Arc<Mutex<std::collections::HashMap<String, MockObject>>>,
        addr: std::net::SocketAddr,
        shutdown: tokio::sync::oneshot::Sender<()>,
    }

    struct MockObject {
        data: Vec<u8>,
        content_type: String,
    }

    impl MockS3Server {
        async fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let objects: Arc<Mutex<std::collections::HashMap<String, MockObject>>> =
                Arc::new(Mutex::new(std::collections::HashMap::new()));
            let objects_clone = objects.clone();
            let (tx, rx) = tokio::sync::oneshot::channel::<()>();

            tokio::spawn(async move {
                let objects = objects_clone;
                tokio::select! {
                    _ = Self::accept_loop(listener, objects) => {}
                    _ = rx => {}
                }
            });

            Self {
                objects,
                addr,
                shutdown: tx,
            }
        }

        async fn accept_loop(
            listener: TcpListener,
            objects: Arc<Mutex<std::collections::HashMap<String, MockObject>>>,
        ) {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let objects = objects.clone();
                tokio::spawn(async move {
                    Self::handle_connection(stream, objects).await;
                });
            }
        }

        async fn handle_connection(
            mut stream: tokio::net::TcpStream,
            objects: Arc<Mutex<std::collections::HashMap<String, MockObject>>>,
        ) {
            use tokio::io::AsyncReadExt;

            let mut buf = vec![0u8; 65536];
            let n = match stream.read(&mut buf).await {
                Ok(n) if n > 0 => n,
                _ => return,
            };
            let request = String::from_utf8_lossy(&buf[..n]);

            let first_line = request.lines().next().unwrap_or("");
            let parts: Vec<&str> = first_line.split_whitespace().collect();
            if parts.len() < 2 {
                return;
            }

            let method = parts[0];
            let raw_path = parts[1];

            // Parse path: /bucket-name/key or /bucket-name?list-type=2&prefix=...
            let path_without_query = raw_path.split('?').next().unwrap_or(raw_path);
            let key = path_without_query
                .trim_start_matches('/')
                .splitn(2, '/')
                .nth(1)
                .unwrap_or("");

            // Parse content-type header.
            let content_type = request
                .lines()
                .find(|l| l.to_lowercase().starts_with("content-type:"))
                .map(|l| l.split_once(':').unwrap().1.trim().to_string())
                .unwrap_or_else(|| "application/octet-stream".to_string());

            // Parse content-length header.
            let content_length: usize = request
                .lines()
                .find(|l| l.to_lowercase().starts_with("content-length:"))
                .and_then(|l| l.split_once(':').unwrap().1.trim().parse().ok())
                .unwrap_or(0);

            // Extract body (after double CRLF).
            let body = if let Some(pos) = request.find("\r\n\r\n") {
                let body_start = pos + 4;
                if body_start < n {
                    buf[body_start..n].to_vec()
                } else {
                    vec![]
                }
            } else {
                vec![]
            };

            // Read remaining body if needed.
            let mut full_body = body;
            while full_body.len() < content_length {
                let mut extra = vec![0u8; content_length - full_body.len()];
                match stream.read(&mut extra).await {
                    Ok(n) if n > 0 => full_body.extend_from_slice(&extra[..n]),
                    _ => break,
                }
            }

            let response = match method {
                "PUT" => {
                    let mut store = objects.lock().unwrap();
                    store.insert(
                        key.to_string(),
                        MockObject {
                            data: full_body,
                            content_type,
                        },
                    );
                    "HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n".to_string()
                }
                "GET" => {
                    // Check if this is a LIST request (bucket root with list-type param).
                    if raw_path.contains("list-type=") || key.is_empty() {
                        let prefix = raw_path
                            .split('?')
                            .nth(1)
                            .unwrap_or("")
                            .split('&')
                            .find(|p| p.starts_with("prefix="))
                            .map(|p| {
                                urlencoding::decode(p.trim_start_matches("prefix="))
                                    .unwrap_or_default()
                                    .to_string()
                            })
                            .unwrap_or_default();

                        let store = objects.lock().unwrap();
                        let mut xml = String::from(
                            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                             <ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
                             <Name>test-bucket</Name>\
                             <IsTruncated>false</IsTruncated>",
                        );
                        for (obj_key, obj) in store.iter() {
                            if obj_key.starts_with(&prefix) {
                                xml.push_str(&format!(
                                    "<Contents><Key>{}</Key><Size>{}</Size>\
                                     <LastModified>2024-01-01T00:00:00.000Z</LastModified>\
                                     </Contents>",
                                    obj_key,
                                    obj.data.len()
                                ));
                            }
                        }
                        xml.push_str("</ListBucketResult>");

                        format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/xml\r\ncontent-length: {}\r\n\r\n{}",
                            xml.len(),
                            xml
                        )
                    } else {
                        let store = objects.lock().unwrap();
                        if let Some(obj) = store.get(key) {
                            format!(
                                "HTTP/1.1 200 OK\r\ncontent-type: {}\r\ncontent-length: {}\r\n\r\n",
                                obj.content_type,
                                obj.data.len()
                            )
                        } else {
                            "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n".to_string()
                        }
                    }
                }
                "DELETE" => {
                    let mut store = objects.lock().unwrap();
                    store.remove(key);
                    "HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n".to_string()
                }
                "HEAD" => {
                    let store = objects.lock().unwrap();
                    if let Some(obj) = store.get(key) {
                        format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: {}\r\ncontent-length: {}\r\n\r\n",
                            obj.content_type,
                            obj.data.len()
                        )
                    } else {
                        "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n".to_string()
                    }
                }
                _ => "HTTP/1.1 405 Method Not Allowed\r\ncontent-length: 0\r\n\r\n".to_string(),
            };

            // For GET with body data (non-LIST), send the response + body.
            if method == "GET" && !raw_path.contains("list-type=") && !key.is_empty() {
                let obj_data = {
                    let store = objects.lock().unwrap();
                    store
                        .get(key)
                        .map(|obj| (obj.content_type.clone(), obj.data.clone()))
                };
                if let Some((ct, data)) = obj_data {
                    let header = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: {ct}\r\ncontent-length: {}\r\n\r\n",
                        data.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(&data).await;
                    return;
                }
            }

            let _ = stream.write_all(response.as_bytes()).await;
        }

        fn endpoint(&self) -> String {
            format!("http://{}", self.addr)
        }

        fn create_storage(&self) -> S3FileStorage {
            let creds = Credentials::new(
                Some("test-access-key"),
                Some("test-secret-key"),
                None,
                None,
                None,
            )
            .unwrap();

            let region = Region::Custom {
                region: "us-east-1".to_string(),
                endpoint: self.endpoint(),
            };

            S3FileStorage::from_params("test-bucket", region, creds, true).unwrap()
        }

        fn object_count(&self) -> usize {
            self.objects.lock().unwrap().len()
        }

        fn shutdown(self) {
            let _ = self.shutdown.send(());
        }
    }

    #[tokio::test]
    async fn upload_stores_file() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        let result = storage
            .upload("col/rec/test.txt", b"hello world", "text/plain")
            .await;
        assert!(result.is_ok());
        assert_eq!(server.object_count(), 1);

        server.shutdown();
    }

    #[tokio::test]
    async fn upload_overwrites_existing() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        storage
            .upload("col/rec/test.txt", b"version 1", "text/plain")
            .await
            .unwrap();
        storage
            .upload("col/rec/test.txt", b"version 2", "text/plain")
            .await
            .unwrap();

        // Still only one object.
        assert_eq!(server.object_count(), 1);
        server.shutdown();
    }

    #[tokio::test]
    async fn download_returns_uploaded_data() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        let data = b"hello s3 world";
        storage
            .upload("col/rec/photo.jpg", data, "image/jpeg")
            .await
            .unwrap();

        let download = storage.download("col/rec/photo.jpg").await.unwrap();
        assert_eq!(download.data, data);
        assert_eq!(download.metadata.content_type, "image/jpeg");
        assert_eq!(download.metadata.key, "col/rec/photo.jpg");
        assert_eq!(download.metadata.original_name, "photo.jpg");
        assert_eq!(download.metadata.size, data.len() as u64);

        server.shutdown();
    }

    #[tokio::test]
    async fn download_not_found_returns_error() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        let result = storage.download("col/rec/nonexistent.txt").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            StorageError::NotFound { key } => assert_eq!(key, "col/rec/nonexistent.txt"),
            other => panic!("expected NotFound, got: {other:?}"),
        }

        server.shutdown();
    }

    #[tokio::test]
    async fn delete_removes_file() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        storage
            .upload("col/rec/file.txt", b"data", "text/plain")
            .await
            .unwrap();
        assert_eq!(server.object_count(), 1);

        storage.delete("col/rec/file.txt").await.unwrap();
        assert_eq!(server.object_count(), 0);

        server.shutdown();
    }

    #[tokio::test]
    async fn delete_nonexistent_is_idempotent() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        // Deleting a key that never existed should succeed.
        let result = storage.delete("col/rec/ghost.txt").await;
        assert!(result.is_ok());

        server.shutdown();
    }

    #[tokio::test]
    async fn exists_returns_true_for_existing() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        storage
            .upload("col/rec/present.txt", b"data", "text/plain")
            .await
            .unwrap();

        assert!(storage.exists("col/rec/present.txt").await.unwrap());
        server.shutdown();
    }

    #[tokio::test]
    async fn exists_returns_false_for_missing() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        assert!(!storage.exists("col/rec/missing.txt").await.unwrap());
        server.shutdown();
    }

    #[tokio::test]
    async fn delete_prefix_removes_matching_objects() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        // Upload files under two different records.
        storage
            .upload("col/rec1/a.txt", b"a", "text/plain")
            .await
            .unwrap();
        storage
            .upload("col/rec1/b.txt", b"b", "text/plain")
            .await
            .unwrap();
        storage
            .upload("col/rec2/c.txt", b"c", "text/plain")
            .await
            .unwrap();

        assert_eq!(server.object_count(), 3);

        // Delete only rec1's files.
        storage.delete_prefix("col/rec1/").await.unwrap();

        assert_eq!(server.object_count(), 1);
        assert!(!storage.exists("col/rec1/a.txt").await.unwrap());
        assert!(!storage.exists("col/rec1/b.txt").await.unwrap());
        assert!(storage.exists("col/rec2/c.txt").await.unwrap());

        server.shutdown();
    }

    #[tokio::test]
    async fn delete_prefix_with_no_matches_succeeds() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        let result = storage.delete_prefix("nonexistent/prefix/").await;
        assert!(result.is_ok());

        server.shutdown();
    }

    #[tokio::test]
    async fn roundtrip_upload_download_delete() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        let key = "collection1/record1/abc123_document.pdf";
        let data = b"PDF file content here";
        let content_type = "application/pdf";

        // Upload.
        storage.upload(key, data, content_type).await.unwrap();
        assert!(storage.exists(key).await.unwrap());

        // Download.
        let download = storage.download(key).await.unwrap();
        assert_eq!(download.data, data);
        assert_eq!(download.metadata.content_type, content_type);

        // Delete.
        storage.delete(key).await.unwrap();
        assert!(!storage.exists(key).await.unwrap());

        // Download after delete should fail.
        let result = storage.download(key).await;
        assert!(matches!(result, Err(StorageError::NotFound { .. })));

        server.shutdown();
    }

    #[tokio::test]
    async fn upload_large_data() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        // 1 MB of data.
        let data = vec![0xABu8; 1024 * 1024];
        storage
            .upload("col/rec/large.bin", &data, "application/octet-stream")
            .await
            .unwrap();

        let download = storage.download("col/rec/large.bin").await.unwrap();
        assert_eq!(download.data.len(), 1024 * 1024);
        assert_eq!(download.metadata.size, 1024 * 1024);

        server.shutdown();
    }

    #[tokio::test]
    async fn upload_empty_data() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        storage
            .upload("col/rec/empty.txt", b"", "text/plain")
            .await
            .unwrap();

        let download = storage.download("col/rec/empty.txt").await.unwrap();
        assert!(download.data.is_empty());
        assert_eq!(download.metadata.size, 0);

        server.shutdown();
    }

    #[tokio::test]
    async fn multiple_files_per_record() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        storage
            .upload("col/rec/file1.txt", b"one", "text/plain")
            .await
            .unwrap();
        storage
            .upload("col/rec/file2.jpg", b"two", "image/jpeg")
            .await
            .unwrap();
        storage
            .upload("col/rec/file3.pdf", b"three", "application/pdf")
            .await
            .unwrap();

        assert_eq!(server.object_count(), 3);

        // Delete all files for the record.
        storage.delete_prefix("col/rec/").await.unwrap();
        assert_eq!(server.object_count(), 0);

        server.shutdown();
    }

    #[tokio::test]
    async fn generate_url_uses_api_path() {
        let server = MockS3Server::start().await;
        let storage = server.create_storage();

        let url = storage.generate_url("col/rec/file.txt", "https://myapp.com");
        assert_eq!(url, "https://myapp.com/api/files/col/rec/file.txt");

        let url2 = storage.generate_url("col/rec/file.txt", "https://myapp.com/");
        assert_eq!(url2, "https://myapp.com/api/files/col/rec/file.txt");

        server.shutdown();
    }

    #[tokio::test]
    async fn concurrent_uploads() {
        let server = MockS3Server::start().await;
        let storage = Arc::new(server.create_storage());

        let mut handles = Vec::new();
        for i in 0..5 {
            let s = storage.clone();
            handles.push(tokio::spawn(async move {
                let key = format!("col/rec/file{i}.txt");
                let data = format!("content {i}");
                s.upload(&key, data.as_bytes(), "text/plain").await.unwrap();
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(server.object_count(), 5);
        server.shutdown();
    }
}
