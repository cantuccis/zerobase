# Number Field Type Implementation

**Date:** 2026-03-21 12:00
**Task ID:** jtxhm6u1b22hhce

## Summary

Enhanced the Number field type (`NumberOptions`) with comprehensive validation, PocketBase-compatible serialization, value coercion, and extensive test coverage.

## What Was Done

### Enhancements to `NumberOptions`

1. **NaN/Infinity validation for options**: `validate()` now rejects `NaN` and infinite values for `min` and `max` bounds, preventing invalid field configurations.

2. **PocketBase `noDecimal` compatibility**: Added `#[serde(alias = "noDecimal")]` to the `only_int` field, so JSON payloads using either `"onlyInt"` or `"noDecimal"` are accepted during deserialization. Serialization always uses `onlyInt` (camelCase).

3. **`prepare_value()` method**: Added value coercion for incoming data:
   - JSON numbers pass through unchanged
   - Numeric strings (e.g., `"42"`, `"3.14"`) are parsed to numbers
   - Empty strings convert to `null`
   - Booleans convert to `1.0`/`0.0`
   - Non-convertible values return `None`
   - Infinite results from string parsing are rejected

4. **Registered `prepare_value` in `FieldType` dispatch**: Number fields now participate in the `FieldType::prepare_value()` pipeline alongside Editor and DateTime fields.

5. **NaN/Infinity validation for values**: `validate_value()` now explicitly rejects `NaN` and infinite JSON numbers.

### Tests Added (41 new tests, 48 total number tests)

- Options validation: equal min/max, NaN min/max, infinite min/max
- Value validation: zero, negative, boundary values (at min, at max), within range, below/above range
- Integer-only mode: negative integers, zero, negative floats, combined with min/max
- Type rejection: strings, booleans, arrays, objects
- Edge cases: very large floats, very small floats
- Field-level: required rejects null, optional accepts null
- SQL type mapping: REAL
- `prepare_value`: number passthrough, null handling, string coercion (int, float, negative, whitespace, empty, non-numeric), boolean conversion, array/object rejection
- Serde: `noDecimal` alias deserialization, `onlyInt` serialization, default serialization skips None

## Files Modified

- `crates/zerobase-core/src/schema/field.rs` — Enhanced `NumberOptions` struct, validation, `prepare_value`, `FieldType` dispatch, and 41 new tests

## Test Results

- **454 total tests passing** (full zerobase-core suite)
- **48 number-specific tests passing**
- No regressions
