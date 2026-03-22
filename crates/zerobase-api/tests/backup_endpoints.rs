//! Integration tests for the backup management REST API.
//!
//! Tests exercise the full HTTP stack (router → middleware → handler → service)
//! using an in-memory [`BackupRepository`] mock. Each test spawns an isolated
//! server on a random port.

mod common;

use std::sync::Arc;

use reqwest::StatusCode;
use serde_json::json;
use tokio::net::TcpListener;

use zerobase_core::services::backup_service::{
    BackupInfo, BackupRepoError, BackupRepository,
};
use zerobase_core::BackupService;

// ── In-memory mock repository ───────────────────────────────────────────────

struct InMemoryBackupRepo {
    backups: std::sync::Mutex<Vec<BackupInfo>>,
}

impl InMemoryBackupRepo {
    fn new() -> Self {
        Self {
            backups: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn with(backups: Vec<BackupInfo>) -> Self {
        Self {
            backups: std::sync::Mutex::new(backups),
        }
    }
}

impl BackupRepository for InMemoryBackupRepo {
    fn create_backup(&self, name: &str) -> Result<BackupInfo, BackupRepoError> {
        let mut backups = self.backups.lock().unwrap();
        if backups.iter().any(|b| b.name == name) {
            return Err(BackupRepoError::AlreadyExists {
                name: name.to_string(),
            });
        }
        let info = BackupInfo {
            name: name.to_string(),
            size: 4096,
            created: "2025-01-15 10:30:00.000Z".to_string(),
            modified: "2025-01-15 10:30:00.000Z".to_string(),
        };
        backups.push(info.clone());
        Ok(info)
    }

    fn list_backups(&self) -> Result<Vec<BackupInfo>, BackupRepoError> {
        let backups = self.backups.lock().unwrap();
        let mut list = backups.clone();
        list.reverse();
        Ok(list)
    }

    fn get_backup(&self, name: &str) -> Result<BackupInfo, BackupRepoError> {
        let backups = self.backups.lock().unwrap();
        backups
            .iter()
            .find(|b| b.name == name)
            .cloned()
            .ok_or(BackupRepoError::NotFound {
                name: name.to_string(),
            })
    }

    fn backup_path(&self, name: &str) -> Result<String, BackupRepoError> {
        let backups = self.backups.lock().unwrap();
        if backups.iter().any(|b| b.name == name) {
            // Return a path that won't exist on disk — download test will get
            // a 500, which is expected for the mock. The important thing is
            // the routing and handler logic.
            Ok(format!("/tmp/nonexistent_backups/{name}"))
        } else {
            Err(BackupRepoError::NotFound {
                name: name.to_string(),
            })
        }
    }

    fn delete_backup(&self, name: &str) -> Result<(), BackupRepoError> {
        let mut backups = self.backups.lock().unwrap();
        let before = backups.len();
        backups.retain(|b| b.name != name);
        if backups.len() == before {
            Err(BackupRepoError::NotFound {
                name: name.to_string(),
            })
        } else {
            Ok(())
        }
    }

    fn restore_backup(&self, name: &str) -> Result<(), BackupRepoError> {
        let backups = self.backups.lock().unwrap();
        if backups.iter().any(|b| b.name == name) {
            Ok(())
        } else {
            Err(BackupRepoError::NotFound {
                name: name.to_string(),
            })
        }
    }
}

// ── Test infrastructure ─────────────────────────────────────────────────────

fn sample_backup(name: &str) -> BackupInfo {
    BackupInfo {
        name: name.to_string(),
        size: 2048,
        created: "2025-01-01 12:00:00.000Z".to_string(),
        modified: "2025-01-01 12:00:00.000Z".to_string(),
    }
}

async fn spawn_app(repo: InMemoryBackupRepo) -> (String, u16, tokio::task::JoinHandle<()>) {
    let service = Arc::new(BackupService::new(repo));

    let app = zerobase_api::api_router().merge(zerobase_api::backup_routes(service));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let port = addr.port();
    let address = format!("http://127.0.0.1:{port}");

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (address, port, handle)
}

fn auth_header() -> (&'static str, &'static str) {
    ("authorization", "Bearer test-superuser-token")
}

// ── POST /_/api/backups (create) ────────────────────────────────────────────

#[tokio::test]
async fn create_backup_returns_200_with_auto_name() {
    let (addr, _, _handle) = spawn_app(InMemoryBackupRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/_/api/backups"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    let name = body["name"].as_str().unwrap();
    assert!(name.starts_with("pb_backup_"));
    assert!(name.ends_with(".db"));
    assert!(body["size"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn create_backup_with_custom_name() {
    let (addr, _, _handle) = spawn_app(InMemoryBackupRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/_/api/backups"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({"name": "my_backup.db"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "my_backup.db");
}

#[tokio::test]
async fn create_backup_rejects_invalid_extension() {
    let (addr, _, _handle) = spawn_app(InMemoryBackupRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/_/api/backups"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({"name": "backup.txt"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_backup_rejects_path_traversal() {
    let (addr, _, _handle) = spawn_app(InMemoryBackupRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/_/api/backups"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({"name": "../evil.db"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_backup_conflict_when_already_exists() {
    let repo = InMemoryBackupRepo::with(vec![sample_backup("existing.db")]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/_/api/backups"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({"name": "existing.db"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn create_backup_without_auth_returns_401() {
    let (addr, _, _handle) = spawn_app(InMemoryBackupRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/_/api/backups"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── GET /_/api/backups (list) ───────────────────────────────────────────────

#[tokio::test]
async fn list_backups_returns_200_with_array() {
    let repo = InMemoryBackupRepo::with(vec![
        sample_backup("first.db"),
        sample_backup("second.db"),
    ]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/backups"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(body.len(), 2);
}

#[tokio::test]
async fn list_backups_returns_empty_array() {
    let (addr, _, _handle) = spawn_app(InMemoryBackupRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/backups"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(body.is_empty());
}

#[tokio::test]
async fn list_backups_without_auth_returns_401() {
    let (addr, _, _handle) = spawn_app(InMemoryBackupRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/backups"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── GET /_/api/backups/:name (download) ─────────────────────────────────────

#[tokio::test]
async fn download_backup_not_found_returns_404() {
    let (addr, _, _handle) = spawn_app(InMemoryBackupRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/backups/missing.db"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn download_backup_without_auth_returns_401() {
    let (addr, _, _handle) = spawn_app(InMemoryBackupRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/backups/test.db"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── DELETE /_/api/backups/:name ─────────────────────────────────────────────

#[tokio::test]
async fn delete_backup_returns_204() {
    let repo = InMemoryBackupRepo::with(vec![sample_backup("to_delete.db")]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{addr}/_/api/backups/to_delete.db"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify it's gone.
    let resp = client
        .get(format!("{addr}/_/api/backups"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    let body: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(body.is_empty());
}

#[tokio::test]
async fn delete_backup_not_found_returns_404() {
    let (addr, _, _handle) = spawn_app(InMemoryBackupRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{addr}/_/api/backups/ghost.db"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_backup_without_auth_returns_401() {
    let (addr, _, _handle) = spawn_app(InMemoryBackupRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{addr}/_/api/backups/test.db"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── POST /_/api/backups/:name/restore ───────────────────────────────────────

#[tokio::test]
async fn restore_backup_returns_204() {
    let repo = InMemoryBackupRepo::with(vec![sample_backup("to_restore.db")]);
    let (addr, _, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/_/api/backups/to_restore.db/restore"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn restore_backup_not_found_returns_404() {
    let (addr, _, _handle) = spawn_app(InMemoryBackupRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/_/api/backups/missing.db/restore"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn restore_backup_without_auth_returns_401() {
    let (addr, _, _handle) = spawn_app(InMemoryBackupRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{addr}/_/api/backups/test.db/restore"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── End-to-end flow ────────────────────────────────────────────────────────

#[tokio::test]
async fn full_backup_lifecycle() {
    let (addr, _, _handle) = spawn_app(InMemoryBackupRepo::new()).await;
    let client = reqwest::Client::new();

    // 1. List — initially empty.
    let resp = client
        .get(format!("{addr}/_/api/backups"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(body.is_empty());

    // 2. Create a backup.
    let resp = client
        .post(format!("{addr}/_/api/backups"))
        .header(auth_header().0, auth_header().1)
        .json(&json!({"name": "lifecycle.db"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let backup: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(backup["name"], "lifecycle.db");

    // 3. List — should have one backup.
    let resp = client
        .get(format!("{addr}/_/api/backups"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(body.len(), 1);
    assert_eq!(body[0]["name"], "lifecycle.db");

    // 4. Restore from the backup.
    let resp = client
        .post(format!("{addr}/_/api/backups/lifecycle.db/restore"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 5. Delete the backup.
    let resp = client
        .delete(format!("{addr}/_/api/backups/lifecycle.db"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 6. List — empty again.
    let resp = client
        .get(format!("{addr}/_/api/backups"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(body.is_empty());

    // 7. Trying to delete the same backup again gives 404.
    let resp = client
        .delete(format!("{addr}/_/api/backups/lifecycle.db"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
