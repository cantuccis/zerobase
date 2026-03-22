# Implement Relation Field Picker Component

**Date:** 2026-03-21 15:03
**Task ID:** 7mvdxdgidjfkvz1
**Phase:** 10

## Summary

Built a reusable `RelationPicker` component for the record editor that enables searching and selecting records from a target collection. The component supports both single and multi-select modes, displays meaningful record labels (not just IDs), includes keyboard navigation, debounced search, and accessible ARIA attributes following the combobox pattern.

The existing inline `RelationInput` in `field-inputs.tsx` was refactored to delegate to the new standalone `RelationPicker` component, maintaining backward compatibility with all existing tests.

## Key Features

- **Search**: Async debounced search with initial results on focus
- **Single select**: Shows selected label with clear button, hides search input when selected
- **Multi select**: Tag chips with remove buttons, filters already-selected from results
- **Labels**: Displays meaningful record labels via `selectedLabels` prop and search results
- **Keyboard navigation**: ArrowUp/Down to navigate, Enter to select, Escape to close
- **Accessibility**: ARIA combobox pattern, listbox roles, aria-activedescendant tracking
- **Click outside**: Closes dropdown when clicking outside the picker container
- **Error handling**: Graceful handling of search errors, loading states, empty states

## Test Coverage

42 new tests covering:
- Rendering (container, collection name, placeholder, ARIA, error styling)
- Single select (display, clear, selection, replacement)
- Multi select (chips, add/remove, duplicate prevention, ARIA roles)
- Search behavior (focus open, query forwarding, results display, labels, empty state, loading, error recovery)
- Keyboard navigation (ArrowDown open, navigate, wrap-around, Enter select, Escape close)
- Click outside (close dropdown)
- Label display (selectedLabels, fallback to ID, accessible remove labels)
- Edge cases (empty value, debounce, no-focus state)

All 700 tests across 22 test files pass.

## Files Modified

- `frontend/src/components/records/RelationPicker.tsx` — **NEW** — Reusable relation picker component
- `frontend/src/components/records/RelationPicker.test.tsx` — **NEW** — 42 comprehensive tests
- `frontend/src/components/records/field-inputs.tsx` — Refactored `RelationInput` to delegate to `RelationPicker`, added `selectedRelationLabels` prop, re-exported `RelationOption` from new module
