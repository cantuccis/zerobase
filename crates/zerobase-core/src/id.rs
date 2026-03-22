//! Record ID generation matching PocketBase's 15-character alphanumeric format.
//!
//! PocketBase generates 15-character IDs from the alphanumeric alphabet
//! (a-z, A-Z, 0-9 — 62 symbols). This module provides the same format
//! using the `nanoid` crate with a custom alphabet.
//!
//! # Collision probability
//!
//! With 62 symbols and 15 characters the ID space is 62^15 ≈ 7.7 × 10^26.
//! At 1 000 IDs per second it would take ~24 billion years to reach a 1%
//! collision probability (birthday bound).

use std::fmt;

use serde::{Deserialize, Serialize};

/// The 62-symbol alphanumeric alphabet used by PocketBase.
pub(crate) const ALPHABET: [char; 62] = [
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L',
    'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '0', '1', '2', '3', '4',
    '5', '6', '7', '8', '9',
];

/// Length of every generated record ID.
const ID_LENGTH: usize = 15;

/// A 15-character alphanumeric record identifier.
///
/// This is a validated newtype: it can only be constructed via [`RecordId::new`]
/// (generates a fresh ID) or [`RecordId::try_from_str`] (parses an existing one).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RecordId(String);

impl RecordId {
    /// Generate a new random 15-character alphanumeric ID.
    pub fn new() -> Self {
        Self(nanoid::nanoid!(ID_LENGTH, &ALPHABET))
    }

    /// Parse and validate an existing ID string.
    ///
    /// Returns `Err` if the string is not exactly 15 alphanumeric characters.
    pub fn try_from_str(s: &str) -> Result<Self, InvalidRecordId> {
        validate_id(s)?;
        Ok(Self(s.to_owned()))
    }

    /// Return the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for RecordId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for RecordId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<RecordId> for String {
    fn from(id: RecordId) -> Self {
        id.0
    }
}

impl TryFrom<String> for RecordId {
    type Error = InvalidRecordId;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        validate_id(&s)?;
        Ok(Self(s))
    }
}

/// Error returned when a string does not conform to the record ID format.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidRecordId {
    #[error("record ID must be {ID_LENGTH} characters, got {0}")]
    WrongLength(usize),
    #[error("record ID must be alphanumeric, found '{0}' at position {1}")]
    InvalidCharacter(char, usize),
}

/// Validate that `s` is exactly [`ID_LENGTH`] ASCII-alphanumeric characters.
fn validate_id(s: &str) -> Result<(), InvalidRecordId> {
    if s.len() != ID_LENGTH {
        return Err(InvalidRecordId::WrongLength(s.len()));
    }
    for (i, c) in s.chars().enumerate() {
        if !c.is_ascii_alphanumeric() {
            return Err(InvalidRecordId::InvalidCharacter(c, i));
        }
    }
    Ok(())
}

/// Convenience function: generate a fresh ID string without wrapping in [`RecordId`].
///
/// Useful in contexts where you just need the raw string (e.g. SQL inserts).
pub fn generate_id() -> String {
    nanoid::nanoid!(ID_LENGTH, &ALPHABET)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn generated_id_is_15_characters() {
        let id = RecordId::new();
        assert_eq!(id.as_str().len(), 15);
    }

    #[test]
    fn generated_id_is_alphanumeric() {
        for _ in 0..1000 {
            let id = RecordId::new();
            for c in id.as_str().chars() {
                assert!(
                    c.is_ascii_alphanumeric(),
                    "Non-alphanumeric character '{c}' in ID: {id}"
                );
            }
        }
    }

    #[test]
    fn generate_id_function_matches_format() {
        for _ in 0..1000 {
            let raw = generate_id();
            assert_eq!(raw.len(), 15);
            assert!(raw.chars().all(|c| c.is_ascii_alphanumeric()));
        }
    }

    #[test]
    fn ids_are_unique_across_many_generations() {
        let count = 10_000;
        let ids: HashSet<String> = (0..count).map(|_| generate_id()).collect();
        assert_eq!(
            ids.len(),
            count,
            "Expected {count} unique IDs, got {}",
            ids.len()
        );
    }

    #[test]
    fn record_id_try_from_valid_string() {
        let raw = generate_id();
        let id = RecordId::try_from_str(&raw).unwrap();
        assert_eq!(id.as_str(), raw);
    }

    #[test]
    fn record_id_rejects_wrong_length() {
        let err = RecordId::try_from_str("short").unwrap_err();
        assert!(matches!(err, InvalidRecordId::WrongLength(5)));

        let err = RecordId::try_from_str("thisistoolongofastringforid").unwrap_err();
        assert!(matches!(err, InvalidRecordId::WrongLength(_)));
    }

    #[test]
    fn record_id_rejects_invalid_characters() {
        // 15 chars with underscore at position 6
        let err = RecordId::try_from_str("abcdef_hijklmno").unwrap_err();
        assert!(matches!(err, InvalidRecordId::InvalidCharacter('_', 6)));

        // 15 chars with dash at position 6
        let err = RecordId::try_from_str("abcdef-hijklmno").unwrap_err();
        assert!(matches!(err, InvalidRecordId::InvalidCharacter('-', 6)));
    }

    #[test]
    fn record_id_display_and_as_ref() {
        let id = RecordId::new();
        let display = format!("{id}");
        assert_eq!(display, id.as_str());
        assert_eq!(id.as_ref(), id.as_str());
    }

    #[test]
    fn record_id_into_string() {
        let id = RecordId::new();
        let s = id.as_str().to_owned();
        let converted: String = id.into();
        assert_eq!(converted, s);
    }

    #[test]
    fn record_id_serde_roundtrip() {
        let id = RecordId::new();
        let json = serde_json::to_string(&id).unwrap();
        // Should serialize as a plain string
        assert!(json.starts_with('"'));
        let deserialized: RecordId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, id);
    }

    #[test]
    fn record_id_serde_rejects_invalid() {
        let result: Result<RecordId, _> = serde_json::from_str("\"too_short\"");
        assert!(result.is_err());
    }

    #[test]
    fn record_id_try_from_string() {
        let raw = generate_id();
        let id = RecordId::try_from(raw.clone()).unwrap();
        assert_eq!(id.as_str(), raw);
    }

    #[test]
    fn record_id_default_generates_valid() {
        let id = RecordId::default();
        assert_eq!(id.as_str().len(), 15);
        assert!(id.as_str().chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn record_id_equality() {
        let raw = generate_id();
        let a = RecordId::try_from_str(&raw).unwrap();
        let b = RecordId::try_from_str(&raw).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn record_id_hash_works_in_collections() {
        let mut set = HashSet::new();
        let id = RecordId::new();
        set.insert(id.clone());
        assert!(set.contains(&id));
    }

    #[test]
    fn alphabet_contains_only_alphanumeric() {
        for c in &ALPHABET {
            assert!(
                c.is_ascii_alphanumeric(),
                "Alphabet contains non-alphanumeric: '{c}'"
            );
        }
        assert_eq!(ALPHABET.len(), 62);
    }

    #[test]
    fn id_generation_is_fast() {
        use std::time::Instant;
        let start = Instant::now();
        let count = 100_000;
        for _ in 0..count {
            let _ = generate_id();
        }
        let elapsed = start.elapsed();
        // Should comfortably generate 100k IDs in under 1 second
        assert!(
            elapsed.as_secs() < 1,
            "Generated {count} IDs in {elapsed:?}, expected < 1s"
        );
    }
}
