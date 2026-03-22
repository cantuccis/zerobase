//! API access rules for collections.
//!
//! Rules follow the PocketBase model:
//! - `None` (null) = locked (superusers only)
//! - `Some("")` (empty string) = open to everyone
//! - `Some("expression")` = conditional access

/// Per-operation API rules for a collection.
///
/// Each rule is an `Option<String>`:
/// - `None` — locked (only superusers can perform this operation).
/// - `Some("")` — open to everyone (no restrictions).
/// - `Some(expr)` — conditional access (the expression is evaluated per request).
///
/// The optional `manage_rule` grants full CRUD access to users matching
/// the expression, bypassing all individual operation rules. This enables
/// delegated administration without granting superuser privileges.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiRules {
    /// Filter applied when listing records.
    pub list_rule: Option<String>,
    /// Filter applied when viewing a single record.
    pub view_rule: Option<String>,
    /// Condition for creating new records.
    pub create_rule: Option<String>,
    /// Condition for updating existing records.
    pub update_rule: Option<String>,
    /// Condition for deleting records.
    pub delete_rule: Option<String>,
    /// Rule that grants full CRUD access when matched, bypassing individual
    /// operation rules. Enables delegated administration.
    ///
    /// - `None` — no manage access (default).
    /// - `Some("")` — any authenticated user can manage all records.
    /// - `Some(expr)` — users matching the expression get full CRUD access.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manage_rule: Option<String>,
}

impl ApiRules {
    /// All rules locked (superusers only). This is the secure default.
    pub fn locked() -> Self {
        Self::default()
    }

    /// All rules open to everyone. **Use with extreme caution.**
    pub fn open() -> Self {
        Self {
            list_rule: Some(String::new()),
            view_rule: Some(String::new()),
            create_rule: Some(String::new()),
            update_rule: Some(String::new()),
            delete_rule: Some(String::new()),
            manage_rule: None,
        }
    }

    /// Extract field names referenced in all rule expressions.
    ///
    /// This performs a lightweight scan of rule strings to find identifiers
    /// that look like field names (alphanumeric + underscore, not starting with
    /// `@` or a digit). It returns the base field name (before any `.` for
    /// dot-notation relations).
    ///
    /// These fields are candidates for automatic index creation.
    pub fn referenced_fields(&self) -> std::collections::HashSet<String> {
        let mut fields = std::collections::HashSet::new();
        let rules = [
            &self.list_rule,
            &self.view_rule,
            &self.create_rule,
            &self.update_rule,
            &self.delete_rule,
            &self.manage_rule,
        ];
        for rule in rules {
            if let Some(expr) = rule.as_deref() {
                extract_field_names(expr, &mut fields);
            }
        }
        fields
    }

    /// Public read, authenticated write.
    pub fn public_read() -> Self {
        let auth_check = "@request.auth.id != \"\"".to_string();
        Self {
            list_rule: Some(String::new()),
            view_rule: Some(String::new()),
            create_rule: Some(auth_check.clone()),
            update_rule: Some(auth_check.clone()),
            delete_rule: Some(auth_check),
            manage_rule: None,
        }
    }

    /// Check whether a manage rule is set and can potentially grant access.
    ///
    /// Returns `true` if the manage_rule is `Some` (either empty or expression).
    pub fn has_manage_rule(&self) -> bool {
        self.manage_rule.is_some()
    }
}

/// Extract field-like identifiers from a filter expression.
///
/// Scans for tokens that look like field names: sequences of `[a-zA-Z_][a-zA-Z0-9_]*`
/// that are not preceded by `@` (which indicates PocketBase macros like `@request`).
/// For dot-notation (e.g., `author.name`), only the base field name (`author`) is extracted.
///
/// Known non-field keywords (`true`, `false`, `null`, `AND`, `OR`) are excluded.
fn extract_field_names(expr: &str, out: &mut std::collections::HashSet<String>) {
    const KEYWORDS: &[&str] = &["true", "false", "null", "AND", "OR", "and", "or"];

    let chars: Vec<char> = expr.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];

        // Skip strings
        if ch == '"' || ch == '\'' {
            i += 1;
            while i < len && chars[i] != ch {
                if chars[i] == '\\' {
                    i += 1; // skip escaped char
                }
                i += 1;
            }
            i += 1; // skip closing quote
            continue;
        }

        // Skip @-prefixed macros (@request, @now, etc.)
        if ch == '@' {
            i += 1;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.') {
                i += 1;
            }
            continue;
        }

        // Identifier
        if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident = &expr[start..i];

            // Skip dot-suffixes (relation traversal) — we only want the base field
            if i < len && chars[i] == '.' {
                while i < len && (chars[i] == '.' || chars[i].is_alphanumeric() || chars[i] == '_')
                {
                    i += 1;
                }
            }

            if !KEYWORDS.contains(&ident) {
                out.insert(ident.to_string());
            }
            continue;
        }

        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rules_are_locked() {
        let rules = ApiRules::default();
        assert!(rules.list_rule.is_none());
        assert!(rules.view_rule.is_none());
        assert!(rules.create_rule.is_none());
        assert!(rules.update_rule.is_none());
        assert!(rules.delete_rule.is_none());
    }

    #[test]
    fn open_rules_are_empty_strings() {
        let rules = ApiRules::open();
        assert_eq!(rules.list_rule, Some(String::new()));
        assert_eq!(rules.view_rule, Some(String::new()));
        assert_eq!(rules.create_rule, Some(String::new()));
        assert_eq!(rules.update_rule, Some(String::new()));
        assert_eq!(rules.delete_rule, Some(String::new()));
    }

    #[test]
    fn locked_equals_default() {
        assert_eq!(ApiRules::locked(), ApiRules::default());
    }

    #[test]
    fn public_read_has_open_reads_and_auth_writes() {
        let rules = ApiRules::public_read();
        assert_eq!(rules.list_rule, Some(String::new()));
        assert_eq!(rules.view_rule, Some(String::new()));
        assert!(rules
            .create_rule
            .as_ref()
            .unwrap()
            .contains("@request.auth.id"));
        assert!(rules
            .update_rule
            .as_ref()
            .unwrap()
            .contains("@request.auth.id"));
        assert!(rules
            .delete_rule
            .as_ref()
            .unwrap()
            .contains("@request.auth.id"));
    }

    #[test]
    fn rules_serialize_to_json() {
        let rules = ApiRules::locked();
        let json = serde_json::to_value(&rules).unwrap();
        assert!(json["listRule"].is_null());
        assert!(json["viewRule"].is_null());
    }

    #[test]
    fn rules_deserialize_from_json() {
        let json = r#"{"listRule":"","viewRule":null,"createRule":"@request.auth.id != \"\"","updateRule":null,"deleteRule":null}"#;
        let rules: ApiRules = serde_json::from_str(json).unwrap();
        assert_eq!(rules.list_rule, Some(String::new()));
        assert!(rules.view_rule.is_none());
        assert!(rules.create_rule.unwrap().contains("@request.auth.id"));
    }

    // ── Field extraction ──────────────────────────────────────────────────

    #[test]
    fn extract_fields_from_simple_filter() {
        let mut fields = std::collections::HashSet::new();
        extract_field_names("status = \"published\"", &mut fields);
        assert!(fields.contains("status"));
        assert!(!fields.contains("published"));
    }

    #[test]
    fn extract_fields_skips_at_macros() {
        let mut fields = std::collections::HashSet::new();
        extract_field_names("@request.auth.id != \"\"", &mut fields);
        assert!(fields.is_empty());
    }

    #[test]
    fn extract_fields_from_compound_filter() {
        let mut fields = std::collections::HashSet::new();
        extract_field_names("category = \"tech\" && views > 100", &mut fields);
        assert!(fields.contains("category"));
        assert!(fields.contains("views"));
        assert!(!fields.contains("tech"));
    }

    #[test]
    fn extract_fields_handles_dot_notation() {
        let mut fields = std::collections::HashSet::new();
        extract_field_names("author.name = \"Alice\"", &mut fields);
        assert!(fields.contains("author"));
        assert!(!fields.contains("name"));
    }

    #[test]
    fn extract_fields_skips_keywords() {
        let mut fields = std::collections::HashSet::new();
        extract_field_names("active = true && status != null", &mut fields);
        assert!(fields.contains("active"));
        assert!(fields.contains("status"));
        assert!(!fields.contains("true"));
        assert!(!fields.contains("null"));
    }

    #[test]
    fn referenced_fields_from_all_rules() {
        let rules = ApiRules {
            list_rule: Some("status = \"published\"".into()),
            view_rule: Some("owner = @request.auth.id".into()),
            create_rule: None,
            update_rule: Some("category = \"tech\"".into()),
            delete_rule: None,
            manage_rule: None,
        };
        let fields = rules.referenced_fields();
        assert!(fields.contains("status"));
        assert!(fields.contains("owner"));
        assert!(fields.contains("category"));
    }
}
