# Design Collection and Field Type System

**Date:** 2026-03-21 03:00
**Task ID:** ieocbkn87185dkt

## Summary

Designed and implemented the complete Rust type system for collections and fields in `zerobase-core`. This is the domain-level type system that models PocketBase-style collections and their fields, entirely I/O-free and framework-agnostic.

## What was implemented

### CollectionType enum
- `Base` — general-purpose data collection
- `Auth` — extends Base with built-in authentication fields
- `View` — read-only collection backed by a SQL SELECT query
- Includes `FromStr`, `Display`, `Serialize/Deserialize`

### FieldType enum (tagged union with options)
All 13 field types with type-specific options:
- **Text** — min/max length, regex pattern
- **Number** — min/max value, integer-only flag
- **Bool** — no options
- **Email** — domain allowlist/denylist
- **Url** — scheme restrictions
- **DateTime** — min/max date
- **AutoDate** — on_create/on_update flags
- **Select** — predefined values, max_select (single/multi)
- **File** — max_select, max_size, MIME types, thumbnails, protected flag
- **Relation** — target collection, max_select, cascade delete
- **Json** — max size
- **Editor** — max length
- **Password** — min/max length, regex pattern

### Collection struct
- Full collection definition with fields, API rules, indexes
- Constructors: `Collection::base()`, `Collection::auth()`, `Collection::view()`
- `AuthOptions` for auth-specific configuration
- `ApiRules` for per-operation access control (locked/open/conditional)
- `IndexSpec` for collection indexes

### Validation
- Collection name validation (reserved names, format)
- Field name validation (duplicates, reserved names)
- Type-specific option validation (e.g., min <= max, non-empty values)
- Record value validation against field type and constraints
- Regex pattern compilation validation

### Key Design Decisions
- Uses `serde(tag = "type", content = "options")` for clean JSON representation
- All types implement `Serialize/Deserialize` for JSON persistence
- `ApiRules` follows PocketBase model: `None` = locked, `Some("")` = open, `Some(expr)` = conditional
- System fields (id, created, updated) are implicit, not user-defined
- Auth collections automatically include email, password, verified, tokenKey

## Test Results

**160 tests total, all passing:**
- ~90 field type tests (value validation, options validation, serde round-trips)
- ~25 collection tests (construction, validation, serde)
- ~10 rules tests
- ~10 validation helper tests
- Remaining are pre-existing tests (error, configuration, telemetry)

## Files Modified

- `crates/zerobase-core/src/lib.rs` — added `schema` module and re-exports
- `crates/zerobase-core/Cargo.toml` — added `regex` dependency

## Files Created

- `crates/zerobase-core/src/schema/mod.rs` — module root, re-exports
- `crates/zerobase-core/src/schema/collection.rs` — Collection, CollectionType, AuthOptions, IndexSpec
- `crates/zerobase-core/src/schema/field.rs` — Field, FieldType, all 13 option structs
- `crates/zerobase-core/src/schema/rules.rs` — ApiRules
- `crates/zerobase-core/src/schema/validation.rs` — shared validation helpers (name, regex)
