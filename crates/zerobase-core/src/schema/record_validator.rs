//! Record-level validation engine.
//!
//! [`RecordValidator`] validates a full record (JSON object with field values)
//! against a collection's field definitions. It collects **all** validation errors
//! rather than failing on the first one, providing a complete error report.

use std::collections::HashMap;

use serde_json::Value;

use super::field::{AutoDateOptions, Field, FieldType};
use crate::error::{Result, ZerobaseError};

/// The kind of operation being performed on a record.
///
/// AutoDate fields use this to decide whether to inject the current timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationContext {
    /// Creating a new record.
    Create,
    /// Updating an existing record.
    Update,
}

/// Validates record data against a collection's field definitions.
///
/// Unlike per-field validation (which stops at the first error), the record
/// validator iterates over **all** fields and accumulates every error into
/// a single [`ZerobaseError::Validation`] response.
///
/// # Usage
///
/// ```ignore
/// let validator = RecordValidator::new(&collection.fields);
/// validator.validate(&record_data)?;
/// ```
pub struct RecordValidator<'a> {
    fields: &'a [Field],
}

impl<'a> RecordValidator<'a> {
    /// Create a new validator for the given field definitions.
    pub fn new(fields: &'a [Field]) -> Self {
        Self { fields }
    }

    /// Validate a record's data against all field definitions.
    ///
    /// `data` should be a JSON object mapping field names to values.
    /// Missing fields are treated as `null`.
    ///
    /// Returns `Ok(())` if all fields pass validation, or a single
    /// `Validation` error containing every field-level error.
    pub fn validate(&self, data: &Value) -> Result<()> {
        let obj = data
            .as_object()
            .ok_or_else(|| ZerobaseError::validation("record data must be a JSON object"))?;

        let mut all_errors: HashMap<String, String> = HashMap::new();

        for field in self.fields {
            let value = obj.get(&field.name).unwrap_or(&Value::Null);

            if let Err(ZerobaseError::Validation { field_errors, .. }) = field.validate_value(value)
            {
                all_errors.extend(field_errors);
            }
        }

        // Check for unknown fields (fields not defined in the schema).
        let known_names: std::collections::HashSet<&str> =
            self.fields.iter().map(|f| f.name.as_str()).collect();
        for key in obj.keys() {
            if !known_names.contains(key.as_str()) {
                all_errors.insert(key.clone(), "unknown field".to_string());
            }
        }

        if all_errors.is_empty() {
            Ok(())
        } else {
            Err(ZerobaseError::validation_with_fields(
                "record validation failed",
                all_errors,
            ))
        }
    }

    /// Validate only the fields present in `data` (partial update).
    ///
    /// Unlike [`validate`](Self::validate), this does not check for required
    /// fields that are absent — only validates fields that are actually provided.
    /// This is useful for PATCH-style updates.
    pub fn validate_partial(&self, data: &Value) -> Result<()> {
        let obj = data
            .as_object()
            .ok_or_else(|| ZerobaseError::validation("record data must be a JSON object"))?;

        let mut all_errors: HashMap<String, String> = HashMap::new();

        let field_map: HashMap<&str, &Field> =
            self.fields.iter().map(|f| (f.name.as_str(), f)).collect();

        for (key, value) in obj {
            if let Some(field) = field_map.get(key.as_str()) {
                // For partial updates, skip required-null check if value is absent.
                // But if a value IS provided, validate it fully.
                if let Err(ZerobaseError::Validation { field_errors, .. }) =
                    field.validate_value(value)
                {
                    all_errors.extend(field_errors);
                }
            } else {
                all_errors.insert(key.clone(), "unknown field".to_string());
            }
        }

        if all_errors.is_empty() {
            Ok(())
        } else {
            Err(ZerobaseError::validation_with_fields(
                "record validation failed",
                all_errors,
            ))
        }
    }

    /// Validate **and** prepare record data for storage.
    ///
    /// This combines validation with field-level sanitization (e.g. HTML
    /// sanitization for Editor fields). Returns the cleaned data on success,
    /// or a validation error containing all field-level issues.
    pub fn validate_and_prepare(&self, data: &Value) -> Result<Value> {
        let obj = data
            .as_object()
            .ok_or_else(|| ZerobaseError::validation("record data must be a JSON object"))?;

        let mut all_errors: HashMap<String, String> = HashMap::new();
        let mut prepared = obj.clone();

        for field in self.fields {
            let value = obj.get(&field.name).unwrap_or(&Value::Null);

            // Prepare (sanitize) the value first.
            if let Some(clean) = field.field_type.prepare_value(value) {
                prepared.insert(field.name.clone(), clean);
            }

            // Validate against the *prepared* value.
            let check_value = prepared.get(&field.name).unwrap_or(&Value::Null);
            if let Err(ZerobaseError::Validation { field_errors, .. }) =
                field.validate_value(check_value)
            {
                all_errors.extend(field_errors);
            }
        }

        // Check for unknown fields.
        let known_names: std::collections::HashSet<&str> =
            self.fields.iter().map(|f| f.name.as_str()).collect();
        for key in obj.keys() {
            if !known_names.contains(key.as_str()) {
                all_errors.insert(key.clone(), "unknown field".to_string());
            }
        }

        if all_errors.is_empty() {
            Ok(Value::Object(prepared))
        } else {
            Err(ZerobaseError::validation_with_fields(
                "record validation failed",
                all_errors,
            ))
        }
    }

    /// Apply auto-date values to record data based on the operation context.
    ///
    /// For **create** operations, AutoDate fields with `on_create = true` are
    /// injected with the current UTC timestamp. Any manually supplied value is
    /// **replaced** — AutoDate fields cannot be set by the caller.
    ///
    /// For **update** operations, AutoDate fields with `on_update = true` are
    /// injected with the current UTC timestamp.
    ///
    /// Fields that don't match the operation context are left untouched (for
    /// updates, on_create-only fields keep their existing value).
    pub fn apply_auto_dates(&self, data: &mut Value, ctx: OperationContext) {
        let obj = match data.as_object_mut() {
            Some(obj) => obj,
            None => return,
        };

        let now = AutoDateOptions::now_utc();

        for field in self.fields {
            if let FieldType::AutoDate(opts) = &field.field_type {
                let should_set = match ctx {
                    OperationContext::Create => opts.on_create,
                    OperationContext::Update => opts.on_update,
                };

                if should_set {
                    obj.insert(field.name.clone(), Value::String(now.clone()));
                }
            }
        }
    }

    /// Validate, prepare, and inject auto-date values in a single call.
    ///
    /// This is the recommended method for record creation and update. It:
    /// 1. Injects auto-date timestamps (overriding any manually supplied values)
    /// 2. Sanitizes field values
    /// 3. Validates all fields
    ///
    /// AutoDate fields are **excluded** from unknown-field checks, since they
    /// are injected automatically and should not be provided by the caller.
    pub fn validate_and_prepare_with_context(
        &self,
        data: &Value,
        ctx: OperationContext,
    ) -> Result<Value> {
        let obj = data
            .as_object()
            .ok_or_else(|| ZerobaseError::validation("record data must be a JSON object"))?;

        let mut all_errors: HashMap<String, String> = HashMap::new();
        let mut prepared = obj.clone();

        // 1. Inject auto-date values first (before validation).
        let now = AutoDateOptions::now_utc();
        for field in self.fields {
            if let FieldType::AutoDate(opts) = &field.field_type {
                let should_set = match ctx {
                    OperationContext::Create => opts.on_create,
                    OperationContext::Update => opts.on_update,
                };
                if should_set {
                    prepared.insert(field.name.clone(), Value::String(now.clone()));
                }
            }
        }

        // 2. Prepare and validate all fields.
        for field in self.fields {
            let value = prepared.get(&field.name).unwrap_or(&Value::Null);

            // Prepare (sanitize) the value.
            if let Some(clean) = field.field_type.prepare_value(value) {
                prepared.insert(field.name.clone(), clean);
            }

            // Validate against the *prepared* value.
            let check_value = prepared.get(&field.name).unwrap_or(&Value::Null);
            if let Err(ZerobaseError::Validation { field_errors, .. }) =
                field.validate_value(check_value)
            {
                all_errors.extend(field_errors);
            }
        }

        // 3. Check for unknown fields (auto-date fields excluded since they're
        //    injected by the system, not user-provided).
        let known_names: std::collections::HashSet<&str> =
            self.fields.iter().map(|f| f.name.as_str()).collect();
        for key in obj.keys() {
            if !known_names.contains(key.as_str()) {
                all_errors.insert(key.clone(), "unknown field".to_string());
            }
        }

        if all_errors.is_empty() {
            Ok(Value::Object(prepared))
        } else {
            Err(ZerobaseError::validation_with_fields(
                "record validation failed",
                all_errors,
            ))
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::field::*;
    use serde_json::json;

    // ── Helper ─────────────────────────────────────────────────────────────

    fn text_field(name: &str, required: bool) -> Field {
        Field::new(name, FieldType::Text(TextOptions::default())).required(required)
    }

    fn number_field(name: &str, min: Option<f64>, max: Option<f64>) -> Field {
        Field::new(
            name,
            FieldType::Number(NumberOptions {
                min,
                max,
                only_int: false,
            }),
        )
    }

    fn email_field(name: &str, required: bool) -> Field {
        Field::new(name, FieldType::Email(EmailOptions::default())).required(required)
    }

    fn bool_field(name: &str) -> Field {
        Field::new(name, FieldType::Bool(BoolOptions::default()))
    }

    fn url_field(name: &str) -> Field {
        Field::new(name, FieldType::Url(UrlOptions::default()))
    }

    fn select_field(name: &str, values: Vec<&str>) -> Field {
        Field::new(
            name,
            FieldType::Select(SelectOptions {
                values: values.into_iter().map(String::from).collect(),
            }),
        )
    }

    fn multiselect_field(name: &str, values: Vec<&str>, max_select: u32) -> Field {
        Field::new(
            name,
            FieldType::MultiSelect(MultiSelectOptions {
                values: values.into_iter().map(String::from).collect(),
                max_select,
            }),
        )
    }

    fn datetime_field(name: &str) -> Field {
        Field::new(name, FieldType::DateTime(DateTimeOptions::default()))
    }

    fn editor_field(name: &str, max_length: u32) -> Field {
        Field::new(
            name,
            FieldType::Editor(EditorOptions {
                max_length,
                ..Default::default()
            }),
        )
    }

    fn password_field(name: &str) -> Field {
        Field::new(name, FieldType::Password(PasswordOptions::default()))
    }

    fn json_field(name: &str) -> Field {
        Field::new(name, FieldType::Json(JsonOptions::default()))
    }

    fn relation_field(name: &str, collection_id: &str) -> Field {
        Field::new(
            name,
            FieldType::Relation(RelationOptions {
                collection_id: collection_id.to_string(),
                max_select: 1,
                ..Default::default()
            }),
        )
    }

    fn extract_field_errors(err: &ZerobaseError) -> &HashMap<String, String> {
        match err {
            ZerobaseError::Validation { field_errors, .. } => field_errors,
            _ => panic!("expected Validation error, got: {err:?}"),
        }
    }

    // ── RecordValidator: basic happy path ──────────────────────────────────

    #[test]
    fn validates_valid_record() {
        let fields = vec![
            text_field("title", true),
            number_field("views", Some(0.0), None),
            email_field("contact", false),
        ];
        let validator = RecordValidator::new(&fields);

        let data = json!({
            "title": "Hello World",
            "views": 42,
            "contact": "user@example.com"
        });

        assert!(validator.validate(&data).is_ok());
    }

    #[test]
    fn validates_record_with_optional_nulls() {
        let fields = vec![text_field("title", true), email_field("contact", false)];
        let validator = RecordValidator::new(&fields);

        let data = json!({
            "title": "Hello",
            "contact": null
        });

        assert!(validator.validate(&data).is_ok());
    }

    #[test]
    fn validates_record_with_missing_optional_fields() {
        let fields = vec![text_field("title", true), email_field("contact", false)];
        let validator = RecordValidator::new(&fields);

        let data = json!({
            "title": "Hello"
        });

        assert!(validator.validate(&data).is_ok());
    }

    // ── RecordValidator: collects multiple errors ──────────────────────────

    #[test]
    fn collects_all_errors_not_just_first() {
        let fields = vec![
            text_field("title", true),
            email_field("email", true),
            number_field("age", Some(0.0), Some(150.0)),
        ];
        let validator = RecordValidator::new(&fields);

        // All three fields have issues
        let data = json!({
            "title": null,
            "email": "not-an-email",
            "age": 200
        });

        let err = validator.validate(&data).unwrap_err();
        let errors = extract_field_errors(&err);

        assert!(errors.contains_key("title"), "should have title error");
        assert!(errors.contains_key("email"), "should have email error");
        assert!(errors.contains_key("age"), "should have age error");
        assert_eq!(errors.len(), 3);
    }

    #[test]
    fn required_field_missing_entirely() {
        let fields = vec![text_field("title", true)];
        let validator = RecordValidator::new(&fields);

        let data = json!({});

        let err = validator.validate(&data).unwrap_err();
        let errors = extract_field_errors(&err);
        assert!(errors.contains_key("title"));
        assert!(errors["title"].contains("required"));
    }

    // ── RecordValidator: unknown fields ────────────────────────────────────

    #[test]
    fn rejects_unknown_fields() {
        let fields = vec![text_field("title", false)];
        let validator = RecordValidator::new(&fields);

        let data = json!({
            "title": "Hello",
            "unknown_field": "surprise"
        });

        let err = validator.validate(&data).unwrap_err();
        let errors = extract_field_errors(&err);
        assert!(errors.contains_key("unknown_field"));
        assert!(errors["unknown_field"].contains("unknown"));
    }

    #[test]
    fn unknown_fields_with_valid_fields() {
        let fields = vec![text_field("title", true)];
        let validator = RecordValidator::new(&fields);

        let data = json!({
            "title": "Valid",
            "extra1": "foo",
            "extra2": 42
        });

        let err = validator.validate(&data).unwrap_err();
        let errors = extract_field_errors(&err);
        assert!(errors.contains_key("extra1"));
        assert!(errors.contains_key("extra2"));
        assert!(!errors.contains_key("title"));
    }

    // ── RecordValidator: rejects non-object data ──────────────────────────

    #[test]
    fn rejects_non_object_data() {
        let fields = vec![text_field("title", false)];
        let validator = RecordValidator::new(&fields);

        assert!(validator.validate(&json!("string")).is_err());
        assert!(validator.validate(&json!(42)).is_err());
        assert!(validator.validate(&json!([1, 2, 3])).is_err());
        assert!(validator.validate(&json!(null)).is_err());
    }

    // ── RecordValidator: partial validation ────────────────────────────────

    #[test]
    fn partial_skips_missing_required_fields() {
        let fields = vec![text_field("title", true), text_field("body", true)];
        let validator = RecordValidator::new(&fields);

        // Only updating body, title not present — should be OK for partial.
        let data = json!({
            "body": "Updated body"
        });

        assert!(validator.validate_partial(&data).is_ok());
    }

    #[test]
    fn partial_still_validates_provided_fields() {
        let fields = vec![email_field("email", true), text_field("name", true)];
        let validator = RecordValidator::new(&fields);

        let data = json!({
            "email": "not-an-email"
        });

        let err = validator.validate_partial(&data).unwrap_err();
        let errors = extract_field_errors(&err);
        assert!(errors.contains_key("email"));
        assert!(!errors.contains_key("name"));
    }

    #[test]
    fn partial_rejects_unknown_fields() {
        let fields = vec![text_field("title", false)];
        let validator = RecordValidator::new(&fields);

        let data = json!({
            "nonexistent": "value"
        });

        let err = validator.validate_partial(&data).unwrap_err();
        let errors = extract_field_errors(&err);
        assert!(errors.contains_key("nonexistent"));
    }

    // ── RecordValidator: all field types integration ───────────────────────

    #[test]
    fn validates_all_field_types_valid() {
        let fields = vec![
            text_field("title", true),
            number_field("count", None, None),
            bool_field("active"),
            email_field("contact", false),
            url_field("website"),
            select_field("status", vec!["draft", "published"]),
            datetime_field("created_at"),
            editor_field("content", 0),
            password_field("secret"),
            json_field("metadata"),
            relation_field("author", "users"),
        ];
        let validator = RecordValidator::new(&fields);

        let data = json!({
            "title": "Test Post",
            "count": 10,
            "active": true,
            "contact": "user@example.com",
            "website": "https://example.com",
            "status": "draft",
            "created_at": "2024-01-15T10:30:00Z",
            "content": "<p>Hello</p>",
            "secret": "secureP@ss1",
            "metadata": {"key": "value"},
            "author": "user123"
        });

        assert!(validator.validate(&data).is_ok());
    }

    #[test]
    fn validates_all_field_types_invalid() {
        let fields = vec![
            text_field("title", true),
            Field::new(
                "count",
                FieldType::Number(NumberOptions {
                    min: Some(0.0),
                    max: Some(100.0),
                    only_int: true,
                }),
            ),
            bool_field("active"),
            email_field("contact", true),
            url_field("website"),
            select_field("status", vec!["draft", "published"]),
        ];
        let validator = RecordValidator::new(&fields);

        let data = json!({
            "title": null,
            "count": 3.14,
            "active": "yes",
            "contact": "not-email",
            "website": "not-a-url",
            "status": "invalid_status"
        });

        let err = validator.validate(&data).unwrap_err();
        let errors = extract_field_errors(&err);

        assert!(errors.contains_key("title"), "title should fail: required");
        assert!(
            errors.contains_key("count"),
            "count should fail: not integer"
        );
        assert!(
            errors.contains_key("active"),
            "active should fail: not bool"
        );
        assert!(
            errors.contains_key("contact"),
            "contact should fail: not email"
        );
        assert!(
            errors.contains_key("website"),
            "website should fail: not url"
        );
        assert!(
            errors.contains_key("status"),
            "status should fail: not in list"
        );
        assert_eq!(errors.len(), 6);
    }

    // ── RecordValidator: empty string required ─────────────────────────────

    #[test]
    fn required_text_rejects_empty_string() {
        let fields = vec![text_field("title", true)];
        let validator = RecordValidator::new(&fields);

        let data = json!({"title": ""});

        let err = validator.validate(&data).unwrap_err();
        let errors = extract_field_errors(&err);
        assert!(errors.contains_key("title"));
        assert!(errors["title"].contains("required"));
    }

    #[test]
    fn required_email_rejects_empty_string() {
        let fields = vec![email_field("email", true)];
        let validator = RecordValidator::new(&fields);

        let data = json!({"email": ""});

        let err = validator.validate(&data).unwrap_err();
        let errors = extract_field_errors(&err);
        assert!(errors.contains_key("email"));
    }

    #[test]
    fn required_url_rejects_empty_string() {
        let fields =
            vec![Field::new("website", FieldType::Url(UrlOptions::default())).required(true)];
        let validator = RecordValidator::new(&fields);

        let data = json!({"website": ""});

        let err = validator.validate(&data).unwrap_err();
        let errors = extract_field_errors(&err);
        assert!(errors.contains_key("website"));
    }

    #[test]
    fn optional_text_accepts_empty_string() {
        let fields = vec![text_field("title", false)];
        let validator = RecordValidator::new(&fields);

        let data = json!({"title": ""});

        // empty string on optional text is valid (no min_length set)
        assert!(validator.validate(&data).is_ok());
    }

    // ── RecordValidator: edge cases ────────────────────────────────────────

    #[test]
    fn empty_fields_empty_data_is_ok() {
        let fields: Vec<Field> = vec![];
        let validator = RecordValidator::new(&fields);

        assert!(validator.validate(&json!({})).is_ok());
    }

    #[test]
    fn empty_fields_with_data_reports_unknown() {
        let fields: Vec<Field> = vec![];
        let validator = RecordValidator::new(&fields);

        let err = validator.validate(&json!({"foo": "bar"})).unwrap_err();
        let errors = extract_field_errors(&err);
        assert!(errors.contains_key("foo"));
    }

    // ── Text field: comprehensive ──────────────────────────────────────────

    #[test]
    fn text_min_length_boundary() {
        let fields = vec![Field::new(
            "code",
            FieldType::Text(TextOptions {
                min_length: 3,
                max_length: 0,
                pattern: None,
                searchable: false,
            }),
        )];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"code": "ab"})).is_err());
        assert!(v.validate(&json!({"code": "abc"})).is_ok());
        assert!(v.validate(&json!({"code": "abcd"})).is_ok());
    }

    #[test]
    fn text_max_length_boundary() {
        let fields = vec![Field::new(
            "code",
            FieldType::Text(TextOptions {
                min_length: 0,
                max_length: 5,
                pattern: None,
                searchable: false,
            }),
        )];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"code": "abcde"})).is_ok());
        assert!(v.validate(&json!({"code": "abcdef"})).is_err());
    }

    #[test]
    fn text_pattern_validation() {
        let fields = vec![Field::new(
            "zip",
            FieldType::Text(TextOptions {
                min_length: 0,
                max_length: 0,
                pattern: Some(r"^\d{5}$".to_string()),
                searchable: false,
            }),
        )];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"zip": "12345"})).is_ok());
        assert!(v.validate(&json!({"zip": "1234"})).is_err());
        assert!(v.validate(&json!({"zip": "abcde"})).is_err());
    }

    #[test]
    fn text_rejects_wrong_type() {
        let fields = vec![text_field("title", false)];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"title": 42})).is_err());
        assert!(v.validate(&json!({"title": true})).is_err());
        assert!(v.validate(&json!({"title": []})).is_err());
    }

    // ── Number field: comprehensive ────────────────────────────────────────

    #[test]
    fn number_min_boundary() {
        let fields = vec![number_field("n", Some(10.0), None)];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"n": 9})).is_err());
        assert!(v.validate(&json!({"n": 10})).is_ok());
        assert!(v.validate(&json!({"n": 11})).is_ok());
    }

    #[test]
    fn number_max_boundary() {
        let fields = vec![number_field("n", None, Some(100.0))];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"n": 100})).is_ok());
        assert!(v.validate(&json!({"n": 101})).is_err());
    }

    #[test]
    fn number_only_int() {
        let fields = vec![Field::new(
            "n",
            FieldType::Number(NumberOptions {
                min: None,
                max: None,
                only_int: true,
            }),
        )];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"n": 42})).is_ok());
        assert!(v.validate(&json!({"n": 42.0})).is_ok()); // 42.0 has no fractional part
        assert!(v.validate(&json!({"n": 42.5})).is_err());
    }

    #[test]
    fn number_rejects_wrong_type() {
        let fields = vec![number_field("n", None, None)];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"n": "42"})).is_err());
        assert!(v.validate(&json!({"n": true})).is_err());
    }

    #[test]
    fn number_negative_values() {
        let fields = vec![number_field("n", Some(-100.0), Some(0.0))];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"n": -50})).is_ok());
        assert!(v.validate(&json!({"n": -101})).is_err());
        assert!(v.validate(&json!({"n": 1})).is_err());
    }

    // ── Bool field: comprehensive ──────────────────────────────────────────

    #[test]
    fn bool_valid_values() {
        let fields = vec![bool_field("active")];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"active": true})).is_ok());
        assert!(v.validate(&json!({"active": false})).is_ok());
    }

    #[test]
    fn bool_rejects_wrong_types() {
        let fields = vec![bool_field("active")];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"active": "true"})).is_err());
        assert!(v.validate(&json!({"active": 1})).is_err());
        assert!(v.validate(&json!({"active": 0})).is_err());
        assert!(v.validate(&json!({"active": "yes"})).is_err());
    }

    // ── Email field: comprehensive ─────────────────────────────────────────

    #[test]
    fn email_valid_addresses() {
        let fields = vec![email_field("email", false)];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"email": "user@example.com"})).is_ok());
        assert!(v
            .validate(&json!({"email": "first.last@company.co.uk"}))
            .is_ok());
        assert!(v.validate(&json!({"email": "user+tag@gmail.com"})).is_ok());
    }

    #[test]
    fn email_invalid_addresses() {
        let fields = vec![email_field("email", false)];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"email": "notanemail"})).is_err());
        assert!(v.validate(&json!({"email": "@example.com"})).is_err());
        assert!(v.validate(&json!({"email": "user@"})).is_err());
        assert!(v.validate(&json!({"email": "user@localhost"})).is_err());
        assert!(v.validate(&json!({"email": ""})).is_ok()); // not required
    }

    #[test]
    fn email_domain_restrictions() {
        let fields = vec![Field::new(
            "email",
            FieldType::Email(EmailOptions {
                only_domains: vec!["corp.com".to_string()],
                except_domains: Vec::new(),
            }),
        )];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"email": "user@corp.com"})).is_ok());
        assert!(v.validate(&json!({"email": "user@other.com"})).is_err());
    }

    #[test]
    fn email_except_domains() {
        let fields = vec![Field::new(
            "email",
            FieldType::Email(EmailOptions {
                only_domains: Vec::new(),
                except_domains: vec!["spam.com".to_string(), "trash.net".to_string()],
            }),
        )];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"email": "user@good.com"})).is_ok());
        assert!(v.validate(&json!({"email": "user@spam.com"})).is_err());
        assert!(v.validate(&json!({"email": "user@trash.net"})).is_err());
    }

    // ── URL field: comprehensive ───────────────────────────────────────────

    #[test]
    fn url_valid_urls() {
        let fields = vec![url_field("website")];
        let v = RecordValidator::new(&fields);

        assert!(v
            .validate(&json!({"website": "https://example.com"}))
            .is_ok());
        assert!(v
            .validate(&json!({"website": "http://localhost:3000"}))
            .is_ok());
        assert!(v
            .validate(&json!({"website": "ftp://files.example.com"}))
            .is_ok());
    }

    #[test]
    fn url_invalid_urls() {
        let fields = vec![url_field("website")];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"website": "not-a-url"})).is_err());
        assert!(v.validate(&json!({"website": "example.com"})).is_err());
        assert!(v
            .validate(&json!({"website": "://missing-scheme"}))
            .is_err());
    }

    #[test]
    fn url_scheme_restriction() {
        let fields = vec![Field::new(
            "website",
            FieldType::Url(UrlOptions {
                only_schemes: vec!["https".to_string()],
                ..Default::default()
            }),
        )];
        let v = RecordValidator::new(&fields);

        assert!(v
            .validate(&json!({"website": "https://example.com"}))
            .is_ok());
        assert!(v
            .validate(&json!({"website": "http://example.com"}))
            .is_err());
        assert!(v
            .validate(&json!({"website": "ftp://example.com"}))
            .is_err());
    }

    // ── Select field: comprehensive ────────────────────────────────────────

    #[test]
    fn select_single_valid_and_invalid() {
        let fields = vec![select_field(
            "status",
            vec!["draft", "published", "archived"],
        )];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"status": "draft"})).is_ok());
        assert!(v.validate(&json!({"status": "published"})).is_ok());
        assert!(v.validate(&json!({"status": "deleted"})).is_err());
        assert!(v.validate(&json!({"status": "DRAFT"})).is_err()); // case-sensitive
    }

    #[test]
    fn multiselect_valid_and_invalid() {
        let fields = vec![multiselect_field("tags", vec!["a", "b", "c", "d"], 3)];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"tags": ["a"]})).is_ok());
        assert!(v.validate(&json!({"tags": ["a", "b", "c"]})).is_ok());
        assert!(v.validate(&json!({"tags": ["a", "b", "c", "d"]})).is_err()); // too many
        assert!(v.validate(&json!({"tags": ["a", "x"]})).is_err()); // invalid value
    }

    #[test]
    fn multiselect_rejects_duplicate_selections() {
        let fields = vec![multiselect_field("tags", vec!["a", "b", "c"], 0)];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"tags": ["a", "a"]})).is_err());
    }

    #[test]
    fn select_wrong_type() {
        let fields = vec![select_field("status", vec!["a", "b"])];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"status": 42})).is_err());
        assert!(v.validate(&json!({"status": true})).is_err());
    }

    // ── DateTime field: comprehensive ──────────────────────────────────────

    #[test]
    fn datetime_valid_formats() {
        let fields = vec![datetime_field("dt")];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"dt": "2024-01-15T10:30:00Z"})).is_ok());
        assert!(v.validate(&json!({"dt": "2024-01-15 10:30:00"})).is_ok());
        assert!(v.validate(&json!({"dt": "2024-01-15"})).is_ok());
    }

    #[test]
    fn datetime_invalid_formats() {
        let fields = vec![datetime_field("dt")];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"dt": "not-a-date"})).is_err());
        assert!(v.validate(&json!({"dt": "2024/01/15"})).is_err());
        assert!(v.validate(&json!({"dt": "15-01-2024"})).is_err());
        assert!(v.validate(&json!({"dt": 20240115})).is_err());
    }

    #[test]
    fn datetime_min_max_range() {
        let fields = vec![Field::new(
            "dt",
            FieldType::DateTime(DateTimeOptions {
                min: Some("2024-01-01".to_string()),
                max: Some("2024-12-31".to_string()),
                ..Default::default()
            }),
        )];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"dt": "2024-06-15"})).is_ok());
        assert!(v.validate(&json!({"dt": "2024-01-01"})).is_ok());
        assert!(v.validate(&json!({"dt": "2024-12-31"})).is_ok());
        assert!(v.validate(&json!({"dt": "2023-12-31"})).is_err());
        assert!(v.validate(&json!({"dt": "2025-01-01"})).is_err());
    }

    // ── Editor field: comprehensive ────────────────────────────────────────

    #[test]
    fn editor_valid_html() {
        let fields = vec![editor_field("content", 0)];
        let v = RecordValidator::new(&fields);

        assert!(v
            .validate(&json!({"content": "<p>Hello World</p>"}))
            .is_ok());
        assert!(v.validate(&json!({"content": "plain text"})).is_ok());
    }

    #[test]
    fn editor_max_length() {
        let fields = vec![editor_field("content", 10)];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"content": "short"})).is_ok());
        assert!(v
            .validate(&json!({"content": "this is way too long"}))
            .is_err());
    }

    #[test]
    fn editor_wrong_type() {
        let fields = vec![editor_field("content", 0)];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"content": 42})).is_err());
    }

    // ── Password field: comprehensive ──────────────────────────────────────

    #[test]
    fn password_valid() {
        let fields = vec![password_field("pass")];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"pass": "securePass123"})).is_ok());
    }

    #[test]
    fn password_too_short() {
        let fields = vec![password_field("pass")]; // default min_length = 8
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"pass": "short"})).is_err());
    }

    #[test]
    fn password_max_length() {
        let fields = vec![Field::new(
            "pass",
            FieldType::Password(PasswordOptions {
                min_length: 1,
                max_length: 10,
                pattern: None,
            }),
        )];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"pass": "ok"})).is_ok());
        assert!(v
            .validate(&json!({"pass": "way too long password"}))
            .is_err());
    }

    #[test]
    fn password_pattern() {
        let fields = vec![Field::new(
            "pass",
            FieldType::Password(PasswordOptions {
                min_length: 1,
                max_length: 0,
                pattern: Some(r"[0-9]".to_string()), // must contain digit
            }),
        )];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"pass": "abc1"})).is_ok());
        assert!(v.validate(&json!({"pass": "abcdef"})).is_err());
    }

    // ── Json field: comprehensive ──────────────────────────────────────────

    #[test]
    fn json_accepts_any_json() {
        let fields = vec![json_field("meta")];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"meta": {"key": "val"}})).is_ok());
        assert!(v.validate(&json!({"meta": [1, 2, 3]})).is_ok());
        assert!(v.validate(&json!({"meta": "string"})).is_ok());
        assert!(v.validate(&json!({"meta": 42})).is_ok());
        assert!(v.validate(&json!({"meta": true})).is_ok());
    }

    #[test]
    fn json_max_size() {
        let fields = vec![Field::new(
            "meta",
            FieldType::Json(JsonOptions {
                max_size: 20,
                ..Default::default()
            }),
        )];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"meta": {"a": 1}})).is_ok());
        assert!(v
            .validate(&json!({"meta": {"key": "a very long value that exceeds limit"}}))
            .is_err());
    }

    // ── Relation field: comprehensive ──────────────────────────────────────

    #[test]
    fn relation_single_valid() {
        let fields = vec![relation_field("author", "users")];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"author": "abc123"})).is_ok());
    }

    #[test]
    fn relation_single_empty_id() {
        let fields = vec![relation_field("author", "users")];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"author": ""})).is_err());
    }

    #[test]
    fn relation_multi_valid() {
        let fields = vec![Field::new(
            "tags",
            FieldType::Relation(RelationOptions {
                collection_id: "tags".to_string(),
                max_select: 5,
                ..Default::default()
            }),
        )];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"tags": ["id1", "id2"]})).is_ok());
    }

    #[test]
    fn relation_multi_too_many() {
        let fields = vec![Field::new(
            "tags",
            FieldType::Relation(RelationOptions {
                collection_id: "tags".to_string(),
                max_select: 2,
                ..Default::default()
            }),
        )];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"tags": ["a", "b", "c"]})).is_err());
    }

    #[test]
    fn relation_wrong_type() {
        let fields = vec![relation_field("author", "users")];
        let v = RecordValidator::new(&fields);

        assert!(v.validate(&json!({"author": 42})).is_err());
        assert!(v.validate(&json!({"author": true})).is_err());
    }

    // ── Mixed scenario: realistic collection ───────────────────────────────

    #[test]
    fn realistic_blog_post_valid() {
        let fields = vec![
            Field::new(
                "title",
                FieldType::Text(TextOptions {
                    min_length: 1,
                    max_length: 200,
                    pattern: None,
                    searchable: false,
                }),
            )
            .required(true),
            Field::new(
                "slug",
                FieldType::Text(TextOptions {
                    min_length: 1,
                    max_length: 100,
                    pattern: Some(r"^[a-z0-9-]+$".to_string()),
                    searchable: false,
                }),
            )
            .required(true)
            .unique(true),
            editor_field("body", 50000),
            select_field("status", vec!["draft", "published", "archived"]),
            multiselect_field("tags", vec!["tech", "science", "art", "sports"], 3),
            number_field("reading_time", Some(1.0), Some(120.0)),
            email_field("author_email", true),
            url_field("cover_image"),
            bool_field("featured"),
            datetime_field("published_at"),
            json_field("seo_metadata"),
            relation_field("author", "users"),
        ];
        let v = RecordValidator::new(&fields);

        let data = json!({
            "title": "My First Blog Post",
            "slug": "my-first-blog-post",
            "body": "<p>Content here</p>",
            "status": "published",
            "tags": ["tech", "science"],
            "reading_time": 5,
            "author_email": "writer@blog.com",
            "cover_image": "https://images.blog.com/cover.jpg",
            "featured": true,
            "published_at": "2024-06-15T12:00:00Z",
            "seo_metadata": {"description": "A great post", "keywords": ["rust", "pocketbase"]},
            "author": "user_abc123"
        });

        assert!(v.validate(&data).is_ok());
    }

    #[test]
    fn realistic_blog_post_multiple_errors() {
        let fields = vec![
            Field::new(
                "title",
                FieldType::Text(TextOptions {
                    min_length: 1,
                    max_length: 200,
                    pattern: None,
                    searchable: false,
                }),
            )
            .required(true),
            Field::new(
                "slug",
                FieldType::Text(TextOptions {
                    min_length: 1,
                    max_length: 100,
                    pattern: Some(r"^[a-z0-9-]+$".to_string()),
                    searchable: false,
                }),
            )
            .required(true),
            select_field("status", vec!["draft", "published"]),
            number_field("reading_time", Some(1.0), Some(120.0)),
            email_field("author_email", true),
        ];
        let v = RecordValidator::new(&fields);

        let data = json!({
            "title": "",
            "slug": "INVALID SLUG!!!",
            "status": "deleted",
            "reading_time": 999,
            "author_email": "not-an-email"
        });

        let err = v.validate(&data).unwrap_err();
        let errors = extract_field_errors(&err);

        assert!(
            errors.len() >= 5,
            "expected at least 5 errors, got {}",
            errors.len()
        );
        assert!(errors.contains_key("title"));
        assert!(errors.contains_key("slug"));
        assert!(errors.contains_key("status"));
        assert!(errors.contains_key("reading_time"));
        assert!(errors.contains_key("author_email"));
    }

    // ── Error message quality ──────────────────────────────────────────────

    #[test]
    fn error_messages_are_descriptive() {
        let fields = vec![Field::new(
            "age",
            FieldType::Number(NumberOptions {
                min: Some(0.0),
                max: Some(150.0),
                only_int: false,
            }),
        )];
        let v = RecordValidator::new(&fields);

        let err = v.validate(&json!({"age": -5})).unwrap_err();
        let errors = extract_field_errors(&err);
        assert!(errors["age"].contains("at least"));

        let err = v.validate(&json!({"age": 200})).unwrap_err();
        let errors = extract_field_errors(&err);
        assert!(errors["age"].contains("at most"));
    }

    #[test]
    fn select_error_lists_allowed_values() {
        let fields = vec![select_field("color", vec!["red", "green", "blue"])];
        let v = RecordValidator::new(&fields);

        let err = v.validate(&json!({"color": "purple"})).unwrap_err();
        let errors = extract_field_errors(&err);
        assert!(errors["color"].contains("red"));
        assert!(errors["color"].contains("green"));
        assert!(errors["color"].contains("blue"));
    }

    // ── RecordValidator returns proper error type ──────────────────────────

    #[test]
    fn error_has_correct_status_code() {
        let fields = vec![text_field("title", true)];
        let v = RecordValidator::new(&fields);

        let err = v.validate(&json!({})).unwrap_err();
        assert_eq!(err.status_code(), 400);
    }

    #[test]
    fn error_is_user_facing() {
        let fields = vec![text_field("title", true)];
        let v = RecordValidator::new(&fields);

        let err = v.validate(&json!({})).unwrap_err();
        assert!(err.is_user_facing());
    }

    #[test]
    fn error_response_body_includes_field_errors() {
        let fields = vec![text_field("title", true), email_field("email", true)];
        let v = RecordValidator::new(&fields);

        let err = v.validate(&json!({})).unwrap_err();
        let body = err.error_response_body();

        assert_eq!(body.code, 400);
        assert!(body.message.contains("validation"));
        assert!(body.data.contains_key("title"));
        assert!(body.data.contains_key("email"));
    }

    // ── Editor validate_and_prepare ──────────────────────────────────────

    #[test]
    fn validate_and_prepare_sanitizes_editor_html() {
        let fields = vec![editor_field("content", 0)];
        let v = RecordValidator::new(&fields);
        let data = json!({
            "content": "<p>Hello</p><script>alert('xss')</script>"
        });
        let prepared = v.validate_and_prepare(&data).unwrap();
        let content = prepared["content"].as_str().unwrap();
        assert!(content.contains("<p>Hello</p>"));
        assert!(!content.contains("<script>"));
        assert!(!content.contains("alert"));
    }

    #[test]
    fn validate_and_prepare_preserves_safe_html() {
        let fields = vec![editor_field("body", 0)];
        let v = RecordValidator::new(&fields);
        let data = json!({
            "body": "<h1>Title</h1><p>A <strong>bold</strong> paragraph.</p>"
        });
        let prepared = v.validate_and_prepare(&data).unwrap();
        let body = prepared["body"].as_str().unwrap();
        assert!(body.contains("<h1>Title</h1>"));
        assert!(body.contains("<strong>bold</strong>"));
    }

    #[test]
    fn validate_and_prepare_mixed_fields() {
        let fields = vec![text_field("title", true), editor_field("content", 0)];
        let v = RecordValidator::new(&fields);
        let data = json!({
            "title": "My Post",
            "content": "<p>Safe</p><script>evil()</script>"
        });
        let prepared = v.validate_and_prepare(&data).unwrap();
        // title unchanged
        assert_eq!(prepared["title"], "My Post");
        // content sanitized
        let content = prepared["content"].as_str().unwrap();
        assert!(content.contains("<p>Safe</p>"));
        assert!(!content.contains("<script>"));
    }

    #[test]
    fn validate_and_prepare_rejects_invalid_data() {
        let fields = vec![text_field("title", true), editor_field("content", 0)];
        let v = RecordValidator::new(&fields);
        // title is required but missing, content has wrong type
        let data = json!({
            "content": 42
        });
        let err = v.validate_and_prepare(&data).unwrap_err();
        match err {
            ZerobaseError::Validation { field_errors, .. } => {
                assert!(field_errors.contains_key("title"));
                assert!(field_errors.contains_key("content"));
            }
            _ => panic!("expected validation error"),
        }
    }

    // ── AutoDate: apply_auto_dates ──────────────────────────────────────────

    fn autodate_field(name: &str, on_create: bool, on_update: bool) -> Field {
        Field::new(
            name,
            FieldType::AutoDate(AutoDateOptions {
                on_create,
                on_update,
            }),
        )
    }

    fn is_valid_datetime(s: &str) -> bool {
        crate::schema::parse_datetime(s).is_some()
    }

    #[test]
    fn apply_auto_dates_injects_on_create() {
        let fields = vec![
            text_field("title", true),
            autodate_field("created_at", true, false),
        ];
        let v = RecordValidator::new(&fields);

        let mut data = json!({ "title": "Hello" });
        v.apply_auto_dates(&mut data, OperationContext::Create);

        let obj = data.as_object().unwrap();
        let created = obj.get("created_at").unwrap().as_str().unwrap();
        assert!(
            is_valid_datetime(created),
            "should be a valid datetime: {created}"
        );
    }

    #[test]
    fn apply_auto_dates_skips_on_create_when_update_only() {
        let fields = vec![
            text_field("title", true),
            autodate_field("modified_at", false, true),
        ];
        let v = RecordValidator::new(&fields);

        let mut data = json!({ "title": "Hello" });
        v.apply_auto_dates(&mut data, OperationContext::Create);

        let obj = data.as_object().unwrap();
        assert!(
            obj.get("modified_at").is_none(),
            "should not set update-only field on create"
        );
    }

    #[test]
    fn apply_auto_dates_injects_on_update() {
        let fields = vec![
            text_field("title", true),
            autodate_field("modified_at", false, true),
        ];
        let v = RecordValidator::new(&fields);

        let mut data = json!({ "title": "Updated" });
        v.apply_auto_dates(&mut data, OperationContext::Update);

        let obj = data.as_object().unwrap();
        let modified = obj.get("modified_at").unwrap().as_str().unwrap();
        assert!(is_valid_datetime(modified));
    }

    #[test]
    fn apply_auto_dates_skips_on_update_when_create_only() {
        let fields = vec![
            text_field("title", true),
            autodate_field("created_at", true, false),
        ];
        let v = RecordValidator::new(&fields);

        let mut data = json!({ "title": "Updated" });
        v.apply_auto_dates(&mut data, OperationContext::Update);

        let obj = data.as_object().unwrap();
        assert!(
            obj.get("created_at").is_none(),
            "should not set create-only field on update"
        );
    }

    #[test]
    fn apply_auto_dates_both_on_create() {
        let fields = vec![autodate_field("timestamp", true, true)];
        let v = RecordValidator::new(&fields);

        let mut data = json!({});
        v.apply_auto_dates(&mut data, OperationContext::Create);

        let obj = data.as_object().unwrap();
        assert!(obj.get("timestamp").is_some());
    }

    #[test]
    fn apply_auto_dates_both_on_update() {
        let fields = vec![autodate_field("timestamp", true, true)];
        let v = RecordValidator::new(&fields);

        let mut data = json!({});
        v.apply_auto_dates(&mut data, OperationContext::Update);

        let obj = data.as_object().unwrap();
        assert!(obj.get("timestamp").is_some());
    }

    #[test]
    fn apply_auto_dates_overrides_manual_value() {
        let fields = vec![autodate_field("created_at", true, false)];
        let v = RecordValidator::new(&fields);

        let mut data = json!({ "created_at": "1999-01-01 00:00:00" });
        v.apply_auto_dates(&mut data, OperationContext::Create);

        let obj = data.as_object().unwrap();
        let created = obj.get("created_at").unwrap().as_str().unwrap();
        // Should be overridden to current time, not 1999
        assert_ne!(
            created, "1999-01-01 00:00:00",
            "manual value should be overridden"
        );
        assert!(is_valid_datetime(created));
    }

    #[test]
    fn apply_auto_dates_ignores_non_autodate_fields() {
        let fields = vec![text_field("title", true), number_field("views", None, None)];
        let v = RecordValidator::new(&fields);

        let mut data = json!({ "title": "Hello", "views": 42 });
        v.apply_auto_dates(&mut data, OperationContext::Create);

        let obj = data.as_object().unwrap();
        assert_eq!(obj.get("title").unwrap(), "Hello");
        assert_eq!(obj.get("views").unwrap(), 42);
    }

    #[test]
    fn apply_auto_dates_noop_on_non_object() {
        let fields = vec![autodate_field("ts", true, true)];
        let v = RecordValidator::new(&fields);

        let mut data = json!("not an object");
        v.apply_auto_dates(&mut data, OperationContext::Create);
        // Should not panic, just return
        assert!(data.is_string());
    }

    // ── AutoDate: validate_and_prepare_with_context ─────────────────────────

    #[test]
    fn validate_and_prepare_with_context_injects_autodate_on_create() {
        let fields = vec![
            text_field("title", true),
            autodate_field("created_at", true, false),
        ];
        let v = RecordValidator::new(&fields);

        let data = json!({ "title": "Hello" });
        let result = v
            .validate_and_prepare_with_context(&data, OperationContext::Create)
            .unwrap();

        let obj = result.as_object().unwrap();
        let created = obj.get("created_at").unwrap().as_str().unwrap();
        assert!(is_valid_datetime(created));
    }

    #[test]
    fn validate_and_prepare_with_context_injects_autodate_on_update() {
        let fields = vec![
            text_field("title", true),
            autodate_field("updated_at", false, true),
        ];
        let v = RecordValidator::new(&fields);

        let data = json!({ "title": "Updated" });
        let result = v
            .validate_and_prepare_with_context(&data, OperationContext::Update)
            .unwrap();

        let obj = result.as_object().unwrap();
        let updated = obj.get("updated_at").unwrap().as_str().unwrap();
        assert!(is_valid_datetime(updated));
    }

    #[test]
    fn validate_and_prepare_with_context_overrides_manual_autodate() {
        let fields = vec![
            text_field("title", true),
            autodate_field("created_at", true, true),
        ];
        let v = RecordValidator::new(&fields);

        // User tries to set created_at manually
        let data = json!({
            "title": "Hello",
            "created_at": "1999-01-01 00:00:00"
        });
        let result = v
            .validate_and_prepare_with_context(&data, OperationContext::Create)
            .unwrap();

        let obj = result.as_object().unwrap();
        let created = obj.get("created_at").unwrap().as_str().unwrap();
        assert_ne!(
            created, "1999-01-01 00:00:00",
            "manual value should be overridden"
        );
        assert!(is_valid_datetime(created));
    }

    #[test]
    fn validate_and_prepare_with_context_does_not_set_update_only_on_create() {
        let fields = vec![
            text_field("title", true),
            autodate_field("modified_at", false, true),
        ];
        let v = RecordValidator::new(&fields);

        let data = json!({ "title": "Hello" });
        let result = v
            .validate_and_prepare_with_context(&data, OperationContext::Create)
            .unwrap();

        let obj = result.as_object().unwrap();
        // modified_at should NOT be set because on_create is false
        assert!(
            obj.get("modified_at").is_none() || obj.get("modified_at").unwrap().is_null(),
            "update-only autodate should not be set on create"
        );
    }

    #[test]
    fn validate_and_prepare_with_context_does_not_set_create_only_on_update() {
        let fields = vec![
            text_field("title", true),
            autodate_field("created_at", true, false),
        ];
        let v = RecordValidator::new(&fields);

        let data = json!({ "title": "Updated" });
        let result = v
            .validate_and_prepare_with_context(&data, OperationContext::Update)
            .unwrap();

        let obj = result.as_object().unwrap();
        // created_at should NOT be injected on update
        assert!(
            obj.get("created_at").is_none() || obj.get("created_at").unwrap().is_null(),
            "create-only autodate should not be set on update"
        );
    }

    #[test]
    fn validate_and_prepare_with_context_rejects_manual_autodate_not_in_schema() {
        // User submits a field that is not in schema at all (unknown field check)
        let fields = vec![
            text_field("title", true),
            autodate_field("created_at", true, false),
        ];
        let v = RecordValidator::new(&fields);

        let data = json!({
            "title": "Hello",
            "not_a_field": "surprise"
        });

        let err = v
            .validate_and_prepare_with_context(&data, OperationContext::Create)
            .unwrap_err();
        let errors = extract_field_errors(&err);
        assert!(errors.contains_key("not_a_field"));
    }

    #[test]
    fn validate_and_prepare_with_context_autodate_not_flagged_as_unknown() {
        // AutoDate fields that the user provides should NOT be flagged as unknown
        // (they are known fields, just auto-managed)
        let fields = vec![
            text_field("title", true),
            autodate_field("created_at", true, true),
        ];
        let v = RecordValidator::new(&fields);

        let data = json!({
            "title": "Hello",
            "created_at": "some value the user tried to set"
        });

        // Even though the user supplied created_at, it should be overridden (not rejected)
        let result = v
            .validate_and_prepare_with_context(&data, OperationContext::Create)
            .unwrap();
        let obj = result.as_object().unwrap();
        let created = obj.get("created_at").unwrap().as_str().unwrap();
        assert!(is_valid_datetime(created));
    }

    #[test]
    fn validate_and_prepare_with_context_multiple_autodate_fields() {
        let fields = vec![
            text_field("title", true),
            autodate_field("created_at", true, false),
            autodate_field("updated_at", true, true),
        ];
        let v = RecordValidator::new(&fields);

        let data = json!({ "title": "Hello" });
        let result = v
            .validate_and_prepare_with_context(&data, OperationContext::Create)
            .unwrap();

        let obj = result.as_object().unwrap();
        // Both should be set on create
        assert!(obj.get("created_at").unwrap().as_str().is_some());
        assert!(obj.get("updated_at").unwrap().as_str().is_some());

        // On update, only updated_at should be injected
        let data2 = json!({ "title": "Updated" });
        let result2 = v
            .validate_and_prepare_with_context(&data2, OperationContext::Update)
            .unwrap();

        let obj2 = result2.as_object().unwrap();
        assert!(
            obj2.get("created_at").is_none() || obj2.get("created_at").unwrap().is_null(),
            "create-only field should not be set on update"
        );
        assert!(obj2.get("updated_at").unwrap().as_str().is_some());
    }

    #[test]
    fn validate_and_prepare_with_context_still_validates_other_fields() {
        let fields = vec![
            text_field("title", true),
            autodate_field("created_at", true, false),
        ];
        let v = RecordValidator::new(&fields);

        // Missing required title
        let data = json!({});
        let err = v
            .validate_and_prepare_with_context(&data, OperationContext::Create)
            .unwrap_err();
        let errors = extract_field_errors(&err);
        assert!(
            errors.contains_key("title"),
            "should still validate required fields"
        );
        // created_at should NOT cause an error (auto-injected)
        assert!(!errors.contains_key("created_at"));
    }
}
