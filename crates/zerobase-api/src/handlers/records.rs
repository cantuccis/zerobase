//! Record CRUD handlers.
//!
//! Provides the HTTP handlers for listing, viewing, creating, updating, and
//! deleting records within a collection. Pagination is supported via `page`
//! and `perPage` query parameters.
//!
//! Each handler enforces the collection's API rules:
//! - `None` (null) = locked, superusers only
//! - `Some("")` (empty) = open to everyone
//! - `Some(expr)` = evaluated against request context and record

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{FromRequest, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use zerobase_core::schema::rule_engine::{check_rule, evaluate_rule_str, RuleDecision};
use zerobase_core::schema::FieldType;
use zerobase_core::services::expand::{expand_record, expand_records, parse_expand};
use zerobase_core::services::record_service::{
    RecordQuery, RecordRepository, RecordService, SchemaLookup, DEFAULT_PER_PAGE,
};
use zerobase_core::ZerobaseError;
use zerobase_files::FileService;

use crate::handlers::realtime::RealtimeHub;
use crate::middleware::auth_context::AuthInfo;
use crate::response::{ListResponse, RecordResponse};

use super::multipart::extract_multipart;

/// Query parameters for the list records endpoint.
///
/// Mirrors PocketBase's query parameter naming with camelCase.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRecordsParams {
    /// Page number (1-based). Defaults to 1.
    pub page: Option<u32>,
    /// Items per page. Defaults to 30, max 500.
    pub per_page: Option<u32>,
    /// Sort expression: comma-separated fields, prefix `-` for descending.
    pub sort: Option<String>,
    /// PocketBase-style filter expression.
    pub filter: Option<String>,
    /// Comma-separated field names to include in response. `id` always included.
    pub fields: Option<String>,
    /// Full-text search query. Searches across fields marked as `searchable`.
    /// Results are ranked by relevance when no explicit sort is provided.
    pub search: Option<String>,
    /// Comma-separated relation field names to expand in the response.
    /// Supports dot-notation for nested expansion: `author.profile`.
    /// Back-relations use the `<collection>_via_<field>` pattern.
    pub expand: Option<String>,
}

impl ListRecordsParams {
    /// Convert query parameters into a validated [`RecordQuery`].
    pub fn into_record_query(self) -> Result<RecordQuery, ZerobaseError> {
        let sort = match &self.sort {
            Some(s) => zerobase_core::services::record_service::parse_sort(s)?,
            None => Vec::new(),
        };

        let fields = match &self.fields {
            Some(f) => {
                let parsed = zerobase_core::services::record_service::parse_fields(f)?;
                if parsed.is_empty() {
                    None
                } else {
                    Some(parsed)
                }
            }
            None => None,
        };

        // Sanitize search: trim whitespace, treat empty as None.
        let search = self.search.and_then(|s| {
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });

        Ok(RecordQuery {
            page: self.page.unwrap_or(1),
            per_page: self.per_page.unwrap_or(DEFAULT_PER_PAGE),
            sort,
            filter: self.filter,
            fields,
            search,
        })
    }
}

/// Query parameters for view/single-record endpoints that support field selection.
#[derive(Debug, Deserialize)]
pub struct FieldsParam {
    /// Comma-separated field names to include in response. `id` always included.
    pub fields: Option<String>,
    /// Comma-separated relation field names to expand in the response.
    pub expand: Option<String>,
}

/// Query parameters for the count records endpoint.
#[derive(Debug, Deserialize)]
pub struct CountRecordsParams {
    /// PocketBase-style filter expression.
    pub filter: Option<String>,
}

// ── State ─────────────────────────────────────────────────────────────────────

/// Combined state for record handlers: record service + optional file service + optional realtime hub.
pub struct RecordState<R: RecordRepository, S: SchemaLookup> {
    pub record_service: Arc<RecordService<R, S>>,
    pub file_service: Option<Arc<FileService>>,
    pub realtime_hub: Option<RealtimeHub>,
}

impl<R: RecordRepository, S: SchemaLookup> Clone for RecordState<R, S> {
    fn clone(&self) -> Self {
        Self {
            record_service: self.record_service.clone(),
            file_service: self.file_service.clone(),
            realtime_hub: self.realtime_hub.clone(),
        }
    }
}

// ── File helpers ──────────────────────────────────────────────────────────────

/// Merge uploaded file names into the record data JSON object.
///
/// For single-file fields (`max_select == 1`), the value is stored as a string.
/// For multi-file fields, the value is stored as a JSON array of strings,
/// appending to any existing filenames already present.
fn merge_file_names(
    data: &mut Value,
    uploaded: Vec<(String, String)>,
    fields: &[zerobase_core::schema::Field],
) {
    let obj = match data.as_object_mut() {
        Some(o) => o,
        None => return,
    };

    // Group uploads by field name.
    let mut by_field: HashMap<String, Vec<String>> = HashMap::new();
    for (field_name, filename) in uploaded {
        by_field.entry(field_name).or_default().push(filename);
    }

    for (field_name, filenames) in by_field {
        let is_multi = fields
            .iter()
            .find(|f| f.name == field_name)
            .and_then(|f| match &f.field_type {
                FieldType::File(opts) => Some(opts.max_select != 1),
                _ => None,
            })
            .unwrap_or(false);

        if is_multi {
            // Append to existing array or create a new one.
            let existing = obj
                .get(&field_name)
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let mut all: Vec<Value> = existing;
            for name in filenames {
                all.push(Value::String(name));
            }
            obj.insert(field_name, Value::Array(all));
        } else {
            // Single file: last upload wins.
            if let Some(name) = filenames.into_iter().last() {
                obj.insert(field_name, Value::String(name));
            }
        }
    }
}

/// Collect filenames currently stored in file-type fields of a record.
fn collect_existing_filenames(
    record: &HashMap<String, Value>,
    fields: &[zerobase_core::schema::Field],
) -> Vec<String> {
    let mut names = Vec::new();
    for field in fields {
        if !matches!(&field.field_type, FieldType::File(_)) {
            continue;
        }
        if let Some(val) = record.get(&field.name) {
            match val {
                Value::String(s) if !s.is_empty() => names.push(s.clone()),
                Value::Array(arr) => {
                    for item in arr {
                        if let Value::String(s) = item {
                            if !s.is_empty() {
                                names.push(s.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    names
}

// ── Rule enforcement helpers ──────────────────────────────────────────────────

use zerobase_core::schema::ApiRules;

/// Check whether the user matches the collection's `manage_rule`.
///
/// If the manage_rule is set and the user satisfies it, the user gets full
/// CRUD access to all records in the collection, bypassing individual
/// operation rules. This enables delegated administration.
///
/// Returns `true` if the user is granted manage access.
fn user_has_manage_access(
    rules: &ApiRules,
    auth: &AuthInfo,
    method: &str,
    record: &HashMap<String, Value>,
) -> bool {
    // Superusers already bypass everything; no need to check manage_rule.
    if auth.is_superuser {
        return true;
    }

    match check_rule(&rules.manage_rule) {
        RuleDecision::Allow => {
            // Empty manage_rule = any request is granted manage access.
            // However, we still require the caller to be authenticated,
            // matching PocketBase's behaviour.
            auth.is_authenticated()
        }
        RuleDecision::Deny => false, // No manage_rule set.
        RuleDecision::Evaluate(expr) => {
            let ctx = auth.to_simple_context(method);
            evaluate_rule_str(&expr, &ctx, record).unwrap_or(false)
        }
    }
}

/// Check a rule and enforce access. Returns `Ok(())` if access is allowed,
/// `Err` with a 403 or appropriate error if denied.
///
/// Before checking the individual operation rule, this function first checks
/// the collection's `manage_rule`. If the user matches it, the operation is
/// allowed regardless of the individual rule.
///
/// For `Deny` (null rule): only superusers may proceed.
/// For `Allow` (empty rule): everyone may proceed.
/// For `Evaluate(expr)`: the expression is evaluated against the request context
/// and the target record.
fn enforce_rule_on_record(
    rule: &Option<String>,
    rules: &ApiRules,
    auth: &AuthInfo,
    method: &str,
    record: &HashMap<String, Value>,
) -> Result<(), ZerobaseError> {
    // Superusers bypass all rules.
    if auth.is_superuser {
        return Ok(());
    }

    // Check manage_rule first — if matched, bypass the individual rule.
    if user_has_manage_access(rules, auth, method, record) {
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
                    // PocketBase returns 404 for view/update/delete to hide existence.
                    if matches!(method, "GET" | "PATCH" | "DELETE") {
                        Err(ZerobaseError::not_found("Record"))
                    } else {
                        Err(ZerobaseError::forbidden(
                            "You are not allowed to perform this action.",
                        ))
                    }
                }
                Err(_parse_err) => Err(ZerobaseError::internal(
                    "failed to evaluate access rule expression",
                )),
            }
        }
    }
}

/// Check a rule for operations that don't have a target record yet (e.g. create,
/// list). For `Evaluate`, uses an empty record context.
fn enforce_rule_no_record(
    rule: &Option<String>,
    rules: &ApiRules,
    auth: &AuthInfo,
    method: &str,
) -> Result<(), ZerobaseError> {
    enforce_rule_on_record(rule, rules, auth, method, &HashMap::new())
}

/// For list operations, a `Deny` rule means the request should return an empty
/// list (200) instead of 403, matching PocketBase behaviour.
///
/// If the user matches the `manage_rule`, they always get access to list.
fn check_list_rule(rules: &ApiRules, auth: &AuthInfo) -> ListRuleResult {
    if auth.is_superuser {
        return ListRuleResult::Proceed;
    }

    // Manage access grants full list access.
    if user_has_manage_access(rules, auth, "GET", &HashMap::new()) {
        return ListRuleResult::Proceed;
    }

    match check_rule(&rules.list_rule) {
        RuleDecision::Allow => ListRuleResult::Proceed,
        RuleDecision::Deny => ListRuleResult::EmptyResult,
        RuleDecision::Evaluate(expr) => {
            // For list operations with expression rules, we check if the user
            // is at least authenticated (expression will be applied as filter).
            // For now, evaluate against empty record to check auth-only rules.
            let ctx = auth.to_simple_context("GET");
            match evaluate_rule_str(&expr, &ctx, &HashMap::new()) {
                Ok(true) => ListRuleResult::Proceed,
                Ok(false) => ListRuleResult::EmptyResult,
                Err(_) => ListRuleResult::EmptyResult,
            }
        }
    }
}

enum ListRuleResult {
    /// Rule allows access — proceed with the query.
    Proceed,
    /// Rule denies access — return empty result set (200 with 0 items).
    EmptyResult,
}

// ── Realtime broadcast helper ─────────────────────────────────────────────────

/// Broadcast a record change event to the realtime hub, if configured.
fn broadcast_change<R: RecordRepository, S: SchemaLookup>(
    state: &RecordState<R, S>,
    collection_name: &str,
    record_id: &str,
    action: &str,
    record: &HashMap<String, Value>,
    rules: &ApiRules,
) {
    if let Some(hub) = &state.realtime_hub {
        hub.broadcast_record_event(collection_name, record_id, action, record, rules);
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `GET /api/collections/:collection_name/records`
///
/// List records with pagination, sorting, and filtering.
/// The collection's `list_rule` is enforced:
/// - Null rule → 200 with empty items (non-superusers)
/// - Empty rule → all records
/// - Expression → evaluated as filter
pub async fn list_records<R: RecordRepository, S: SchemaLookup>(
    State(state): State<RecordState<R, S>>,
    Path(collection_name): Path<String>,
    Query(params): Query<ListRecordsParams>,
    auth: AuthInfo,
) -> impl IntoResponse {
    let service = &state.record_service;

    // Parse expand paths before consuming params.
    let expand_paths = match &params.expand {
        Some(expand_str) => match parse_expand(expand_str) {
            Ok(paths) => paths,
            Err(e) => return error_response(e),
        },
        None => Vec::new(),
    };

    let query = match params.into_record_query() {
        Ok(q) => q,
        Err(e) => return error_response(e),
    };

    // Look up collection for metadata and rules.
    let collection = match service.get_collection(&collection_name) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    // Enforce list_rule (manage_rule checked internally).
    match check_list_rule(&collection.rules, &auth) {
        ListRuleResult::EmptyResult => {
            let response: ListResponse<RecordResponse> = ListResponse {
                page: query.page.max(1),
                per_page: query.per_page,
                total_pages: 1,
                total_items: 0,
                items: vec![],
            };
            return (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
                .into_response();
        }
        ListRuleResult::Proceed => {}
    }

    match service.list_records(&collection_name, &query) {
        Ok(mut record_list) => {
            // Apply relation expansion if requested.
            if !expand_paths.is_empty() {
                if let Err(e) = expand_records(
                    &mut record_list.items,
                    &collection,
                    &expand_paths,
                    service.repo(),
                    service.schema(),
                ) {
                    return error_response(e);
                }
            }

            let response = ListResponse {
                page: record_list.page,
                per_page: record_list.per_page,
                total_pages: record_list.total_pages,
                total_items: record_list.total_items,
                items: record_list
                    .items
                    .into_iter()
                    .map(|data| {
                        RecordResponse::new(collection.id.clone(), collection.name.clone(), data)
                    })
                    .collect(),
            };
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
                .into_response()
        }
        Err(e) => error_response(e),
    }
}

/// `GET /api/collections/:collection_name/records/:id`
///
/// View a single record by its ID. The collection's `view_rule` is enforced.
/// Returns 404 if the rule denies access (hides existence).
pub async fn view_record<R: RecordRepository, S: SchemaLookup>(
    State(state): State<RecordState<R, S>>,
    Path((collection_name, record_id)): Path<(String, String)>,
    Query(params): Query<FieldsParam>,
    auth: AuthInfo,
) -> impl IntoResponse {
    let service = &state.record_service;
    let expand_paths = match &params.expand {
        Some(e) => match parse_expand(e) {
            Ok(paths) => paths,
            Err(e) => return error_response(e),
        },
        None => vec![],
    };
    let fields = match &params.fields {
        Some(f) => match zerobase_core::services::record_service::parse_fields(f) {
            Ok(parsed) if !parsed.is_empty() => Some(parsed),
            Ok(_) => None,
            Err(e) => return error_response(e),
        },
        None => None,
    };

    let collection = match service.get_collection(&collection_name) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    // Fetch the record first, then check the rule against it.
    let mut record =
        match service.get_record_with_fields(&collection_name, &record_id, fields.as_deref()) {
            Ok(r) => r,
            Err(e) => return error_response(e),
        };

    // Enforce view_rule against the fetched record (manage_rule checked internally).
    if let Err(e) = enforce_rule_on_record(
        &collection.rules.view_rule,
        &collection.rules,
        &auth,
        "GET",
        &record,
    ) {
        return error_response(e);
    }

    // Apply relation expansion if requested.
    if !expand_paths.is_empty() {
        let mut visited = std::collections::HashSet::new();
        match expand_record(
            &record,
            &collection,
            &expand_paths,
            service.repo(),
            service.schema(),
            &mut visited,
            0,
        ) {
            Ok(expand_map) if !expand_map.is_empty() => {
                record.insert(
                    "expand".to_string(),
                    serde_json::to_value(&expand_map).unwrap_or(serde_json::Value::Null),
                );
            }
            Err(e) => return error_response(e),
            _ => {}
        }
    }

    let response = RecordResponse::new(collection.id, collection.name, record);
    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap()),
    )
        .into_response()
}

/// `POST /api/collections/:collection_name/records`
///
/// Create a new record. Accepts both `application/json` and `multipart/form-data`.
/// When multipart is used, file parts are uploaded via [`FileService`] and their
/// generated filenames are merged into the record data.
///
/// The collection's `create_rule` is enforced. Returns 403 if denied.
pub async fn create_record<R: RecordRepository, S: SchemaLookup>(
    State(state): State<RecordState<R, S>>,
    Path(collection_name): Path<String>,
    auth: AuthInfo,
    request: axum::extract::Request,
) -> impl IntoResponse {
    let service = &state.record_service;

    let collection = match service.get_collection(&collection_name) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    // Determine content type and extract body + files.
    let is_multipart = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.starts_with("multipart/"))
        .unwrap_or(false);

    let (body, files) = if is_multipart {
        let multipart = match axum::extract::Multipart::from_request(request, &()).await {
            Ok(m) => m,
            Err(e) => {
                return error_response(ZerobaseError::validation(format!(
                    "invalid multipart request: {e}"
                )));
            }
        };
        match extract_multipart(multipart).await {
            Ok(ext) => (ext.data, ext.files),
            Err(e) => return error_response(e),
        }
    } else {
        let bytes = match axum::body::to_bytes(request.into_body(), 100 * 1024 * 1024).await {
            Ok(b) => b,
            Err(e) => {
                return error_response(ZerobaseError::validation(format!(
                    "failed to read request body: {e}"
                )));
            }
        };
        let value: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(e) => {
                return error_response(ZerobaseError::validation(format!("invalid JSON: {e}")));
            }
        };
        (value, Vec::new())
    };

    // Enforce create_rule.
    let body_as_record: HashMap<String, Value> = body
        .as_object()
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    if let Err(e) = enforce_rule_on_record(
        &collection.rules.create_rule,
        &collection.rules,
        &auth,
        "POST",
        &body_as_record,
    ) {
        return error_response(e);
    }

    // Process file uploads if any.
    if !files.is_empty() {
        if let Some(file_service) = &state.file_service {
            // We need to create the record first to get its ID, then upload files.
            // To do this, we create the record, then upload, then update with filenames.
            match service.create_record(&collection_name, body.clone()) {
                Ok(record) => {
                    let record_id = record
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();

                    match file_service
                        .process_uploads(&collection.id, &record_id, files, &collection.fields)
                        .await
                    {
                        Ok(uploaded) => {
                            // Merge file names into the record data and update.
                            let mut updated_body = body;
                            merge_file_names(&mut updated_body, uploaded, &collection.fields);
                            match service.update_record(&collection_name, &record_id, updated_body)
                            {
                                Ok(final_record) => {
                                    broadcast_change(
                                        &state,
                                        &collection_name,
                                        &record_id,
                                        "create",
                                        &final_record,
                                        &collection.rules,
                                    );
                                    let response = RecordResponse::new(
                                        collection.id,
                                        collection.name,
                                        final_record,
                                    );
                                    return (
                                        StatusCode::OK,
                                        Json(serde_json::to_value(response).unwrap()),
                                    )
                                        .into_response();
                                }
                                Err(e) => return error_response(e),
                            }
                        }
                        Err(e) => return error_response(ZerobaseError::from(e)),
                    }
                }
                Err(e) => return error_response(e),
            }
        }
        // If no file service, ignore files and proceed with JSON-only create.
    }

    match service.create_record(&collection_name, body) {
        Ok(record) => {
            let record_id = record
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            broadcast_change(
                &state,
                &collection_name,
                record_id,
                "create",
                &record,
                &collection.rules,
            );
            let response = RecordResponse::new(collection.id, collection.name, record);
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
                .into_response()
        }
        Err(e) => error_response(e),
    }
}

/// `PATCH /api/collections/:collection_name/records/:id`
///
/// Update an existing record. Accepts both `application/json` and
/// `multipart/form-data`. When multipart is used, new files are uploaded and
/// files removed from the field value are deleted from storage.
///
/// The collection's `update_rule` is enforced against the existing record.
/// Returns 404 if denied (hides existence).
pub async fn update_record<R: RecordRepository, S: SchemaLookup>(
    State(state): State<RecordState<R, S>>,
    Path((collection_name, record_id)): Path<(String, String)>,
    auth: AuthInfo,
    request: axum::extract::Request,
) -> impl IntoResponse {
    let service = &state.record_service;

    let collection = match service.get_collection(&collection_name) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    // Fetch existing record to evaluate the rule against it.
    let existing = match service.get_record(&collection_name, &record_id) {
        Ok(r) => r,
        Err(e) => return error_response(e),
    };

    // Enforce update_rule against the existing record (manage_rule checked internally).
    if let Err(e) = enforce_rule_on_record(
        &collection.rules.update_rule,
        &collection.rules,
        &auth,
        "PATCH",
        &existing,
    ) {
        return error_response(e);
    }

    // Determine content type and extract body + files.
    let is_multipart = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.starts_with("multipart/"))
        .unwrap_or(false);

    let (mut body, files) = if is_multipart {
        let multipart = match axum::extract::Multipart::from_request(request, &()).await {
            Ok(m) => m,
            Err(e) => {
                return error_response(ZerobaseError::validation(format!(
                    "invalid multipart request: {e}"
                )));
            }
        };
        match extract_multipart(multipart).await {
            Ok(ext) => (ext.data, ext.files),
            Err(e) => return error_response(e),
        }
    } else {
        let bytes = match axum::body::to_bytes(request.into_body(), 100 * 1024 * 1024).await {
            Ok(b) => b,
            Err(e) => {
                return error_response(ZerobaseError::validation(format!(
                    "failed to read request body: {e}"
                )));
            }
        };
        let value: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(e) => {
                return error_response(ZerobaseError::validation(format!("invalid JSON: {e}")));
            }
        };
        (value, Vec::new())
    };

    // Process file uploads if any.
    if !files.is_empty() {
        if let Some(file_service) = &state.file_service {
            // Collect old filenames before update for cleanup.
            let old_filenames = collect_existing_filenames(&existing, &collection.fields);

            match file_service
                .process_uploads(&collection.id, &record_id, files, &collection.fields)
                .await
            {
                Ok(uploaded) => {
                    merge_file_names(&mut body, uploaded, &collection.fields);

                    match service.update_record(&collection_name, &record_id, body) {
                        Ok(record) => {
                            // Clean up old files that are no longer referenced.
                            // Storage errors are logged but do not block the update response.
                            let new_filenames =
                                collect_existing_filenames(&record, &collection.fields);
                            for old_name in &old_filenames {
                                if !new_filenames.contains(old_name) {
                                    if let Err(e) = file_service
                                        .delete_file(&collection.id, &record_id, old_name)
                                        .await
                                    {
                                        warn!(
                                            collection = %collection_name,
                                            record_id = %record_id,
                                            filename = %old_name,
                                            error = %e,
                                            "failed to clean up orphaned file after record update"
                                        );
                                    }
                                }
                            }

                            broadcast_change(
                                &state,
                                &collection_name,
                                &record_id,
                                "update",
                                &record,
                                &collection.rules,
                            );
                            let response =
                                RecordResponse::new(collection.id, collection.name, record);
                            return (
                                StatusCode::OK,
                                Json(serde_json::to_value(response).unwrap()),
                            )
                                .into_response();
                        }
                        Err(e) => return error_response(e),
                    }
                }
                Err(e) => return error_response(ZerobaseError::from(e)),
            }
        }
    }

    match service.update_record(&collection_name, &record_id, body) {
        Ok(record) => {
            broadcast_change(
                &state,
                &collection_name,
                &record_id,
                "update",
                &record,
                &collection.rules,
            );
            let response = RecordResponse::new(collection.id, collection.name, record);
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
                .into_response()
        }
        Err(e) => error_response(e),
    }
}

/// `DELETE /api/collections/:collection_name/records/:id`
///
/// Delete a record by its ID. The collection's `delete_rule` is enforced
/// against the existing record. Returns 404 if denied (hides existence).
pub async fn delete_record<R: RecordRepository, S: SchemaLookup>(
    State(state): State<RecordState<R, S>>,
    Path((collection_name, record_id)): Path<(String, String)>,
    auth: AuthInfo,
) -> impl IntoResponse {
    let service = &state.record_service;

    let collection = match service.get_collection(&collection_name) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    // Fetch existing record to evaluate the rule against it.
    let existing = match service.get_record(&collection_name, &record_id) {
        Ok(r) => r,
        Err(e) => return error_response(e),
    };

    // Enforce delete_rule against the existing record (manage_rule checked internally).
    if let Err(e) = enforce_rule_on_record(
        &collection.rules.delete_rule,
        &collection.rules,
        &auth,
        "DELETE",
        &existing,
    ) {
        return error_response(e);
    }

    match service.delete_record(&collection_name, &record_id) {
        Ok(()) => {
            // Broadcast delete event with the pre-deletion record data.
            broadcast_change(
                &state,
                &collection_name,
                &record_id,
                "delete",
                &existing,
                &collection.rules,
            );

            // Clean up all files associated with this record.
            // Storage errors are logged but do not block the deletion response.
            if let Some(file_service) = &state.file_service {
                if let Err(e) = file_service
                    .delete_record_files(&collection.id, &record_id)
                    .await
                {
                    warn!(
                        collection = %collection_name,
                        record_id = %record_id,
                        error = %e,
                        "failed to clean up files after record deletion"
                    );
                }
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => error_response(e),
    }
}

/// `GET /api/collections/:collection_name/records/count`
///
/// Return the total number of records in a collection, optionally filtered.
/// The collection's `list_rule` is enforced (same as list endpoint).
pub async fn count_records<R: RecordRepository, S: SchemaLookup>(
    State(state): State<RecordState<R, S>>,
    Path(collection_name): Path<String>,
    Query(params): Query<CountRecordsParams>,
    auth: AuthInfo,
) -> impl IntoResponse {
    let service = &state.record_service;
    let collection = match service.get_collection(&collection_name) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    // Enforce list_rule (count shares the same access as list; manage_rule checked internally).
    match check_list_rule(&collection.rules, &auth) {
        ListRuleResult::EmptyResult => {
            let body = serde_json::json!({ "totalItems": 0 });
            return (StatusCode::OK, Json(body)).into_response();
        }
        ListRuleResult::Proceed => {}
    }

    match service.count_records(&collection_name, params.filter.as_deref()) {
        Ok(total) => {
            let body = serde_json::json!({ "totalItems": total });
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => error_response(e),
    }
}

/// Convert a [`ZerobaseError`] into an axum HTTP response.
fn error_response(err: ZerobaseError) -> axum::response::Response {
    let status =
        StatusCode::from_u16(err.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = err.error_response_body();
    (status, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerobase_core::services::record_service::SortDirection;

    #[test]
    fn list_params_defaults() {
        let params = ListRecordsParams {
            page: None,
            per_page: None,
            sort: None,
            filter: None,
            fields: None,
            search: None,
            expand: None,
        };
        let query = params.into_record_query().unwrap();
        assert_eq!(query.page, 1);
        assert_eq!(query.per_page, DEFAULT_PER_PAGE);
        assert!(query.sort.is_empty());
        assert!(query.filter.is_none());
        assert!(query.fields.is_none());
    }

    #[test]
    fn list_params_custom_values() {
        let params = ListRecordsParams {
            page: Some(3),
            per_page: Some(50),
            sort: Some("-created,title".to_string()),
            filter: Some("status = \"active\"".to_string()),
            fields: None,
            search: None,
            expand: None,
        };
        let query = params.into_record_query().unwrap();
        assert_eq!(query.page, 3);
        assert_eq!(query.per_page, 50);
        assert_eq!(query.sort.len(), 2);
        assert_eq!(query.sort[0], ("created".to_string(), SortDirection::Desc));
        assert_eq!(query.sort[1], ("title".to_string(), SortDirection::Asc));
        assert_eq!(query.filter.unwrap(), "status = \"active\"");
    }

    #[test]
    fn list_params_with_fields() {
        let params = ListRecordsParams {
            page: None,
            per_page: None,
            sort: None,
            filter: None,
            fields: Some("title,views".to_string()),
            search: None,
            expand: None,
        };
        let query = params.into_record_query().unwrap();
        let fields = query.fields.unwrap();
        assert!(fields.contains(&"id".to_string()));
        assert!(fields.contains(&"title".to_string()));
        assert!(fields.contains(&"views".to_string()));
    }

    #[test]
    fn list_params_with_empty_fields_results_in_none() {
        let params = ListRecordsParams {
            page: None,
            per_page: None,
            sort: None,
            filter: None,
            fields: Some("".to_string()),
            search: None,
            expand: None,
        };
        let query = params.into_record_query().unwrap();
        assert!(query.fields.is_none());
    }

    #[test]
    fn list_params_invalid_field_name_returns_error() {
        let params = ListRecordsParams {
            page: None,
            per_page: None,
            sort: None,
            filter: None,
            fields: Some("title; DROP TABLE".to_string()),
            search: None,
            expand: None,
        };
        let err = params.into_record_query().unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn list_params_invalid_sort_returns_error() {
        let params = ListRecordsParams {
            page: None,
            per_page: None,
            sort: Some("-".to_string()),
            filter: None,
            fields: None,
            search: None,
            expand: None,
        };
        let err = params.into_record_query().unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn list_params_deserializes_camel_case() {
        let json = serde_json::json!({
            "page": 2,
            "perPage": 50,
            "sort": "-created",
            "filter": "active = true",
            "fields": "id,title"
        });
        let params: ListRecordsParams = serde_json::from_value(json).unwrap();
        assert_eq!(params.page, Some(2));
        assert_eq!(params.per_page, Some(50));
        assert_eq!(params.sort.as_deref(), Some("-created"));
        assert_eq!(params.filter.as_deref(), Some("active = true"));
        assert_eq!(params.fields.as_deref(), Some("id,title"));
    }

    // ── Rule enforcement unit tests ──────────────────────────────────────

    /// Helper: ApiRules with no manage_rule (default behaviour).
    fn no_manage_rules() -> ApiRules {
        ApiRules::default()
    }

    #[test]
    fn null_rule_denies_non_superuser() {
        let auth = AuthInfo::anonymous();
        let rules = no_manage_rules();
        let result = enforce_rule_no_record(&None, &rules, &auth, "GET");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 403);
    }

    #[test]
    fn null_rule_allows_superuser() {
        let auth = AuthInfo::superuser();
        let rules = no_manage_rules();
        let result = enforce_rule_no_record(&None, &rules, &auth, "GET");
        assert!(result.is_ok());
    }

    #[test]
    fn empty_rule_allows_everyone() {
        let auth = AuthInfo::anonymous();
        let rules = no_manage_rules();
        let result = enforce_rule_no_record(&Some(String::new()), &rules, &auth, "GET");
        assert!(result.is_ok());
    }

    #[test]
    fn expression_rule_allows_matching_auth() {
        let mut record = HashMap::new();
        record.insert("id".to_string(), serde_json::json!("user1"));
        let auth = AuthInfo::authenticated(record);
        let rules = no_manage_rules();
        let rule = Some(r#"@request.auth.id != """#.to_string());
        let result = enforce_rule_no_record(&rule, &rules, &auth, "POST");
        assert!(result.is_ok());
    }

    #[test]
    fn expression_rule_denies_anonymous() {
        let auth = AuthInfo::anonymous();
        let rules = no_manage_rules();
        let rule = Some(r#"@request.auth.id != """#.to_string());
        let result = enforce_rule_no_record(&rule, &rules, &auth, "POST");
        assert!(result.is_err());
    }

    // ── manage_rule tests ────────────────────────────────────────────────

    #[test]
    fn manage_rule_bypasses_locked_rule() {
        // An authenticated user with manage access can bypass a locked (None) rule.
        let mut record = HashMap::new();
        record.insert("id".to_string(), serde_json::json!("admin1"));
        record.insert("role".to_string(), serde_json::json!("admin"));
        let auth = AuthInfo::authenticated(record);

        let rules = ApiRules {
            list_rule: None,   // locked
            view_rule: None,   // locked
            create_rule: None, // locked
            update_rule: None, // locked
            delete_rule: None, // locked
            manage_rule: Some(r#"@request.auth.role = "admin""#.to_string()),
        };

        // All operations should be allowed via manage_rule.
        assert!(enforce_rule_no_record(&rules.create_rule, &rules, &auth, "POST").is_ok());
        assert!(
            enforce_rule_on_record(&rules.view_rule, &rules, &auth, "GET", &HashMap::new()).is_ok()
        );
        assert!(enforce_rule_on_record(
            &rules.update_rule,
            &rules,
            &auth,
            "PATCH",
            &HashMap::new()
        )
        .is_ok());
        assert!(enforce_rule_on_record(
            &rules.delete_rule,
            &rules,
            &auth,
            "DELETE",
            &HashMap::new()
        )
        .is_ok());
    }

    #[test]
    fn manage_rule_does_not_match_wrong_role() {
        let mut record = HashMap::new();
        record.insert("id".to_string(), serde_json::json!("user1"));
        record.insert("role".to_string(), serde_json::json!("viewer"));
        let auth = AuthInfo::authenticated(record);

        let rules = ApiRules {
            create_rule: None, // locked
            manage_rule: Some(r#"@request.auth.role = "admin""#.to_string()),
            ..Default::default()
        };

        // Should be denied — manage_rule doesn't match and create_rule is locked.
        let result = enforce_rule_no_record(&rules.create_rule, &rules, &auth, "POST");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status_code(), 403);
    }

    #[test]
    fn manage_rule_empty_string_requires_authentication() {
        // Empty manage_rule means any authenticated user can manage.
        let rules = ApiRules {
            create_rule: None, // locked
            manage_rule: Some(String::new()),
            ..Default::default()
        };

        // Anonymous should NOT get manage access.
        let anon = AuthInfo::anonymous();
        let result = enforce_rule_no_record(&rules.create_rule, &rules, &anon, "POST");
        assert!(result.is_err());

        // Authenticated user should get manage access.
        let mut record = HashMap::new();
        record.insert("id".to_string(), serde_json::json!("user1"));
        let authed = AuthInfo::authenticated(record);
        let result = enforce_rule_no_record(&rules.create_rule, &rules, &authed, "POST");
        assert!(result.is_ok());
    }

    #[test]
    fn manage_rule_none_means_no_manage_access() {
        // No manage_rule — individual rules take effect.
        let rules = ApiRules {
            create_rule: None, // locked
            manage_rule: None,
            ..Default::default()
        };

        let mut record = HashMap::new();
        record.insert("id".to_string(), serde_json::json!("user1"));
        let auth = AuthInfo::authenticated(record);

        let result = enforce_rule_no_record(&rules.create_rule, &rules, &auth, "POST");
        assert!(result.is_err());
    }

    #[test]
    fn manage_rule_bypasses_list_rule() {
        let mut record = HashMap::new();
        record.insert("id".to_string(), serde_json::json!("mgr1"));
        record.insert("role".to_string(), serde_json::json!("manager"));
        let auth = AuthInfo::authenticated(record);

        let rules = ApiRules {
            list_rule: None, // locked
            manage_rule: Some(r#"@request.auth.role = "manager""#.to_string()),
            ..Default::default()
        };

        match check_list_rule(&rules, &auth) {
            ListRuleResult::Proceed => {} // expected
            ListRuleResult::EmptyResult => panic!("manage_rule should have granted list access"),
        }
    }

    #[test]
    fn manage_rule_does_not_grant_list_to_non_matching() {
        let mut record = HashMap::new();
        record.insert("id".to_string(), serde_json::json!("user1"));
        record.insert("role".to_string(), serde_json::json!("viewer"));
        let auth = AuthInfo::authenticated(record);

        let rules = ApiRules {
            list_rule: None, // locked
            manage_rule: Some(r#"@request.auth.role = "manager""#.to_string()),
            ..Default::default()
        };

        match check_list_rule(&rules, &auth) {
            ListRuleResult::EmptyResult => {} // expected
            ListRuleResult::Proceed => panic!("non-matching user should not get list access"),
        }
    }

    #[test]
    fn user_has_manage_access_helper() {
        let rules = ApiRules {
            manage_rule: Some(r#"@request.auth.id = "admin1""#.to_string()),
            ..Default::default()
        };

        // Matching user
        let mut rec = HashMap::new();
        rec.insert("id".to_string(), serde_json::json!("admin1"));
        let auth = AuthInfo::authenticated(rec);
        assert!(user_has_manage_access(
            &rules,
            &auth,
            "GET",
            &HashMap::new()
        ));

        // Non-matching user
        let mut rec2 = HashMap::new();
        rec2.insert("id".to_string(), serde_json::json!("user2"));
        let auth2 = AuthInfo::authenticated(rec2);
        assert!(!user_has_manage_access(
            &rules,
            &auth2,
            "GET",
            &HashMap::new()
        ));

        // Superuser always has manage access
        let su = AuthInfo::superuser();
        assert!(user_has_manage_access(&rules, &su, "GET", &HashMap::new()));
    }
}
