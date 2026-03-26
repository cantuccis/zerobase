# Redesign CollectionsPage to Monolith Design System

**Date:** 2026-03-22
**Task ID:** 6yy8ul7om3zcal3

## Summary

Restyled the CollectionsPage component to match the Architectural Monolith design system. All visual elements updated from the previous soft/rounded UI to the brutalist, zero-radius, high-contrast aesthetic.

## Changes Made

### File Modified
- `frontend/src/components/pages/CollectionsPage.tsx`

### Styling Updates

1. **Page heading**: Added `text-display-lg` editorial typography with subtitle in `text-body-lg`
2. **Collections table**: Black header row (`bg-primary`) with white uppercase text (`text-label-md`, `text-on-primary`), 1px black borders on all cells (`border-primary`), hover state using `bg-surface-container-low`, zero border radius
3. **Search input**: Visible uppercase `SEARCH` label (`text-label-md`), 1px black border, no radius, `focus:border-2` on focus, search icon in `text-outline`
4. **New Collection button**: Black background (`bg-primary`), white text (`text-on-primary`), 0px radius, uppercase label text, `active:scale-[0.98]`
5. **Delete confirmation dialog**: Removed rounded corners and shadows, 1px black border, black/white color scheme, uppercase heading and button labels, error-colored delete button
6. **Collection type badges**: Bordered pills with uppercase text (`text-label-sm`, `border-primary`), no colored backgrounds
7. **Empty state**: Clean typography using design system tokens, no decorative SVG icons, uppercase label text
8. **Loading skeleton**: Matches table structure with black header and bordered rows
9. **Error/success alerts**: Border-based with design system color tokens (`border-error`, `bg-error-container`)
10. **Action buttons**: Edit = secondary style (bordered, inverts on hover), Delete = error-colored border style
11. **Summary count**: Uppercase label style (`text-label-sm`)
12. **Dark mode**: All elements use design system CSS custom properties that auto-invert via `.dark` class overrides
13. **Responsive layout**: Maintained flex-col to flex-row breakpoints for toolbar

## Verification
- TypeScript compilation: Clean (no errors)
- All CRUD operations preserved (no logic changes)
- All data-testid attributes preserved for testing
- Accessibility: aria attributes, focus trap in dialog, keyboard handlers all retained
