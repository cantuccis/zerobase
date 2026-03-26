# Redesign CollectionEditorPage and Schema Components

**Date:** 2026-03-22
**Task ID:** 8g5jlw2wwx9din6

## Summary

Restyled the CollectionEditorPage and all 6 schema sub-components to match the "Architectural Monolith" design system. Applied the numbered-section form layout pattern (12-column grid), editorial typography, zero-radius borders, and monochrome palette throughout.

## Key Design Changes

- **Layout**: 12-column grid with left column (4 cols) for section labels/descriptions and right column (8 cols) for form inputs
- **Section Headers**: Numbered sections (01. General, 02. Fields, etc.) with `text-label-md` editorial typography and `border-b border-primary` dividers
- **Inputs**: 1px black border, 0px radius, focus state keeps 1px border (no blue ring)
- **Field Editor Cards**: 1px primary border, no shadow, no radius, clean bordered cards
- **Select Dropdowns**: Black border, white background, sharp corners
- **Checkboxes**: Styled with `accent-[var(--color-primary)]` for black checked state
- **RulesEditor**: Monospace font for rule expressions, primary-colored syntax highlighting
- **ApiPreview**: Method badges (GET=black outline, POST/DELETE=black fill), monospace code blocks with primary border header
- **AuthFieldsDisplay**: Bordered cards with label-sm type badges, lock icons via Material Symbols
- **AuthSettingsEditor**: Toggle switches with primary-colored border, section dividers with `border-primary`
- **Buttons**: Primary (black bg) and secondary (bordered) following monolith spec
- **Dark Mode**: All components use design token CSS variables that auto-invert

## Files Modified

### Components (7 files)
- `frontend/src/components/pages/CollectionEditorPage.tsx` - Main editor with numbered sections, 12-col grid layout
- `frontend/src/components/schema/FieldEditor.tsx` - Bordered field cards, monolith inputs
- `frontend/src/components/schema/FieldTypeOptions.tsx` - Label-sm labels, monolith inputs/checkboxes
- `frontend/src/components/schema/RulesEditor.tsx` - Monospace rules, monolith badges, removed wrapper section
- `frontend/src/components/schema/ApiPreview.tsx` - Method badges, monospace paths, primary header
- `frontend/src/components/schema/AuthFieldsDisplay.tsx` - Bordered field list, Material Symbols icons
- `frontend/src/components/schema/AuthSettingsEditor.tsx` - Primary borders, monolith toggles, removed wrapper section

### Tests (4 files)
- `frontend/src/components/pages/CollectionEditorPage.test.tsx` - Updated badge/text assertions
- `frontend/src/components/schema/RulesEditor.test.tsx` - Updated badge text matchers to regex
- `frontend/src/components/schema/AuthFieldsDisplay.test.tsx` - Updated badge text to uppercase
- `frontend/src/components/schema/AuthSettingsEditor.test.tsx` - Removed heading/description tests (moved to parent)

## Test Results

All 204 schema/editor tests pass (0 failures).
