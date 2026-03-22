//! Shared validation helpers for schema types.

use std::collections::HashMap;

use crate::error::{Result, ZerobaseError};

/// Valid identifier pattern: starts with a letter, then letters, digits, or underscores.
/// Must be 1–100 characters.
pub fn validate_name(name: &str, label: &str) -> Result<()> {
    let mut errors = HashMap::new();

    if name.is_empty() {
        errors.insert(label.to_string(), "must not be empty".to_string());
        return Err(ZerobaseError::validation_with_fields(
            format!("invalid {label}"),
            errors,
        ));
    }

    if name.len() > 100 {
        errors.insert(
            label.to_string(),
            "must be at most 100 characters".to_string(),
        );
        return Err(ZerobaseError::validation_with_fields(
            format!("invalid {label}"),
            errors,
        ));
    }

    let first = name.chars().next().unwrap();
    if !first.is_ascii_lowercase() && !first.is_ascii_uppercase() && first != '_' {
        errors.insert(
            label.to_string(),
            "must start with a letter or underscore".to_string(),
        );
        return Err(ZerobaseError::validation_with_fields(
            format!("invalid {label}"),
            errors,
        ));
    }

    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        errors.insert(
            label.to_string(),
            "must contain only letters, digits, and underscores".to_string(),
        );
        return Err(ZerobaseError::validation_with_fields(
            format!("invalid {label}"),
            errors,
        ));
    }

    // Disallow names that start with underscore (reserved for system tables).
    if name.starts_with('_') {
        errors.insert(
            label.to_string(),
            "names starting with underscore are reserved for system use".to_string(),
        );
        return Err(ZerobaseError::validation_with_fields(
            format!("invalid {label}"),
            errors,
        ));
    }

    Ok(())
}

/// Validate that an optional regex pattern compiles.
pub fn validate_regex_pattern(pattern: &str) -> Result<()> {
    regex::Regex::new(pattern).map_err(|e| {
        let mut errors = HashMap::new();
        errors.insert("pattern".to_string(), format!("invalid regex: {e}"));
        ZerobaseError::validation_with_fields("invalid field options", errors)
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names() {
        assert!(validate_name("posts", "name").is_ok());
        assert!(validate_name("UserProfiles", "name").is_ok());
        assert!(validate_name("order_items", "name").is_ok());
        assert!(validate_name("x1", "name").is_ok());
    }

    #[test]
    fn empty_name_rejected() {
        assert!(validate_name("", "name").is_err());
    }

    #[test]
    fn too_long_name_rejected() {
        let long = "a".repeat(101);
        assert!(validate_name(&long, "name").is_err());
    }

    #[test]
    fn name_starting_with_digit_rejected() {
        assert!(validate_name("1posts", "name").is_err());
    }

    #[test]
    fn name_with_spaces_rejected() {
        assert!(validate_name("my posts", "name").is_err());
    }

    #[test]
    fn name_with_hyphens_rejected() {
        assert!(validate_name("my-posts", "name").is_err());
    }

    #[test]
    fn name_starting_with_underscore_reserved() {
        assert!(validate_name("_system", "name").is_err());
    }

    #[test]
    fn max_length_name_accepted() {
        let name = "a".repeat(100);
        assert!(validate_name(&name, "name").is_ok());
    }

    #[test]
    fn valid_regex_accepted() {
        assert!(validate_regex_pattern(r"^\d{3}-\d{4}$").is_ok());
    }

    #[test]
    fn invalid_regex_rejected() {
        assert!(validate_regex_pattern(r"[invalid").is_err());
    }
}
