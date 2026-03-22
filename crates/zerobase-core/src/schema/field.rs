//! Field definitions and type-specific options.
//!
//! Each [`Field`] has a name, a [`FieldType`] (which carries its type-specific
//! options), and common properties like `required` and `unique`.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::validation::{validate_name, validate_regex_pattern};
use crate::error::{Result, ZerobaseError};

// ── Field ──────────────────────────────────────────────────────────────────────

/// A single field within a collection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    /// Unique identifier for the field (system-assigned, stable across renames).
    #[serde(default = "default_field_id")]
    pub id: String,
    /// Field name (used as column name and JSON key).
    pub name: String,
    /// The field's type and type-specific options.
    #[serde(rename = "type")]
    pub field_type: FieldType,
    /// Whether a non-empty value is required.
    #[serde(default)]
    pub required: bool,
    /// Whether the value must be unique across all records.
    #[serde(default)]
    pub unique: bool,
    /// Display ordering hint (lower = earlier).
    #[serde(default)]
    pub sort_order: i32,
}

fn default_field_id() -> String {
    uuid::Uuid::new_v4().to_string().replace('-', "")[..15].to_string()
}

impl Field {
    /// Create a new field with the given name and type.
    pub fn new(name: impl Into<String>, field_type: FieldType) -> Self {
        Self {
            id: default_field_id(),
            name: name.into(),
            field_type,
            required: false,
            unique: false,
            sort_order: 0,
        }
    }

    /// Set the `required` flag (builder style).
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// Set the `unique` flag (builder style).
    pub fn unique(mut self, unique: bool) -> Self {
        self.unique = unique;
        self
    }

    /// Validate the field definition itself (not a record value).
    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name, "field name")?;
        self.field_type.validate_options()?;
        Ok(())
    }

    /// Whether this field is marked as searchable for full-text search.
    ///
    /// A field is searchable when:
    /// 1. Its type supports search (`FieldType::is_searchable()`)
    /// 2. Its type-specific options have `searchable = true`
    pub fn is_searchable(&self) -> bool {
        match &self.field_type {
            FieldType::Text(opts) => opts.searchable,
            FieldType::Editor(opts) => opts.searchable,
            // Email and Url fields are searchable by type but don't have an
            // explicit searchable toggle — they are always searchable if the
            // type itself supports it. For now, only Text and Editor have the
            // explicit opt-in flag.
            _ => false,
        }
    }

    /// Validate a record value against this field's type and constraints.
    ///
    /// Returns `Ok(())` if the value is acceptable, or a `Validation` error
    /// with details about what's wrong.
    pub fn validate_value(&self, value: &serde_json::Value) -> Result<()> {
        // Null / missing check
        if value.is_null() {
            if self.required {
                return Err(field_error(&self.name, "value is required"));
            }
            return Ok(());
        }

        // For text-like fields, handle empty strings specially.
        if let Some(s) = value.as_str() {
            if s.is_empty() && self.field_type.is_text_like() {
                if self.required {
                    return Err(field_error(&self.name, "value is required"));
                }
                // Non-required empty string is treated as "no value" — skip type validation.
                return Ok(());
            }
        }

        self.field_type.validate_value(&self.name, value)
    }

    /// The SQLite type affinity for this field.
    pub fn sql_type(&self) -> &'static str {
        self.field_type.sql_type()
    }
}

// ── FieldType ──────────────────────────────────────────────────────────────────

/// All supported field types with their type-specific options.
///
/// Modeled after PocketBase's field system. Each variant carries an options
/// struct so that field constraints are stored together with the type tag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "options", rename_all = "camelCase")]
pub enum FieldType {
    /// Plain or rich text.
    Text(TextOptions),
    /// Integer or floating-point number.
    Number(NumberOptions),
    /// Boolean (true/false).
    Bool(BoolOptions),
    /// Email address with format validation.
    Email(EmailOptions),
    /// URL with format validation.
    Url(UrlOptions),
    /// Date and/or time value (ISO 8601).
    DateTime(DateTimeOptions),
    /// Auto-set timestamp on create and/or update.
    AutoDate(AutoDateOptions),
    /// Single select from a predefined list of values.
    Select(SelectOptions),
    /// Multiple select from a predefined list of values (stored as JSON array).
    MultiSelect(MultiSelectOptions),
    /// File attachment(s).
    File(FileOptions),
    /// Relation to another collection (foreign key).
    Relation(RelationOptions),
    /// Arbitrary JSON data.
    Json(JsonOptions),
    /// Rich text HTML content.
    Editor(EditorOptions),
    /// Password field (hashed, never returned in API responses).
    Password(PasswordOptions),
}

impl FieldType {
    /// Validate the options for this field type.
    pub fn validate_options(&self) -> Result<()> {
        match self {
            Self::Text(opts) => opts.validate(),
            Self::Number(opts) => opts.validate(),
            Self::Bool(_) => Ok(()),
            Self::Email(opts) => opts.validate(),
            Self::Url(opts) => opts.validate(),
            Self::DateTime(opts) => opts.validate(),
            Self::AutoDate(opts) => opts.validate(),
            Self::Select(opts) => opts.validate(),
            Self::MultiSelect(opts) => opts.validate(),
            Self::File(opts) => opts.validate(),
            Self::Relation(opts) => opts.validate(),
            Self::Json(opts) => opts.validate(),
            Self::Editor(opts) => opts.validate(),
            Self::Password(opts) => opts.validate(),
        }
    }

    /// Validate a non-null record value against this field type.
    pub fn validate_value(&self, field_name: &str, value: &serde_json::Value) -> Result<()> {
        match self {
            Self::Text(opts) => opts.validate_value(field_name, value),
            Self::Number(opts) => opts.validate_value(field_name, value),
            Self::Bool(_) => validate_bool_value(field_name, value),
            Self::Email(opts) => opts.validate_value(field_name, value),
            Self::Url(opts) => opts.validate_value(field_name, value),
            Self::DateTime(opts) => opts.validate_value(field_name, value),
            Self::AutoDate(opts) => opts.validate_value(field_name, value),
            Self::Select(opts) => opts.validate_value(field_name, value),
            Self::MultiSelect(opts) => opts.validate_value(field_name, value),
            Self::File(opts) => opts.validate_value(field_name, value),
            Self::Relation(opts) => opts.validate_value(field_name, value),
            Self::Json(opts) => opts.validate_value(field_name, value),
            Self::Editor(opts) => opts.validate_value(field_name, value),
            Self::Password(opts) => opts.validate_value(field_name, value),
        }
    }

    /// The SQLite type affinity for this field type.
    pub fn sql_type(&self) -> &'static str {
        match self {
            Self::Text(_)
            | Self::Email(_)
            | Self::Url(_)
            | Self::DateTime(_)
            | Self::AutoDate(_)
            | Self::Select(_)
            | Self::MultiSelect(_)
            | Self::Relation(_)
            | Self::Editor(_)
            | Self::Password(_) => "TEXT",
            Self::Number(_) => "REAL",
            Self::Bool(_) => "INTEGER",
            Self::File(_) | Self::Json(_) => "TEXT",
        }
    }

    /// Whether this field type stores string data (for empty-string required checks).
    pub fn is_text_like(&self) -> bool {
        matches!(
            self,
            Self::Text(_)
                | Self::Email(_)
                | Self::Url(_)
                | Self::DateTime(_)
                | Self::Select(_)
                | Self::Editor(_)
                | Self::Password(_)
        )
    }

    /// Whether this field type supports full-text search.
    ///
    /// Only text-like fields that store user-visible content can be searchable.
    /// Password and AutoDate fields are excluded.
    pub fn is_searchable(&self) -> bool {
        matches!(
            self,
            Self::Text(_) | Self::Email(_) | Self::Url(_) | Self::Editor(_)
        )
    }

    /// Human-readable type name (matches the serde tag).
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Number(_) => "number",
            Self::Bool(_) => "bool",
            Self::Email(_) => "email",
            Self::Url(_) => "url",
            Self::DateTime(_) => "dateTime",
            Self::AutoDate(_) => "autoDate",
            Self::Select(_) => "select",
            Self::MultiSelect(_) => "multiSelect",
            Self::File(_) => "file",
            Self::Relation(_) => "relation",
            Self::Json(_) => "json",
            Self::Editor(_) => "editor",
            Self::Password(_) => "password",
        }
    }

    /// Prepare/sanitize a value before storage.
    ///
    /// Most field types return the value unchanged. The **Editor** field
    /// sanitizes HTML to prevent XSS.
    ///
    /// Returns `None` if the value is `Null` (callers should keep it as-is).
    pub fn prepare_value(&self, value: &serde_json::Value) -> Option<serde_json::Value> {
        if value.is_null() {
            return None;
        }
        match self {
            Self::Bool(opts) => opts.prepare_value(value),
            Self::Number(opts) => opts.prepare_value(value),
            Self::Editor(opts) => opts.prepare_value(value),
            Self::DateTime(opts) => opts.prepare_value(value),
            _ => Some(value.clone()),
        }
    }
}

// ── Options structs ────────────────────────────────────────────────────────────

/// Options for [`FieldType::Text`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextOptions {
    /// Minimum length (0 = no minimum).
    #[serde(default)]
    pub min_length: u32,
    /// Maximum length (0 = no limit).
    #[serde(default)]
    pub max_length: u32,
    /// Optional regex pattern the value must match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Whether this field is included in full-text search indexes.
    #[serde(default)]
    pub searchable: bool,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            min_length: 0,
            max_length: 0,
            pattern: None,
            searchable: false,
        }
    }
}

impl TextOptions {
    pub fn validate(&self) -> Result<()> {
        if self.max_length > 0 && self.min_length > self.max_length {
            return Err(field_error("minLength", "must not exceed maxLength"));
        }
        if let Some(ref p) = self.pattern {
            validate_regex_pattern(p)?;
        }
        Ok(())
    }

    pub fn validate_value(&self, field_name: &str, value: &serde_json::Value) -> Result<()> {
        let s = value
            .as_str()
            .ok_or_else(|| field_error(field_name, "must be a string"))?;

        // Use char count (Unicode code points), not byte length,
        // so multi-byte characters count as one character each.
        let char_count = s.chars().count() as u32;

        if self.min_length > 0 && char_count < self.min_length {
            return Err(field_error(
                field_name,
                &format!("must be at least {} characters", self.min_length),
            ));
        }

        if self.max_length > 0 && char_count > self.max_length {
            return Err(field_error(
                field_name,
                &format!("must be at most {} characters", self.max_length),
            ));
        }

        if let Some(ref pattern) = self.pattern {
            let re = regex::Regex::new(pattern)
                .map_err(|e| field_error(field_name, &format!("invalid pattern: {e}")))?;
            if !re.is_match(s) {
                return Err(field_error(
                    field_name,
                    &format!("must match pattern {pattern}"),
                ));
            }
        }

        Ok(())
    }
}

/// Options for [`FieldType::Number`].
///
/// Supports both integer and floating-point numbers. When `only_int` is set,
/// only whole numbers are accepted. The value is stored as `REAL` in SQLite.
///
/// The `only_int` field is also accepted as `noDecimal` during deserialization
/// for PocketBase compatibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberOptions {
    /// Minimum value (inclusive). If `Some`, values below this are rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Maximum value (inclusive). If `Some`, values above this are rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Whether only integer values are allowed (no fractional part).
    /// Also accepted as `noDecimal` during deserialization (PocketBase compat).
    #[serde(default, alias = "noDecimal")]
    pub only_int: bool,
}

impl Default for NumberOptions {
    fn default() -> Self {
        Self {
            min: None,
            max: None,
            only_int: false,
        }
    }
}

impl NumberOptions {
    /// Validate the options themselves (not a record value).
    ///
    /// Checks that min ≤ max when both are specified, and that neither
    /// min nor max are NaN or infinite.
    pub fn validate(&self) -> Result<()> {
        if let Some(min) = self.min {
            if min.is_nan() || min.is_infinite() {
                return Err(field_error("min", "must be a finite number"));
            }
        }
        if let Some(max) = self.max {
            if max.is_nan() || max.is_infinite() {
                return Err(field_error("max", "must be a finite number"));
            }
        }
        if let (Some(min), Some(max)) = (self.min, self.max) {
            if min > max {
                return Err(field_error("min", "must not exceed max"));
            }
        }
        Ok(())
    }

    /// Validate a record value against these number constraints.
    ///
    /// The value must be a JSON number. If `only_int` is set, the number must
    /// have no fractional part. Min/max bounds are enforced when present.
    /// NaN and infinite values are rejected.
    pub fn validate_value(&self, field_name: &str, value: &serde_json::Value) -> Result<()> {
        let n = value
            .as_f64()
            .ok_or_else(|| field_error(field_name, "must be a number"))?;

        if n.is_nan() || n.is_infinite() {
            return Err(field_error(field_name, "must be a finite number"));
        }

        if self.only_int && n.fract() != 0.0 {
            return Err(field_error(field_name, "must be an integer"));
        }

        if let Some(min) = self.min {
            if n < min {
                return Err(field_error(field_name, &format!("must be at least {min}")));
            }
        }

        if let Some(max) = self.max {
            if n > max {
                return Err(field_error(field_name, &format!("must be at most {max}")));
            }
        }

        Ok(())
    }

    /// Coerce a JSON value into a number if possible.
    ///
    /// - JSON numbers pass through unchanged.
    /// - Strings that parse as a number are converted to a JSON number.
    /// - Returns `None` for null (caller keeps as-is) or unconvertible values.
    pub fn prepare_value(&self, value: &serde_json::Value) -> Option<serde_json::Value> {
        match value {
            serde_json::Value::Null => None,
            serde_json::Value::Number(_) => Some(value.clone()),
            serde_json::Value::String(s) => {
                let s = s.trim();
                if s.is_empty() {
                    return Some(serde_json::Value::Null);
                }
                s.parse::<f64>().ok().and_then(|n| {
                    if n.is_finite() {
                        serde_json::Number::from_f64(n).map(serde_json::Value::Number)
                    } else {
                        None
                    }
                })
            }
            serde_json::Value::Bool(b) => {
                let n = if *b { 1.0 } else { 0.0 };
                serde_json::Number::from_f64(n).map(serde_json::Value::Number)
            }
            _ => None,
        }
    }
}

/// Options for [`FieldType::Bool`]. (No options currently.)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BoolOptions {}

impl BoolOptions {
    /// Normalize various truthy/falsy inputs to a JSON boolean.
    ///
    /// Accepted inputs:
    /// - `true` / `false` (JSON booleans) → pass through
    /// - `1` / `0` (JSON numbers) → `true` / `false`
    /// - `"true"` / `"1"` / `"yes"` (case-insensitive, trimmed) → `true`
    /// - `"false"` / `"0"` / `"no"` (case-insensitive, trimmed) → `false`
    /// - `""` (empty or whitespace-only string) → `null` (treated as absent)
    /// - `null` → `None` (caller keeps as-is)
    ///
    /// Returns `None` for null or for inputs that cannot be interpreted as a
    /// boolean (arrays, objects, unrecognised strings).
    pub fn prepare_value(&self, value: &serde_json::Value) -> Option<serde_json::Value> {
        match value {
            serde_json::Value::Null => None,
            serde_json::Value::Bool(_) => Some(value.clone()),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    match i {
                        0 => Some(serde_json::Value::Bool(false)),
                        1 => Some(serde_json::Value::Bool(true)),
                        _ => None,
                    }
                } else if let Some(f) = n.as_f64() {
                    if f == 0.0 {
                        Some(serde_json::Value::Bool(false))
                    } else if f == 1.0 {
                        Some(serde_json::Value::Bool(true))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            serde_json::Value::String(s) => {
                let s = s.trim();
                if s.is_empty() {
                    return Some(serde_json::Value::Null);
                }
                match s.to_lowercase().as_str() {
                    "true" | "1" | "yes" => Some(serde_json::Value::Bool(true)),
                    "false" | "0" | "no" => Some(serde_json::Value::Bool(false)),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

fn validate_bool_value(field_name: &str, value: &serde_json::Value) -> Result<()> {
    if !value.is_boolean() {
        return Err(field_error(field_name, "must be a boolean"));
    }
    Ok(())
}

/// Options for [`FieldType::Email`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailOptions {
    /// If non-empty, only emails with these domains are accepted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only_domains: Vec<String>,
    /// If non-empty, emails from these domains are rejected.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub except_domains: Vec<String>,
}

impl Default for EmailOptions {
    fn default() -> Self {
        Self {
            only_domains: Vec::new(),
            except_domains: Vec::new(),
        }
    }
}

impl EmailOptions {
    pub fn validate(&self) -> Result<()> {
        if !self.only_domains.is_empty() && !self.except_domains.is_empty() {
            return Err(field_error(
                "onlyDomains",
                "cannot specify both onlyDomains and exceptDomains",
            ));
        }

        // Validate that domain entries are non-empty and well-formed.
        for d in &self.only_domains {
            if d.trim().is_empty() {
                return Err(field_error(
                    "onlyDomains",
                    "domain entries must not be empty",
                ));
            }
        }
        for d in &self.except_domains {
            if d.trim().is_empty() {
                return Err(field_error(
                    "exceptDomains",
                    "domain entries must not be empty",
                ));
            }
        }

        Ok(())
    }

    pub fn validate_value(&self, field_name: &str, value: &serde_json::Value) -> Result<()> {
        let s = value
            .as_str()
            .ok_or_else(|| field_error(field_name, "must be a string"))?;

        // Reject whitespace anywhere in the address.
        if s.chars().any(|c| c.is_whitespace()) {
            return Err(field_error(field_name, "must be a valid email address"));
        }

        // Must contain exactly one @ with non-empty local and domain parts.
        let parts: Vec<&str> = s.splitn(2, '@').collect();
        if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
            return Err(field_error(field_name, "must be a valid email address"));
        }

        let local = parts[0];
        let domain_raw = parts[1];

        // RFC 5321 length limits.
        if local.len() > 64 {
            return Err(field_error(field_name, "must be a valid email address"));
        }
        if domain_raw.len() > 255 {
            return Err(field_error(field_name, "must be a valid email address"));
        }

        let domain = domain_raw.to_lowercase();

        // Domain must contain at least one dot (TLD required).
        if !domain.contains('.') {
            return Err(field_error(field_name, "must be a valid email address"));
        }

        // Validate domain labels: no empty labels, no leading/trailing hyphens,
        // only alphanumeric and hyphens, TLD at least 2 chars.
        let labels: Vec<&str> = domain.split('.').collect();
        for label in &labels {
            if label.is_empty()
                || label.starts_with('-')
                || label.ends_with('-')
                || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            {
                return Err(field_error(field_name, "must be a valid email address"));
            }
        }

        // TLD must be at least 2 characters.
        if let Some(tld) = labels.last() {
            if tld.len() < 2 {
                return Err(field_error(field_name, "must be a valid email address"));
            }
        }

        // Domain allow-list.
        if !self.only_domains.is_empty() {
            let allowed = self.only_domains.iter().any(|d| domain == d.to_lowercase());
            if !allowed {
                return Err(field_error(field_name, "email domain is not allowed"));
            }
        }

        // Domain block-list.
        if !self.except_domains.is_empty() {
            let blocked = self
                .except_domains
                .iter()
                .any(|d| domain == d.to_lowercase());
            if blocked {
                return Err(field_error(field_name, "email domain is not allowed"));
            }
        }

        Ok(())
    }
}

/// Options for [`FieldType::Url`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UrlOptions {
    /// If non-empty, only URLs with these schemes are accepted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only_schemes: Vec<String>,
    /// If non-empty, only URLs with these domains are accepted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only_domains: Vec<String>,
    /// If non-empty, URLs from these domains are rejected.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub except_domains: Vec<String>,
}

impl Default for UrlOptions {
    fn default() -> Self {
        Self {
            only_schemes: Vec::new(),
            only_domains: Vec::new(),
            except_domains: Vec::new(),
        }
    }
}

impl UrlOptions {
    pub fn validate(&self) -> Result<()> {
        if !self.only_domains.is_empty() && !self.except_domains.is_empty() {
            return Err(field_error(
                "onlyDomains",
                "cannot specify both onlyDomains and exceptDomains",
            ));
        }

        for d in &self.only_domains {
            if d.trim().is_empty() {
                return Err(field_error(
                    "onlyDomains",
                    "domain entries must not be empty",
                ));
            }
        }
        for d in &self.except_domains {
            if d.trim().is_empty() {
                return Err(field_error(
                    "exceptDomains",
                    "domain entries must not be empty",
                ));
            }
        }

        Ok(())
    }

    /// Extract the host (domain) portion from the authority of a URL.
    ///
    /// Given the part after `://`, this strips optional `userinfo@`, port,
    /// and any path/query/fragment that follows.
    fn extract_host(authority_and_rest: &str) -> Option<&str> {
        // Strip userinfo (everything before the last `@` in the authority).
        let after_userinfo = match authority_and_rest.find('@') {
            Some(idx) => &authority_and_rest[idx + 1..],
            None => authority_and_rest,
        };

        // The host extends until the first `/`, `?`, `#`, or `:` (port).
        let host_end = after_userinfo
            .find(|c: char| c == '/' || c == '?' || c == '#' || c == ':')
            .unwrap_or(after_userinfo.len());

        let host = &after_userinfo[..host_end];
        if host.is_empty() {
            return None;
        }
        Some(host)
    }

    pub fn validate_value(&self, field_name: &str, value: &serde_json::Value) -> Result<()> {
        let s = value
            .as_str()
            .ok_or_else(|| field_error(field_name, "must be a string"))?;

        // Reject whitespace anywhere in the URL.
        if s.chars().any(|c| c.is_whitespace()) {
            return Err(field_error(field_name, "must be a valid URL"));
        }

        // Must contain "://" with a non-empty scheme.
        let scheme_end = s
            .find("://")
            .ok_or_else(|| field_error(field_name, "must be a valid URL"))?;

        let scheme = &s[..scheme_end];

        // Scheme must be non-empty and consist of ASCII letters, digits, `+`, `-`, `.`
        // (first char must be a letter per RFC 3986).
        if scheme.is_empty() {
            return Err(field_error(field_name, "must be a valid URL"));
        }
        if !scheme.as_bytes()[0].is_ascii_alphabetic() {
            return Err(field_error(field_name, "must be a valid URL"));
        }
        if !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
        {
            return Err(field_error(field_name, "must be a valid URL"));
        }

        // Must have something after "://".
        let after_scheme = &s[scheme_end + 3..];
        if after_scheme.is_empty() {
            return Err(field_error(field_name, "must be a valid URL"));
        }

        // Check scheme allow-list.
        if !self.only_schemes.is_empty() {
            let allowed = self
                .only_schemes
                .iter()
                .any(|sc| scheme.eq_ignore_ascii_case(sc));
            if !allowed {
                return Err(field_error(
                    field_name,
                    &format!(
                        "URL scheme must be one of: {}",
                        self.only_schemes.join(", ")
                    ),
                ));
            }
        }

        // Domain filtering – extract host if present.
        if !self.only_domains.is_empty() || !self.except_domains.is_empty() {
            let host = Self::extract_host(after_scheme)
                .ok_or_else(|| field_error(field_name, "must be a valid URL"))?;
            let host_lower = host.to_lowercase();

            // Domain allow-list.
            if !self.only_domains.is_empty() {
                let allowed = self
                    .only_domains
                    .iter()
                    .any(|d| host_lower == d.to_lowercase());
                if !allowed {
                    return Err(field_error(field_name, "URL domain is not allowed"));
                }
            }

            // Domain block-list.
            if !self.except_domains.is_empty() {
                let blocked = self
                    .except_domains
                    .iter()
                    .any(|d| host_lower == d.to_lowercase());
                if blocked {
                    return Err(field_error(field_name, "URL domain is not allowed"));
                }
            }
        }

        Ok(())
    }
}

/// Timezone handling mode for [`DateTimeOptions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DateTimeMode {
    /// Accept any valid ISO 8601 date/time; store as-is.
    /// Timezone-aware values are converted to UTC for storage.
    UtcOnly,
    /// Accept timezone-aware ISO 8601 values and preserve the timezone offset.
    TimezoneAware,
    /// Accept date-only values (`YYYY-MM-DD`); reject values with a time component.
    DateOnly,
}

impl Default for DateTimeMode {
    fn default() -> Self {
        Self::UtcOnly
    }
}

/// Options for [`FieldType::DateTime`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DateTimeOptions {
    /// Minimum date (ISO 8601 string), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    /// Maximum date (ISO 8601 string), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
    /// How to handle timezones.
    #[serde(default)]
    pub mode: DateTimeMode,
}

impl DateTimeOptions {
    /// Validate the options themselves (not a record value).
    pub fn validate(&self) -> Result<()> {
        if let Some(ref min_str) = self.min {
            if parse_datetime(min_str).is_none() {
                return Err(field_error(
                    "min",
                    "must be a valid ISO 8601 date/time string",
                ));
            }
        }
        if let Some(ref max_str) = self.max {
            if parse_datetime(max_str).is_none() {
                return Err(field_error(
                    "max",
                    "must be a valid ISO 8601 date/time string",
                ));
            }
        }
        // If both are set, min must not exceed max.
        if let (Some(ref min_str), Some(ref max_str)) = (&self.min, &self.max) {
            if let (Some(min_dt), Some(max_dt)) = (parse_datetime(min_str), parse_datetime(max_str))
            {
                if min_dt > max_dt {
                    return Err(field_error("min", "must not exceed max"));
                }
            }
        }
        Ok(())
    }

    pub fn validate_value(&self, field_name: &str, value: &serde_json::Value) -> Result<()> {
        let s = value
            .as_str()
            .ok_or_else(|| field_error(field_name, "must be a string"))?;

        match self.mode {
            DateTimeMode::DateOnly => {
                // Only accept YYYY-MM-DD
                let _date = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
                    field_error(field_name, "must be a valid date string (YYYY-MM-DD)")
                })?;
            }
            DateTimeMode::TimezoneAware => {
                // Must include timezone offset
                let _dt = chrono::DateTime::parse_from_rfc3339(s).map_err(|_| {
                    field_error(
                        field_name,
                        "must be a valid ISO 8601 date/time with timezone offset (e.g. 2024-01-15T10:30:00+05:00)",
                    )
                })?;
            }
            DateTimeMode::UtcOnly => {
                // Accept any valid format; will be normalised to UTC for storage.
                if parse_datetime(s).is_none() {
                    return Err(field_error(
                        field_name,
                        "must be a valid date/time string (ISO 8601)",
                    ));
                }
            }
        }

        // Range checks always use parse_datetime (normalised to NaiveDateTime).
        let parsed = parse_datetime(s).ok_or_else(|| {
            field_error(field_name, "must be a valid date/time string (ISO 8601)")
        })?;

        if let Some(ref min_str) = self.min {
            if let Some(min_dt) = parse_datetime(min_str) {
                if parsed < min_dt {
                    return Err(field_error(
                        field_name,
                        &format!("must be at or after {min_str}"),
                    ));
                }
            }
        }

        if let Some(ref max_str) = self.max {
            if let Some(max_dt) = parse_datetime(max_str) {
                if parsed > max_dt {
                    return Err(field_error(
                        field_name,
                        &format!("must be at or before {max_str}"),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Normalise a value for storage.
    ///
    /// In `UtcOnly` mode, timezone-aware values are converted to UTC.
    /// In `DateOnly` mode, the value is stored as `YYYY-MM-DD`.
    /// In `TimezoneAware` mode, the original string is preserved.
    pub fn prepare_value(&self, value: &serde_json::Value) -> Option<serde_json::Value> {
        let s = value.as_str()?;
        match self.mode {
            DateTimeMode::UtcOnly => {
                // Convert to UTC if timezone info is present, otherwise keep naive.
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                    Some(serde_json::Value::String(
                        dt.with_timezone(&chrono::Utc)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string(),
                    ))
                } else {
                    Some(value.clone())
                }
            }
            DateTimeMode::DateOnly => {
                // Ensure stored as YYYY-MM-DD only.
                if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                    Some(serde_json::Value::String(d.format("%Y-%m-%d").to_string()))
                } else {
                    Some(value.clone())
                }
            }
            DateTimeMode::TimezoneAware => {
                // Preserve as-is.
                Some(value.clone())
            }
        }
    }
}

/// Parse a datetime string in various supported formats, returning a NaiveDateTime.
///
/// Supports:
/// - RFC 3339 / ISO 8601 with timezone (converted to UTC)
/// - `YYYY-MM-DD HH:MM:SS` (naive)
/// - `YYYY-MM-DD` (date-only, time set to 00:00:00)
pub fn parse_datetime(s: &str) -> Option<chrono::NaiveDateTime> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.naive_utc());
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(dt);
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(d.and_hms_opt(0, 0, 0).unwrap());
    }
    None
}

/// Compare two datetime strings.
///
/// Returns `None` if either string is not a valid datetime.
/// Returns `Some(Ordering)` comparing the two datetimes chronologically.
pub fn compare_datetimes(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    let a_dt = parse_datetime(a)?;
    let b_dt = parse_datetime(b)?;
    Some(a_dt.cmp(&b_dt))
}

/// Options for [`FieldType::AutoDate`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoDateOptions {
    /// Set the field's value on record creation.
    #[serde(default = "default_true")]
    pub on_create: bool,
    /// Update the field's value on record update.
    #[serde(default = "default_true")]
    pub on_update: bool,
}

fn default_true() -> bool {
    true
}

impl Default for AutoDateOptions {
    fn default() -> Self {
        Self {
            on_create: true,
            on_update: true,
        }
    }
}

impl AutoDateOptions {
    pub fn validate(&self) -> Result<()> {
        if !self.on_create && !self.on_update {
            return Err(field_error(
                "autoDate",
                "at least one of onCreate or onUpdate must be true",
            ));
        }
        Ok(())
    }

    /// Validate that a non-null value is a valid datetime string.
    ///
    /// AutoDate fields are auto-managed, but if a value is already present
    /// (e.g. read back from the database) it must be a valid datetime.
    pub fn validate_value(&self, field_name: &str, value: &serde_json::Value) -> Result<()> {
        let s = value
            .as_str()
            .ok_or_else(|| field_error(field_name, "must be a string"))?;
        if parse_datetime(s).is_none() {
            return Err(field_error(field_name, "invalid datetime format"));
        }
        Ok(())
    }

    /// Generate the current UTC datetime as an ISO 8601 string.
    pub fn now_utc() -> String {
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
    }
}

/// Options for [`FieldType::Select`] (single value from a predefined list).
///
/// The value is stored as a plain TEXT string in SQLite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectOptions {
    /// The allowed values.
    pub values: Vec<String>,
}

impl Default for SelectOptions {
    fn default() -> Self {
        Self { values: Vec::new() }
    }
}

impl SelectOptions {
    pub fn validate(&self) -> Result<()> {
        if self.values.is_empty() {
            return Err(field_error("values", "must have at least one value"));
        }

        // Check for duplicate values.
        let mut seen = std::collections::HashSet::new();
        for v in &self.values {
            if !seen.insert(v) {
                return Err(field_error("values", &format!("duplicate value: {v}")));
            }
        }

        Ok(())
    }

    pub fn validate_value(&self, field_name: &str, value: &serde_json::Value) -> Result<()> {
        let s = value
            .as_str()
            .ok_or_else(|| field_error(field_name, "must be a string"))?;
        if !self.values.contains(&s.to_string()) {
            return Err(field_error(
                field_name,
                &format!("must be one of: {}", self.values.join(", ")),
            ));
        }
        Ok(())
    }
}

/// Options for [`FieldType::MultiSelect`] (multiple values from a predefined list).
///
/// Values are stored as a JSON array in a TEXT column in SQLite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiSelectOptions {
    /// The allowed values.
    pub values: Vec<String>,
    /// Maximum number of values that can be selected (0 = no limit).
    #[serde(default)]
    pub max_select: u32,
}

fn default_one() -> u32 {
    1
}

impl Default for MultiSelectOptions {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            max_select: 0,
        }
    }
}

impl MultiSelectOptions {
    pub fn validate(&self) -> Result<()> {
        if self.values.is_empty() {
            return Err(field_error("values", "must have at least one value"));
        }

        // Check for duplicate values.
        let mut seen = std::collections::HashSet::new();
        for v in &self.values {
            if !seen.insert(v) {
                return Err(field_error("values", &format!("duplicate value: {v}")));
            }
        }

        Ok(())
    }

    pub fn validate_value(&self, field_name: &str, value: &serde_json::Value) -> Result<()> {
        let arr = value
            .as_array()
            .ok_or_else(|| field_error(field_name, "must be an array"))?;

        if self.max_select > 0 && arr.len() as u32 > self.max_select {
            return Err(field_error(
                field_name,
                &format!("must select at most {} values", self.max_select),
            ));
        }

        // Check for duplicate selections.
        let mut selected = std::collections::HashSet::new();
        for item in arr {
            let s = item
                .as_str()
                .ok_or_else(|| field_error(field_name, "array items must be strings"))?;
            if !self.values.contains(&s.to_string()) {
                return Err(field_error(
                    field_name,
                    &format!("{s} is not an allowed value"),
                ));
            }
            if !selected.insert(s) {
                return Err(field_error(
                    field_name,
                    &format!("duplicate selection: {s}"),
                ));
            }
        }

        Ok(())
    }
}

/// Options for [`FieldType::File`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOptions {
    /// Maximum number of files (1 = single file).
    #[serde(default = "default_one")]
    pub max_select: u32,
    /// Maximum file size in bytes (0 = default server limit).
    #[serde(default)]
    pub max_size: u64,
    /// Allowed MIME types (empty = all types).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mime_types: Vec<String>,
    /// Thumbnail sizes to auto-generate (e.g., "100x100", "200x200f").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thumbs: Vec<String>,
    /// Whether files require a token for download.
    #[serde(default)]
    pub protected: bool,
}

impl Default for FileOptions {
    fn default() -> Self {
        Self {
            max_select: 1,
            max_size: 0,
            mime_types: Vec::new(),
            thumbs: Vec::new(),
            protected: false,
        }
    }
}

/// Regex for validating thumbnail size specifications.
///
/// Accepts formats like `"100x100"`, `"200x0"`, `"0x150"`, and optional
/// suffix characters for resize mode: `t` (top), `b` (bottom), `f` (fit/force).
/// Examples: `"100x100"`, `"200x200f"`, `"50x0t"`.
static THUMB_SIZE_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"^(\d+)x(\d+)([tbf]?)$").unwrap());

/// Regex for validating MIME type format (`type/subtype`).
static MIME_TYPE_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9!#$&\-^_.+]*\/[a-zA-Z0-9][a-zA-Z0-9!#$&\-^_.+*]*$")
        .unwrap()
});

impl FileOptions {
    /// Validate the file field options (called when defining/updating a field).
    pub fn validate(&self) -> Result<()> {
        if self.max_select == 0 {
            return Err(field_error("maxSelect", "must be at least 1"));
        }

        // Validate MIME type format.
        for mt in &self.mime_types {
            if !MIME_TYPE_RE.is_match(mt) {
                return Err(field_error(
                    "mimeTypes",
                    &format!("invalid MIME type format: {mt}"),
                ));
            }
        }

        // Validate thumbnail size specifications.
        for thumb in &self.thumbs {
            let caps = THUMB_SIZE_RE.captures(thumb).ok_or_else(|| {
                field_error(
                    "thumbs",
                    &format!(
                        "invalid thumbnail size: {thumb} (expected format: WxH or WxH[t|b|f])"
                    ),
                )
            })?;

            let w: u32 = caps[1].parse().unwrap_or(0);
            let h: u32 = caps[2].parse().unwrap_or(0);
            if w == 0 && h == 0 {
                return Err(field_error(
                    "thumbs",
                    &format!("thumbnail size must have at least one non-zero dimension: {thumb}"),
                ));
            }
        }

        Ok(())
    }

    /// Validate file metadata stored in a record value.
    ///
    /// File fields store filename(s) as metadata:
    /// - Single file (`max_select == 1`): a non-empty string filename.
    /// - Multiple files (`max_select > 1`): a JSON array of non-empty string filenames.
    pub fn validate_value(&self, field_name: &str, value: &serde_json::Value) -> Result<()> {
        if self.max_select == 1 {
            // Single file: must be a non-empty string filename.
            let s = value
                .as_str()
                .ok_or_else(|| field_error(field_name, "must be a string (filename)"))?;
            if s.is_empty() {
                return Err(field_error(field_name, "filename must not be empty"));
            }
        } else {
            // Multiple files: must be an array of non-empty string filenames.
            let arr = value
                .as_array()
                .ok_or_else(|| field_error(field_name, "must be an array of filenames"))?;

            if arr.len() as u32 > self.max_select {
                return Err(field_error(
                    field_name,
                    &format!("must have at most {} file(s)", self.max_select),
                ));
            }

            // Check for duplicate filenames.
            let mut seen = HashSet::new();
            for item in arr {
                let s = item
                    .as_str()
                    .ok_or_else(|| field_error(field_name, "each filename must be a string"))?;
                if s.is_empty() {
                    return Err(field_error(field_name, "filename must not be empty"));
                }
                if !seen.insert(s) {
                    return Err(field_error(field_name, &format!("duplicate filename: {s}")));
                }
            }
        }

        Ok(())
    }
}

/// Action to take on referencing records when the referenced record is deleted.
///
/// Controls the cascade behavior for relation fields, similar to SQL foreign key
/// ON DELETE actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OnDeleteAction {
    /// Delete all referencing records when the referenced record is deleted.
    Cascade,
    /// Set the relation field to null (or remove from array) when the referenced
    /// record is deleted.
    SetNull,
    /// Prevent deletion of the referenced record while references exist.
    Restrict,
    /// Do nothing — leave dangling references. This is the default for
    /// backwards compatibility and for cases where the application manages
    /// referential integrity itself.
    NoAction,
}

impl Default for OnDeleteAction {
    fn default() -> Self {
        Self::NoAction
    }
}

impl std::fmt::Display for OnDeleteAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cascade => write!(f, "CASCADE"),
            Self::SetNull => write!(f, "SET_NULL"),
            Self::Restrict => write!(f, "RESTRICT"),
            Self::NoAction => write!(f, "NO_ACTION"),
        }
    }
}

/// Options for [`FieldType::Relation`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationOptions {
    /// The ID or name of the related collection.
    pub collection_id: String,
    /// Maximum number of related records (1 = single relation, 0 = unlimited).
    #[serde(default = "default_one")]
    pub max_select: u32,
    /// Action to take on referencing records when the referenced record is deleted.
    #[serde(default)]
    pub on_delete: OnDeleteAction,
    /// Legacy field: when `true`, treated as `OnDeleteAction::Cascade`.
    /// Prefer `on_delete` for new code. If both are set, `on_delete` takes priority
    /// unless it is `NoAction` and `cascade_delete` is `true`.
    #[serde(default)]
    pub cascade_delete: bool,
}

impl Default for RelationOptions {
    fn default() -> Self {
        Self {
            collection_id: String::new(),
            max_select: 1,
            on_delete: OnDeleteAction::NoAction,
            cascade_delete: false,
        }
    }
}

impl RelationOptions {
    pub fn validate(&self) -> Result<()> {
        if self.collection_id.is_empty() {
            return Err(field_error("collectionId", "must not be empty"));
        }
        Ok(())
    }

    /// Resolve the effective on-delete action, accounting for the legacy
    /// `cascade_delete` boolean.
    pub fn effective_on_delete(&self) -> OnDeleteAction {
        if self.on_delete != OnDeleteAction::NoAction {
            return self.on_delete;
        }
        if self.cascade_delete {
            OnDeleteAction::Cascade
        } else {
            OnDeleteAction::NoAction
        }
    }

    /// Extract the list of referenced record IDs from a relation value.
    ///
    /// Returns an empty vec for null values.
    pub fn extract_ids(value: &serde_json::Value) -> Vec<String> {
        match value {
            serde_json::Value::String(s) if !s.is_empty() => vec![s.clone()],
            serde_json::Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect(),
            _ => vec![],
        }
    }

    pub fn validate_value(&self, field_name: &str, value: &serde_json::Value) -> Result<()> {
        if self.max_select == 1 {
            // Single relation: must be a non-empty string (record ID).
            let s = value
                .as_str()
                .ok_or_else(|| field_error(field_name, "must be a string (record ID)"))?;
            if s.is_empty() {
                return Err(field_error(field_name, "record ID must not be empty"));
            }
        } else {
            // Multi-relation: must be an array of non-empty string IDs.
            let arr = value
                .as_array()
                .ok_or_else(|| field_error(field_name, "must be an array of record IDs"))?;

            if self.max_select > 0 && arr.len() as u32 > self.max_select {
                return Err(field_error(
                    field_name,
                    &format!("must have at most {} relations", self.max_select),
                ));
            }

            for item in arr {
                let s = item
                    .as_str()
                    .ok_or_else(|| field_error(field_name, "each relation must be a string ID"))?;
                if s.is_empty() {
                    return Err(field_error(field_name, "record ID must not be empty"));
                }
            }
        }

        Ok(())
    }
}

/// Options for [`FieldType::Json`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonOptions {
    /// Maximum size in bytes for the JSON payload (0 = no limit).
    #[serde(default)]
    pub max_size: u64,

    /// Optional JSON Schema (draft 2020-12 / earlier drafts also accepted)
    /// used to validate the structure of incoming values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

impl JsonOptions {
    /// Validate the options themselves (called when defining/updating a field).
    pub fn validate(&self) -> Result<()> {
        if let Some(ref schema) = self.schema {
            // Ensure the schema is a JSON object (as required by JSON Schema spec).
            if !schema.is_object() {
                return Err(field_error("schema", "JSON schema must be an object"));
            }
            // Try to compile the schema to catch syntax/reference errors early.
            jsonschema::validator_for(schema)
                .map_err(|e| field_error("schema", &format!("invalid JSON schema: {e}")))?;
        }
        Ok(())
    }

    /// Validate a non-null record value.
    pub fn validate_value(&self, field_name: &str, value: &serde_json::Value) -> Result<()> {
        // 1. Size check
        if self.max_size > 0 {
            let serialized = serde_json::to_string(value)
                .map_err(|e| field_error(field_name, &format!("invalid JSON: {e}")))?;
            if serialized.len() as u64 > self.max_size {
                return Err(field_error(
                    field_name,
                    &format!(
                        "JSON payload exceeds maximum size of {} bytes",
                        self.max_size
                    ),
                ));
            }
        }

        // 2. Schema validation
        if let Some(ref schema) = self.schema {
            let validator = jsonschema::validator_for(schema)
                .map_err(|e| field_error(field_name, &format!("invalid JSON schema: {e}")))?;

            if let Err(err) = validator.validate(value) {
                return Err(field_error(
                    field_name,
                    &format!("JSON schema validation failed: {err}"),
                ));
            }
        }

        Ok(())
    }
}

/// Default allowed HTML tags for the Editor field.
///
/// This is the same safe set that PocketBase allows by default, covering
/// common rich-text formatting without permitting dangerous elements.
fn default_editor_allowed_tags() -> HashSet<String> {
    [
        // Block elements
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "p",
        "div",
        "span",
        "blockquote",
        "pre",
        "code",
        "hr",
        "br",
        // Lists
        "ul",
        "ol",
        "li",
        // Inline formatting
        "strong",
        "b",
        "em",
        "i",
        "u",
        "s",
        "sub",
        "sup",
        "mark",
        "small",
        // Tables
        "table",
        "thead",
        "tbody",
        "tfoot",
        "tr",
        "th",
        "td",
        "caption",
        // Media (src is controlled via allowed attributes)
        "img",
        "figure",
        "figcaption",
        // Links
        "a",
        // Definition lists
        "dl",
        "dt",
        "dd",
        // Details
        "details",
        "summary",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// Default allowed HTML attributes for the Editor field, keyed by tag name.
///
/// A special key `"*"` applies to all tags.
fn default_editor_allowed_attributes() -> HashMap<String, HashSet<String>> {
    let mut map: HashMap<String, HashSet<String>> = HashMap::new();

    // Global attributes allowed on every tag.
    map.insert(
        "*".to_string(),
        ["class", "id", "title", "dir", "lang"]
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
    );

    // Tag-specific attributes.
    // Note: "rel" is NOT listed here because ammonia manages it via link_rel().
    map.insert(
        "a".to_string(),
        ["href", "target"]
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
    );
    map.insert(
        "img".to_string(),
        ["src", "alt", "width", "height", "loading"]
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
    );
    map.insert(
        "td".to_string(),
        ["colspan", "rowspan"]
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
    );
    map.insert(
        "th".to_string(),
        ["colspan", "rowspan", "scope"]
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
    );
    map.insert(
        "blockquote".to_string(),
        ["cite"].iter().map(|s| (*s).to_string()).collect(),
    );

    map
}

/// Options for [`FieldType::Editor`].
///
/// The Editor field stores rich-text HTML content. All input is sanitized
/// through [`ammonia`] before validation and storage to prevent XSS attacks.
///
/// The set of allowed tags and attributes is configurable. When not specified,
/// a sensible default set covering common rich-text formatting is used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorOptions {
    /// Maximum length in characters of the **sanitized** HTML (0 = no limit).
    #[serde(default)]
    pub max_length: u32,

    /// Allowed HTML tags. When `None`, a default safe set is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tags: Option<HashSet<String>>,

    /// Allowed HTML attributes per tag. A key of `"*"` means "all tags".
    /// When `None`, a default safe set is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_attributes: Option<HashMap<String, HashSet<String>>>,

    /// Whether this field is included in full-text search indexes.
    #[serde(default)]
    pub searchable: bool,
}

impl Default for EditorOptions {
    fn default() -> Self {
        Self {
            max_length: 0,
            allowed_tags: None,
            allowed_attributes: None,
            searchable: false,
        }
    }
}

impl EditorOptions {
    /// Validate the field definition options.
    pub fn validate(&self) -> Result<()> {
        // If custom tags are provided, ensure they are non-empty strings.
        if let Some(tags) = &self.allowed_tags {
            for tag in tags {
                if tag.trim().is_empty() {
                    return Err(ZerobaseError::validation(
                        "allowed_tags must not contain empty strings",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Sanitize HTML input through ammonia using the configured allow-lists.
    pub fn sanitize_html(&self, raw: &str) -> String {
        let tags = self
            .allowed_tags
            .clone()
            .unwrap_or_else(default_editor_allowed_tags);
        let attrs = self
            .allowed_attributes
            .clone()
            .unwrap_or_else(default_editor_allowed_attributes);

        let tag_refs: HashSet<&str> = tags.iter().map(|s| s.as_str()).collect();

        let mut builder = ammonia::Builder::new();
        builder.tags(tag_refs);

        // Convert our HashMap<String, HashSet<String>> into the format ammonia expects.
        for (tag, attr_set) in &attrs {
            let attr_refs: Vec<&str> = attr_set.iter().map(|s| s.as_str()).collect();
            if tag == "*" {
                // Generic attributes allowed on every tag.
                for attr in &attr_refs {
                    builder.add_generic_attributes([*attr]);
                }
            } else {
                builder.add_tag_attributes(tag.as_str(), attr_refs.iter().copied());
            }
        }

        // Enforce safe link protocols.
        builder.url_schemes(HashSet::from(["http", "https", "mailto"]));
        // Strip dangerous protocols like javascript: from links.
        builder.strip_comments(true);
        // Add rel=noopener to links with target.
        builder.link_rel(Some("noopener noreferrer"));

        builder.clean(raw).to_string()
    }

    /// Validate a non-null record value for an Editor field.
    ///
    /// The value must be a string. It is sanitized and then checked against
    /// the max_length constraint (on the **sanitized** output).
    pub fn validate_value(&self, field_name: &str, value: &serde_json::Value) -> Result<()> {
        let s = value
            .as_str()
            .ok_or_else(|| field_error(field_name, "must be a string"))?;

        let sanitized = self.sanitize_html(s);

        if self.max_length > 0 && (sanitized.len() as u32) > self.max_length {
            return Err(field_error(
                field_name,
                &format!("must be at most {} characters", self.max_length),
            ));
        }

        Ok(())
    }

    /// Return the sanitized version of the value, ready for storage.
    ///
    /// Returns `None` if the value is not a string (caller should validate first).
    pub fn prepare_value(&self, value: &serde_json::Value) -> Option<serde_json::Value> {
        value
            .as_str()
            .map(|s| serde_json::Value::String(self.sanitize_html(s)))
    }
}

/// Options for [`FieldType::Password`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordOptions {
    /// Minimum length for the password.
    #[serde(default = "default_min_password_length")]
    pub min_length: u32,
    /// Maximum length for the password (0 = no limit).
    #[serde(default)]
    pub max_length: u32,
    /// Optional regex pattern the password must match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

fn default_min_password_length() -> u32 {
    8
}

impl Default for PasswordOptions {
    fn default() -> Self {
        Self {
            min_length: 8,
            max_length: 0,
            pattern: None,
        }
    }
}

impl PasswordOptions {
    pub fn validate(&self) -> Result<()> {
        if self.min_length == 0 {
            return Err(field_error(
                "minLength",
                "password must have a minimum length",
            ));
        }
        if self.max_length > 0 && self.min_length > self.max_length {
            return Err(field_error("minLength", "must not exceed maxLength"));
        }
        if let Some(ref p) = self.pattern {
            validate_regex_pattern(p)?;
        }
        Ok(())
    }

    pub fn validate_value(&self, field_name: &str, value: &serde_json::Value) -> Result<()> {
        let s = value
            .as_str()
            .ok_or_else(|| field_error(field_name, "must be a string"))?;

        if (s.len() as u32) < self.min_length {
            return Err(field_error(
                field_name,
                &format!("must be at least {} characters", self.min_length),
            ));
        }

        if self.max_length > 0 && (s.len() as u32) > self.max_length {
            return Err(field_error(
                field_name,
                &format!("must be at most {} characters", self.max_length),
            ));
        }

        if let Some(ref pattern) = self.pattern {
            let re = regex::Regex::new(pattern)
                .map_err(|e| field_error(field_name, &format!("invalid pattern: {e}")))?;
            if !re.is_match(s) {
                return Err(field_error(
                    field_name,
                    "password does not meet complexity requirements",
                ));
            }
        }

        Ok(())
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn field_error(field: &str, message: &str) -> ZerobaseError {
    let mut errors = HashMap::new();
    errors.insert(field.to_string(), message.to_string());
    ZerobaseError::validation_with_fields(format!("validation failed for {field}"), errors)
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Field construction & validation ────────────────────────────────────

    #[test]
    fn field_new_sets_defaults() {
        let f = Field::new("title", FieldType::Text(TextOptions::default()));
        assert_eq!(f.name, "title");
        assert!(!f.required);
        assert!(!f.unique);
        assert!(!f.id.is_empty());
    }

    #[test]
    fn field_builder_methods() {
        let f = Field::new("email", FieldType::Email(EmailOptions::default()))
            .required(true)
            .unique(true);
        assert!(f.required);
        assert!(f.unique);
    }

    #[test]
    fn field_validate_rejects_invalid_name() {
        let f = Field::new("1bad", FieldType::Bool(BoolOptions::default()));
        assert!(f.validate().is_err());
    }

    #[test]
    fn field_validate_accepts_valid_name() {
        let f = Field::new("good_name", FieldType::Bool(BoolOptions::default()));
        assert!(f.validate().is_ok());
    }

    // ── Required field validation ──────────────────────────────────────────

    #[test]
    fn required_field_rejects_null() {
        let f = Field::new("title", FieldType::Text(TextOptions::default())).required(true);
        assert!(f.validate_value(&json!(null)).is_err());
    }

    #[test]
    fn optional_field_accepts_null() {
        let f = Field::new("title", FieldType::Text(TextOptions::default()));
        assert!(f.validate_value(&json!(null)).is_ok());
    }

    // ── Text ───────────────────────────────────────────────────────────────

    #[test]
    fn text_accepts_valid_string() {
        let opts = TextOptions {
            min_length: 2,
            max_length: 10,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("hello")).is_ok());
    }

    #[test]
    fn text_rejects_too_short() {
        let opts = TextOptions {
            min_length: 5,
            max_length: 0,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("hi")).is_err());
    }

    #[test]
    fn text_rejects_too_long() {
        let opts = TextOptions {
            min_length: 0,
            max_length: 3,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("toolong")).is_err());
    }

    #[test]
    fn text_rejects_non_string() {
        let opts = TextOptions::default();
        assert!(opts.validate_value("f", &json!(42)).is_err());
    }

    #[test]
    fn text_validates_pattern() {
        let opts = TextOptions {
            min_length: 0,
            max_length: 0,
            pattern: Some(r"^\d{3}$".to_string()),
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("123")).is_ok());
        assert!(opts.validate_value("f", &json!("abc")).is_err());
    }

    #[test]
    fn text_options_validate_rejects_min_gt_max() {
        let opts = TextOptions {
            min_length: 10,
            max_length: 5,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn text_options_validate_rejects_bad_regex() {
        let opts = TextOptions {
            min_length: 0,
            max_length: 0,
            pattern: Some("[invalid".to_string()),
            searchable: false,
        };
        assert!(opts.validate().is_err());
    }

    // ── Text: comprehensive coverage ──────────────────────────────────────

    #[test]
    fn text_options_validate_accepts_valid_config() {
        // Both min and max set, min < max
        let opts = TextOptions {
            min_length: 1,
            max_length: 100,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate().is_ok());

        // Only min set
        let opts = TextOptions {
            min_length: 5,
            max_length: 0,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate().is_ok());

        // Only max set
        let opts = TextOptions {
            min_length: 0,
            max_length: 50,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate().is_ok());

        // No constraints
        let opts = TextOptions::default();
        assert!(opts.validate().is_ok());

        // Valid regex pattern
        let opts = TextOptions {
            min_length: 0,
            max_length: 0,
            pattern: Some(r"^[a-zA-Z0-9]+$".to_string()),
            searchable: false,
        };
        assert!(opts.validate().is_ok());

        // Min equals max
        let opts = TextOptions {
            min_length: 5,
            max_length: 5,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn text_accepts_exact_min_length() {
        let opts = TextOptions {
            min_length: 3,
            max_length: 0,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("abc")).is_ok());
    }

    #[test]
    fn text_accepts_exact_max_length() {
        let opts = TextOptions {
            min_length: 0,
            max_length: 5,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("abcde")).is_ok());
    }

    #[test]
    fn text_rejects_one_below_min_length() {
        let opts = TextOptions {
            min_length: 3,
            max_length: 0,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("ab")).is_err());
    }

    #[test]
    fn text_rejects_one_above_max_length() {
        let opts = TextOptions {
            min_length: 0,
            max_length: 5,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("abcdef")).is_err());
    }

    #[test]
    fn text_accepts_when_min_equals_max() {
        let opts = TextOptions {
            min_length: 3,
            max_length: 3,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("abc")).is_ok());
        assert!(opts.validate_value("f", &json!("ab")).is_err());
        assert!(opts.validate_value("f", &json!("abcd")).is_err());
    }

    #[test]
    fn text_unicode_counts_characters_not_bytes() {
        let opts = TextOptions {
            min_length: 0,
            max_length: 3,
            pattern: None,
            searchable: false,
        };
        // "héllo" has 5 chars but 6 bytes (é is 2 bytes in UTF-8)
        // "日本語" has 3 chars but 9 bytes
        assert!(opts.validate_value("f", &json!("日本語")).is_ok()); // 3 chars
        assert!(opts.validate_value("f", &json!("日本語あ")).is_err()); // 4 chars

        let opts = TextOptions {
            min_length: 2,
            max_length: 0,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("日本")).is_ok()); // 2 chars
        assert!(opts.validate_value("f", &json!("日")).is_err()); // 1 char
    }

    #[test]
    fn text_unicode_emoji_counts_correctly() {
        let opts = TextOptions {
            min_length: 0,
            max_length: 3,
            pattern: None,
            searchable: false,
        };
        // Each emoji is one or more Unicode code points
        assert!(opts.validate_value("f", &json!("abc")).is_ok());
        // Single emoji is 1 char
        assert!(opts.validate_value("f", &json!("a😀b")).is_ok()); // 3 chars
    }

    #[test]
    fn text_no_constraints_accepts_anything() {
        let opts = TextOptions::default();
        assert!(opts.validate_value("f", &json!("")).is_ok());
        assert!(opts.validate_value("f", &json!("a")).is_ok());
        assert!(opts
            .validate_value("f", &json!("a very long string that goes on and on"))
            .is_ok());
    }

    #[test]
    fn text_rejects_non_string_types() {
        let opts = TextOptions::default();
        assert!(opts.validate_value("f", &json!(42)).is_err());
        assert!(opts.validate_value("f", &json!(3.14)).is_err());
        assert!(opts.validate_value("f", &json!(true)).is_err());
        assert!(opts.validate_value("f", &json!(false)).is_err());
        assert!(opts.validate_value("f", &json!(["array"])).is_err());
        assert!(opts.validate_value("f", &json!({"key": "val"})).is_err());
    }

    #[test]
    fn text_pattern_full_match_anchored() {
        let opts = TextOptions {
            min_length: 0,
            max_length: 0,
            pattern: Some(r"^\d{3}-\d{4}$".to_string()),
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("123-4567")).is_ok());
        assert!(opts.validate_value("f", &json!("12-4567")).is_err());
        assert!(opts.validate_value("f", &json!("123-456")).is_err());
        assert!(opts.validate_value("f", &json!("abc-defg")).is_err());
    }

    #[test]
    fn text_pattern_partial_match_unanchored() {
        // Without anchors, regex matches anywhere in the string
        let opts = TextOptions {
            min_length: 0,
            max_length: 0,
            pattern: Some(r"\d+".to_string()),
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("abc123def")).is_ok());
        assert!(opts.validate_value("f", &json!("no digits here")).is_err());
    }

    #[test]
    fn text_pattern_case_sensitive_by_default() {
        let opts = TextOptions {
            min_length: 0,
            max_length: 0,
            pattern: Some(r"^[a-z]+$".to_string()),
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("hello")).is_ok());
        assert!(opts.validate_value("f", &json!("Hello")).is_err());
    }

    #[test]
    fn text_pattern_case_insensitive_flag() {
        let opts = TextOptions {
            min_length: 0,
            max_length: 0,
            pattern: Some(r"(?i)^[a-z]+$".to_string()),
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("Hello")).is_ok());
        assert!(opts.validate_value("f", &json!("HELLO")).is_ok());
    }

    #[test]
    fn text_pattern_with_length_constraints() {
        let opts = TextOptions {
            min_length: 3,
            max_length: 10,
            pattern: Some(r"^[a-z]+$".to_string()),
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("hello")).is_ok());
        // Too short (min_length fails first)
        assert!(opts.validate_value("f", &json!("ab")).is_err());
        // Too long
        assert!(opts.validate_value("f", &json!("abcdefghijk")).is_err());
        // Right length but wrong pattern
        assert!(opts.validate_value("f", &json!("ABC")).is_err());
    }

    #[test]
    fn text_pattern_email_like() {
        let opts = TextOptions {
            min_length: 0,
            max_length: 0,
            pattern: Some(r"^[^@]+@[^@]+\.[^@]+$".to_string()),
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("user@example.com")).is_ok());
        assert!(opts.validate_value("f", &json!("not-an-email")).is_err());
    }

    #[test]
    fn text_pattern_unicode_aware() {
        let opts = TextOptions {
            min_length: 0,
            max_length: 0,
            pattern: Some(r"^\p{L}+$".to_string()), // Unicode letter class,
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("héllo")).is_ok());
        assert!(opts.validate_value("f", &json!("日本語")).is_ok());
        assert!(opts.validate_value("f", &json!("abc123")).is_err());
    }

    #[test]
    fn text_options_validate_accepts_valid_regex() {
        let patterns = vec![
            r"^\d+$",
            r"^[a-zA-Z0-9_]+$",
            r"^.{3,10}$",
            r"(?i)^test$",
            r"\b\w+\b",
        ];
        for p in patterns {
            let opts = TextOptions {
                min_length: 0,
                max_length: 0,
                pattern: Some(p.to_string()),
                searchable: false,
            };
            assert!(opts.validate().is_ok(), "pattern should be valid: {p}");
        }
    }

    #[test]
    fn text_options_validate_rejects_invalid_regex_patterns() {
        let patterns = vec!["[invalid", "(?P<unmatched", "*invalid", "(unclosed"];
        for p in patterns {
            let opts = TextOptions {
                min_length: 0,
                max_length: 0,
                pattern: Some(p.to_string()),
                searchable: false,
            };
            assert!(opts.validate().is_err(), "pattern should be invalid: {p}");
        }
    }

    #[test]
    fn text_serde_round_trip_all_options() {
        let opts = TextOptions {
            min_length: 5,
            max_length: 100,
            pattern: Some(r"^[a-z]+$".to_string()),
            searchable: false,
        };
        let json = serde_json::to_value(&opts).unwrap();
        assert_eq!(json["minLength"], 5);
        assert_eq!(json["maxLength"], 100);
        assert_eq!(json["pattern"], r"^[a-z]+$");

        let deserialized: TextOptions = serde_json::from_value(json).unwrap();
        assert_eq!(opts, deserialized);
    }

    #[test]
    fn text_serde_defaults_when_missing() {
        let json = serde_json::json!({});
        let opts: TextOptions = serde_json::from_value(json).unwrap();
        assert_eq!(opts.min_length, 0);
        assert_eq!(opts.max_length, 0);
        assert_eq!(opts.pattern, None);
    }

    #[test]
    fn text_serde_pattern_omitted_when_none() {
        let opts = TextOptions {
            min_length: 0,
            max_length: 0,
            pattern: None,
            searchable: false,
        };
        let json = serde_json::to_value(&opts).unwrap();
        assert!(json.get("pattern").is_none());
    }

    #[test]
    fn text_field_type_sql_type_is_text() {
        let ft = FieldType::Text(TextOptions::default());
        assert_eq!(ft.sql_type(), "TEXT");
    }

    #[test]
    fn text_field_type_name() {
        let ft = FieldType::Text(TextOptions::default());
        assert_eq!(ft.type_name(), "text");
    }

    #[test]
    fn text_field_type_is_text_like() {
        let ft = FieldType::Text(TextOptions::default());
        assert!(ft.is_text_like());
    }

    #[test]
    fn text_field_required_rejects_null() {
        let f = Field::new("title", FieldType::Text(TextOptions::default())).required(true);
        assert!(f.validate_value(&json!(null)).is_err());
    }

    #[test]
    fn text_field_required_rejects_empty_string() {
        let f = Field::new("title", FieldType::Text(TextOptions::default())).required(true);
        assert!(f.validate_value(&json!("")).is_err());
    }

    #[test]
    fn text_field_optional_accepts_null() {
        let f = Field::new("title", FieldType::Text(TextOptions::default()));
        assert!(f.validate_value(&json!(null)).is_ok());
    }

    #[test]
    fn text_field_optional_accepts_empty_string() {
        let f = Field::new("title", FieldType::Text(TextOptions::default()));
        assert!(f.validate_value(&json!("")).is_ok());
    }

    #[test]
    fn text_field_empty_string_skips_min_length_when_optional() {
        // An optional field with min_length should still accept empty string
        // (empty = "no value", not "too short").
        let f = Field::new(
            "bio",
            FieldType::Text(TextOptions {
                min_length: 10,
                max_length: 0,
                pattern: None,
                searchable: false,
            }),
        );
        assert!(f.validate_value(&json!("")).is_ok());
    }

    #[test]
    fn text_field_serde_full_round_trip() {
        let ft = FieldType::Text(TextOptions {
            min_length: 2,
            max_length: 50,
            pattern: Some(r"^\w+$".to_string()),
            searchable: false,
        });
        let json = serde_json::to_value(&ft).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["options"]["minLength"], 2);
        assert_eq!(json["options"]["maxLength"], 50);
        assert_eq!(json["options"]["pattern"], r"^\w+$");

        let deserialized: FieldType = serde_json::from_value(json).unwrap();
        assert_eq!(ft, deserialized);
    }

    #[test]
    fn text_field_validate_definition_rejects_min_gt_max() {
        let f = Field::new(
            "title",
            FieldType::Text(TextOptions {
                min_length: 10,
                max_length: 5,
                pattern: None,
                searchable: false,
            }),
        );
        assert!(f.validate().is_err());
    }

    #[test]
    fn text_field_validate_definition_rejects_bad_pattern() {
        let f = Field::new(
            "code",
            FieldType::Text(TextOptions {
                min_length: 0,
                max_length: 0,
                pattern: Some("[bad".to_string()),
                searchable: false,
            }),
        );
        assert!(f.validate().is_err());
    }

    #[test]
    fn text_error_message_includes_field_name() {
        let opts = TextOptions {
            min_length: 5,
            max_length: 0,
            pattern: None,
            searchable: false,
        };
        let err = opts.validate_value("my_field", &json!("hi")).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("my_field"),
            "error should include field name: {msg}"
        );
    }

    #[test]
    fn text_whitespace_is_counted_as_characters() {
        let opts = TextOptions {
            min_length: 3,
            max_length: 5,
            pattern: None,
            searchable: false,
        };
        assert!(opts.validate_value("f", &json!("   ")).is_ok()); // 3 spaces = 3 chars
        assert!(opts.validate_value("f", &json!("a b")).is_ok()); // 3 chars
        assert!(opts.validate_value("f", &json!("\t\n\r")).is_ok()); // 3 chars
    }

    #[test]
    fn text_empty_pattern_matches_everything() {
        // An empty regex pattern matches everything
        let opts = TextOptions {
            min_length: 0,
            max_length: 0,
            pattern: Some(String::new()),
            searchable: false,
        };
        assert!(opts.validate().is_ok());
        assert!(opts.validate_value("f", &json!("anything")).is_ok());
    }

    // ── Number ─────────────────────────────────────────────────────────────

    #[test]
    fn number_accepts_valid_float() {
        let opts = NumberOptions::default();
        assert!(opts.validate_value("f", &json!(3.14)).is_ok());
    }

    #[test]
    fn number_accepts_valid_integer() {
        let opts = NumberOptions {
            only_int: true,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(42)).is_ok());
    }

    #[test]
    fn number_rejects_float_when_only_int() {
        let opts = NumberOptions {
            only_int: true,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(3.14)).is_err());
    }

    #[test]
    fn number_rejects_below_min() {
        let opts = NumberOptions {
            min: Some(10.0),
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(5)).is_err());
    }

    #[test]
    fn number_rejects_above_max() {
        let opts = NumberOptions {
            max: Some(100.0),
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(200)).is_err());
    }

    #[test]
    fn number_rejects_non_number() {
        let opts = NumberOptions::default();
        assert!(opts.validate_value("f", &json!("not a number")).is_err());
    }

    #[test]
    fn number_options_validate_rejects_min_gt_max() {
        let opts = NumberOptions {
            min: Some(100.0),
            max: Some(10.0),
            only_int: false,
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn number_options_validate_accepts_equal_min_max() {
        let opts = NumberOptions {
            min: Some(42.0),
            max: Some(42.0),
            only_int: false,
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn number_options_validate_rejects_nan_min() {
        let opts = NumberOptions {
            min: Some(f64::NAN),
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn number_options_validate_rejects_nan_max() {
        let opts = NumberOptions {
            max: Some(f64::NAN),
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn number_options_validate_rejects_infinite_min() {
        let opts = NumberOptions {
            min: Some(f64::INFINITY),
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn number_options_validate_rejects_neg_infinite_max() {
        let opts = NumberOptions {
            max: Some(f64::NEG_INFINITY),
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn number_accepts_zero() {
        let opts = NumberOptions::default();
        assert!(opts.validate_value("f", &json!(0)).is_ok());
    }

    #[test]
    fn number_accepts_negative() {
        let opts = NumberOptions::default();
        assert!(opts.validate_value("f", &json!(-42.5)).is_ok());
    }

    #[test]
    fn number_accepts_at_min_boundary() {
        let opts = NumberOptions {
            min: Some(10.0),
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(10.0)).is_ok());
    }

    #[test]
    fn number_accepts_at_max_boundary() {
        let opts = NumberOptions {
            max: Some(100.0),
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(100.0)).is_ok());
    }

    #[test]
    fn number_accepts_within_range() {
        let opts = NumberOptions {
            min: Some(1.0),
            max: Some(100.0),
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(50)).is_ok());
    }

    #[test]
    fn number_rejects_below_range() {
        let opts = NumberOptions {
            min: Some(1.0),
            max: Some(100.0),
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(0)).is_err());
    }

    #[test]
    fn number_rejects_above_range() {
        let opts = NumberOptions {
            min: Some(1.0),
            max: Some(100.0),
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(101)).is_err());
    }

    #[test]
    fn number_only_int_accepts_negative_integer() {
        let opts = NumberOptions {
            only_int: true,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(-7)).is_ok());
    }

    #[test]
    fn number_only_int_accepts_zero() {
        let opts = NumberOptions {
            only_int: true,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(0)).is_ok());
    }

    #[test]
    fn number_only_int_rejects_negative_float() {
        let opts = NumberOptions {
            only_int: true,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(-3.14)).is_err());
    }

    #[test]
    fn number_only_int_with_min_max() {
        let opts = NumberOptions {
            min: Some(0.0),
            max: Some(10.0),
            only_int: true,
        };
        assert!(opts.validate_value("f", &json!(5)).is_ok());
        assert!(opts.validate_value("f", &json!(5.5)).is_err());
        assert!(opts.validate_value("f", &json!(-1)).is_err());
        assert!(opts.validate_value("f", &json!(11)).is_err());
    }

    #[test]
    fn number_rejects_string() {
        let opts = NumberOptions::default();
        assert!(opts.validate_value("f", &json!("42")).is_err());
    }

    #[test]
    fn number_rejects_bool() {
        let opts = NumberOptions::default();
        assert!(opts.validate_value("f", &json!(true)).is_err());
    }

    #[test]
    fn number_rejects_array() {
        let opts = NumberOptions::default();
        assert!(opts.validate_value("f", &json!([1, 2, 3])).is_err());
    }

    #[test]
    fn number_rejects_object() {
        let opts = NumberOptions::default();
        assert!(opts.validate_value("f", &json!({"n": 42})).is_err());
    }

    #[test]
    fn number_accepts_large_float() {
        let opts = NumberOptions::default();
        assert!(opts
            .validate_value("f", &json!(1.7976931348623157e+308))
            .is_ok());
    }

    #[test]
    fn number_accepts_small_float() {
        let opts = NumberOptions::default();
        assert!(opts.validate_value("f", &json!(5e-324)).is_ok());
    }

    #[test]
    fn number_field_required_rejects_null() {
        let field = Field::new("score", FieldType::Number(NumberOptions::default())).required(true);
        assert!(field.validate_value(&serde_json::Value::Null).is_err());
    }

    #[test]
    fn number_field_optional_accepts_null() {
        let field = Field::new("score", FieldType::Number(NumberOptions::default()));
        assert!(field.validate_value(&serde_json::Value::Null).is_ok());
    }

    #[test]
    fn number_sql_type_is_real() {
        assert_eq!(
            FieldType::Number(NumberOptions::default()).sql_type(),
            "REAL"
        );
    }

    // ── Number prepare_value ──────────────────────────────────────────────

    #[test]
    fn number_prepare_value_passes_through_number() {
        let opts = NumberOptions::default();
        let val = json!(42);
        assert_eq!(opts.prepare_value(&val), Some(json!(42)));
    }

    #[test]
    fn number_prepare_value_returns_none_for_null() {
        let opts = NumberOptions::default();
        assert_eq!(opts.prepare_value(&serde_json::Value::Null), None);
    }

    #[test]
    fn number_prepare_value_coerces_int_string() {
        let opts = NumberOptions::default();
        assert_eq!(opts.prepare_value(&json!("42")), Some(json!(42.0)));
    }

    #[test]
    fn number_prepare_value_coerces_float_string() {
        let opts = NumberOptions::default();
        assert_eq!(opts.prepare_value(&json!("3.14")), Some(json!(3.14)));
    }

    #[test]
    fn number_prepare_value_coerces_negative_string() {
        let opts = NumberOptions::default();
        assert_eq!(opts.prepare_value(&json!("-7.5")), Some(json!(-7.5)));
    }

    #[test]
    fn number_prepare_value_trims_whitespace_string() {
        let opts = NumberOptions::default();
        assert_eq!(opts.prepare_value(&json!("  42  ")), Some(json!(42.0)));
    }

    #[test]
    fn number_prepare_value_empty_string_to_null() {
        let opts = NumberOptions::default();
        assert_eq!(
            opts.prepare_value(&json!("")),
            Some(serde_json::Value::Null)
        );
    }

    #[test]
    fn number_prepare_value_non_numeric_string_returns_none() {
        let opts = NumberOptions::default();
        assert_eq!(opts.prepare_value(&json!("abc")), None);
    }

    #[test]
    fn number_prepare_value_bool_true_to_one() {
        let opts = NumberOptions::default();
        assert_eq!(opts.prepare_value(&json!(true)), Some(json!(1.0)));
    }

    #[test]
    fn number_prepare_value_bool_false_to_zero() {
        let opts = NumberOptions::default();
        assert_eq!(opts.prepare_value(&json!(false)), Some(json!(0.0)));
    }

    #[test]
    fn number_prepare_value_array_returns_none() {
        let opts = NumberOptions::default();
        assert_eq!(opts.prepare_value(&json!([1])), None);
    }

    #[test]
    fn number_prepare_value_object_returns_none() {
        let opts = NumberOptions::default();
        assert_eq!(opts.prepare_value(&json!({"n": 1})), None);
    }

    // ── Number serde aliases ──────────────────────────────────────────────

    #[test]
    fn number_options_deserialize_no_decimal_alias() {
        let json_str = r#"{"min": 0.0, "max": 100.0, "noDecimal": true}"#;
        let opts: NumberOptions = serde_json::from_str(json_str).unwrap();
        assert!(opts.only_int);
        assert_eq!(opts.min, Some(0.0));
        assert_eq!(opts.max, Some(100.0));
    }

    #[test]
    fn number_options_deserialize_only_int() {
        let json_str = r#"{"onlyInt": true}"#;
        let opts: NumberOptions = serde_json::from_str(json_str).unwrap();
        assert!(opts.only_int);
    }

    #[test]
    fn number_options_serializes_as_only_int() {
        let opts = NumberOptions {
            only_int: true,
            ..Default::default()
        };
        let json = serde_json::to_value(&opts).unwrap();
        assert_eq!(json["onlyInt"], json!(true));
        // noDecimal should NOT appear in serialized output (only used as alias)
        assert!(json.get("noDecimal").is_none());
    }

    #[test]
    fn number_options_default_serialization_skips_none() {
        let opts = NumberOptions::default();
        let json = serde_json::to_value(&opts).unwrap();
        assert!(json.get("min").is_none());
        assert!(json.get("max").is_none());
        assert_eq!(json["onlyInt"], json!(false));
    }

    // ── Bool ───────────────────────────────────────────────────────────────

    // --- validate_bool_value (post-prepare validation) ---

    #[test]
    fn bool_validate_accepts_true() {
        assert!(validate_bool_value("f", &json!(true)).is_ok());
    }

    #[test]
    fn bool_validate_accepts_false() {
        assert!(validate_bool_value("f", &json!(false)).is_ok());
    }

    #[test]
    fn bool_validate_rejects_string() {
        assert!(validate_bool_value("f", &json!("true")).is_err());
    }

    #[test]
    fn bool_validate_rejects_number() {
        assert!(validate_bool_value("f", &json!(1)).is_err());
    }

    // --- Bool prepare_value ---

    #[test]
    fn bool_prepare_passes_through_true() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!(true)), Some(json!(true)));
    }

    #[test]
    fn bool_prepare_passes_through_false() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!(false)), Some(json!(false)));
    }

    #[test]
    fn bool_prepare_returns_none_for_null() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&serde_json::Value::Null), None);
    }

    #[test]
    fn bool_prepare_coerces_number_one_to_true() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!(1)), Some(json!(true)));
    }

    #[test]
    fn bool_prepare_coerces_number_zero_to_false() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!(0)), Some(json!(false)));
    }

    #[test]
    fn bool_prepare_coerces_float_one_to_true() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!(1.0)), Some(json!(true)));
    }

    #[test]
    fn bool_prepare_coerces_float_zero_to_false() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!(0.0)), Some(json!(false)));
    }

    #[test]
    fn bool_prepare_rejects_number_two() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!(2)), None);
    }

    #[test]
    fn bool_prepare_rejects_negative_number() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!(-1)), None);
    }

    #[test]
    fn bool_prepare_coerces_string_true() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!("true")), Some(json!(true)));
    }

    #[test]
    fn bool_prepare_coerces_string_false() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!("false")), Some(json!(false)));
    }

    #[test]
    fn bool_prepare_coerces_string_one() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!("1")), Some(json!(true)));
    }

    #[test]
    fn bool_prepare_coerces_string_zero() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!("0")), Some(json!(false)));
    }

    #[test]
    fn bool_prepare_coerces_string_yes() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!("yes")), Some(json!(true)));
    }

    #[test]
    fn bool_prepare_coerces_string_no() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!("no")), Some(json!(false)));
    }

    #[test]
    fn bool_prepare_case_insensitive_true() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!("TRUE")), Some(json!(true)));
        assert_eq!(opts.prepare_value(&json!("True")), Some(json!(true)));
    }

    #[test]
    fn bool_prepare_case_insensitive_false() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!("FALSE")), Some(json!(false)));
        assert_eq!(opts.prepare_value(&json!("False")), Some(json!(false)));
    }

    #[test]
    fn bool_prepare_trims_whitespace() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!("  true  ")), Some(json!(true)));
        assert_eq!(opts.prepare_value(&json!("  0  ")), Some(json!(false)));
    }

    #[test]
    fn bool_prepare_empty_string_to_null() {
        let opts = BoolOptions::default();
        assert_eq!(
            opts.prepare_value(&json!("")),
            Some(serde_json::Value::Null)
        );
    }

    #[test]
    fn bool_prepare_whitespace_only_string_to_null() {
        let opts = BoolOptions::default();
        assert_eq!(
            opts.prepare_value(&json!("   ")),
            Some(serde_json::Value::Null)
        );
    }

    #[test]
    fn bool_prepare_rejects_unrecognised_string() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!("maybe")), None);
        assert_eq!(opts.prepare_value(&json!("2")), None);
    }

    #[test]
    fn bool_prepare_rejects_array() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!([true])), None);
    }

    #[test]
    fn bool_prepare_rejects_object() {
        let opts = BoolOptions::default();
        assert_eq!(opts.prepare_value(&json!({"v": true})), None);
    }

    // --- Bool end-to-end via FieldType::prepare_value ---

    #[test]
    fn bool_field_type_prepare_normalizes_string_true() {
        let ft = FieldType::Bool(BoolOptions::default());
        assert_eq!(ft.prepare_value(&json!("true")), Some(json!(true)));
    }

    #[test]
    fn bool_field_type_prepare_normalizes_number_zero() {
        let ft = FieldType::Bool(BoolOptions::default());
        assert_eq!(ft.prepare_value(&json!(0)), Some(json!(false)));
    }

    #[test]
    fn bool_field_type_prepare_null_returns_none() {
        let ft = FieldType::Bool(BoolOptions::default());
        assert!(ft.prepare_value(&serde_json::Value::Null).is_none());
    }

    // ── Email ──────────────────────────────────────────────────────────────

    // ── Format validation ─────────────────────────────────────────────

    #[test]
    fn email_accepts_valid_simple() {
        let opts = EmailOptions::default();
        assert!(opts.validate_value("f", &json!("user@example.com")).is_ok());
    }

    #[test]
    fn email_accepts_with_plus_tag() {
        let opts = EmailOptions::default();
        assert!(opts
            .validate_value("f", &json!("user+tag@example.com"))
            .is_ok());
    }

    #[test]
    fn email_accepts_with_dots_in_local() {
        let opts = EmailOptions::default();
        assert!(opts
            .validate_value("f", &json!("first.last@example.com"))
            .is_ok());
    }

    #[test]
    fn email_accepts_subdomain() {
        let opts = EmailOptions::default();
        assert!(opts
            .validate_value("f", &json!("user@mail.example.co.uk"))
            .is_ok());
    }

    #[test]
    fn email_accepts_hyphen_in_domain() {
        let opts = EmailOptions::default();
        assert!(opts
            .validate_value("f", &json!("user@my-domain.com"))
            .is_ok());
    }

    #[test]
    fn email_accepts_numeric_local() {
        let opts = EmailOptions::default();
        assert!(opts
            .validate_value("f", &json!("12345@example.com"))
            .is_ok());
    }

    #[test]
    fn email_rejects_missing_at() {
        let opts = EmailOptions::default();
        assert!(opts.validate_value("f", &json!("userexample.com")).is_err());
    }

    #[test]
    fn email_rejects_missing_domain_dot() {
        let opts = EmailOptions::default();
        assert!(opts.validate_value("f", &json!("user@localhost")).is_err());
    }

    #[test]
    fn email_rejects_empty_local() {
        let opts = EmailOptions::default();
        assert!(opts.validate_value("f", &json!("@example.com")).is_err());
    }

    #[test]
    fn email_rejects_empty_domain() {
        let opts = EmailOptions::default();
        assert!(opts.validate_value("f", &json!("user@")).is_err());
    }

    #[test]
    fn email_rejects_double_at() {
        let opts = EmailOptions::default();
        // splitn(2, '@') gives ["user", "name@example.com"] which has a dot,
        // but the domain part "name@example.com" contains '@' — however our
        // current check only splits on first '@'. The domain labels check will
        // reject because 'name@example' is not alphanumeric-hyphen only.
        assert!(opts
            .validate_value("f", &json!("user@name@example.com"))
            .is_err());
    }

    #[test]
    fn email_rejects_spaces() {
        let opts = EmailOptions::default();
        assert!(opts
            .validate_value("f", &json!("user @example.com"))
            .is_err());
    }

    #[test]
    fn email_rejects_leading_space() {
        let opts = EmailOptions::default();
        assert!(opts
            .validate_value("f", &json!(" user@example.com"))
            .is_err());
    }

    #[test]
    fn email_rejects_trailing_space() {
        let opts = EmailOptions::default();
        assert!(opts
            .validate_value("f", &json!("user@example.com "))
            .is_err());
    }

    #[test]
    fn email_rejects_tab_character() {
        let opts = EmailOptions::default();
        assert!(opts
            .validate_value("f", &json!("user\t@example.com"))
            .is_err());
    }

    #[test]
    fn email_rejects_empty_string() {
        let opts = EmailOptions::default();
        assert!(opts.validate_value("f", &json!("")).is_err());
    }

    #[test]
    fn email_rejects_just_at() {
        let opts = EmailOptions::default();
        assert!(opts.validate_value("f", &json!("@")).is_err());
    }

    #[test]
    fn email_rejects_non_string_number() {
        let opts = EmailOptions::default();
        assert!(opts.validate_value("f", &json!(42)).is_err());
    }

    #[test]
    fn email_rejects_non_string_bool() {
        let opts = EmailOptions::default();
        assert!(opts.validate_value("f", &json!(true)).is_err());
    }

    #[test]
    fn email_rejects_non_string_object() {
        let opts = EmailOptions::default();
        assert!(opts
            .validate_value("f", &json!({"email": "a@b.com"}))
            .is_err());
    }

    #[test]
    fn email_rejects_domain_leading_hyphen() {
        let opts = EmailOptions::default();
        assert!(opts
            .validate_value("f", &json!("user@-example.com"))
            .is_err());
    }

    #[test]
    fn email_rejects_domain_trailing_hyphen() {
        let opts = EmailOptions::default();
        assert!(opts
            .validate_value("f", &json!("user@example-.com"))
            .is_err());
    }

    #[test]
    fn email_rejects_domain_double_dot() {
        let opts = EmailOptions::default();
        assert!(opts
            .validate_value("f", &json!("user@example..com"))
            .is_err());
    }

    #[test]
    fn email_rejects_single_char_tld() {
        let opts = EmailOptions::default();
        assert!(opts.validate_value("f", &json!("user@example.c")).is_err());
    }

    #[test]
    fn email_accepts_two_char_tld() {
        let opts = EmailOptions::default();
        assert!(opts.validate_value("f", &json!("user@example.co")).is_ok());
    }

    #[test]
    fn email_rejects_domain_trailing_dot() {
        let opts = EmailOptions::default();
        assert!(opts
            .validate_value("f", &json!("user@example.com."))
            .is_err());
    }

    #[test]
    fn email_rejects_local_part_too_long() {
        let opts = EmailOptions::default();
        let long_local = "a".repeat(65);
        let email = format!("{long_local}@example.com");
        assert!(opts.validate_value("f", &json!(email)).is_err());
    }

    #[test]
    fn email_accepts_max_local_length() {
        let opts = EmailOptions::default();
        let local = "a".repeat(64);
        let email = format!("{local}@example.com");
        assert!(opts.validate_value("f", &json!(email)).is_ok());
    }

    // ── Domain allow-list (onlyDomains) ───────────────────────────────

    #[test]
    fn email_only_domains_accepts_matching() {
        let opts = EmailOptions {
            only_domains: vec!["example.com".to_string()],
            except_domains: Vec::new(),
        };
        assert!(opts.validate_value("f", &json!("user@example.com")).is_ok());
    }

    #[test]
    fn email_only_domains_rejects_non_matching() {
        let opts = EmailOptions {
            only_domains: vec!["example.com".to_string()],
            except_domains: Vec::new(),
        };
        assert!(opts.validate_value("f", &json!("user@other.com")).is_err());
    }

    #[test]
    fn email_only_domains_case_insensitive() {
        let opts = EmailOptions {
            only_domains: vec!["Example.COM".to_string()],
            except_domains: Vec::new(),
        };
        assert!(opts.validate_value("f", &json!("user@EXAMPLE.com")).is_ok());
    }

    #[test]
    fn email_only_domains_multiple() {
        let opts = EmailOptions {
            only_domains: vec!["a.com".to_string(), "b.com".to_string()],
            except_domains: Vec::new(),
        };
        assert!(opts.validate_value("f", &json!("user@a.com")).is_ok());
        assert!(opts.validate_value("f", &json!("user@b.com")).is_ok());
        assert!(opts.validate_value("f", &json!("user@c.com")).is_err());
    }

    // ── Domain block-list (exceptDomains) ─────────────────────────────

    #[test]
    fn email_except_domains_rejects_matching() {
        let opts = EmailOptions {
            only_domains: Vec::new(),
            except_domains: vec!["spam.com".to_string()],
        };
        assert!(opts.validate_value("f", &json!("user@spam.com")).is_err());
    }

    #[test]
    fn email_except_domains_accepts_non_matching() {
        let opts = EmailOptions {
            only_domains: Vec::new(),
            except_domains: vec!["spam.com".to_string()],
        };
        assert!(opts.validate_value("f", &json!("user@good.com")).is_ok());
    }

    #[test]
    fn email_except_domains_case_insensitive() {
        let opts = EmailOptions {
            only_domains: Vec::new(),
            except_domains: vec!["SPAM.COM".to_string()],
        };
        assert!(opts.validate_value("f", &json!("user@spam.com")).is_err());
    }

    #[test]
    fn email_except_domains_multiple() {
        let opts = EmailOptions {
            only_domains: Vec::new(),
            except_domains: vec!["spam.com".to_string(), "junk.net".to_string()],
        };
        assert!(opts.validate_value("f", &json!("user@spam.com")).is_err());
        assert!(opts.validate_value("f", &json!("user@junk.net")).is_err());
        assert!(opts.validate_value("f", &json!("user@good.com")).is_ok());
    }

    // ── Options validation ────────────────────────────────────────────

    #[test]
    fn email_options_reject_both_domain_lists() {
        let opts = EmailOptions {
            only_domains: vec!["a.com".to_string()],
            except_domains: vec!["b.com".to_string()],
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn email_options_accept_only_domains_alone() {
        let opts = EmailOptions {
            only_domains: vec!["a.com".to_string()],
            except_domains: Vec::new(),
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn email_options_accept_except_domains_alone() {
        let opts = EmailOptions {
            only_domains: Vec::new(),
            except_domains: vec!["b.com".to_string()],
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn email_options_accept_empty_lists() {
        let opts = EmailOptions::default();
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn email_options_reject_empty_domain_in_only() {
        let opts = EmailOptions {
            only_domains: vec!["".to_string()],
            except_domains: Vec::new(),
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn email_options_reject_whitespace_domain_in_except() {
        let opts = EmailOptions {
            only_domains: Vec::new(),
            except_domains: vec!["  ".to_string()],
        };
        assert!(opts.validate().is_err());
    }

    // ── Serde round-trip ──────────────────────────────────────────────

    #[test]
    fn email_field_serde_round_trip_default() {
        let ft = FieldType::Email(EmailOptions::default());
        let json = serde_json::to_value(&ft).unwrap();
        assert_eq!(json["type"], "email");
        let back: FieldType = serde_json::from_value(json).unwrap();
        assert_eq!(ft, back);
    }

    #[test]
    fn email_field_serde_round_trip_with_domains() {
        let ft = FieldType::Email(EmailOptions {
            only_domains: vec!["example.com".to_string()],
            except_domains: Vec::new(),
        });
        let json = serde_json::to_value(&ft).unwrap();
        assert_eq!(json["options"]["onlyDomains"], json!(["example.com"]));
        let back: FieldType = serde_json::from_value(json).unwrap();
        assert_eq!(ft, back);
    }

    #[test]
    fn email_sql_type_is_text() {
        assert_eq!(FieldType::Email(EmailOptions::default()).sql_type(), "TEXT");
    }

    #[test]
    fn email_type_name() {
        assert_eq!(
            FieldType::Email(EmailOptions::default()).type_name(),
            "email"
        );
    }

    // ── Integration with Field wrapper ────────────────────────────────

    #[test]
    fn email_required_field_rejects_null() {
        let f = Field::new("email", FieldType::Email(EmailOptions::default())).required(true);
        assert!(f.validate_value(&json!(null)).is_err());
    }

    #[test]
    fn email_optional_field_accepts_null() {
        let f = Field::new("email", FieldType::Email(EmailOptions::default()));
        assert!(f.validate_value(&json!(null)).is_ok());
    }

    #[test]
    fn email_required_field_rejects_empty_string() {
        let f = Field::new("email", FieldType::Email(EmailOptions::default())).required(true);
        assert!(f.validate_value(&json!("")).is_err());
    }

    // ── URL ────────────────────────────────────────────────────────────────

    // -- Basic validation --

    #[test]
    fn url_accepts_valid_https() {
        let opts = UrlOptions::default();
        assert!(opts
            .validate_value("f", &json!("https://example.com"))
            .is_ok());
    }

    #[test]
    fn url_accepts_valid_http() {
        let opts = UrlOptions::default();
        assert!(opts
            .validate_value("f", &json!("http://example.com"))
            .is_ok());
    }

    #[test]
    fn url_accepts_with_path() {
        let opts = UrlOptions::default();
        assert!(opts
            .validate_value("f", &json!("https://example.com/path/to/page"))
            .is_ok());
    }

    #[test]
    fn url_accepts_with_query() {
        let opts = UrlOptions::default();
        assert!(opts
            .validate_value("f", &json!("https://example.com?q=search&lang=en"))
            .is_ok());
    }

    #[test]
    fn url_accepts_with_fragment() {
        let opts = UrlOptions::default();
        assert!(opts
            .validate_value("f", &json!("https://example.com#section"))
            .is_ok());
    }

    #[test]
    fn url_accepts_with_port() {
        let opts = UrlOptions::default();
        assert!(opts
            .validate_value("f", &json!("http://localhost:8080"))
            .is_ok());
    }

    #[test]
    fn url_accepts_with_userinfo() {
        let opts = UrlOptions::default();
        assert!(opts
            .validate_value("f", &json!("ftp://user:pass@example.com/file"))
            .is_ok());
    }

    #[test]
    fn url_accepts_ftp_scheme() {
        let opts = UrlOptions::default();
        assert!(opts
            .validate_value("f", &json!("ftp://files.example.com"))
            .is_ok());
    }

    #[test]
    fn url_accepts_custom_scheme() {
        let opts = UrlOptions::default();
        assert!(opts
            .validate_value("f", &json!("myapp://deep-link/page"))
            .is_ok());
    }

    #[test]
    fn url_rejects_no_scheme() {
        let opts = UrlOptions::default();
        assert!(opts.validate_value("f", &json!("example.com")).is_err());
    }

    #[test]
    fn url_rejects_empty_scheme() {
        let opts = UrlOptions::default();
        assert!(opts.validate_value("f", &json!("://example.com")).is_err());
    }

    #[test]
    fn url_rejects_empty_after_scheme() {
        let opts = UrlOptions::default();
        assert!(opts.validate_value("f", &json!("https://")).is_err());
    }

    #[test]
    fn url_rejects_scheme_starting_with_digit() {
        let opts = UrlOptions::default();
        assert!(opts
            .validate_value("f", &json!("3http://example.com"))
            .is_err());
    }

    #[test]
    fn url_rejects_scheme_with_spaces() {
        let opts = UrlOptions::default();
        assert!(opts
            .validate_value("f", &json!("ht tp://example.com"))
            .is_err());
    }

    #[test]
    fn url_rejects_whitespace_in_url() {
        let opts = UrlOptions::default();
        assert!(opts
            .validate_value("f", &json!("https://example .com"))
            .is_err());
    }

    #[test]
    fn url_rejects_non_string() {
        let opts = UrlOptions::default();
        assert!(opts.validate_value("f", &json!(42)).is_err());
    }

    #[test]
    fn url_rejects_boolean() {
        let opts = UrlOptions::default();
        assert!(opts.validate_value("f", &json!(true)).is_err());
    }

    // -- Scheme filtering --

    #[test]
    fn url_only_schemes_accepts() {
        let opts = UrlOptions {
            only_schemes: vec!["https".to_string()],
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("https://example.com"))
            .is_ok());
    }

    #[test]
    fn url_only_schemes_rejects() {
        let opts = UrlOptions {
            only_schemes: vec!["https".to_string()],
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("http://example.com"))
            .is_err());
    }

    #[test]
    fn url_only_schemes_case_insensitive() {
        let opts = UrlOptions {
            only_schemes: vec!["HTTPS".to_string()],
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("https://example.com"))
            .is_ok());
    }

    #[test]
    fn url_only_schemes_multiple() {
        let opts = UrlOptions {
            only_schemes: vec!["http".to_string(), "https".to_string()],
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("http://example.com"))
            .is_ok());
        assert!(opts
            .validate_value("f", &json!("https://example.com"))
            .is_ok());
        assert!(opts
            .validate_value("f", &json!("ftp://example.com"))
            .is_err());
    }

    // -- Domain allow-list (onlyDomains) --

    #[test]
    fn url_only_domains_accepts_listed() {
        let opts = UrlOptions {
            only_domains: vec!["example.com".to_string()],
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("https://example.com"))
            .is_ok());
    }

    #[test]
    fn url_only_domains_rejects_unlisted() {
        let opts = UrlOptions {
            only_domains: vec!["example.com".to_string()],
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("https://other.com"))
            .is_err());
    }

    #[test]
    fn url_only_domains_case_insensitive() {
        let opts = UrlOptions {
            only_domains: vec!["Example.COM".to_string()],
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("https://example.com"))
            .is_ok());
    }

    #[test]
    fn url_only_domains_multiple() {
        let opts = UrlOptions {
            only_domains: vec!["example.com".to_string(), "test.org".to_string()],
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("https://example.com/page"))
            .is_ok());
        assert!(opts.validate_value("f", &json!("https://test.org")).is_ok());
        assert!(opts
            .validate_value("f", &json!("https://evil.com"))
            .is_err());
    }

    #[test]
    fn url_only_domains_with_port() {
        let opts = UrlOptions {
            only_domains: vec!["localhost".to_string()],
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("http://localhost:3000"))
            .is_ok());
    }

    #[test]
    fn url_only_domains_with_userinfo() {
        let opts = UrlOptions {
            only_domains: vec!["example.com".to_string()],
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("ftp://user@example.com/file"))
            .is_ok());
    }

    // -- Domain block-list (exceptDomains) --

    #[test]
    fn url_except_domains_blocks_listed() {
        let opts = UrlOptions {
            except_domains: vec!["evil.com".to_string()],
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("https://evil.com"))
            .is_err());
    }

    #[test]
    fn url_except_domains_allows_unlisted() {
        let opts = UrlOptions {
            except_domains: vec!["evil.com".to_string()],
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("https://good.com")).is_ok());
    }

    #[test]
    fn url_except_domains_case_insensitive() {
        let opts = UrlOptions {
            except_domains: vec!["Evil.COM".to_string()],
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("https://evil.com"))
            .is_err());
    }

    #[test]
    fn url_except_domains_multiple() {
        let opts = UrlOptions {
            except_domains: vec!["evil.com".to_string(), "bad.org".to_string()],
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("https://evil.com"))
            .is_err());
        assert!(opts.validate_value("f", &json!("https://bad.org")).is_err());
        assert!(opts.validate_value("f", &json!("https://good.com")).is_ok());
    }

    // -- Options validation --

    #[test]
    fn url_options_rejects_both_only_and_except_domains() {
        let opts = UrlOptions {
            only_domains: vec!["a.com".to_string()],
            except_domains: vec!["b.com".to_string()],
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn url_options_rejects_empty_only_domain_entry() {
        let opts = UrlOptions {
            only_domains: vec!["".to_string()],
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn url_options_rejects_blank_except_domain_entry() {
        let opts = UrlOptions {
            except_domains: vec!["  ".to_string()],
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn url_options_accepts_valid_only_domains() {
        let opts = UrlOptions {
            only_domains: vec!["example.com".to_string()],
            ..Default::default()
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn url_options_accepts_valid_except_domains() {
        let opts = UrlOptions {
            except_domains: vec!["evil.com".to_string()],
            ..Default::default()
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn url_options_accepts_default() {
        let opts = UrlOptions::default();
        assert!(opts.validate().is_ok());
    }

    // -- Combined scheme + domain filtering --

    #[test]
    fn url_scheme_and_domain_both_enforced() {
        let opts = UrlOptions {
            only_schemes: vec!["https".to_string()],
            only_domains: vec!["example.com".to_string()],
            ..Default::default()
        };
        // Both match
        assert!(opts
            .validate_value("f", &json!("https://example.com"))
            .is_ok());
        // Wrong scheme
        assert!(opts
            .validate_value("f", &json!("http://example.com"))
            .is_err());
        // Wrong domain
        assert!(opts
            .validate_value("f", &json!("https://other.com"))
            .is_err());
    }

    // -- SQL type and type name --

    #[test]
    fn url_sql_type_is_text() {
        assert_eq!(FieldType::Url(UrlOptions::default()).sql_type(), "TEXT");
    }

    #[test]
    fn url_type_name() {
        assert_eq!(FieldType::Url(UrlOptions::default()).type_name(), "url");
    }

    // -- Serialization round-trip --

    #[test]
    fn url_options_serde_round_trip() {
        let opts = UrlOptions {
            only_schemes: vec!["https".to_string()],
            only_domains: vec!["example.com".to_string()],
            except_domains: Vec::new(),
        };
        let json_str = serde_json::to_string(&opts).unwrap();
        let deserialized: UrlOptions = serde_json::from_str(&json_str).unwrap();
        assert_eq!(opts, deserialized);
    }

    #[test]
    fn url_options_serde_empty_vecs_omitted() {
        let opts = UrlOptions::default();
        let json_str = serde_json::to_string(&opts).unwrap();
        assert_eq!(json_str, "{}");
    }

    // ── DateTime ───────────────────────────────────────────────────────────

    // -- UTC-only mode (default) --

    #[test]
    fn datetime_utc_accepts_rfc3339() {
        let opts = DateTimeOptions::default();
        assert!(opts
            .validate_value("f", &json!("2024-01-15T10:30:00Z"))
            .is_ok());
    }

    #[test]
    fn datetime_utc_accepts_rfc3339_with_offset() {
        let opts = DateTimeOptions::default();
        assert!(opts
            .validate_value("f", &json!("2024-01-15T10:30:00+05:30"))
            .is_ok());
    }

    #[test]
    fn datetime_utc_accepts_naive_datetime() {
        let opts = DateTimeOptions::default();
        assert!(opts
            .validate_value("f", &json!("2024-01-15 10:30:00"))
            .is_ok());
    }

    #[test]
    fn datetime_utc_accepts_date_only() {
        let opts = DateTimeOptions::default();
        assert!(opts.validate_value("f", &json!("2024-01-15")).is_ok());
    }

    #[test]
    fn datetime_utc_rejects_invalid() {
        let opts = DateTimeOptions::default();
        assert!(opts.validate_value("f", &json!("not-a-date")).is_err());
    }

    #[test]
    fn datetime_utc_rejects_non_string() {
        let opts = DateTimeOptions::default();
        assert!(opts.validate_value("f", &json!(20240115)).is_err());
    }

    #[test]
    fn datetime_utc_rejects_partial_date() {
        let opts = DateTimeOptions::default();
        assert!(opts.validate_value("f", &json!("2024-01")).is_err());
    }

    #[test]
    fn datetime_utc_rejects_empty() {
        let opts = DateTimeOptions::default();
        // Empty string is handled at the Field level (text-like), but the type
        // validator itself should reject it.
        assert!(opts.validate_value("f", &json!("")).is_err());
    }

    // -- Timezone-aware mode --

    #[test]
    fn datetime_tz_accepts_offset() {
        let opts = DateTimeOptions {
            mode: DateTimeMode::TimezoneAware,
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("2024-01-15T10:30:00+05:30"))
            .is_ok());
    }

    #[test]
    fn datetime_tz_accepts_utc_zulu() {
        let opts = DateTimeOptions {
            mode: DateTimeMode::TimezoneAware,
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("2024-01-15T10:30:00Z"))
            .is_ok());
    }

    #[test]
    fn datetime_tz_rejects_naive() {
        let opts = DateTimeOptions {
            mode: DateTimeMode::TimezoneAware,
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("2024-01-15 10:30:00"))
            .is_err());
    }

    #[test]
    fn datetime_tz_rejects_date_only() {
        let opts = DateTimeOptions {
            mode: DateTimeMode::TimezoneAware,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("2024-01-15")).is_err());
    }

    // -- Date-only mode --

    #[test]
    fn datetime_dateonly_accepts_valid() {
        let opts = DateTimeOptions {
            mode: DateTimeMode::DateOnly,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("2024-01-15")).is_ok());
    }

    #[test]
    fn datetime_dateonly_rejects_with_time() {
        let opts = DateTimeOptions {
            mode: DateTimeMode::DateOnly,
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("2024-01-15T10:30:00Z"))
            .is_err());
    }

    #[test]
    fn datetime_dateonly_rejects_naive_datetime() {
        let opts = DateTimeOptions {
            mode: DateTimeMode::DateOnly,
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("2024-01-15 10:30:00"))
            .is_err());
    }

    #[test]
    fn datetime_dateonly_rejects_invalid() {
        let opts = DateTimeOptions {
            mode: DateTimeMode::DateOnly,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("2024-13-45")).is_err());
    }

    // -- Range checks --

    #[test]
    fn datetime_min_range() {
        let opts = DateTimeOptions {
            min: Some("2024-06-01".to_string()),
            max: None,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("2024-06-15")).is_ok());
        assert!(opts.validate_value("f", &json!("2024-05-01")).is_err());
    }

    #[test]
    fn datetime_max_range() {
        let opts = DateTimeOptions {
            min: None,
            max: Some("2024-12-31".to_string()),
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("2024-06-15")).is_ok());
        assert!(opts.validate_value("f", &json!("2025-01-01")).is_err());
    }

    #[test]
    fn datetime_min_and_max_range() {
        let opts = DateTimeOptions {
            min: Some("2024-01-01T00:00:00Z".to_string()),
            max: Some("2024-12-31T23:59:59Z".to_string()),
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!("2024-06-15T12:00:00Z"))
            .is_ok());
        assert!(opts
            .validate_value("f", &json!("2023-12-31T23:59:59Z"))
            .is_err());
        assert!(opts
            .validate_value("f", &json!("2025-01-01T00:00:00Z"))
            .is_err());
    }

    #[test]
    fn datetime_range_works_across_modes() {
        // DateOnly mode with date-only min/max boundaries
        let opts = DateTimeOptions {
            min: Some("2024-03-01".to_string()),
            max: Some("2024-03-31".to_string()),
            mode: DateTimeMode::DateOnly,
        };
        assert!(opts.validate_value("f", &json!("2024-03-15")).is_ok());
        assert!(opts.validate_value("f", &json!("2024-02-28")).is_err());
        assert!(opts.validate_value("f", &json!("2024-04-01")).is_err());
    }

    #[test]
    fn datetime_boundary_inclusive() {
        let opts = DateTimeOptions {
            min: Some("2024-06-01".to_string()),
            max: Some("2024-06-30".to_string()),
            ..Default::default()
        };
        // Boundaries themselves should be accepted
        assert!(opts.validate_value("f", &json!("2024-06-01")).is_ok());
        assert!(opts.validate_value("f", &json!("2024-06-30")).is_ok());
    }

    // -- Options validation --

    #[test]
    fn datetime_options_valid_default() {
        let opts = DateTimeOptions::default();
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn datetime_options_valid_min_only() {
        let opts = DateTimeOptions {
            min: Some("2024-01-01".to_string()),
            ..Default::default()
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn datetime_options_rejects_invalid_min() {
        let opts = DateTimeOptions {
            min: Some("not-a-date".to_string()),
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn datetime_options_rejects_invalid_max() {
        let opts = DateTimeOptions {
            max: Some("nope".to_string()),
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn datetime_options_rejects_min_gt_max() {
        let opts = DateTimeOptions {
            min: Some("2024-12-31".to_string()),
            max: Some("2024-01-01".to_string()),
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn datetime_options_accepts_min_eq_max() {
        let opts = DateTimeOptions {
            min: Some("2024-06-15".to_string()),
            max: Some("2024-06-15".to_string()),
            ..Default::default()
        };
        assert!(opts.validate().is_ok());
    }

    // -- Value preparation / normalization --

    #[test]
    fn datetime_prepare_utc_converts_offset() {
        let opts = DateTimeOptions::default();
        let val = json!("2024-01-15T10:30:00+05:30");
        let prepared = opts.prepare_value(&val).unwrap();
        // +05:30 → UTC = 05:00
        assert_eq!(prepared.as_str().unwrap(), "2024-01-15 05:00:00");
    }

    #[test]
    fn datetime_prepare_utc_keeps_naive() {
        let opts = DateTimeOptions::default();
        let val = json!("2024-01-15 10:30:00");
        let prepared = opts.prepare_value(&val).unwrap();
        assert_eq!(prepared.as_str().unwrap(), "2024-01-15 10:30:00");
    }

    #[test]
    fn datetime_prepare_dateonly_strips_time() {
        let opts = DateTimeOptions {
            mode: DateTimeMode::DateOnly,
            ..Default::default()
        };
        let val = json!("2024-01-15");
        let prepared = opts.prepare_value(&val).unwrap();
        assert_eq!(prepared.as_str().unwrap(), "2024-01-15");
    }

    #[test]
    fn datetime_prepare_tz_preserves_original() {
        let opts = DateTimeOptions {
            mode: DateTimeMode::TimezoneAware,
            ..Default::default()
        };
        let val = json!("2024-01-15T10:30:00+05:30");
        let prepared = opts.prepare_value(&val).unwrap();
        assert_eq!(prepared.as_str().unwrap(), "2024-01-15T10:30:00+05:30");
    }

    // -- parse_datetime function --

    #[test]
    fn parse_datetime_rfc3339() {
        let dt = parse_datetime("2024-06-15T12:00:00Z").unwrap();
        assert_eq!(dt.to_string(), "2024-06-15 12:00:00");
    }

    #[test]
    fn parse_datetime_with_offset() {
        // +03:00 → UTC = 09:00
        let dt = parse_datetime("2024-06-15T12:00:00+03:00").unwrap();
        assert_eq!(dt.to_string(), "2024-06-15 09:00:00");
    }

    #[test]
    fn parse_datetime_naive() {
        let dt = parse_datetime("2024-06-15 12:00:00").unwrap();
        assert_eq!(dt.to_string(), "2024-06-15 12:00:00");
    }

    #[test]
    fn parse_datetime_date_only() {
        let dt = parse_datetime("2024-06-15").unwrap();
        assert_eq!(dt.to_string(), "2024-06-15 00:00:00");
    }

    #[test]
    fn parse_datetime_invalid_returns_none() {
        assert!(parse_datetime("garbage").is_none());
        assert!(parse_datetime("").is_none());
        assert!(parse_datetime("2024").is_none());
        assert!(parse_datetime("2024-13-01").is_none());
    }

    // -- compare_datetimes function --

    #[test]
    fn compare_datetimes_equal() {
        assert_eq!(
            compare_datetimes("2024-06-15T12:00:00Z", "2024-06-15T12:00:00Z"),
            Some(std::cmp::Ordering::Equal)
        );
    }

    #[test]
    fn compare_datetimes_less() {
        assert_eq!(
            compare_datetimes("2024-01-01", "2024-12-31"),
            Some(std::cmp::Ordering::Less)
        );
    }

    #[test]
    fn compare_datetimes_greater() {
        assert_eq!(
            compare_datetimes("2025-01-01T00:00:00Z", "2024-12-31T23:59:59Z"),
            Some(std::cmp::Ordering::Greater)
        );
    }

    #[test]
    fn compare_datetimes_cross_format() {
        // RFC3339 vs date-only (both represent same moment at midnight UTC)
        assert_eq!(
            compare_datetimes("2024-06-15T00:00:00Z", "2024-06-15"),
            Some(std::cmp::Ordering::Equal)
        );
    }

    #[test]
    fn compare_datetimes_with_offset() {
        // 12:00+03:00 = 09:00 UTC, which is less than 10:00 UTC
        assert_eq!(
            compare_datetimes("2024-06-15T12:00:00+03:00", "2024-06-15T10:00:00Z"),
            Some(std::cmp::Ordering::Less)
        );
    }

    #[test]
    fn compare_datetimes_invalid_returns_none() {
        assert!(compare_datetimes("bad", "2024-01-01").is_none());
        assert!(compare_datetimes("2024-01-01", "bad").is_none());
    }

    // -- Serialization round-trip --

    #[test]
    fn datetime_mode_serialization() {
        let opts = DateTimeOptions {
            min: Some("2024-01-01".to_string()),
            max: None,
            mode: DateTimeMode::TimezoneAware,
        };
        let json_str = serde_json::to_string(&opts).unwrap();
        let deserialized: DateTimeOptions = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.mode, DateTimeMode::TimezoneAware);
        assert_eq!(deserialized.min, Some("2024-01-01".to_string()));
    }

    #[test]
    fn datetime_mode_defaults_to_utc_only() {
        let json_str = r#"{"min": null, "max": null}"#;
        let opts: DateTimeOptions = serde_json::from_str(json_str).unwrap();
        assert_eq!(opts.mode, DateTimeMode::UtcOnly);
    }

    #[test]
    fn datetime_dateonly_mode_roundtrip() {
        let json_str = r#"{"mode": "dateOnly"}"#;
        let opts: DateTimeOptions = serde_json::from_str(json_str).unwrap();
        assert_eq!(opts.mode, DateTimeMode::DateOnly);
    }

    // ── AutoDate ───────────────────────────────────────────────────────────

    #[test]
    fn autodate_default_both_true() {
        let opts = AutoDateOptions::default();
        assert!(opts.on_create);
        assert!(opts.on_update);
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn autodate_rejects_both_false() {
        let opts = AutoDateOptions {
            on_create: false,
            on_update: false,
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn autodate_accepts_create_only() {
        let opts = AutoDateOptions {
            on_create: true,
            on_update: false,
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn autodate_accepts_update_only() {
        let opts = AutoDateOptions {
            on_create: false,
            on_update: true,
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn autodate_validate_value_accepts_valid_datetime() {
        let opts = AutoDateOptions::default();
        assert!(opts
            .validate_value("f", &json!("2024-06-15 12:30:00"))
            .is_ok());
    }

    #[test]
    fn autodate_validate_value_accepts_date_only() {
        let opts = AutoDateOptions::default();
        assert!(opts.validate_value("f", &json!("2024-06-15")).is_ok());
    }

    #[test]
    fn autodate_validate_value_accepts_rfc3339() {
        let opts = AutoDateOptions::default();
        assert!(opts
            .validate_value("f", &json!("2024-06-15T12:30:00Z"))
            .is_ok());
    }

    #[test]
    fn autodate_validate_value_rejects_non_string() {
        let opts = AutoDateOptions::default();
        assert!(opts.validate_value("f", &json!(12345)).is_err());
    }

    #[test]
    fn autodate_validate_value_rejects_invalid_format() {
        let opts = AutoDateOptions::default();
        assert!(opts.validate_value("f", &json!("not-a-date")).is_err());
    }

    #[test]
    fn autodate_validate_value_rejects_boolean() {
        let opts = AutoDateOptions::default();
        assert!(opts.validate_value("f", &json!(true)).is_err());
    }

    #[test]
    fn autodate_now_utc_produces_valid_datetime() {
        let now = AutoDateOptions::now_utc();
        assert!(
            parse_datetime(&now).is_some(),
            "now_utc should produce parseable datetime: {now}"
        );
    }

    #[test]
    fn autodate_serialization_roundtrip() {
        let opts = AutoDateOptions {
            on_create: true,
            on_update: false,
        };
        let json = serde_json::to_string(&opts).unwrap();
        let parsed: AutoDateOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(opts, parsed);
    }

    #[test]
    fn autodate_deserialization_defaults() {
        // Empty JSON should default both to true
        let opts: AutoDateOptions = serde_json::from_str("{}").unwrap();
        assert!(opts.on_create);
        assert!(opts.on_update);
    }

    #[test]
    fn autodate_field_type_validate_value_delegates() {
        let ft = FieldType::AutoDate(AutoDateOptions::default());
        assert!(ft
            .validate_value("f", &json!("2024-01-01 00:00:00"))
            .is_ok());
        assert!(ft.validate_value("f", &json!(42)).is_err());
    }

    // ── Select (single value) ────────────────────────────────────────────

    #[test]
    fn select_accepts_valid_value() {
        let opts = SelectOptions {
            values: vec!["a".into(), "b".into(), "c".into()],
        };
        assert!(opts.validate_value("f", &json!("b")).is_ok());
    }

    #[test]
    fn select_rejects_invalid_value() {
        let opts = SelectOptions {
            values: vec!["a".into(), "b".into()],
        };
        assert!(opts.validate_value("f", &json!("z")).is_err());
    }

    #[test]
    fn select_rejects_non_string() {
        let opts = SelectOptions {
            values: vec!["a".into(), "b".into()],
        };
        assert!(opts.validate_value("f", &json!(42)).is_err());
    }

    #[test]
    fn select_rejects_array() {
        let opts = SelectOptions {
            values: vec!["a".into(), "b".into()],
        };
        assert!(opts.validate_value("f", &json!(["a"])).is_err());
    }

    #[test]
    fn select_options_reject_empty_values() {
        let opts = SelectOptions { values: Vec::new() };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn select_options_reject_duplicate_values() {
        let opts = SelectOptions {
            values: vec!["a".into(), "b".into(), "a".into()],
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn select_options_accept_valid_values() {
        let opts = SelectOptions {
            values: vec!["draft".into(), "published".into(), "archived".into()],
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn select_is_text_like() {
        let ft = FieldType::Select(SelectOptions {
            values: vec!["a".into()],
        });
        assert!(ft.is_text_like());
    }

    #[test]
    fn select_required_rejects_empty_string() {
        let f = Field::new(
            "status",
            FieldType::Select(SelectOptions {
                values: vec!["a".into(), "b".into()],
            }),
        )
        .required(true);
        assert!(f.validate_value(&json!("")).is_err());
    }

    #[test]
    fn select_optional_accepts_empty_string() {
        let f = Field::new(
            "status",
            FieldType::Select(SelectOptions {
                values: vec!["a".into(), "b".into()],
            }),
        );
        assert!(f.validate_value(&json!("")).is_ok());
    }

    // ── MultiSelect ──────────────────────────────────────────────────────

    #[test]
    fn multiselect_accepts_valid_array() {
        let opts = MultiSelectOptions {
            values: vec!["a".into(), "b".into(), "c".into()],
            max_select: 0,
        };
        assert!(opts.validate_value("f", &json!(["a", "c"])).is_ok());
    }

    #[test]
    fn multiselect_accepts_empty_array() {
        let opts = MultiSelectOptions {
            values: vec!["a".into(), "b".into()],
            max_select: 0,
        };
        assert!(opts.validate_value("f", &json!([])).is_ok());
    }

    #[test]
    fn multiselect_accepts_single_element_array() {
        let opts = MultiSelectOptions {
            values: vec!["a".into(), "b".into()],
            max_select: 0,
        };
        assert!(opts.validate_value("f", &json!(["a"])).is_ok());
    }

    #[test]
    fn multiselect_rejects_too_many() {
        let opts = MultiSelectOptions {
            values: vec!["a".into(), "b".into(), "c".into()],
            max_select: 2,
        };
        assert!(opts.validate_value("f", &json!(["a", "b", "c"])).is_err());
    }

    #[test]
    fn multiselect_rejects_invalid_value() {
        let opts = MultiSelectOptions {
            values: vec!["a".into(), "b".into()],
            max_select: 0,
        };
        assert!(opts.validate_value("f", &json!(["a", "z"])).is_err());
    }

    #[test]
    fn multiselect_rejects_non_array() {
        let opts = MultiSelectOptions {
            values: vec!["a".into(), "b".into()],
            max_select: 0,
        };
        assert!(opts.validate_value("f", &json!("a")).is_err());
    }

    #[test]
    fn multiselect_rejects_non_string_items() {
        let opts = MultiSelectOptions {
            values: vec!["a".into(), "b".into()],
            max_select: 0,
        };
        assert!(opts.validate_value("f", &json!([1, 2])).is_err());
    }

    #[test]
    fn multiselect_rejects_duplicate_selections() {
        let opts = MultiSelectOptions {
            values: vec!["a".into(), "b".into(), "c".into()],
            max_select: 0,
        };
        assert!(opts.validate_value("f", &json!(["a", "a"])).is_err());
    }

    #[test]
    fn multiselect_no_limit_accepts_all() {
        let opts = MultiSelectOptions {
            values: vec!["a".into(), "b".into(), "c".into()],
            max_select: 0,
        };
        assert!(opts.validate_value("f", &json!(["a", "b", "c"])).is_ok());
    }

    #[test]
    fn multiselect_max_select_allows_exact() {
        let opts = MultiSelectOptions {
            values: vec!["a".into(), "b".into(), "c".into()],
            max_select: 2,
        };
        assert!(opts.validate_value("f", &json!(["a", "b"])).is_ok());
    }

    #[test]
    fn multiselect_options_reject_empty_values() {
        let opts = MultiSelectOptions {
            values: Vec::new(),
            max_select: 0,
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn multiselect_options_reject_duplicate_values() {
        let opts = MultiSelectOptions {
            values: vec!["a".into(), "b".into(), "a".into()],
            max_select: 0,
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn multiselect_options_accept_valid() {
        let opts = MultiSelectOptions {
            values: vec!["tag1".into(), "tag2".into(), "tag3".into()],
            max_select: 5,
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn multiselect_is_not_text_like() {
        let ft = FieldType::MultiSelect(MultiSelectOptions {
            values: vec!["a".into()],
            max_select: 0,
        });
        assert!(!ft.is_text_like());
    }

    #[test]
    fn multiselect_sql_type_is_text() {
        let ft = FieldType::MultiSelect(MultiSelectOptions {
            values: vec!["a".into()],
            max_select: 0,
        });
        assert_eq!(ft.sql_type(), "TEXT");
    }

    // ── Relation ───────────────────────────────────────────────────────────

    #[test]
    fn relation_single_accepts_string_id() {
        let opts = RelationOptions {
            collection_id: "users".into(),
            max_select: 1,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("abc123")).is_ok());
    }

    #[test]
    fn relation_single_rejects_empty_id() {
        let opts = RelationOptions {
            collection_id: "users".into(),
            max_select: 1,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("")).is_err());
    }

    #[test]
    fn relation_multi_accepts_array_of_ids() {
        let opts = RelationOptions {
            collection_id: "tags".into(),
            max_select: 5,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(["id1", "id2"])).is_ok());
    }

    #[test]
    fn relation_multi_rejects_too_many() {
        let opts = RelationOptions {
            collection_id: "tags".into(),
            max_select: 2,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(["a", "b", "c"])).is_err());
    }

    #[test]
    fn relation_options_reject_empty_collection_id() {
        let opts = RelationOptions {
            collection_id: String::new(),
            max_select: 1,
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    // ── Editor ─────────────────────────────────────────────────────────────

    // --- Basic acceptance ---

    #[test]
    fn editor_accepts_html() {
        let opts = EditorOptions::default();
        assert!(opts.validate_value("f", &json!("<p>Hello</p>")).is_ok());
    }

    #[test]
    fn editor_accepts_plain_text() {
        let opts = EditorOptions::default();
        assert!(opts.validate_value("f", &json!("just plain text")).is_ok());
    }

    #[test]
    fn editor_accepts_empty_string() {
        let opts = EditorOptions::default();
        assert!(opts.validate_value("f", &json!("")).is_ok());
    }

    #[test]
    fn editor_accepts_complex_html() {
        let opts = EditorOptions::default();
        let html = r#"<h1>Title</h1><p>A paragraph with <strong>bold</strong> and <em>italic</em>.</p><ul><li>Item 1</li><li>Item 2</li></ul>"#;
        assert!(opts.validate_value("f", &json!(html)).is_ok());
    }

    // --- Type rejection ---

    #[test]
    fn editor_rejects_non_string() {
        let opts = EditorOptions::default();
        assert!(opts.validate_value("f", &json!(42)).is_err());
    }

    #[test]
    fn editor_rejects_boolean() {
        let opts = EditorOptions::default();
        assert!(opts.validate_value("f", &json!(true)).is_err());
    }

    #[test]
    fn editor_rejects_array() {
        let opts = EditorOptions::default();
        assert!(opts.validate_value("f", &json!([1, 2])).is_err());
    }

    // --- Length constraint ---

    #[test]
    fn editor_rejects_too_long() {
        let opts = EditorOptions {
            max_length: 5,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("toolong")).is_err());
    }

    #[test]
    fn editor_accepts_at_max_length() {
        let opts = EditorOptions {
            max_length: 5,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("exact")).is_ok());
    }

    #[test]
    fn editor_zero_max_length_means_no_limit() {
        let opts = EditorOptions::default();
        let long_text = "a".repeat(100_000);
        assert!(opts.validate_value("f", &json!(long_text)).is_ok());
    }

    // --- XSS prevention / HTML sanitization ---

    #[test]
    fn editor_strips_script_tags() {
        let opts = EditorOptions::default();
        let result = opts.sanitize_html("<p>Hello</p><script>alert('xss')</script>");
        assert!(!result.contains("<script>"));
        assert!(!result.contains("alert"));
        assert!(result.contains("<p>Hello</p>"));
    }

    #[test]
    fn editor_strips_onerror_attribute() {
        let opts = EditorOptions::default();
        let result = opts.sanitize_html(r#"<img src="x" onerror="alert('xss')">"#);
        assert!(!result.contains("onerror"));
        assert!(!result.contains("alert"));
    }

    #[test]
    fn editor_strips_onclick_attribute() {
        let opts = EditorOptions::default();
        let result = opts.sanitize_html(r#"<p onclick="alert('xss')">Click me</p>"#);
        assert!(!result.contains("onclick"));
        assert!(!result.contains("alert"));
        assert!(result.contains("Click me"));
    }

    #[test]
    fn editor_strips_javascript_href() {
        let opts = EditorOptions::default();
        let result = opts.sanitize_html(r#"<a href="javascript:alert('xss')">Click</a>"#);
        assert!(!result.contains("javascript:"));
    }

    #[test]
    fn editor_strips_data_uri_in_href() {
        let opts = EditorOptions::default();
        let result =
            opts.sanitize_html(r#"<a href="data:text/html,<script>alert(1)</script>">x</a>"#);
        assert!(!result.contains("data:"));
    }

    #[test]
    fn editor_strips_style_tags() {
        let opts = EditorOptions::default();
        let result = opts.sanitize_html("<style>body { display: none }</style><p>Hi</p>");
        assert!(!result.contains("<style>"));
        assert!(result.contains("<p>Hi</p>"));
    }

    #[test]
    fn editor_strips_iframe_tags() {
        let opts = EditorOptions::default();
        let result = opts.sanitize_html(r#"<iframe src="https://evil.com"></iframe><p>Hi</p>"#);
        assert!(!result.contains("<iframe"));
        assert!(result.contains("<p>Hi</p>"));
    }

    #[test]
    fn editor_strips_event_handlers_on_any_tag() {
        let opts = EditorOptions::default();
        let vectors = vec![
            r#"<div onmouseover="alert(1)">hi</div>"#,
            r#"<img src=x onerror=alert(1)>"#,
            r#"<body onload="alert(1)">"#,
            r#"<svg onload="alert(1)">"#,
            r#"<input onfocus="alert(1)">"#,
        ];
        for v in vectors {
            let result = opts.sanitize_html(v);
            assert!(!result.contains("alert"), "XSS not sanitized in: {v}");
        }
    }

    #[test]
    fn editor_strips_svg_with_script() {
        let opts = EditorOptions::default();
        let result = opts.sanitize_html(r#"<svg><script>alert(1)</script></svg>"#);
        assert!(!result.contains("<script>"));
        assert!(!result.contains("alert"));
    }

    #[test]
    fn editor_strips_meta_refresh() {
        let opts = EditorOptions::default();
        let result =
            opts.sanitize_html(r#"<meta http-equiv="refresh" content="0;url=https://evil.com">"#);
        assert!(!result.contains("<meta"));
    }

    #[test]
    fn editor_strips_object_embed() {
        let opts = EditorOptions::default();
        let result =
            opts.sanitize_html(r#"<object data="evil.swf"></object><embed src="evil.swf">"#);
        assert!(!result.contains("<object"));
        assert!(!result.contains("<embed"));
    }

    #[test]
    fn editor_strips_form_tags() {
        let opts = EditorOptions::default();
        let result =
            opts.sanitize_html(r#"<form action="https://evil.com"><input type="text"></form>"#);
        assert!(!result.contains("<form"));
        assert!(!result.contains("<input"));
    }

    #[test]
    fn editor_preserves_safe_content() {
        let opts = EditorOptions::default();
        let html = r#"<h1>Title</h1><p>Paragraph with <strong>bold</strong>, <em>italic</em>, and <a href="https://example.com" target="_blank">a link</a>.</p><ul><li>Item 1</li></ul><img src="https://example.com/img.png" alt="photo">"#;
        let result = opts.sanitize_html(html);
        assert!(result.contains("<h1>Title</h1>"));
        assert!(result.contains("<strong>bold</strong>"));
        assert!(result.contains("<em>italic</em>"));
        assert!(result.contains(r#"href="https://example.com""#));
        assert!(result.contains("<ul>"));
        assert!(result.contains("<li>Item 1</li>"));
        assert!(result.contains(r#"src="https://example.com/img.png""#));
        assert!(result.contains(r#"alt="photo""#));
    }

    #[test]
    fn editor_adds_rel_noopener_to_links() {
        let opts = EditorOptions::default();
        let result =
            opts.sanitize_html(r#"<a href="https://example.com" target="_blank">link</a>"#);
        assert!(result.contains("noopener"));
        assert!(result.contains("noreferrer"));
    }

    #[test]
    fn editor_strips_html_comments() {
        let opts = EditorOptions::default();
        let result = opts.sanitize_html("<!-- comment --><p>visible</p>");
        assert!(!result.contains("<!--"));
        assert!(result.contains("<p>visible</p>"));
    }

    // --- prepare_value (sanitization for storage) ---

    #[test]
    fn editor_prepare_value_sanitizes_xss() {
        let opts = EditorOptions::default();
        let input = json!("<p>Hello</p><script>alert('xss')</script>");
        let prepared = opts.prepare_value(&input).unwrap();
        let s = prepared.as_str().unwrap();
        assert!(s.contains("<p>Hello</p>"));
        assert!(!s.contains("<script>"));
    }

    #[test]
    fn editor_prepare_value_returns_none_for_non_string() {
        let opts = EditorOptions::default();
        assert!(opts.prepare_value(&json!(42)).is_none());
    }

    // --- Custom allowed tags ---

    #[test]
    fn editor_custom_tags_restrict_output() {
        let mut allowed = HashSet::new();
        allowed.insert("b".to_string());
        allowed.insert("i".to_string());

        let opts = EditorOptions {
            max_length: 0,
            allowed_tags: Some(allowed),
            allowed_attributes: Some(HashMap::new()),
            searchable: false,
        };

        let result = opts.sanitize_html("<p>Hello</p><b>Bold</b><i>Italic</i>");
        // <p> should be stripped since it's not in allowed tags.
        assert!(!result.contains("<p>"));
        assert!(result.contains("<b>Bold</b>"));
        assert!(result.contains("<i>Italic</i>"));
        assert!(result.contains("Hello")); // text preserved
    }

    #[test]
    fn editor_custom_attributes_restrict_output() {
        let mut attrs = HashMap::new();
        let mut a_attrs = HashSet::new();
        a_attrs.insert("href".to_string());
        attrs.insert("a".to_string(), a_attrs);

        let mut tags = HashSet::new();
        tags.insert("a".to_string());

        let opts = EditorOptions {
            max_length: 0,
            allowed_tags: Some(tags),
            allowed_attributes: Some(attrs),
            searchable: false,
        };

        let result = opts.sanitize_html(
            r#"<a href="https://example.com" target="_blank" class="link">link</a>"#,
        );
        assert!(result.contains(r#"href="https://example.com""#));
        // target and class should be stripped (not in custom allowed).
        assert!(!result.contains("target="));
        assert!(!result.contains("class="));
    }

    // --- Options validation ---

    #[test]
    fn editor_options_reject_empty_tag_in_allowed_tags() {
        let mut tags = HashSet::new();
        tags.insert("".to_string());
        let opts = EditorOptions {
            max_length: 0,
            allowed_tags: Some(tags),
            allowed_attributes: None,
            searchable: false,
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn editor_options_accept_valid_custom_tags() {
        let mut tags = HashSet::new();
        tags.insert("p".to_string());
        tags.insert("b".to_string());
        let opts = EditorOptions {
            max_length: 0,
            allowed_tags: Some(tags),
            allowed_attributes: None,
            searchable: false,
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn editor_options_default_validate_ok() {
        let opts = EditorOptions::default();
        assert!(opts.validate().is_ok());
    }

    // --- FieldType integration ---

    #[test]
    fn editor_field_type_prepare_sanitizes() {
        let ft = FieldType::Editor(EditorOptions::default());
        let val = json!("<p>ok</p><script>evil()</script>");
        let prepared = ft.prepare_value(&val).unwrap();
        let s = prepared.as_str().unwrap();
        assert!(s.contains("<p>ok</p>"));
        assert!(!s.contains("<script>"));
    }

    #[test]
    fn editor_field_type_prepare_null_returns_none() {
        let ft = FieldType::Editor(EditorOptions::default());
        assert!(ft.prepare_value(&serde_json::Value::Null).is_none());
    }

    // --- Serialization round-trip ---

    #[test]
    fn editor_options_serde_roundtrip_default() {
        let opts = EditorOptions::default();
        let json = serde_json::to_string(&opts).unwrap();
        let parsed: EditorOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(opts, parsed);
    }

    #[test]
    fn editor_options_serde_roundtrip_custom() {
        let mut tags = HashSet::new();
        tags.insert("p".to_string());
        let mut attrs = HashMap::new();
        attrs.insert("*".to_string(), {
            let mut s = HashSet::new();
            s.insert("class".to_string());
            s
        });
        let opts = EditorOptions {
            max_length: 1000,
            allowed_tags: Some(tags),
            allowed_attributes: Some(attrs),
            searchable: false,
        };
        let json = serde_json::to_string(&opts).unwrap();
        let parsed: EditorOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(opts, parsed);
    }

    #[test]
    fn editor_options_none_fields_omitted_in_json() {
        let opts = EditorOptions::default();
        let json = serde_json::to_string(&opts).unwrap();
        assert!(!json.contains("allowedTags"));
        assert!(!json.contains("allowedAttributes"));
    }

    // --- Max length applies to sanitized output ---

    #[test]
    fn editor_max_length_applies_to_sanitized_output() {
        let opts = EditorOptions {
            max_length: 20,
            ..Default::default()
        };
        // After sanitization, <script> tags are removed, so the text may be shorter.
        let html = "<p>Hi</p><script>alert('x')</script>";
        let sanitized = opts.sanitize_html(html);
        // The sanitized result is "<p>Hi</p>" which is 9 chars, under the limit.
        assert!(opts.validate_value("f", &json!(html)).is_ok());
        assert!(sanitized.len() <= 20);
    }

    // ── Password ───────────────────────────────────────────────────────────

    #[test]
    fn password_accepts_valid() {
        let opts = PasswordOptions::default();
        assert!(opts.validate_value("f", &json!("secureP@ss1")).is_ok());
    }

    #[test]
    fn password_rejects_too_short() {
        let opts = PasswordOptions {
            min_length: 8,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("short")).is_err());
    }

    #[test]
    fn password_rejects_too_long() {
        let opts = PasswordOptions {
            min_length: 1,
            max_length: 5,
            pattern: None,
        };
        assert!(opts.validate_value("f", &json!("toolong")).is_err());
    }

    #[test]
    fn password_validates_pattern() {
        let opts = PasswordOptions {
            min_length: 1,
            max_length: 0,
            pattern: Some(r"[A-Z]".to_string()), // requires uppercase
        };
        assert!(opts.validate_value("f", &json!("hasUpper")).is_ok());
        assert!(opts.validate_value("f", &json!("alllower")).is_err());
    }

    #[test]
    fn password_options_reject_zero_min_length() {
        let opts = PasswordOptions {
            min_length: 0,
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn password_options_reject_min_gt_max() {
        let opts = PasswordOptions {
            min_length: 20,
            max_length: 10,
            pattern: None,
        };
        assert!(opts.validate().is_err());
    }

    // ── FieldType metadata ─────────────────────────────────────────────────

    #[test]
    fn field_type_names() {
        assert_eq!(FieldType::Text(TextOptions::default()).type_name(), "text");
        assert_eq!(
            FieldType::Number(NumberOptions::default()).type_name(),
            "number"
        );
        assert_eq!(FieldType::Bool(BoolOptions::default()).type_name(), "bool");
        assert_eq!(
            FieldType::Email(EmailOptions::default()).type_name(),
            "email"
        );
        assert_eq!(FieldType::Url(UrlOptions::default()).type_name(), "url");
        assert_eq!(
            FieldType::DateTime(DateTimeOptions::default()).type_name(),
            "dateTime"
        );
        assert_eq!(
            FieldType::AutoDate(AutoDateOptions::default()).type_name(),
            "autoDate"
        );
        assert_eq!(
            FieldType::Select(SelectOptions {
                values: vec!["a".into()]
            })
            .type_name(),
            "select"
        );
        assert_eq!(
            FieldType::MultiSelect(MultiSelectOptions {
                values: vec!["a".into()],
                max_select: 0
            })
            .type_name(),
            "multiSelect"
        );
        assert_eq!(FieldType::File(FileOptions::default()).type_name(), "file");
        assert_eq!(
            FieldType::Relation(RelationOptions {
                collection_id: "x".into(),
                ..Default::default()
            })
            .type_name(),
            "relation"
        );
        assert_eq!(FieldType::Json(JsonOptions::default()).type_name(), "json");
        assert_eq!(
            FieldType::Editor(EditorOptions::default()).type_name(),
            "editor"
        );
        assert_eq!(
            FieldType::Password(PasswordOptions::default()).type_name(),
            "password"
        );
    }

    #[test]
    fn sql_type_mapping() {
        assert_eq!(FieldType::Text(TextOptions::default()).sql_type(), "TEXT");
        assert_eq!(
            FieldType::Number(NumberOptions::default()).sql_type(),
            "REAL"
        );
        assert_eq!(
            FieldType::Bool(BoolOptions::default()).sql_type(),
            "INTEGER"
        );
        assert_eq!(FieldType::Email(EmailOptions::default()).sql_type(), "TEXT");
        assert_eq!(FieldType::Json(JsonOptions::default()).sql_type(), "TEXT");
    }

    // ── Serde round-trip ───────────────────────────────────────────────────

    #[test]
    fn field_serializes_and_deserializes() {
        let field = Field::new(
            "title",
            FieldType::Text(TextOptions {
                min_length: 1,
                max_length: 255,
                pattern: None,
                searchable: false,
            }),
        )
        .required(true);

        let json = serde_json::to_string(&field).unwrap();
        let deserialized: Field = serde_json::from_str(&json).unwrap();

        assert_eq!(field.name, deserialized.name);
        assert_eq!(field.required, deserialized.required);
        assert_eq!(field.field_type, deserialized.field_type);
    }

    #[test]
    fn field_type_serde_round_trip_select() {
        let ft = FieldType::Select(SelectOptions {
            values: vec!["draft".into(), "published".into(), "archived".into()],
        });
        let json = serde_json::to_value(&ft).unwrap();
        assert_eq!(json["type"], "select");
        assert_eq!(json["options"]["values"].as_array().unwrap().len(), 3);

        let deserialized: FieldType = serde_json::from_value(json).unwrap();
        assert_eq!(ft, deserialized);
    }

    #[test]
    fn field_type_serde_round_trip_multiselect() {
        let ft = FieldType::MultiSelect(MultiSelectOptions {
            values: vec!["tag1".into(), "tag2".into(), "tag3".into()],
            max_select: 5,
        });
        let json = serde_json::to_value(&ft).unwrap();
        assert_eq!(json["type"], "multiSelect");
        assert_eq!(json["options"]["values"].as_array().unwrap().len(), 3);
        assert_eq!(json["options"]["maxSelect"], 5);

        let deserialized: FieldType = serde_json::from_value(json).unwrap();
        assert_eq!(ft, deserialized);
    }

    #[test]
    fn field_type_serde_round_trip_multiselect_no_limit() {
        let ft = FieldType::MultiSelect(MultiSelectOptions {
            values: vec!["a".into(), "b".into()],
            max_select: 0,
        });
        let json = serde_json::to_value(&ft).unwrap();
        assert_eq!(json["type"], "multiSelect");

        let deserialized: FieldType = serde_json::from_value(json).unwrap();
        assert_eq!(ft, deserialized);
    }

    #[test]
    fn field_type_serde_round_trip_relation() {
        let ft = FieldType::Relation(RelationOptions {
            collection_id: "users".into(),
            max_select: 1,
            cascade_delete: true,
            ..Default::default()
        });
        let json = serde_json::to_value(&ft).unwrap();
        assert_eq!(json["type"], "relation");
        assert_eq!(json["options"]["collectionId"], "users");
        assert_eq!(json["options"]["cascadeDelete"], true);

        let deserialized: FieldType = serde_json::from_value(json).unwrap();
        assert_eq!(ft, deserialized);
    }

    #[test]
    fn field_type_serde_round_trip_number() {
        let ft = FieldType::Number(NumberOptions {
            min: Some(0.0),
            max: Some(100.0),
            only_int: true,
        });
        let json = serde_json::to_value(&ft).unwrap();
        let deserialized: FieldType = serde_json::from_value(json).unwrap();
        assert_eq!(ft, deserialized);
    }

    #[test]
    fn field_type_serde_round_trip_file() {
        let ft = FieldType::File(FileOptions {
            max_select: 5,
            max_size: 10_485_760,
            mime_types: vec!["image/jpeg".into(), "image/png".into()],
            thumbs: vec!["100x100".into(), "200x200".into()],
            protected: true,
        });
        let json = serde_json::to_value(&ft).unwrap();
        let deserialized: FieldType = serde_json::from_value(json).unwrap();
        assert_eq!(ft, deserialized);
    }

    // ── Json field ─────────────────────────────────────────────────────────

    #[test]
    fn json_field_accepts_any_json() {
        let ft = FieldType::Json(JsonOptions::default());
        assert!(ft.validate_value("f", &json!({"key": "value"})).is_ok());
        assert!(ft.validate_value("f", &json!([1, 2, 3])).is_ok());
        assert!(ft.validate_value("f", &json!("string")).is_ok());
        assert!(ft.validate_value("f", &json!(42)).is_ok());
    }

    #[test]
    fn json_max_size_accepts_small() {
        let opts = JsonOptions {
            max_size: 100,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!({"a": 1})).is_ok());
    }

    #[test]
    fn json_max_size_rejects_large() {
        let opts = JsonOptions {
            max_size: 10,
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!({"key": "a very long value here"}))
            .is_err());
    }

    #[test]
    fn json_no_limit_accepts_anything() {
        let opts = JsonOptions {
            max_size: 0,
            ..Default::default()
        };
        let big = json!({"data": "x".repeat(10000)});
        assert!(opts.validate_value("f", &big).is_ok());
    }

    // ── Json schema validation ────────────────────────────────────────────

    #[test]
    fn json_schema_validates_matching_object() {
        let opts = JsonOptions {
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "age":  { "type": "integer", "minimum": 0 }
                },
                "required": ["name"]
            })),
            ..Default::default()
        };
        assert!(opts.validate().is_ok());
        assert!(opts
            .validate_value("f", &json!({"name": "Alice", "age": 30}))
            .is_ok());
    }

    #[test]
    fn json_schema_rejects_missing_required_property() {
        let opts = JsonOptions {
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                },
                "required": ["name"]
            })),
            ..Default::default()
        };
        let result = opts.validate_value("f", &json!({"age": 30}));
        assert!(result.is_err());
    }

    #[test]
    fn json_schema_rejects_wrong_type() {
        let opts = JsonOptions {
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "count": { "type": "integer" }
                }
            })),
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!({"count": "not a number"}))
            .is_err());
    }

    #[test]
    fn json_schema_accepts_array_schema() {
        let opts = JsonOptions {
            schema: Some(json!({
                "type": "array",
                "items": { "type": "string" }
            })),
            ..Default::default()
        };
        assert!(opts.validate().is_ok());
        assert!(opts.validate_value("f", &json!(["a", "b", "c"])).is_ok());
        assert!(opts.validate_value("f", &json!([1, 2, 3])).is_err());
    }

    #[test]
    fn json_schema_no_schema_accepts_anything() {
        let opts = JsonOptions {
            schema: None,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(42)).is_ok());
        assert!(opts.validate_value("f", &json!("hello")).is_ok());
        assert!(opts.validate_value("f", &json!(null)).is_ok());
        assert!(opts.validate_value("f", &json!([1, {"a": true}])).is_ok());
    }

    #[test]
    fn json_options_validate_rejects_non_object_schema() {
        let opts = JsonOptions {
            schema: Some(json!("not an object")),
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn json_options_validate_accepts_valid_schema() {
        let opts = JsonOptions {
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "x": { "type": "number" }
                }
            })),
            ..Default::default()
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn json_options_validate_accepts_no_schema() {
        let opts = JsonOptions::default();
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn json_schema_with_max_size_both_apply() {
        let opts = JsonOptions {
            max_size: 50,
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                },
                "required": ["name"]
            })),
        };
        // Valid against schema but too large
        let large_name = "x".repeat(100);
        let result = opts.validate_value("f", &json!({"name": large_name}));
        assert!(result.is_err());

        // Small enough but missing required field
        let result = opts.validate_value("f", &json!({"x": 1}));
        assert!(result.is_err());

        // Valid for both
        let result = opts.validate_value("f", &json!({"name": "ok"}));
        assert!(result.is_ok());
    }

    #[test]
    fn json_schema_enum_constraint() {
        let opts = JsonOptions {
            schema: Some(json!({
                "type": "string",
                "enum": ["draft", "published", "archived"]
            })),
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("draft")).is_ok());
        assert!(opts.validate_value("f", &json!("unknown")).is_err());
    }

    #[test]
    fn json_schema_nested_object_validation() {
        let opts = JsonOptions {
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "address": {
                        "type": "object",
                        "properties": {
                            "street": { "type": "string" },
                            "zip": { "type": "string", "pattern": "^[0-9]{5}$" }
                        },
                        "required": ["street", "zip"]
                    }
                },
                "required": ["address"]
            })),
            ..Default::default()
        };
        assert!(opts
            .validate_value(
                "f",
                &json!({
                    "address": { "street": "123 Main St", "zip": "12345" }
                })
            )
            .is_ok());

        assert!(opts
            .validate_value(
                "f",
                &json!({
                    "address": { "street": "123 Main St", "zip": "bad" }
                })
            )
            .is_err());
    }

    #[test]
    fn json_serde_round_trip_with_schema() {
        let ft = FieldType::Json(JsonOptions {
            max_size: 1024,
            schema: Some(json!({"type": "object"})),
        });
        let json_val = serde_json::to_value(&ft).unwrap();
        assert_eq!(json_val["type"], "json");
        assert_eq!(json_val["options"]["maxSize"], 1024);
        assert_eq!(json_val["options"]["schema"]["type"], "object");

        let deserialized: FieldType = serde_json::from_value(json_val).unwrap();
        assert_eq!(ft, deserialized);
    }

    #[test]
    fn json_serde_round_trip_without_schema() {
        let ft = FieldType::Json(JsonOptions::default());
        let json_val = serde_json::to_value(&ft).unwrap();
        assert_eq!(json_val["type"], "json");
        // schema should be absent (skip_serializing_if)
        assert!(json_val["options"].get("schema").is_none());

        let deserialized: FieldType = serde_json::from_value(json_val).unwrap();
        assert_eq!(ft, deserialized);
    }

    // ── File options validation ──────────────────────────────────────────

    #[test]
    fn file_options_accept_defaults() {
        let opts = FileOptions::default();
        assert!(opts.validate().is_ok());
        assert_eq!(opts.max_select, 1);
        assert_eq!(opts.max_size, 0);
        assert!(opts.mime_types.is_empty());
        assert!(opts.thumbs.is_empty());
        assert!(!opts.protected);
    }

    #[test]
    fn file_options_reject_zero_max_select() {
        let opts = FileOptions {
            max_select: 0,
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn file_options_accept_valid_max_select() {
        for n in [1, 2, 5, 10, 100] {
            let opts = FileOptions {
                max_select: n,
                ..Default::default()
            };
            assert!(opts.validate().is_ok(), "max_select={n} should be valid");
        }
    }

    #[test]
    fn file_options_accept_valid_mime_types() {
        let opts = FileOptions {
            mime_types: vec![
                "image/jpeg".into(),
                "image/png".into(),
                "application/pdf".into(),
                "text/plain".into(),
                "video/mp4".into(),
                "audio/mpeg".into(),
                "application/octet-stream".into(),
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document".into(),
            ],
            ..Default::default()
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn file_options_reject_invalid_mime_types() {
        let invalid = vec![
            "invalid",
            "just-a-word",
            "/leading-slash",
            "missing/",
            "/",
            "",
            "double//slash",
        ];
        for mt in invalid {
            let opts = FileOptions {
                mime_types: vec![mt.to_string()],
                ..Default::default()
            };
            assert!(
                opts.validate().is_err(),
                "MIME type '{mt}' should be rejected"
            );
        }
    }

    #[test]
    fn file_options_accept_valid_thumb_sizes() {
        let valid = vec![
            "100x100",
            "200x200f",
            "50x0",
            "0x150",
            "300x300t",
            "100x100b",
            "1920x1080",
            "1x1",
        ];
        let opts = FileOptions {
            thumbs: valid.into_iter().map(String::from).collect(),
            ..Default::default()
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn file_options_reject_invalid_thumb_sizes() {
        let invalid = vec![
            "0x0",       // both dimensions zero
            "abc",       // not a size
            "100",       // missing height
            "x100",      // missing width
            "100x",      // missing height after x
            "100x100z",  // invalid suffix
            "-1x100",    // negative
            "100x-1",    // negative
            "100 x 100", // spaces
        ];
        for thumb in invalid {
            let opts = FileOptions {
                thumbs: vec![thumb.to_string()],
                ..Default::default()
            };
            assert!(
                opts.validate().is_err(),
                "thumb '{thumb}' should be rejected"
            );
        }
    }

    #[test]
    fn file_options_reject_zero_by_zero_thumb() {
        let opts = FileOptions {
            thumbs: vec!["0x0".into()],
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn file_options_accept_all_thumb_suffixes() {
        for suffix in ["", "t", "b", "f"] {
            let thumb = format!("100x100{suffix}");
            let opts = FileOptions {
                thumbs: vec![thumb.clone()],
                ..Default::default()
            };
            assert!(opts.validate().is_ok(), "thumb '{thumb}' should be valid");
        }
    }

    #[test]
    fn file_options_accept_full_configuration() {
        let opts = FileOptions {
            max_select: 5,
            max_size: 10_485_760,
            mime_types: vec!["image/jpeg".into(), "image/png".into()],
            thumbs: vec!["100x100".into(), "200x200f".into()],
            protected: true,
        };
        assert!(opts.validate().is_ok());
    }

    // ── File value validation (single file) ──────────────────────────────

    #[test]
    fn file_single_accepts_filename_string() {
        let opts = FileOptions {
            max_select: 1,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("photo.jpg")).is_ok());
    }

    #[test]
    fn file_single_rejects_empty_filename() {
        let opts = FileOptions {
            max_select: 1,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("")).is_err());
    }

    #[test]
    fn file_single_rejects_non_string() {
        let opts = FileOptions {
            max_select: 1,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(42)).is_err());
        assert!(opts.validate_value("f", &json!(true)).is_err());
        assert!(opts.validate_value("f", &json!(["file.jpg"])).is_err());
        assert!(opts
            .validate_value("f", &json!({"name": "file.jpg"}))
            .is_err());
    }

    // ── File value validation (multiple files) ───────────────────────────

    #[test]
    fn file_multi_accepts_array_of_filenames() {
        let opts = FileOptions {
            max_select: 5,
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!(["a.jpg", "b.png", "c.pdf"]))
            .is_ok());
    }

    #[test]
    fn file_multi_accepts_empty_array() {
        let opts = FileOptions {
            max_select: 5,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!([])).is_ok());
    }

    #[test]
    fn file_multi_accepts_single_item_array() {
        let opts = FileOptions {
            max_select: 5,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!(["only.jpg"])).is_ok());
    }

    #[test]
    fn file_multi_rejects_exceeding_max_select() {
        let opts = FileOptions {
            max_select: 2,
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!(["a.jpg", "b.png", "c.pdf"]))
            .is_err());
    }

    #[test]
    fn file_multi_accepts_exactly_max_select() {
        let opts = FileOptions {
            max_select: 3,
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!(["a.jpg", "b.png", "c.pdf"]))
            .is_ok());
    }

    #[test]
    fn file_multi_rejects_non_array() {
        let opts = FileOptions {
            max_select: 5,
            ..Default::default()
        };
        assert!(opts.validate_value("f", &json!("single.jpg")).is_err());
        assert!(opts.validate_value("f", &json!(42)).is_err());
    }

    #[test]
    fn file_multi_rejects_empty_filename_in_array() {
        let opts = FileOptions {
            max_select: 5,
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!(["a.jpg", "", "c.pdf"]))
            .is_err());
    }

    #[test]
    fn file_multi_rejects_non_string_in_array() {
        let opts = FileOptions {
            max_select: 5,
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!(["a.jpg", 42, "c.pdf"]))
            .is_err());
    }

    #[test]
    fn file_multi_rejects_duplicate_filenames() {
        let opts = FileOptions {
            max_select: 5,
            ..Default::default()
        };
        assert!(opts
            .validate_value("f", &json!(["a.jpg", "b.png", "a.jpg"]))
            .is_err());
    }

    // ── File field-level validation (required, null) ─────────────────────

    #[test]
    fn file_field_required_rejects_null() {
        let f = Field::new("avatar", FieldType::File(FileOptions::default())).required(true);
        assert!(f.validate_value(&json!(null)).is_err());
    }

    #[test]
    fn file_field_optional_accepts_null() {
        let f = Field::new("avatar", FieldType::File(FileOptions::default()));
        assert!(f.validate_value(&json!(null)).is_ok());
    }

    #[test]
    fn file_field_validates_value_through_field_type() {
        let f = Field::new(
            "docs",
            FieldType::File(FileOptions {
                max_select: 2,
                ..Default::default()
            }),
        );
        assert!(f.validate_value(&json!(["a.pdf", "b.pdf"])).is_ok());
        assert!(f
            .validate_value(&json!(["a.pdf", "b.pdf", "c.pdf"]))
            .is_err());
    }

    // ── File serde ───────────────────────────────────────────────────────

    #[test]
    fn file_serde_round_trip_full() {
        let ft = FieldType::File(FileOptions {
            max_select: 5,
            max_size: 10_485_760,
            mime_types: vec!["image/jpeg".into(), "image/png".into()],
            thumbs: vec!["100x100".into(), "200x200f".into()],
            protected: true,
        });
        let json_val = serde_json::to_value(&ft).unwrap();
        assert_eq!(json_val["type"], "file");
        assert_eq!(json_val["options"]["maxSelect"], 5);
        assert_eq!(json_val["options"]["maxSize"], 10_485_760);
        assert_eq!(
            json_val["options"]["mimeTypes"],
            json!(["image/jpeg", "image/png"])
        );
        assert_eq!(
            json_val["options"]["thumbs"],
            json!(["100x100", "200x200f"])
        );
        assert_eq!(json_val["options"]["protected"], true);

        let deserialized: FieldType = serde_json::from_value(json_val).unwrap();
        assert_eq!(ft, deserialized);
    }

    #[test]
    fn file_serde_defaults_when_minimal() {
        let json_val = json!({ "type": "file", "options": {} });
        let ft: FieldType = serde_json::from_value(json_val).unwrap();
        if let FieldType::File(opts) = ft {
            assert_eq!(opts.max_select, 1);
            assert_eq!(opts.max_size, 0);
            assert!(opts.mime_types.is_empty());
            assert!(opts.thumbs.is_empty());
            assert!(!opts.protected);
        } else {
            panic!("expected File variant");
        }
    }

    #[test]
    fn file_serde_omits_empty_vecs() {
        let opts = FileOptions::default();
        let json_val = serde_json::to_value(&opts).unwrap();
        assert!(json_val.get("mimeTypes").is_none());
        assert!(json_val.get("thumbs").is_none());
    }

    // ── File SQL type and metadata ───────────────────────────────────────

    #[test]
    fn file_sql_type_is_text() {
        let ft = FieldType::File(FileOptions::default());
        assert_eq!(ft.sql_type(), "TEXT");
    }

    #[test]
    fn file_type_name() {
        let ft = FieldType::File(FileOptions::default());
        assert_eq!(ft.type_name(), "file");
    }

    #[test]
    fn file_is_not_text_like() {
        let ft = FieldType::File(FileOptions::default());
        assert!(!ft.is_text_like());
    }
}
