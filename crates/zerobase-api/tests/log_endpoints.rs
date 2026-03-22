//! Integration tests for the request log REST API.
//!
//! Tests exercise the full HTTP stack (router -> middleware -> handler -> service)
//! using an in-memory [`LogRepository`] mock. Each test spawns an isolated
//! server on a random port.

mod common;

use std::sync::Arc;

use reqwest::StatusCode;
use tokio::net::TcpListener;

use zerobase_core::services::log_service::{
    LogEntry, LogList, LogQuery, LogRepoError, LogRepository, LogStats, LogStatsQuery,
    StatusCounts, TimelineEntry,
};
use zerobase_core::LogService;

// ── In-memory mock repository ───────────────────────────────────────────────

struct InMemoryLogRepo {
    logs: std::sync::Mutex<Vec<LogEntry>>,
}

impl InMemoryLogRepo {
    fn new() -> Self {
        Self {
            logs: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn with(logs: Vec<LogEntry>) -> Self {
        Self {
            logs: std::sync::Mutex::new(logs),
        }
    }
}

impl LogRepository for InMemoryLogRepo {
    fn create_log(&self, entry: &LogEntry) -> Result<(), LogRepoError> {
        self.logs.lock().unwrap().push(entry.clone());
        Ok(())
    }

    fn get_log(&self, id: &str) -> Result<LogEntry, LogRepoError> {
        self.logs
            .lock()
            .unwrap()
            .iter()
            .find(|l| l.id == id)
            .cloned()
            .ok_or(LogRepoError::NotFound { id: id.to_string() })
    }

    fn list_logs(&self, query: &LogQuery) -> Result<LogList, LogRepoError> {
        let logs = self.logs.lock().unwrap();
        let mut filtered: Vec<_> = logs.iter().cloned().collect();

        if let Some(ref method) = query.method {
            filtered.retain(|l| &l.method == method);
        }
        if let Some(ref url) = query.url {
            filtered.retain(|l| l.url.contains(url.as_str()));
        }
        if let Some(min) = query.status_min {
            filtered.retain(|l| l.status >= min);
        }
        if let Some(max) = query.status_max {
            filtered.retain(|l| l.status <= max);
        }

        let total = filtered.len() as u64;
        let per_page = if query.per_page == 0 { 20 } else { query.per_page };
        let page = if query.page == 0 { 1 } else { query.page };
        let total_pages = ((total as f64) / (per_page as f64)).ceil() as u32;
        let start = ((page - 1) * per_page) as usize;
        let items: Vec<_> = filtered.into_iter().skip(start).take(per_page as usize).collect();

        Ok(LogList {
            items,
            total_items: total,
            page,
            per_page,
            total_pages,
        })
    }

    fn get_stats(&self, _query: &LogStatsQuery) -> Result<LogStats, LogRepoError> {
        let logs = self.logs.lock().unwrap();
        let total = logs.len() as u64;
        let mut success = 0u64;
        let mut redirect = 0u64;
        let mut client_error = 0u64;
        let mut server_error = 0u64;
        let mut total_duration = 0u64;
        let mut max_duration = 0u64;

        for log in logs.iter() {
            total_duration += log.duration_ms;
            if log.duration_ms > max_duration {
                max_duration = log.duration_ms;
            }
            match log.status {
                200..=299 => success += 1,
                300..=399 => redirect += 1,
                400..=499 => client_error += 1,
                500..=599 => server_error += 1,
                _ => {}
            }
        }

        Ok(LogStats {
            total_requests: total,
            status_counts: StatusCounts {
                success,
                redirect,
                client_error,
                server_error,
            },
            avg_duration_ms: if total > 0 {
                total_duration as f64 / total as f64
            } else {
                0.0
            },
            max_duration_ms: max_duration,
            timeline: vec![],
        })
    }

    fn cleanup_old_logs(&self, _retention_days: u32) -> Result<u64, LogRepoError> {
        let mut logs = self.logs.lock().unwrap();
        let count = logs.len() as u64;
        logs.clear();
        Ok(count)
    }
}

// ── Test helpers ────────────────────────────────────────────────────────────

fn sample_log(id: &str, method: &str, status: u16) -> LogEntry {
    LogEntry {
        id: id.to_string(),
        method: method.to_string(),
        url: "/api/health".to_string(),
        status,
        ip: "127.0.0.1".to_string(),
        auth_id: String::new(),
        duration_ms: 5,
        user_agent: "test-agent".to_string(),
        request_id: "req-001".to_string(),
        created: "2024-01-15T10:30:00.000Z".to_string(),
    }
}

async fn spawn_app(repo: InMemoryLogRepo) -> (String, tokio::task::JoinHandle<()>) {
    let service = Arc::new(LogService::new(Arc::new(repo), 7));

    let app = zerobase_api::api_router().merge(zerobase_api::log_routes(service));

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

fn auth_header() -> (&'static str, &'static str) {
    ("authorization", "Bearer test-superuser-token")
}

// ── GET /_/api/logs (list) ──────────────────────────────────────────────────

#[tokio::test]
async fn list_logs_returns_200_with_items() {
    let repo = InMemoryLogRepo::with(vec![
        sample_log("l1", "GET", 200),
        sample_log("l2", "POST", 201),
    ]);
    let (addr, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/logs"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 2);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn list_logs_returns_empty_list() {
    let (addr, _handle) = spawn_app(InMemoryLogRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/logs"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 0);
    assert!(body["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn list_logs_with_method_filter() {
    let repo = InMemoryLogRepo::with(vec![
        sample_log("l1", "GET", 200),
        sample_log("l2", "POST", 201),
        sample_log("l3", "GET", 200),
    ]);
    let (addr, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/logs?method=GET"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 2);
}

#[tokio::test]
async fn list_logs_with_pagination() {
    let logs: Vec<LogEntry> = (0..5)
        .map(|i| sample_log(&format!("l{i}"), "GET", 200))
        .collect();
    let repo = InMemoryLogRepo::with(logs);
    let (addr, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/logs?page=1&perPage=2"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 5);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    assert_eq!(body["page"], 1);
    assert_eq!(body["perPage"], 2);
    assert_eq!(body["totalPages"], 3);
}

#[tokio::test]
async fn list_logs_without_auth_returns_401() {
    let (addr, _handle) = spawn_app(InMemoryLogRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/logs"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── GET /_/api/logs/stats ───────────────────────────────────────────────────

#[tokio::test]
async fn stats_returns_200_with_aggregates() {
    let repo = InMemoryLogRepo::with(vec![
        sample_log("l1", "GET", 200),
        sample_log("l2", "POST", 201),
        sample_log("l3", "GET", 404),
        sample_log("l4", "DELETE", 500),
    ]);
    let (addr, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/logs/stats"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["totalRequests"], 4);
    assert_eq!(body["statusCounts"]["success"], 2);
    assert_eq!(body["statusCounts"]["clientError"], 1);
    assert_eq!(body["statusCounts"]["serverError"], 1);
}

#[tokio::test]
async fn stats_without_auth_returns_401() {
    let (addr, _handle) = spawn_app(InMemoryLogRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/logs/stats"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── GET /_/api/logs/:id ─────────────────────────────────────────────────────

#[tokio::test]
async fn get_log_returns_200() {
    let repo = InMemoryLogRepo::with(vec![sample_log("log1", "GET", 200)]);
    let (addr, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/logs/log1"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["id"], "log1");
    assert_eq!(body["method"], "GET");
    assert_eq!(body["status"], 200);
}

#[tokio::test]
async fn get_log_not_found_returns_404() {
    let (addr, _handle) = spawn_app(InMemoryLogRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/logs/missing"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_log_without_auth_returns_401() {
    let (addr, _handle) = spawn_app(InMemoryLogRepo::new()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{addr}/_/api/logs/log1"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── End-to-end flow ─────────────────────────────────────────────────────────

#[tokio::test]
async fn full_log_query_lifecycle() {
    let repo = InMemoryLogRepo::with(vec![
        sample_log("l1", "GET", 200),
        sample_log("l2", "POST", 201),
        sample_log("l3", "GET", 404),
        sample_log("l4", "DELETE", 500),
        sample_log("l5", "PATCH", 200),
    ]);
    let (addr, _handle) = spawn_app(repo).await;
    let client = reqwest::Client::new();

    // 1. List all
    let resp = client
        .get(format!("{addr}/_/api/logs"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 5);

    // 2. Filter by method
    let resp = client
        .get(format!("{addr}/_/api/logs?method=GET"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["totalItems"], 2);

    // 3. Get stats
    let resp = client
        .get(format!("{addr}/_/api/logs/stats"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["totalRequests"], 5);
    assert_eq!(body["statusCounts"]["success"], 3);

    // 4. Get single log
    let resp = client
        .get(format!("{addr}/_/api/logs/l3"))
        .header(auth_header().0, auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["id"], "l3");
    assert_eq!(body["status"], 404);
}
