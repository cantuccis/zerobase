# Collection Schema Editor UI

**Date:** 2026-03-21 13:25
**Task:** Implement collection schema editor

## Summary

Built a complete collection schema editor UI for the Zerobase admin dashboard. The editor supports creating new collections and editing existing ones, with a dynamic field list supporting all 13 field types with type-specific configuration options, validation, reordering, and an API endpoint preview panel.

## What Was Built

### Routing & Pages
- `/_/collections/new` - Create a new collection
- `/_/collections/[id]/edit` - Edit an existing collection

### Components
- **CollectionEditorPage** - Main form with collection name, type selection (Base/Auth/View), field management, validation, and save/create logic
- **FieldEditor** - Per-field configuration: name, type dropdown, required/unique toggles, move up/down/remove, type-specific options
- **FieldTypeOptions** - Renders type-specific configuration for all 13 field types (text, number, bool, email, url, dateTime, select, multiSelect, autoDate, file, relation, json, editor)
- **ApiPreview** - Shows auto-generated REST API endpoints based on collection name and type
- **field-defaults** - Default values for each field type and ID generation

### Features
- Create and edit modes with proper loading/error states
- Client-side validation (name format, empty names, duplicates)
- Server-side validation error mapping
- Field reordering (move up/down)
- Relation fields show dropdown of all available collections
- Auth type shows additional auth-specific API endpoints in preview
- Success/error feedback on save
- Accessible: proper labels, aria attributes, keyboard support

### Tests
- **CollectionEditorPage.test.tsx** - 30 tests covering create mode, edit mode, validation, API calls, field operations, type changes, reordering, API preview
- **FieldEditor.test.tsx** - 28 tests covering rendering, name editing, type changes, toggles, move/remove, all type-specific options, accessibility

**All 248 tests pass (including pre-existing tests).**

## Files Created

- `frontend/src/pages/collections/new.astro`
- `frontend/src/pages/collections/[id]/edit.astro`
- `frontend/src/components/pages/CollectionEditorPage.tsx`
- `frontend/src/components/pages/CollectionEditorPage.test.tsx`
- `frontend/src/components/schema/FieldEditor.tsx`
- `frontend/src/components/schema/FieldEditor.test.tsx`
- `frontend/src/components/schema/FieldTypeOptions.tsx`
- `frontend/src/components/schema/ApiPreview.tsx`
- `frontend/src/components/schema/field-defaults.ts`
