//! Schema types for Zerobase collections and fields.
//!
//! This module defines the domain-level type system that models PocketBase-style
//! collections and their fields. It is entirely I/O-free and framework-agnostic.
//!
//! # Overview
//!
//! - [`CollectionType`] — Base, Auth, or View collection.
//! - [`FieldType`] — The full set of supported field types and their options.
//! - [`Collection`] — A complete collection definition with fields, rules, and indexes.
//! - [`Field`] — A single field within a collection, with type-specific options.
//!
//! All types implement `Serialize`/`Deserialize` for JSON persistence and API transport,
//! and provide `validate()` methods that return structured [`ZerobaseError::Validation`] errors.

mod collection;
mod field;
mod record_validator;
pub mod rule_engine;
pub mod rule_parser;
mod rules;
mod validation;

pub use collection::{
    is_system_collection, AuthOptions, Collection, CollectionType, IndexColumn, IndexSortDirection,
    IndexSpec, AUTH_SYSTEM_FIELDS, BASE_SYSTEM_FIELDS, SYSTEM_COLLECTION_NAMES,
};
pub use field::{
    compare_datetimes, parse_datetime, AutoDateOptions, BoolOptions, DateTimeMode, DateTimeOptions,
    EditorOptions, EmailOptions, Field, FieldType, FileOptions, JsonOptions, MultiSelectOptions,
    NumberOptions, OnDeleteAction, PasswordOptions, RelationOptions, SelectOptions, TextOptions,
    UrlOptions,
};
pub use record_validator::{OperationContext, RecordValidator};
pub use rule_engine::{
    check_rule, evaluate_rule, evaluate_rule_str, rule_str_to_sql, rule_to_sql, RequestContext,
    RuleDecision, RuleSqlClause, RuleSqlParam,
};
pub use rule_parser::{parse_rule, validate_rule, ComparisonOp, Operand, RuleExpr, RuleParseError};
pub use rules::ApiRules;
pub use validation::validate_name;
