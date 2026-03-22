//! Shared API response types matching PocketBase's JSON format.
//!
//! These types ensure all API responses are consistent with PocketBase's
//! response structure, making the API a drop-in replacement.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

/// Paginated list response matching PocketBase's format.
///
/// Used for both collection listings and record listings.
///
/// # JSON format
///
/// ```json
/// {
///   "page": 1,
///   "perPage": 30,
///   "totalPages": 2,
///   "totalItems": 42,
///   "items": [...]
/// }
/// ```
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResponse<T: Serialize> {
    pub page: u32,
    pub per_page: u32,
    pub total_pages: u32,
    pub total_items: u64,
    pub items: Vec<T>,
}

/// A single record response enriched with collection metadata.
///
/// PocketBase records include `collectionId` and `collectionName` alongside
/// the record's own fields (`id`, `created`, `updated`, plus user data).
///
/// # JSON format
///
/// ```json
/// {
///   "id": "abc123",
///   "collectionId": "xyz789",
///   "collectionName": "posts",
///   "created": "2025-01-01 00:00:00.000Z",
///   "updated": "2025-01-01 00:00:00.000Z",
///   "title": "Hello World"
/// }
/// ```
#[derive(Debug, Clone)]
pub struct RecordResponse {
    /// The collection's unique identifier.
    pub collection_id: String,
    /// The collection's name.
    pub collection_name: String,
    /// The record data (already includes id, created, updated, plus user fields).
    pub data: HashMap<String, Value>,
}

impl Serialize for RecordResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        // Build a flat map: collectionId + collectionName + all record fields
        let mut map = serializer.serialize_map(Some(self.data.len() + 2))?;
        map.serialize_entry("collectionId", &self.collection_id)?;
        map.serialize_entry("collectionName", &self.collection_name)?;
        for (key, value) in &self.data {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl RecordResponse {
    /// Wrap a raw record data map with collection metadata.
    pub fn new(
        collection_id: impl Into<String>,
        collection_name: impl Into<String>,
        data: HashMap<String, Value>,
    ) -> Self {
        Self {
            collection_id: collection_id.into(),
            collection_name: collection_name.into(),
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn list_response_serializes_to_pocketbase_format() {
        let response: ListResponse<serde_json::Value> = ListResponse {
            page: 1,
            per_page: 30,
            total_pages: 2,
            total_items: 42,
            items: vec![json!({"id": "abc123"})],
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["page"], 1);
        assert_eq!(json["perPage"], 30);
        assert_eq!(json["totalPages"], 2);
        assert_eq!(json["totalItems"], 42);
        assert!(json["items"].is_array());
        assert_eq!(json["items"][0]["id"], "abc123");
    }

    #[test]
    fn record_response_flattens_with_collection_metadata() {
        let mut data = HashMap::new();
        data.insert("id".to_string(), json!("rec123"));
        data.insert("title".to_string(), json!("Hello"));
        data.insert("created".to_string(), json!("2025-01-01 00:00:00.000Z"));
        data.insert("updated".to_string(), json!("2025-01-01 00:00:00.000Z"));

        let response = RecordResponse::new("col_abc", "posts", data);
        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["collectionId"], "col_abc");
        assert_eq!(json["collectionName"], "posts");
        assert_eq!(json["id"], "rec123");
        assert_eq!(json["title"], "Hello");
        assert_eq!(json["created"], "2025-01-01 00:00:00.000Z");
        assert_eq!(json["updated"], "2025-01-01 00:00:00.000Z");
    }

    #[test]
    fn empty_list_response_serializes_correctly() {
        let response: ListResponse<serde_json::Value> = ListResponse {
            page: 1,
            per_page: 30,
            total_pages: 1,
            total_items: 0,
            items: vec![],
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["totalItems"], 0);
        assert!(json["items"].as_array().unwrap().is_empty());
    }

    #[test]
    fn list_response_with_record_responses() {
        let mut data = HashMap::new();
        data.insert("id".to_string(), json!("rec1"));
        data.insert("title".to_string(), json!("Test"));

        let record = RecordResponse::new("col1", "posts", data);
        let response = ListResponse {
            page: 1,
            per_page: 30,
            total_pages: 1,
            total_items: 1,
            items: vec![record],
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["items"][0]["collectionId"], "col1");
        assert_eq!(json["items"][0]["collectionName"], "posts");
        assert_eq!(json["items"][0]["id"], "rec1");
    }
}
