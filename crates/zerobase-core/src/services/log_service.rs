//! Request logging and audit trail service.
//!
//! [`LogService`] manages creating, querying, and cleaning up API request logs.
//! All HTTP requests are logged to a `_logs` table for auditing and diagnostics.
//!
//! # Design
//!
//! - The service is generic over `R: LogRepository` for testability.
//! - Logs capture: method, URL, status, IP, auth user, duration, and timestamp.
//! - Auto-cleanup removes logs older than a configurable retention period.
//! - Stats endpoint provides aggregate metrics (requests per day, error rates, etc.).

use serde::{Deserialize, Serialize};

use crate::error::{Result, ZerobaseError};

// ── Repository trait ────────────────────────────────────────────────────────

/// Persistence contract for request log operations.
///
/// Defined in core so the service doesn't depend on `zerobase-db` directly.
/// The DB crate implements this trait on `Database`.
pub trait LogRepository: Send + Sync {
    /// Insert a new log entry.
    fn create_log(&self, entry: &LogEntry) -> std::result::Result<(), LogRepoError>;

    /// Retrieve a single log entry by ID.
    fn get_log(&self, id: &str) -> std::result::Result<LogEntry, LogRepoError>;

    /// List log entries matching the given query.
    fn list_logs(&self, query: &LogQuery) -> std::result::Result<LogList, LogRepoError>;

    /// Get aggregate statistics for the given time range.
    fn get_stats(&self, query: &LogStatsQuery) -> std::result::Result<LogStats, LogRepoError>;

    /// Delete logs older than the given retention period (in days).
    /// Returns the number of deleted entries.
    fn cleanup_old_logs(&self, retention_days: u32) -> std::result::Result<u64, LogRepoError>;
}

/// Errors that a log repository can produce.
#[derive(Debug, thiserror::Error)]
pub enum LogRepoError {
    #[error("log not found: {id}")]
    NotFound { id: String },
    #[error("log operation failed: {message}")]
    OperationFailed { message: String },
}

// ── Domain types ────────────────────────────────────────────────────────────

/// A single request log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// Unique log entry ID.
    pub id: String,
    /// HTTP method (GET, POST, PATCH, DELETE, etc.).
    pub method: String,
    /// Request URL path (e.g. `/api/collections/posts/records`).
    pub url: String,
    /// HTTP response status code.
    pub status: u16,
    /// Client IP address.
    pub ip: String,
    /// Authenticated user ID (empty string if unauthenticated).
    pub auth_id: String,
    /// Request duration in milliseconds.
    pub duration_ms: u64,
    /// User-Agent header value.
    pub user_agent: String,
    /// Request ID (from x-request-id header).
    pub request_id: String,
    /// ISO 8601 timestamp when the request was received.
    pub created: String,
}

/// Query parameters for listing logs.
#[derive(Debug, Clone, Default)]
pub struct LogQuery {
    /// Filter by HTTP method (e.g. "GET", "POST").
    pub method: Option<String>,
    /// Filter by URL path (substring match).
    pub url: Option<String>,
    /// Filter by status code range: minimum (inclusive).
    pub status_min: Option<u16>,
    /// Filter by status code range: maximum (inclusive).
    pub status_max: Option<u16>,
    /// Filter by auth user ID.
    pub auth_id: Option<String>,
    /// Filter by IP address.
    pub ip: Option<String>,
    /// Filter: created after this ISO datetime.
    pub created_after: Option<String>,
    /// Filter: created before this ISO datetime.
    pub created_before: Option<String>,
    /// General filter expression (Zerobase-style).
    pub filter: Option<String>,
    /// Page number (1-based).
    pub page: u32,
    /// Items per page.
    pub per_page: u32,
    /// Sort field and direction (e.g. "-created" for descending).
    pub sort: Option<String>,
}

/// A paginated list of log entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogList {
    pub items: Vec<LogEntry>,
    pub total_items: u64,
    pub page: u32,
    pub per_page: u32,
    pub total_pages: u32,
}

/// Query parameters for log statistics.
#[derive(Debug, Clone, Default)]
pub struct LogStatsQuery {
    /// Filter: stats for logs created after this ISO datetime.
    pub created_after: Option<String>,
    /// Filter: stats for logs created before this ISO datetime.
    pub created_before: Option<String>,
    /// Group by time interval: "hour", "day" (default), "month".
    pub group_by: Option<String>,
}

/// Aggregate log statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogStats {
    /// Total number of requests in the period.
    pub total_requests: u64,
    /// Breakdown by status category.
    pub status_counts: StatusCounts,
    /// Average response time in milliseconds.
    pub avg_duration_ms: f64,
    /// Maximum response time in milliseconds.
    pub max_duration_ms: u64,
    /// Time-series data grouped by the requested interval.
    pub timeline: Vec<TimelineEntry>,
}

/// Request counts by HTTP status category.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusCounts {
    /// 2xx responses.
    pub success: u64,
    /// 3xx responses.
    pub redirect: u64,
    /// 4xx responses.
    pub client_error: u64,
    /// 5xx responses.
    pub server_error: u64,
}

/// A single data point in the timeline series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    /// Time bucket label (ISO date or datetime).
    pub date: String,
    /// Number of requests in this bucket.
    pub total: u64,
}

// ── Service ─────────────────────────────────────────────────────────────────

/// Service for managing request logs and audit trails.
///
/// Generic over `R: LogRepository` for testability. The service coordinates
/// log creation, querying, statistics, and auto-cleanup.
pub struct LogService<R: LogRepository> {
    repo: std::sync::Arc<R>,
    /// Log retention period in days. Logs older than this are auto-cleaned.
    retention_days: u32,
}

impl<R: LogRepository> LogService<R> {
    /// Create a new log service.
    ///
    /// `retention_days` controls how long logs are kept (default: 7 days).
    pub fn new(repo: std::sync::Arc<R>, retention_days: u32) -> Self {
        Self {
            repo,
            retention_days,
        }
    }

    /// Record a new log entry.
    pub fn create(&self, entry: &LogEntry) -> Result<()> {
        self.repo.create_log(entry).map_err(|e| match e {
            LogRepoError::OperationFailed { message } => {
                ZerobaseError::internal(format!("failed to create log: {message}"))
            }
            LogRepoError::NotFound { id } => ZerobaseError::not_found_with_id("Log", id),
        })
    }

    /// Retrieve a single log entry by ID.
    pub fn get(&self, id: &str) -> Result<LogEntry> {
        self.repo.get_log(id).map_err(|e| match e {
            LogRepoError::NotFound { id } => ZerobaseError::not_found_with_id("Log", id),
            LogRepoError::OperationFailed { message } => {
                ZerobaseError::internal(format!("failed to get log: {message}"))
            }
        })
    }

    /// List log entries matching the query.
    pub fn list(&self, query: &LogQuery) -> Result<LogList> {
        self.repo.list_logs(query).map_err(|e| match e {
            LogRepoError::OperationFailed { message } => {
                ZerobaseError::internal(format!("failed to list logs: {message}"))
            }
            LogRepoError::NotFound { id } => ZerobaseError::not_found_with_id("Log", id),
        })
    }

    /// Get aggregate statistics.
    pub fn stats(&self, query: &LogStatsQuery) -> Result<LogStats> {
        self.repo.get_stats(query).map_err(|e| match e {
            LogRepoError::OperationFailed { message } => {
                ZerobaseError::internal(format!("failed to get log stats: {message}"))
            }
            LogRepoError::NotFound { id } => ZerobaseError::not_found_with_id("Log", id),
        })
    }

    /// Run auto-cleanup: delete logs older than the retention period.
    /// Returns the number of deleted entries.
    pub fn cleanup(&self) -> Result<u64> {
        self.repo
            .cleanup_old_logs(self.retention_days)
            .map_err(|e| match e {
                LogRepoError::OperationFailed { message } => {
                    ZerobaseError::internal(format!("failed to cleanup logs: {message}"))
                }
                LogRepoError::NotFound { id } => ZerobaseError::not_found_with_id("Log", id),
            })
    }

    /// Get the configured retention period in days.
    pub fn retention_days(&self) -> u32 {
        self.retention_days
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// In-memory mock log repository for testing the service layer.
    struct MockLogRepo {
        logs: Mutex<Vec<LogEntry>>,
    }

    impl MockLogRepo {
        fn new() -> Self {
            Self {
                logs: Mutex::new(Vec::new()),
            }
        }
    }

    impl LogRepository for MockLogRepo {
        fn create_log(&self, entry: &LogEntry) -> std::result::Result<(), LogRepoError> {
            self.logs.lock().unwrap().push(entry.clone());
            Ok(())
        }

        fn get_log(&self, id: &str) -> std::result::Result<LogEntry, LogRepoError> {
            self.logs
                .lock()
                .unwrap()
                .iter()
                .find(|l| l.id == id)
                .cloned()
                .ok_or(LogRepoError::NotFound { id: id.to_string() })
        }

        fn list_logs(&self, query: &LogQuery) -> std::result::Result<LogList, LogRepoError> {
            let logs = self.logs.lock().unwrap();
            let mut filtered: Vec<_> = logs.iter().cloned().collect();

            if let Some(ref method) = query.method {
                filtered.retain(|l| &l.method == method);
            }

            let total = filtered.len() as u64;
            let per_page = if query.per_page == 0 {
                20
            } else {
                query.per_page
            };
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

        fn get_stats(
            &self,
            _query: &LogStatsQuery,
        ) -> std::result::Result<LogStats, LogRepoError> {
            let logs = self.logs.lock().unwrap();
            let total = logs.len() as u64;
            let mut success = 0u64;
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
                    400..=499 => client_error += 1,
                    500..=599 => server_error += 1,
                    _ => {}
                }
            }

            Ok(LogStats {
                total_requests: total,
                status_counts: StatusCounts {
                    success,
                    redirect: 0,
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

        fn cleanup_old_logs(&self, _retention_days: u32) -> std::result::Result<u64, LogRepoError> {
            // Mock: just clear all logs
            let mut logs = self.logs.lock().unwrap();
            let count = logs.len() as u64;
            logs.clear();
            Ok(count)
        }
    }

    fn sample_entry(id: &str, method: &str, status: u16) -> LogEntry {
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
            created: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn create_and_get_log() {
        let repo = Arc::new(MockLogRepo::new());
        let svc = LogService::new(repo, 7);
        let entry = sample_entry("log1", "GET", 200);

        svc.create(&entry).unwrap();
        let retrieved = svc.get("log1").unwrap();
        assert_eq!(retrieved.id, "log1");
        assert_eq!(retrieved.method, "GET");
        assert_eq!(retrieved.status, 200);
    }

    #[test]
    fn get_nonexistent_log_returns_not_found() {
        let repo = Arc::new(MockLogRepo::new());
        let svc = LogService::new(repo, 7);

        let result = svc.get("nonexistent");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 404);
    }

    #[test]
    fn list_logs_with_method_filter() {
        let repo = Arc::new(MockLogRepo::new());
        let svc = LogService::new(repo, 7);

        svc.create(&sample_entry("l1", "GET", 200)).unwrap();
        svc.create(&sample_entry("l2", "POST", 201)).unwrap();
        svc.create(&sample_entry("l3", "GET", 200)).unwrap();

        let query = LogQuery {
            method: Some("GET".to_string()),
            per_page: 20,
            page: 1,
            ..Default::default()
        };

        let list = svc.list(&query).unwrap();
        assert_eq!(list.total_items, 2);
        assert!(list.items.iter().all(|l| l.method == "GET"));
    }

    #[test]
    fn stats_calculates_correctly() {
        let repo = Arc::new(MockLogRepo::new());
        let svc = LogService::new(repo, 7);

        svc.create(&sample_entry("l1", "GET", 200)).unwrap();
        svc.create(&sample_entry("l2", "POST", 201)).unwrap();
        svc.create(&sample_entry("l3", "GET", 404)).unwrap();
        svc.create(&sample_entry("l4", "DELETE", 500)).unwrap();

        let stats = svc.stats(&LogStatsQuery::default()).unwrap();
        assert_eq!(stats.total_requests, 4);
        assert_eq!(stats.status_counts.success, 2);
        assert_eq!(stats.status_counts.client_error, 1);
        assert_eq!(stats.status_counts.server_error, 1);
    }

    #[test]
    fn cleanup_removes_logs() {
        let repo = Arc::new(MockLogRepo::new());
        let svc = LogService::new(repo, 7);

        svc.create(&sample_entry("l1", "GET", 200)).unwrap();
        svc.create(&sample_entry("l2", "POST", 201)).unwrap();

        let deleted = svc.cleanup().unwrap();
        assert_eq!(deleted, 2);
    }

    #[test]
    fn retention_days_accessor() {
        let repo = Arc::new(MockLogRepo::new());
        let svc = LogService::new(repo, 30);
        assert_eq!(svc.retention_days(), 30);
    }
}
