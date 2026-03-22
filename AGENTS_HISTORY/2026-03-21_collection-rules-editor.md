# Collection Rules Editor Implementation

**Date**: 2026-03-21 13:35
**Task**: Implement collection rules editor (Phase 10)

## Summary

Built the access rules editor for collections in the Zerobase admin dashboard. The editor allows setting per-collection API rules (list, view, create, update, delete, manage) with syntax highlighting, validation, preset support, and helper documentation.

## Features Implemented

### RulesEditor Component
- **6 rule inputs**: listRule, viewRule, createRule, updateRule, deleteRule, manageRule
- **Lock/unlock toggle**: Switch between locked (null/superusers only) and unlocked states
- **Status badges**: Visual indicators showing Locked, Public, or Conditional state per rule
- **Preset system**: Quick-apply presets (Locked, Public, Authenticated, Owner only)
- **ManageRule visibility**: Hidden for view collections, shown for base/auth

### Syntax Highlighting
- Custom tokenizer that highlights:
  - `@request.*` macros (purple)
  - Operators (`=`, `!=`, `>`, `<`, `~`, etc.) (red)
  - Logical operators (`&&`, `||`, `AND`, `OR`) (blue)
  - String literals (green)
  - Numbers (amber)
  - Boolean/null literals (amber italic)
  - Field identifiers (dark)
- Overlay technique: transparent textarea with highlighted div underneath

### Rule Validation
- Unterminated string detection
- Balanced parentheses checking
- Operator position validation (no leading/trailing operators)
- Unknown `@variable` root detection (only @request, @collection, @now allowed)
- Invalid operator sequence detection
- Per-input error display with aria-invalid for accessibility
- Validation summary banner when errors exist

### Helper Documentation Panel
- Toggle-able reference panel with sections:
  - Request Variables (@request.auth.*, @request.data.*, etc.)
  - Record Fields (id, created, updated, dot-notation)
  - Other Variables (@collection, @now)
  - Operators reference
  - Example expressions
  - Rule Values explanation (locked/empty/expression)

### CollectionEditorPage Integration
- Added `rules` to FormState with locked defaults
- Rules loaded from existing collection in edit mode
- Rules included in save payload for both create and update
- RulesEditor positioned between Fields section and API Preview

## Tests Written

### RulesEditor.test.tsx (70 tests)
- Rendering: 12 tests (all rule fields, badges, descriptions, states)
- Lock/unlock toggle: 5 tests (toggle behavior, preserves other rules, accessibility)
- Expression editing: 2 tests (typing, preserves other rules)
- Presets: 3 tests (authenticated, public, locked presets)
- Helper documentation: 8 tests (toggle, sections, variables, operators, examples)
- Syntax highlighting: 4 tests (macros, operators, strings, fields)
- Validation: 8 tests (errors, valid expressions, summary, aria-invalid)
- Mixed state rendering: 1 test
- validateRuleExpression unit tests: 27 tests

### CollectionEditorPage.test.tsx (7 new tests)
- Rules editor renders in create mode
- Default locked state in create mode
- Unlock and type expression
- Rules included in create payload
- Existing rules loaded in edit mode
- Updated rules in edit payload
- Helper docs toggle

## Files Modified
- `frontend/src/components/schema/RulesEditor.tsx` (new)
- `frontend/src/components/schema/RulesEditor.test.tsx` (new)
- `frontend/src/components/pages/CollectionEditorPage.tsx` (modified)
- `frontend/src/components/pages/CollectionEditorPage.test.tsx` (modified)

## Test Results
- All 326 tests pass across 11 test files
- No regressions in existing tests
