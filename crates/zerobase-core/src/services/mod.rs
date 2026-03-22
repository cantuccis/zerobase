//! Application services — domain logic orchestration.
//!
//! Services coordinate domain validation, persistence, and business rules.
//! They depend on repository traits (not concrete implementations) for testability.

pub mod backup_service;
pub mod collection_service;
pub mod expand;
pub mod external_auth;
pub mod log_service;
pub mod record_service;
pub mod settings_service;
pub mod superuser_service;
pub mod webauthn_credential;

pub use backup_service::BackupService;
pub use collection_service::CollectionService;
pub use expand::{expand_record, expand_records, parse_expand, ExpandPath, MAX_EXPAND_DEPTH};
pub use log_service::LogService;
pub use record_service::{
    parse_fields, parse_sort, project_fields, project_record_list, validate_and_filter_fields,
    validate_sort_fields, RecordList, RecordQuery, RecordService, SortDirection, DEFAULT_PER_PAGE,
    MAX_PER_PAGE,
};
pub use settings_service::SettingsService;
pub use superuser_service::SuperuserService;
