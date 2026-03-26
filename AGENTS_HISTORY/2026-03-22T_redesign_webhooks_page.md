# Redesign WebhooksPage

**Date:** 2026-03-22
**Task ID:** c4hbtfzyhx8r91i

## Summary

Restyled the WebhooksPage (`frontend/src/components/pages/WebhooksPage.tsx`) to match the monolith design system ("The Architectural Monolith"). All components were updated from the old Tailwind color classes to the project's CSS custom property design tokens.

## Changes Made

### Page Header
- Replaced `text-2xl font-bold text-gray-900` with `text-display-lg text-on-surface` (editorial display typography)
- Subtitle uses `text-body-lg text-on-surface-variant`

### Webhooks Table
- Black header row (`bg-primary dark:bg-on-primary`) with white uppercase text (`text-label-sm text-on-primary`)
- 1px borders using `border-primary dark:border-on-primary`
- URLs displayed in monospace via `font-data` class
- Row hover uses `hover:bg-surface-container-low`

### Event Badges
- Replaced colored badges (green/amber/red backgrounds) with bordered monolith badges
- `border border-primary` with `text-label-sm` uppercase text
- Consistent across table and delivery history

### Status Indicators
- Active: filled black badge (`bg-primary text-on-primary`) with filled dot
- Inactive: outline-only badge (`border-outline`) with outlined dot
- Dark mode properly inverts all states

### Action Buttons
- Connected button group with `-ml-px` for continuous borders
- Hover inversion pattern: `hover:bg-primary hover:text-on-primary`
- Delete button uses error color tokens: `border-error text-error hover:bg-error hover:text-on-error`

### Form Modal (Create/Edit)
- Sharp corners (0px radius), 1px black borders
- Event selection: toggle button group instead of checkboxes
  - Selected state: filled (`bg-primary text-on-primary`)
  - Unselected: transparent with border
- Monolith toggle switch for enabled/disabled
- Labels use `text-label-md` (uppercase, bold)
- Inputs use design token borders with `focus:border-2` pattern
- URL input uses `font-data` for monospace
- Cancel button: outline with hover inversion
- Submit button: filled with hover inversion (reverse)

### Delivery History Modal
- Same black header table pattern
- Status badges: success = filled black, failed = filled error, pending = outlined
- Pagination buttons with hover inversion
- Page indicator uses `font-data`

### Delete Confirmation Modal
- Sharp corners, 1px borders
- Cancel: outline button with hover inversion
- Delete: error-colored filled button

### Dark Mode
- Full inversion works via `dark:` prefixes on all design tokens
- `bg-primary/30` overlay becomes `bg-on-primary/30` in dark mode
- Modal surfaces use `dark:bg-surface-container`
- All borders invert via `dark:border-on-primary`

### Loading & Empty States
- Spinner: square border animation (no rounded-full)
- Empty state: bordered container, uppercase label via `text-label-md text-secondary`

## Files Modified

- `frontend/src/components/pages/WebhooksPage.tsx`
