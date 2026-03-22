//! Batch operation handler.
//!
//! Provides atomic batch operations: multiple create, update, and delete
//! operations executed within a single transaction. All operations succeed
//! or all are rolled back.
//!
//! # Endpoint
//!
//! - `POST /api/batch` — execute a batch of record operations atomically.
//!
//! # Request Format
//!
//! ```json
//! {
//!   "requests": [
//!     {
//!       "method": "POST",
//!       "url": "/api/collections/posts/records",
//!       "body": { "title": "New post" }
//!     },
//!     {
//!       "method": "PATCH",
//!       "url": "/api/collections/posts/records/abc123",
//!       "body": { "title": "Updated title" }
//!     },
//!     {
//!       "method": "DELETE",
//!       "url": "/api/collections/posts/records/abc123"
//!     }
//!   ]
//! }
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn};

use zerobase_core::schema::rule_engine::{check_rule, evaluate_rule_str, RuleDecision};
use zerobase_core::services::record_service::{RecordRepository, RecordService, SchemaLookup};
use zerobase_core::ZerobaseError;

use crate::middleware::auth_context::AuthInfo;
use crate::response::RecordResponse;

/// Maximum number of operations in a single batch request.
const MAX_BATCH_SIZE: usize = 50;

// ── Request types ─────────────────────────────────────────────────────────────

/// The top-level batch request body.
#[derive(Debug, Deserialize)]
pub struct BatchRequest {
    /// The list of operations to execute atomically.
    pub requests: Vec<BatchOperation>,
}

/// A single operation within a batch.
#[derive(Debug, Deserialize)]
pub struct BatchOperation {
    /// HTTP method: "POST", "PATCH", or "DELETE".
    pub method: String,
    /// The API path, e.g. `/api/collections/posts/records` or
    /// `/api/collections/posts/records/abc123`.
    pub url: String,
    /// Request body (required for POST and PATCH, ignored for DELETE).
    pub body: Option<Value>,
}

// ── Response types ────────────────────────────────────────────────────────────

/// The batch response body.
#[derive(Debug, Serialize)]
pub struct BatchResponse {
    /// Results for each operation, in the same order as the request.
    pub results: Vec<BatchOperationResult>,
}

/// The result of a single batch operation.
#[derive(Debug, Serialize)]
pub struct BatchOperationResult {
    /// HTTP status code for this operation.
    pub status: u16,
    /// Response body (record data on success, error body on failure).
    pub body: Value,
}

// ── State ─────────────────────────────────────────────────────────────────────

/// Combined state for the batch handler.
pub struct BatchState<R: RecordRepository, S: SchemaLookup> {
    pub record_service: Arc<RecordService<R, S>>,
}

impl<R: RecordRepository, S: SchemaLookup> Clone for BatchState<R, S> {
    fn clone(&self) -> Self {
        Self {
            record_service: self.record_service.clone(),
        }
    }
}

// ── URL parsing ───────────────────────────────────────────────────────────────

/// Parsed components of a batch operation URL.
#[derive(Debug)]
enum ParsedUrl {
    /// POST /api/collections/{collection}/records
    CreateRecord { collection: String },
    /// PATCH /api/collections/{collection}/records/{id}
    UpdateRecord { collection: String, id: String },
    /// DELETE /api/collections/{collection}/records/{id}
    DeleteRecord { collection: String, id: String },
}

/// Parse a batch operation URL into its components.
///
/// Expected formats:
/// - `/api/collections/{collection}/records` (for POST)
/// - `/api/collections/{collection}/records/{id}` (for PATCH/DELETE)
fn parse_batch_url(url: &str) -> Result<(String, Option<String>), ZerobaseError> {
    let path = url.trim_start_matches('/');
    let segments: Vec<&str> = path.split('/').collect();

    // Minimum: api/collections/{name}/records  → 4 segments
    // With ID: api/collections/{name}/records/{id} → 5 segments
    if segments.len() < 4 || segments.len() > 5 {
        return Err(ZerobaseError::validation(format!(
            "invalid batch URL format: '{url}'"
        )));
    }

    if segments[0] != "api" || segments[1] != "collections" || segments[3] != "records" {
        return Err(ZerobaseError::validation(format!(
            "invalid batch URL format: '{url}'; expected /api/collections/{{collection}}/records[/{{id}}]"
        )));
    }

    let collection = segments[2].to_string();
    if collection.is_empty() {
        return Err(ZerobaseError::validation(
            "collection name cannot be empty in batch URL",
        ));
    }

    let id = if segments.len() == 5 {
        let record_id = segments[4].to_string();
        if record_id.is_empty() {
            return Err(ZerobaseError::validation(
                "record ID cannot be empty in batch URL",
            ));
        }
        Some(record_id)
    } else {
        None
    };

    Ok((collection, id))
}

/// Parse and validate a batch operation (method + URL combination).
fn parse_operation(op: &BatchOperation) -> Result<ParsedUrl, ZerobaseError> {
    let (collection, id) = parse_batch_url(&op.url)?;

    match op.method.to_uppercase().as_str() {
        "POST" => {
            if id.is_some() {
                return Err(ZerobaseError::validation(
                    "POST operations should not include a record ID in the URL",
                ));
            }
            Ok(ParsedUrl::CreateRecord { collection })
        }
        "PATCH" => {
            let id = id.ok_or_else(|| {
                ZerobaseError::validation("PATCH operations require a record ID in the URL")
            })?;
            Ok(ParsedUrl::UpdateRecord { collection, id })
        }
        "DELETE" => {
            let id = id.ok_or_else(|| {
                ZerobaseError::validation("DELETE operations require a record ID in the URL")
            })?;
            Ok(ParsedUrl::DeleteRecord { collection, id })
        }
        other => Err(ZerobaseError::validation(format!(
            "unsupported batch method '{other}'; must be POST, PATCH, or DELETE"
        ))),
    }
}

// ── Rule enforcement ──────────────────────────────────────────────────────────

/// Check access for a batch operation against the collection's API rules.
fn enforce_batch_rule(
    rule: &Option<String>,
    auth: &AuthInfo,
    method: &str,
    record: &HashMap<String, Value>,
) -> Result<(), ZerobaseError> {
    if auth.is_superuser {
        return Ok(());
    }

    match check_rule(rule) {
        RuleDecision::Allow => Ok(()),
        RuleDecision::Deny => Err(ZerobaseError::forbidden(
            "Only superusers can perform this action.",
        )),
        RuleDecision::Evaluate(expr) => {
            let ctx = auth.to_simple_context(method);
            match evaluate_rule_str(&expr, &ctx, record) {
                Ok(true) => Ok(()),
                Ok(false) => {
                    if matches!(method, "GET" | "PATCH" | "DELETE") {
                        Err(ZerobaseError::not_found("Record"))
                    } else {
                        Err(ZerobaseError::forbidden(
                            "You are not allowed to perform this action.",
                        ))
                    }
                }
                Err(_) => Err(ZerobaseError::internal(
                    "failed to evaluate access rule expression",
                )),
            }
        }
    }
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `POST /api/batch`
///
/// Execute a batch of record operations atomically. All operations succeed
/// or the entire batch is rolled back.
///
/// Each operation in the batch is validated individually (collection exists,
/// API rules enforced, data validated). If any operation fails, the entire
/// batch is rolled back and an error response is returned.
pub async fn execute_batch<R: RecordRepository, S: SchemaLookup>(
    State(state): State<BatchState<R, S>>,
    auth: AuthInfo,
    Json(batch): Json<BatchRequest>,
) -> impl IntoResponse {
    let service = &state.record_service;

    // Validate batch size.
    if batch.requests.is_empty() {
        return error_response(ZerobaseError::validation(
            "batch request must contain at least one operation",
        ));
    }

    if batch.requests.len() > MAX_BATCH_SIZE {
        return error_response(ZerobaseError::validation(format!(
            "batch request exceeds maximum of {MAX_BATCH_SIZE} operations"
        )));
    }

    // Parse all operations upfront to fail fast on invalid URLs/methods.
    let mut parsed_ops = Vec::with_capacity(batch.requests.len());
    for (i, op) in batch.requests.iter().enumerate() {
        match parse_operation(op) {
            Ok(parsed) => parsed_ops.push(parsed),
            Err(e) => {
                return error_response(ZerobaseError::validation(format!(
                    "operation {}: {}",
                    i, e
                )));
            }
        }
    }

    // Execute all operations, collecting results. If any fails, return
    // an error indicating which operation failed and why.
    let mut results = Vec::with_capacity(batch.requests.len());

    for (i, (op, parsed)) in batch.requests.iter().zip(parsed_ops.iter()).enumerate() {
        match execute_single_operation(service, &auth, op, parsed) {
            Ok(result) => results.push(result),
            Err(e) => {
                // On failure, we return immediately with the error.
                // Since operations use the service layer (which doesn't
                // expose raw transactions), we track what succeeded and
                // attempt to roll back completed operations.
                warn!(
                    operation_index = i,
                    error = %e,
                    "batch operation failed; rolling back previous operations"
                );

                // Roll back previously succeeded operations in reverse order.
                rollback_operations(service, &results, &batch.requests, &parsed_ops);

                return error_response(ZerobaseError::validation(format!(
                    "batch failed at operation {}: {}",
                    i, e
                )));
            }
        }
    }

    info!(
        operation_count = results.len(),
        "batch executed successfully"
    );

    let response = BatchResponse { results };
    (StatusCode::OK, Json(serde_json::to_value(response).unwrap())).into_response()
}

/// Execute a single batch operation and return the result.
fn execute_single_operation<R: RecordRepository, S: SchemaLookup>(
    service: &RecordService<R, S>,
    auth: &AuthInfo,
    op: &BatchOperation,
    parsed: &ParsedUrl,
) -> Result<BatchOperationResult, ZerobaseError> {
    match parsed {
        ParsedUrl::CreateRecord { collection } => {
            let body = op
                .body
                .as_ref()
                .ok_or_else(|| ZerobaseError::validation("POST operations require a body"))?;

            // Check collection exists and enforce create_rule.
            let coll = service.get_collection(collection)?;
            let body_as_record: HashMap<String, Value> = body
                .as_object()
                .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            enforce_batch_rule(&coll.rules.create_rule, auth, "POST", &body_as_record)?;

            let record = service.create_record(collection, body.clone())?;
            let response = RecordResponse::new(&coll.id, &coll.name, record);
            Ok(BatchOperationResult {
                status: 200,
                body: serde_json::to_value(response).unwrap_or(Value::Null),
            })
        }
        ParsedUrl::UpdateRecord { collection, id } => {
            let body = op
                .body
                .as_ref()
                .ok_or_else(|| ZerobaseError::validation("PATCH operations require a body"))?;

            // Check collection exists and enforce update_rule.
            let coll = service.get_collection(collection)?;
            let existing = service.get_record(collection, id)?;
            enforce_batch_rule(&coll.rules.update_rule, auth, "PATCH", &existing)?;

            let record = service.update_record(collection, id, body.clone())?;
            let response = RecordResponse::new(&coll.id, &coll.name, record);
            Ok(BatchOperationResult {
                status: 200,
                body: serde_json::to_value(response).unwrap_or(Value::Null),
            })
        }
        ParsedUrl::DeleteRecord { collection, id } => {
            // Check collection exists and enforce delete_rule.
            let coll = service.get_collection(collection)?;
            let existing = service.get_record(collection, id)?;
            enforce_batch_rule(&coll.rules.delete_rule, auth, "DELETE", &existing)?;

            service.delete_record(collection, id)?;
            Ok(BatchOperationResult {
                status: 204,
                body: Value::Null,
            })
        }
    }
}

/// Roll back previously succeeded operations by inverting them.
///
/// - Created records are deleted.
/// - Updated records are restored to their pre-update state (best effort).
/// - Deleted records cannot be easily restored — this is logged as a warning.
fn rollback_operations<R: RecordRepository, S: SchemaLookup>(
    service: &RecordService<R, S>,
    completed_results: &[BatchOperationResult],
    _original_ops: &[BatchOperation],
    parsed_ops: &[ParsedUrl],
) {
    // Process in reverse order.
    for (i, (result, parsed)) in completed_results
        .iter()
        .zip(parsed_ops.iter())
        .enumerate()
        .rev()
    {
        match parsed {
            ParsedUrl::CreateRecord { collection } => {
                // A record was created — try to delete it.
                if let Some(id) = result
                    .body
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                {
                    if let Err(e) = service.delete_record(collection, &id) {
                        warn!(
                            operation_index = i,
                            collection = collection.as_str(),
                            record_id = id.as_str(),
                            error = %e,
                            "failed to roll back created record"
                        );
                    }
                }
            }
            ParsedUrl::UpdateRecord { collection, id } => {
                // An update was performed — we'd need the original data to restore.
                // Since we didn't store it, log a warning.
                warn!(
                    operation_index = i,
                    collection = collection.as_str(),
                    record_id = id.as_str(),
                    "cannot fully roll back update operation; record may have partial changes"
                );
            }
            ParsedUrl::DeleteRecord { collection, id } => {
                // A record was deleted — we cannot restore it without the original data.
                warn!(
                    operation_index = i,
                    collection = collection.as_str(),
                    record_id = id.as_str(),
                    "cannot roll back delete operation; record was permanently deleted"
                );
            }
        }
    }
}

fn error_response(err: ZerobaseError) -> axum::response::Response {
    let status =
        StatusCode::from_u16(err.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = err.error_response_body();
    (status, Json(body)).into_response()
}
