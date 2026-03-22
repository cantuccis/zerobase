//! Multipart form data extraction for record endpoints.
//!
//! When a record endpoint receives `multipart/form-data`, this module extracts
//! both the regular form fields (as a JSON [`Value`] object) and file uploads
//! (as [`FileUpload`] structs).
//!
//! The approach mirrors PocketBase: any form field that doesn't look like a file
//! part is treated as a record field. File parts are matched against file-type
//! fields in the collection schema.

use axum::extract::Multipart;
use serde_json::Value;
use zerobase_core::storage::FileUpload;
use zerobase_core::ZerobaseError;

/// Result of extracting a multipart form body.
///
/// Contains the regular (non-file) fields as a JSON object and any uploaded
/// files as [`FileUpload`] structs ready for [`FileService::process_uploads`].
pub struct MultipartExtraction {
    /// Non-file form fields merged into a JSON object.
    pub data: Value,
    /// File uploads keyed by field name.
    pub files: Vec<FileUpload>,
}

/// Extract regular fields and file uploads from a multipart form.
///
/// Each multipart part is classified as either:
/// - **File**: if it has a `filename` in the content-disposition header.
/// - **Field**: otherwise, interpreted as a JSON value or plain string.
///
/// For regular fields, the value is first attempted to parse as JSON (so
/// numbers, booleans, arrays, and objects are preserved). If parsing fails,
/// the raw text is stored as a JSON string.
pub async fn extract_multipart(
    mut multipart: Multipart,
) -> Result<MultipartExtraction, ZerobaseError> {
    let mut fields = serde_json::Map::new();
    let mut files = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ZerobaseError::validation(format!("failed to read multipart field: {e}")))?
    {
        let field_name = field.name().map(|s| s.to_string()).unwrap_or_default();
        let file_name = field.file_name().map(|s| s.to_string());
        let content_type = field
            .content_type()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());

        if let Some(original_name) = file_name {
            // This part has a filename → treat as file upload.
            let data = field
                .bytes()
                .await
                .map_err(|e| ZerobaseError::validation(format!("failed to read file data: {e}")))?;

            // Skip empty file parts (browser sends empty file inputs).
            if data.is_empty() && original_name.is_empty() {
                continue;
            }

            files.push(FileUpload {
                field_name,
                original_name,
                content_type,
                data: data.to_vec(),
            });
        } else {
            // Regular form field → parse as JSON or string.
            let text = field.text().await.map_err(|e| {
                ZerobaseError::validation(format!("failed to read form field: {e}"))
            })?;

            let value =
                serde_json::from_str::<Value>(&text).unwrap_or_else(|_| Value::String(text));

            fields.insert(field_name, value);
        }
    }

    Ok(MultipartExtraction {
        data: Value::Object(fields),
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_parsing_of_field_values() {
        // Numbers
        let v: Value = serde_json::from_str("42").unwrap_or_else(|_| Value::String("42".into()));
        assert_eq!(v, Value::Number(42.into()));

        // Booleans
        let v: Value =
            serde_json::from_str("true").unwrap_or_else(|_| Value::String("true".into()));
        assert_eq!(v, Value::Bool(true));

        // Strings that aren't valid JSON remain as strings
        let v: Value = serde_json::from_str("hello world")
            .unwrap_or_else(|_| Value::String("hello world".into()));
        assert_eq!(v, Value::String("hello world".into()));

        // JSON arrays
        let v: Value = serde_json::from_str(r#"["a","b"]"#)
            .unwrap_or_else(|_| Value::String(r#"["a","b"]"#.into()));
        assert!(v.is_array());
    }
}
