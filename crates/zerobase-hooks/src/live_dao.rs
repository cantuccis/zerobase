//! Live DAO handler that bridges JS `$app.dao()` calls to a real
//! [`RecordRepository`] implementation.
//!
//! This is the production handler used when JS hooks need actual database
//! access. It translates [`DaoRequest`] values into repository method calls
//! and returns [`DaoResponse`] values back to the JS runtime.

use std::sync::Arc;

use serde_json::Value;
use tracing::{debug, warn};

use zerobase_core::services::record_service::{
    RecordQuery, RecordRepository, SortDirection,
};

use crate::bindings::{DaoHandler, DaoRequest, DaoResponse};

/// A [`DaoHandler`] backed by a real [`RecordRepository`].
///
/// Translates JS DAO operations into repository method calls against the
/// application database. This handler is `Send + Sync` and can be shared
/// across hook executions.
pub struct LiveDaoHandler<R: RecordRepository> {
    repo: Arc<R>,
}

impl<R: RecordRepository> LiveDaoHandler<R> {
    /// Create a new handler wrapping the given repository.
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }
}

impl<R: RecordRepository + 'static> DaoHandler for LiveDaoHandler<R> {
    fn handle(&self, request: &DaoRequest) -> DaoResponse {
        match request {
            DaoRequest::FindById { collection, id } => {
                debug!(collection = %collection, id = %id, "JS DAO: findRecordById");
                match self.repo.find_one(collection, id) {
                    Ok(record) => DaoResponse::Record(Some(record)),
                    Err(e) => {
                        // Not-found is normal — return null, don't error.
                        debug!(error = %e, "JS DAO: findRecordById returned error");
                        DaoResponse::Record(None)
                    }
                }
            }

            DaoRequest::FindByFilter { collection, filter } => {
                debug!(collection = %collection, filter = %filter, "JS DAO: findFirstRecordByFilter");
                let query = RecordQuery {
                    filter: if filter.is_empty() {
                        None
                    } else {
                        Some(filter.clone())
                    },
                    sort: Vec::new(),
                    page: 1,
                    per_page: 1,
                    fields: None,
                    search: None,
                };

                match self.repo.find_many(collection, &query) {
                    Ok(result) => {
                        let first = result.items.into_iter().next();
                        DaoResponse::Record(first)
                    }
                    Err(e) => {
                        warn!(error = %e, "JS DAO: findFirstRecordByFilter error");
                        DaoResponse::Error(e.to_string())
                    }
                }
            }

            DaoRequest::FindMany {
                collection,
                filter,
                sort,
                limit,
                offset,
            } => {
                debug!(
                    collection = %collection,
                    filter = %filter,
                    sort = %sort,
                    limit = limit,
                    offset = offset,
                    "JS DAO: findRecordsByFilter"
                );

                let sort_fields = parse_sort_string(sort);
                let per_page = (*limit).max(1) as u32;
                let page = if *offset == 0 {
                    1
                } else {
                    ((*offset as u32) / per_page) + 1
                };

                let query = RecordQuery {
                    filter: if filter.is_empty() {
                        None
                    } else {
                        Some(filter.clone())
                    },
                    sort: sort_fields,
                    page,
                    per_page,
                    fields: None,
                    search: None,
                };

                match self.repo.find_many(collection, &query) {
                    Ok(result) => DaoResponse::Records(result.items),
                    Err(e) => {
                        warn!(error = %e, "JS DAO: findRecordsByFilter error");
                        DaoResponse::Error(e.to_string())
                    }
                }
            }

            DaoRequest::Save { collection, data } => {
                debug!(collection = %collection, "JS DAO: saveRecord");

                // If data contains an "id" field, treat as update; otherwise insert.
                let has_id = data
                    .get("id")
                    .map(|v| !v.is_null() && v.as_str().map_or(false, |s| !s.is_empty()))
                    .unwrap_or(false);

                if has_id {
                    let id = data["id"].as_str().unwrap_or_default();
                    match self.repo.update(collection, id, data) {
                        Ok(true) => {
                            // Return the data as-is (the repo doesn't return the full record).
                            DaoResponse::Saved(data.clone())
                        }
                        Ok(false) => DaoResponse::Error(format!(
                            "record '{id}' not found in '{collection}'"
                        )),
                        Err(e) => {
                            warn!(error = %e, "JS DAO: saveRecord (update) error");
                            DaoResponse::Error(e.to_string())
                        }
                    }
                } else {
                    // Generate an ID if not present.
                    let mut data_with_id = data.clone();
                    if !data_with_id.contains_key("id") {
                        data_with_id.insert(
                            "id".to_string(),
                            Value::String(zerobase_core::generate_id()),
                        );
                    }

                    match self.repo.insert(collection, &data_with_id) {
                        Ok(()) => DaoResponse::Saved(data_with_id),
                        Err(e) => {
                            warn!(error = %e, "JS DAO: saveRecord (insert) error");
                            DaoResponse::Error(e.to_string())
                        }
                    }
                }
            }

            DaoRequest::Delete { collection, id } => {
                debug!(collection = %collection, id = %id, "JS DAO: deleteRecord");
                match self.repo.delete(collection, id) {
                    Ok(deleted) => DaoResponse::Deleted(deleted),
                    Err(e) => {
                        warn!(error = %e, "JS DAO: deleteRecord error");
                        DaoResponse::Error(e.to_string())
                    }
                }
            }
        }
    }
}

/// Parse a PocketBase-style sort string (e.g. "-created,+title,name") into
/// `(column, direction)` pairs.
fn parse_sort_string(sort: &str) -> Vec<(String, SortDirection)> {
    if sort.is_empty() {
        return Vec::new();
    }

    sort.split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }

            if let Some(field) = part.strip_prefix('-') {
                Some((field.to_string(), SortDirection::Desc))
            } else if let Some(field) = part.strip_prefix('+') {
                Some((field.to_string(), SortDirection::Asc))
            } else {
                Some((part.to_string(), SortDirection::Asc))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sort_empty() {
        assert!(parse_sort_string("").is_empty());
    }

    #[test]
    fn parse_sort_single_asc() {
        let result = parse_sort_string("name");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "name");
        assert!(matches!(result[0].1, SortDirection::Asc));
    }

    #[test]
    fn parse_sort_single_desc() {
        let result = parse_sort_string("-created");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "created");
        assert!(matches!(result[0].1, SortDirection::Desc));
    }

    #[test]
    fn parse_sort_explicit_asc() {
        let result = parse_sort_string("+name");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "name");
        assert!(matches!(result[0].1, SortDirection::Asc));
    }

    #[test]
    fn parse_sort_multiple() {
        let result = parse_sort_string("-created,+title,name");
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].0, "created");
        assert!(matches!(result[0].1, SortDirection::Desc));
        assert_eq!(result[1].0, "title");
        assert!(matches!(result[1].1, SortDirection::Asc));
        assert_eq!(result[2].0, "name");
        assert!(matches!(result[2].1, SortDirection::Asc));
    }

    #[test]
    fn parse_sort_with_whitespace() {
        let result = parse_sort_string(" -created , +title ");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "created");
        assert_eq!(result[1].0, "title");
    }
}
