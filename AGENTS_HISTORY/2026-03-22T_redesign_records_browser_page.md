# Redesign Records Browser Page & Sub-Components

**Date:** 2026-03-22
**Task ID:** dpbo8y2e7jdg0fg
**Status:** Complete

## Summary

Redesigned the RecordsBrowserPage and all record sub-components to match the "Architectural Monolith" design system defined in `docs/design/stitch/`.

## Files Modified

### `frontend/src/components/pages/RecordsBrowserPage.tsx`
- **Records table**: Black header (`bg-primary`) with white uppercase text (`text-on-primary`), 1px borders, monospace for IDs and dates, hover = `surface-container-low`
- **Search/filter bar**: Inline inputs with 1px black borders, grouped filter buttons with `-ml-px` shared borders, active = black bg
- **Pagination**: Numbered page buttons with shared borders, active page = `bg-primary text-on-primary`
- **RecordDetail panel**: Black header, `border-l` divider (no shadow), labels uppercase with `text-label-sm`
- **ColumnToggle**: Square dropdown, design tokens, no shadow
- **Empty state**: `text-secondary`, underline "Clear filter" link
- **Error state**: `border border-error`, no background color
- **Breadcrumb**: `text-secondary` with underline hover links

### `frontend/src/components/records/RecordFormModal.tsx`
- Sharp-cornered modal with 1px black border, no shadow, `bg-primary/40` overlay
- Black header with white text
- Footer with `bg-surface-container-low`
- Square cancel/submit buttons using design tokens

### `frontend/src/components/records/field-inputs.tsx`
- Updated shared `inputClasses` and `errorInputClasses` to use design tokens
- All 13+ field input types: 1px borders, 0px radius, consistent sizing
- `BoolInput` toggle: square, `bg-primary text-on-primary` when checked
- `MultiSelectInput`: square option buttons with design token colors
- All labels: `text-label-sm font-bold uppercase tracking-[0.05em]`

### `frontend/src/components/records/RelationPicker.tsx`
- Square chips with `border-primary bg-surface`
- Dropdown with 1px border, no shadow, no rounded corners
- Active item: `bg-primary text-on-primary`
- Search input matches shared input class pattern

### `frontend/src/components/records/FileUpload.tsx`
- Dashed 1px black border drop zone, no rounded corners
- Black upload icon, underline "Click to browse" text
- Thin `h-1 bg-primary` progress bar
- Square file chips with `border-outline-variant`
- Square remove buttons with `bg-primary text-on-primary`

## Design Principles Applied
- Zero border-radius everywhere (0px, 90-degree angles)
- Binary contrast: `--color-primary` (#000) vs `--color-on-primary` (#e2e2e2)
- No shadows, no gradients
- All colors via CSS custom property design tokens (dark mode automatic)
- Typography: uppercase labels with letter-spacing
- Surface hierarchy: surface → container-low → container → container-high
