//! Log repository implementation for [`Database`].
//!
//! Implements [`LogRepository`] on `Database` to persist request logs
//! in the `_logs` SQLite table created by migration v5.

use rusqlite::params;

use zerobase_core::services::log_service::{
    LogEntry, LogList, LogQuery, LogRepoError, LogRepository, LogStats, LogStatsQuery,
    StatusCounts, TimelineEntry,
};

use crate::pool::Database;

impl LogRepository for Database {
    fn create_log(&self, entry: &LogEntry) -> std::result::Result<(), LogRepoError> {
        self.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO _logs (id, method, url, status, ip, auth_id, duration_ms, user_agent, request_id, created)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    entry.id,
                    entry.method,
                    entry.url,
                    entry.status,
                    entry.ip,
                    entry.auth_id,
                    entry.duration_ms,
                    entry.user_agent,
                    entry.request_id,
                    entry.created,
                ],
            )
            .map_err(crate::error::DbError::Query)?;
            Ok(())
        })
        .map_err(|e| LogRepoError::OperationFailed {
            message: format!("{e}"),
        })
    }

    fn get_log(&self, id: &str) -> std::result::Result<LogEntry, LogRepoError> {
        let conn = self.read_conn().map_err(|e| LogRepoError::OperationFailed {
            message: format!("{e}"),
        })?;
        conn.query_row(
            "SELECT id, method, url, status, ip, auth_id, duration_ms, user_agent, request_id, created
             FROM _logs WHERE id = ?1",
            params![id],
            row_to_entry,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => LogRepoError::NotFound { id: id.to_string() },
            _ => LogRepoError::OperationFailed {
                message: format!("{e}"),
            },
        })
    }

    fn list_logs(&self, query: &LogQuery) -> std::result::Result<LogList, LogRepoError> {
        let conn = self.read_conn().map_err(|e| LogRepoError::OperationFailed {
            message: format!("{e}"),
        })?;

        let mut where_clauses = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref method) = query.method {
            param_values.push(Box::new(method.clone()));
            where_clauses.push(format!("method = ?{}", param_values.len()));
        }
        if let Some(ref url) = query.url {
            param_values.push(Box::new(format!("%{url}%")));
            where_clauses.push(format!("url LIKE ?{}", param_values.len()));
        }
        if let Some(min) = query.status_min {
            param_values.push(Box::new(min));
            where_clauses.push(format!("status >= ?{}", param_values.len()));
        }
        if let Some(max) = query.status_max {
            param_values.push(Box::new(max));
            where_clauses.push(format!("status <= ?{}", param_values.len()));
        }
        if let Some(ref auth_id) = query.auth_id {
            param_values.push(Box::new(auth_id.clone()));
            where_clauses.push(format!("auth_id = ?{}", param_values.len()));
        }
        if let Some(ref ip) = query.ip {
            param_values.push(Box::new(ip.clone()));
            where_clauses.push(format!("ip = ?{}", param_values.len()));
        }
        if let Some(ref after) = query.created_after {
            param_values.push(Box::new(after.clone()));
            where_clauses.push(format!("created >= ?{}", param_values.len()));
        }
        if let Some(ref before) = query.created_before {
            param_values.push(Box::new(before.clone()));
            where_clauses.push(format!("created <= ?{}", param_values.len()));
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        // Sort
        let sort_sql = match query.sort.as_deref() {
            Some(s) if s.starts_with('-') => format!("ORDER BY \"{}\" DESC", &s[1..]),
            Some(s) if s.starts_with('+') => format!("ORDER BY \"{}\" ASC", &s[1..]),
            Some(s) => format!("ORDER BY \"{s}\" DESC"),
            None => "ORDER BY created DESC".to_string(),
        };

        // Count
        let count_sql = format!("SELECT COUNT(*) FROM _logs {where_sql}");
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let total: u64 = conn
            .query_row(&count_sql, params_ref.as_slice(), |row| row.get(0))
            .map_err(|e| LogRepoError::OperationFailed {
                message: format!("{e}"),
            })?;

        let per_page = if query.per_page == 0 { 20 } else { query.per_page };
        let page = if query.page == 0 { 1 } else { query.page };
        let total_pages = ((total as f64) / (per_page as f64)).ceil() as u32;
        let offset = ((page - 1) * per_page) as u64;

        let select_sql = format!(
            "SELECT id, method, url, status, ip, auth_id, duration_ms, user_agent, request_id, created
             FROM _logs {where_sql} {sort_sql} LIMIT ?{} OFFSET ?{}",
            param_values.len() + 1,
            param_values.len() + 2,
        );

        let mut params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let limit_val = per_page as i64;
        let offset_val = offset as i64;
        params_ref.push(&limit_val);
        params_ref.push(&offset_val);

        let mut stmt = conn.prepare(&select_sql).map_err(|e| LogRepoError::OperationFailed {
            message: format!("{e}"),
        })?;
        let items: Vec<LogEntry> = stmt
            .query_map(params_ref.as_slice(), row_to_entry)
            .map_err(|e| LogRepoError::OperationFailed {
                message: format!("{e}"),
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| LogRepoError::OperationFailed {
                message: format!("{e}"),
            })?;

        Ok(LogList {
            items,
            total_items: total,
            page,
            per_page,
            total_pages,
        })
    }

    fn get_stats(&self, query: &LogStatsQuery) -> std::result::Result<LogStats, LogRepoError> {
        let conn = self.read_conn().map_err(|e| LogRepoError::OperationFailed {
            message: format!("{e}"),
        })?;

        let mut where_clauses = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref after) = query.created_after {
            param_values.push(Box::new(after.clone()));
            where_clauses.push(format!("created >= ?{}", param_values.len()));
        }
        if let Some(ref before) = query.created_before {
            param_values.push(Box::new(before.clone()));
            where_clauses.push(format!("created <= ?{}", param_values.len()));
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        // Aggregate stats
        let stats_sql = format!(
            "SELECT
                COUNT(*) as total,
                COALESCE(SUM(CASE WHEN status >= 200 AND status < 300 THEN 1 ELSE 0 END), 0) as success,
                COALESCE(SUM(CASE WHEN status >= 300 AND status < 400 THEN 1 ELSE 0 END), 0) as redirect,
                COALESCE(SUM(CASE WHEN status >= 400 AND status < 500 THEN 1 ELSE 0 END), 0) as client_error,
                COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_error,
                COALESCE(AVG(duration_ms), 0) as avg_duration,
                COALESCE(MAX(duration_ms), 0) as max_duration
             FROM _logs {where_sql}"
        );

        let (total_requests, success, redirect, client_error, server_error, avg_duration_ms, max_duration_ms) = conn
            .query_row(&stats_sql, params_ref.as_slice(), |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, u64>(4)?,
                    row.get::<_, f64>(5)?,
                    row.get::<_, u64>(6)?,
                ))
            })
            .map_err(|e| LogRepoError::OperationFailed {
                message: format!("{e}"),
            })?;

        // Timeline
        let group_by = query.group_by.as_deref().unwrap_or("day");
        let date_expr = match group_by {
            "hour" => "strftime('%Y-%m-%dT%H:00:00Z', created)",
            "month" => "strftime('%Y-%m-01', created)",
            _ => "strftime('%Y-%m-%d', created)", // day
        };

        let timeline_sql = format!(
            "SELECT {date_expr} as date_bucket, COUNT(*) as cnt
             FROM _logs {where_sql}
             GROUP BY date_bucket
             ORDER BY date_bucket ASC"
        );

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&timeline_sql).map_err(|e| LogRepoError::OperationFailed {
            message: format!("{e}"),
        })?;
        let timeline: Vec<TimelineEntry> = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok(TimelineEntry {
                    date: row.get(0)?,
                    total: row.get(1)?,
                })
            })
            .map_err(|e| LogRepoError::OperationFailed {
                message: format!("{e}"),
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| LogRepoError::OperationFailed {
                message: format!("{e}"),
            })?;

        Ok(LogStats {
            total_requests,
            status_counts: StatusCounts {
                success,
                redirect,
                client_error,
                server_error,
            },
            avg_duration_ms,
            max_duration_ms,
            timeline,
        })
    }

    fn cleanup_old_logs(&self, retention_days: u32) -> std::result::Result<u64, LogRepoError> {
        self.with_write_conn(|conn| {
            let deleted = conn
                .execute(
                    "DELETE FROM _logs WHERE created < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?1)",
                    params![format!("-{retention_days} days")],
                )
                .map_err(crate::error::DbError::Query)?;
            Ok(deleted as u64)
        })
        .map_err(|e| LogRepoError::OperationFailed {
            message: format!("{e}"),
        })
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<LogEntry> {
    Ok(LogEntry {
        id: row.get(0)?,
        method: row.get(1)?,
        url: row.get(2)?,
        status: row.get(3)?,
        ip: row.get(4)?,
        auth_id: row.get(5)?,
        duration_ms: row.get(6)?,
        user_agent: row.get(7)?,
        request_id: row.get(8)?,
        created: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::PoolConfig;

    fn test_db() -> Database {
        let db = Database::open_in_memory(&PoolConfig::default()).unwrap();
        db.run_system_migrations().unwrap();
        db
    }

    fn sample_entry(id: &str) -> LogEntry {
        LogEntry {
            id: id.to_string(),
            method: "GET".to_string(),
            url: "/api/health".to_string(),
            status: 200,
            ip: "127.0.0.1".to_string(),
            auth_id: String::new(),
            duration_ms: 5,
            user_agent: "test-agent".to_string(),
            request_id: "req-001".to_string(),
            created: "2024-01-15T10:30:00.000Z".to_string(),
        }
    }

    #[test]
    fn create_and_get_log() {
        let db = test_db();
        let entry = sample_entry("log1");
        db.create_log(&entry).unwrap();

        let retrieved = db.get_log("log1").unwrap();
        assert_eq!(retrieved.id, "log1");
        assert_eq!(retrieved.method, "GET");
        assert_eq!(retrieved.status, 200);
    }

    #[test]
    fn get_log_not_found() {
        let db = test_db();
        let err = db.get_log("missing").unwrap_err();
        assert!(matches!(err, LogRepoError::NotFound { .. }));
    }

    #[test]
    fn list_logs_with_filter() {
        let db = test_db();
        db.create_log(&LogEntry {
            method: "POST".to_string(),
            ..sample_entry("l1")
        })
        .unwrap();
        db.create_log(&sample_entry("l2")).unwrap();
        db.create_log(&sample_entry("l3")).unwrap();

        let query = LogQuery {
            method: Some("GET".to_string()),
            page: 1,
            per_page: 20,
            ..Default::default()
        };
        let list = db.list_logs(&query).unwrap();
        assert_eq!(list.total_items, 2);
        assert!(list.items.iter().all(|l| l.method == "GET"));
    }

    #[test]
    fn stats_aggregate() {
        let db = test_db();
        db.create_log(&sample_entry("l1")).unwrap();
        db.create_log(&LogEntry {
            id: "l2".to_string(),
            status: 404,
            ..sample_entry("l2")
        })
        .unwrap();
        db.create_log(&LogEntry {
            id: "l3".to_string(),
            status: 500,
            ..sample_entry("l3")
        })
        .unwrap();

        let stats = db.get_stats(&LogStatsQuery::default()).unwrap();
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.status_counts.success, 1);
        assert_eq!(stats.status_counts.client_error, 1);
        assert_eq!(stats.status_counts.server_error, 1);
    }

    #[test]
    fn cleanup_old_logs() {
        let db = test_db();
        // Insert logs with old timestamp
        db.create_log(&LogEntry {
            created: "2020-01-01T00:00:00.000Z".to_string(),
            ..sample_entry("old1")
        })
        .unwrap();
        db.create_log(&sample_entry("recent1")).unwrap();

        let deleted = db.cleanup_old_logs(1).unwrap();
        assert!(deleted >= 1);
    }

    #[test]
    fn list_logs_pagination() {
        let db = test_db();
        for i in 0..5 {
            db.create_log(&sample_entry(&format!("p{i}"))).unwrap();
        }

        let query = LogQuery {
            page: 1,
            per_page: 2,
            ..Default::default()
        };
        let list = db.list_logs(&query).unwrap();
        assert_eq!(list.items.len(), 2);
        assert_eq!(list.total_items, 5);
        assert_eq!(list.total_pages, 3);
        assert_eq!(list.page, 1);
    }
}
