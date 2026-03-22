# Editor (Rich Text) Field Type Implementation

**Date:** 2026-03-21 09:00
**Task ID:** pd1gam4hcelscx8

## Summary

Enhanced the existing `EditorOptions` field type with full HTML sanitization using the `ammonia` crate to prevent XSS attacks. The Editor field now:

- Sanitizes all HTML input through ammonia before validation and storage
- Supports configurable allowed tags (defaults to a safe rich-text set)
- Supports configurable allowed attributes per tag (with `*` wildcard for global attrs)
- Enforces safe link protocols (http, https, mailto only)
- Strips event handlers, script tags, iframes, forms, meta tags, etc.
- Adds `rel="noopener noreferrer"` to all links
- Strips HTML comments
- Applies max_length to sanitized output (not raw input)
- Provides `prepare_value()` for pre-storage sanitization
- Added `validate_and_prepare()` to `RecordValidator` for combined validation + sanitization

## Tests Written (42 editor-specific tests)

- Basic acceptance: HTML, plain text, empty string, complex HTML
- Type rejection: non-string, boolean, array
- Length constraints: max_length, at-limit, zero-means-unlimited
- XSS prevention: script tags, onerror, onclick, javascript: href, data: URI, style tags, iframe, SVG+script, meta refresh, object/embed, form tags, event handlers on various tags
- Safe content preservation: headings, paragraphs, bold/italic, links, images, lists
- Link security: rel=noopener added, HTML comments stripped
- prepare_value: sanitization for storage, non-string returns None
- Custom tags: restrict output to only specified tags
- Custom attributes: restrict output to only specified attributes
- Options validation: reject empty tags, accept valid tags, default validates
- FieldType integration: prepare sanitizes, null returns None
- Serialization round-trip: default and custom options, None fields omitted in JSON
- Max length on sanitized output
- RecordValidator integration: sanitizes editor HTML, preserves safe HTML, mixed fields, rejects invalid data

## Files Modified

- `Cargo.toml` (workspace) - Added `ammonia = "4"` dependency
- `crates/zerobase-core/Cargo.toml` - Added `ammonia` dependency
- `crates/zerobase-core/src/schema/field.rs` - Enhanced `EditorOptions` with sanitization, added `prepare_value()` to `FieldType`, added default allowed tags/attributes functions, 42 comprehensive tests
- `crates/zerobase-core/src/schema/record_validator.rs` - Added `validate_and_prepare()` method, fixed existing `editor_field` helper, added 4 integration tests

## Test Results

- 42 editor-specific tests: all pass
- 347 total tests in zerobase-core: all pass
- Full workspace build: clean
