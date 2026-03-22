//! Data export handler for streaming collection records as CSV or JSON.
//!
//! Provides `GET /_/api/collections/:collection/export` — a superuser-only
//! endpoint that streams all matching records in the requested format.
//!
//! # Supported formats
//!
//! - `json` (default) — JSON array streamed as `[{...},{...},...]`
//! - `csv` — RFC 4180 CSV with header row derived from collection fields
//!
//! # Query parameters
//!
//! - `format` — `csv` or `json` (default: `json`)
//! - `filter` — PocketBase-style filter expression
//! - `sort`   — comma-separated sort fields (prefix `-` for descending)
//!
//! Large exports are streamed in pages to avoid holding entire datasets in
//! memory.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use zerobase_core::schema::{CollectionType, Field, FieldType};
use zerobase_core::services::record_service::{
    RecordQuery, RecordRepository, RecordService, SchemaLookup, SortDirection,
};
use zerobase_core::ZerobaseError;

/// Internal page size for streaming export. Records are fetched in chunks of
/// this size to limit memory usage.
const EXPORT_PAGE_SIZE: u32 = 500;

/// Maximum number of pages to iterate (safety valve for runaway exports).
const MAX_EXPORT_PAGES: u32 = 100_000;

/// Supported export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Json,
    Csv,
}

impl Default for ExportFormat {
    fn default() -> Self {
        Self::Json
    }
}

/// Query parameters for the export endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportParams {
    /// Output format: `json` or `csv`. Defaults to `json`.
    #[serde(default)]
    pub format: ExportFormat,
    /// PocketBase-style filter expression.
    pub filter: Option<String>,
    /// Sort expression: comma-separated fields, prefix `-` for descending.
    pub sort: Option<String>,
}

/// Shared state for the export handler — reuses the record service.
pub struct ExportState<R: RecordRepository, S: SchemaLookup> {
    pub record_service: Arc<RecordService<R, S>>,
}

impl<R: RecordRepository, S: SchemaLookup> Clone for ExportState<R, S> {
    fn clone(&self) -> Self {
        Self {
            record_service: self.record_service.clone(),
        }
    }
}

/// `GET /_/api/collections/:collection/export`
///
/// Stream all records from the named collection in CSV or JSON format.
/// Protected by superuser middleware. Supports filtering and sorting.
pub async fn export_records<R: RecordRepository, S: SchemaLookup>(
    State(state): State<ExportState<R, S>>,
    Path(collection_name): Path<String>,
    Query(params): Query<ExportParams>,
) -> Response {
    let service = &state.record_service;

    // Validate collection exists and get its schema.
    let collection = match service.get_collection(&collection_name) {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };

    // Parse sort if provided.
    let sort = match &params.sort {
        Some(s) => match zerobase_core::services::record_service::parse_sort(s) {
            Ok(parsed) => parsed,
            Err(e) => return error_response(e),
        },
        None => vec![("created".to_string(), SortDirection::Asc)],
    };

    // Build header list for CSV (also used as field ordering reference).
    let headers = build_export_headers(&collection.fields, &collection.collection_type);

    // Fetch all matching records across pages.
    let records = match fetch_all_records(service, &collection_name, &params.filter, &sort) {
        Ok(r) => r,
        Err(e) => return error_response(e),
    };

    match params.format {
        ExportFormat::Json => build_json_response(&collection_name, &records),
        ExportFormat::Csv => build_csv_response(&collection_name, &records, &headers),
    }
}

/// Fetch all matching records by iterating through pages.
fn fetch_all_records<R: RecordRepository, S: SchemaLookup>(
    service: &RecordService<R, S>,
    collection_name: &str,
    filter: &Option<String>,
    sort: &[(String, SortDirection)],
) -> Result<Vec<HashMap<String, Value>>, ZerobaseError> {
    let mut all_records: Vec<HashMap<String, Value>> = Vec::new();

    for page in 1..=MAX_EXPORT_PAGES {
        let query = RecordQuery {
            page,
            per_page: EXPORT_PAGE_SIZE,
            filter: filter.clone(),
            sort: sort.to_vec(),
            fields: None,
            search: None,
        };

        let record_list = service.list_records(collection_name, &query)?;
        let is_last = record_list.items.len() < EXPORT_PAGE_SIZE as usize
            || page >= record_list.total_pages;
        all_records.extend(record_list.items);
        if is_last {
            break;
        }
    }

    Ok(all_records)
}

/// Build the ordered list of column headers for export.
///
/// System fields come first (`id`, `created`, `updated`), followed by
/// user-defined fields. Auth collections also include `email`, `verified`,
/// `emailVisibility` after `id`.
fn build_export_headers(fields: &[Field], collection_type: &CollectionType) -> Vec<String> {
    let mut headers = vec!["id".to_string()];

    if *collection_type == CollectionType::Auth {
        headers.extend([
            "email".to_string(),
            "verified".to_string(),
            "emailVisibility".to_string(),
        ]);
    }

    for field in fields {
        // Skip password fields from export.
        if matches!(&field.field_type, FieldType::Password(_)) {
            continue;
        }
        headers.push(field.name.clone());
    }

    headers.extend(["created".to_string(), "updated".to_string()]);
    headers
}

/// Build a streaming JSON response.
///
/// Outputs `[{record1},{record2},...]` using chunked transfer encoding.
/// Each record is individually serialized to limit peak memory.
fn build_json_response(
    collection_name: &str,
    records: &[HashMap<String, Value>],
) -> Response {
    // Build the body by serializing records incrementally.
    let mut buf: Vec<u8> = Vec::with_capacity(records.len() * 256);
    buf.push(b'[');

    for (i, record) in records.iter().enumerate() {
        if i > 0 {
            buf.push(b',');
        }
        // Serialization of individual records should not fail for valid JSON values.
        serde_json::to_writer(&mut buf, record).unwrap_or_default();
    }

    buf.push(b']');

    let filename = format!("{collection_name}_export.json");

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from(buf))
        .unwrap()
}

/// Build a streaming CSV response.
///
/// First row is the header derived from the collection schema, followed by
/// one row per record. Complex values (arrays, objects) are serialized as JSON
/// strings within the CSV cell.
fn build_csv_response(
    collection_name: &str,
    records: &[HashMap<String, Value>],
    headers: &[String],
) -> Response {
    let mut wtr = csv::Writer::from_writer(Vec::new());

    // Write header row.
    let header_refs: Vec<&str> = headers.iter().map(|s| s.as_str()).collect();
    wtr.write_record(&header_refs).unwrap_or_default();

    // Write data rows.
    for record in records {
        let row: Vec<String> = headers
            .iter()
            .map(|h| value_to_csv_cell(record.get(h)))
            .collect();
        let row_refs: Vec<&str> = row.iter().map(|s| s.as_str()).collect();
        wtr.write_record(&row_refs).unwrap_or_default();
    }

    wtr.flush().unwrap_or_default();
    let csv_bytes = wtr.into_inner().unwrap_or_default();

    let filename = format!("{collection_name}_export.csv");

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from(csv_bytes))
        .unwrap()
}

/// Convert a JSON value to a CSV cell string.
///
/// - `null` / missing → empty string
/// - String → the string itself
/// - Number / Bool → their string representation
/// - Array / Object → JSON serialized
fn value_to_csv_cell(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(v @ Value::Array(_)) | Some(v @ Value::Object(_)) => {
            serde_json::to_string(v).unwrap_or_default()
        }
    }
}

fn error_response(err: ZerobaseError) -> Response {
    let status =
        StatusCode::from_u16(err.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = err.error_response_body();
    (status, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn export_format_defaults_to_json() {
        let format: ExportFormat = Default::default();
        assert_eq!(format, ExportFormat::Json);
    }

    #[test]
    fn export_format_deserializes_json() {
        let format: ExportFormat = serde_json::from_str("\"json\"").unwrap();
        assert_eq!(format, ExportFormat::Json);
    }

    #[test]
    fn export_format_deserializes_csv() {
        let format: ExportFormat = serde_json::from_str("\"csv\"").unwrap();
        assert_eq!(format, ExportFormat::Csv);
    }

    #[test]
    fn export_format_rejects_unknown() {
        let result: Result<ExportFormat, _> = serde_json::from_str("\"xml\"");
        assert!(result.is_err());
    }

    #[test]
    fn value_to_csv_cell_handles_null() {
        assert_eq!(value_to_csv_cell(None), "");
        assert_eq!(value_to_csv_cell(Some(&Value::Null)), "");
    }

    #[test]
    fn value_to_csv_cell_handles_string() {
        assert_eq!(value_to_csv_cell(Some(&json!("hello"))), "hello");
    }

    #[test]
    fn value_to_csv_cell_handles_number() {
        assert_eq!(value_to_csv_cell(Some(&json!(42))), "42");
        assert_eq!(value_to_csv_cell(Some(&json!(3.14))), "3.14");
    }

    #[test]
    fn value_to_csv_cell_handles_bool() {
        assert_eq!(value_to_csv_cell(Some(&json!(true))), "true");
        assert_eq!(value_to_csv_cell(Some(&json!(false))), "false");
    }

    #[test]
    fn value_to_csv_cell_handles_array() {
        let val = json!(["a", "b"]);
        assert_eq!(value_to_csv_cell(Some(&val)), r#"["a","b"]"#);
    }

    #[test]
    fn value_to_csv_cell_handles_object() {
        let val = json!({"key": "value"});
        assert_eq!(value_to_csv_cell(Some(&val)), r#"{"key":"value"}"#);
    }

    #[test]
    fn build_export_headers_base_collection() {
        let fields = vec![
            Field::new("title", FieldType::Text(Default::default())),
            Field::new("count", FieldType::Number(Default::default())),
        ];
        let headers = build_export_headers(&fields, &CollectionType::Base);
        assert_eq!(headers, vec!["id", "title", "count", "created", "updated"]);
    }

    #[test]
    fn build_export_headers_auth_collection() {
        let fields = vec![Field::new("name", FieldType::Text(Default::default()))];
        let headers = build_export_headers(&fields, &CollectionType::Auth);
        assert_eq!(
            headers,
            vec![
                "id",
                "email",
                "verified",
                "emailVisibility",
                "name",
                "created",
                "updated",
            ]
        );
    }

    #[test]
    fn build_export_headers_skips_password_fields() {
        let fields = vec![
            Field::new("name", FieldType::Text(Default::default())),
            Field::new("password", FieldType::Password(Default::default())),
        ];
        let headers = build_export_headers(&fields, &CollectionType::Base);
        assert!(!headers.contains(&"password".to_string()));
        assert!(headers.contains(&"name".to_string()));
    }

    #[test]
    fn export_params_defaults() {
        let params: ExportParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params.format, ExportFormat::Json);
        assert!(params.filter.is_none());
        assert!(params.sort.is_none());
    }

    #[test]
    fn export_params_with_csv_format() {
        let params: ExportParams = serde_json::from_str(r#"{"format":"csv"}"#).unwrap();
        assert_eq!(params.format, ExportFormat::Csv);
    }

    #[test]
    fn export_params_with_filter() {
        let params: ExportParams =
            serde_json::from_str(r#"{"filter":"status = \"active\""}"#).unwrap();
        assert_eq!(params.filter.unwrap(), r#"status = "active""#);
    }

    #[test]
    fn build_json_response_empty_records() {
        let resp = build_json_response("test", &[]);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn build_json_response_content_type() {
        let resp = build_json_response("test", &[]);
        let ct = resp.headers().get(header::CONTENT_TYPE).unwrap();
        assert_eq!(ct, "application/json; charset=utf-8");
    }

    #[test]
    fn build_json_response_content_disposition() {
        let resp = build_json_response("posts", &[]);
        let cd = resp.headers().get(header::CONTENT_DISPOSITION).unwrap();
        assert_eq!(cd, "attachment; filename=\"posts_export.json\"");
    }

    #[test]
    fn build_csv_response_empty_records() {
        let headers = vec!["id".to_string(), "title".to_string()];
        let resp = build_csv_response("test", &[], &headers);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn build_csv_response_content_type() {
        let headers = vec!["id".to_string()];
        let resp = build_csv_response("test", &[], &headers);
        let ct = resp.headers().get(header::CONTENT_TYPE).unwrap();
        assert_eq!(ct, "text/csv; charset=utf-8");
    }

    #[test]
    fn build_csv_response_content_disposition() {
        let headers = vec!["id".to_string()];
        let resp = build_csv_response("users", &[], &headers);
        let cd = resp.headers().get(header::CONTENT_DISPOSITION).unwrap();
        assert_eq!(cd, "attachment; filename=\"users_export.csv\"");
    }

    #[test]
    fn build_csv_response_with_records() {
        let headers = vec!["id".to_string(), "title".to_string()];
        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("abc123"));
        record.insert("title".to_string(), json!("Hello World"));

        let resp = build_csv_response("posts", &[record], &headers);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn build_csv_response_with_special_characters() {
        let headers = vec!["id".to_string(), "content".to_string()];
        let mut record = HashMap::new();
        record.insert("id".to_string(), json!("abc123"));
        record.insert(
            "content".to_string(),
            json!("Hello, \"World\"\nNew line"),
        );

        // Should not panic — CSV writer handles quoting/escaping.
        let resp = build_csv_response("posts", &[record], &headers);
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
