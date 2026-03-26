# Redesign BackupsPage

**Date:** 2026-03-22
**Task ID:** h025fdwu2f739qk
**Status:** Completed

## Summary

Restyled the BackupsPage (`frontend/src/components/pages/BackupsPage.tsx`) to match the `database_backups_monolith` design reference from the Stitch design system ("The Architectural Monolith").

## Changes Made

### Hero Section
- Large editorial heading: `DATABASE BACKUPS` at 3.5rem, font-extrabold (800), tight tracking
- Breadcrumb-style label: `STORAGE / SYSTEM` in 12px uppercase with wide letter-spacing
- Accent bar: 1px-high primary color bar below heading
- Primary CTA: black background, white text, uppercase tracking, icon + text

### Backups Table (12-Column Grid)
- Replaced HTML `<table>` with CSS Grid (12-column) using 6-2-2-2 column split
- Table header: black background (`bg-primary`), white uppercase text
- Row cells: bordered with `outline-variant`, hover state uses `surface-container-low`
- Filenames displayed in monospace font
- Sizes in bold monospace, dates in regular monospace

### Action Buttons
- Hover inversion pattern: transparent → black bg + white text on hover
- Delete button: hover uses error color (red) instead of black
- Material Symbols Outlined icons (folder_zip, settings_backup_restore, download, delete)

### Warning Callout
- Left 4px border accent in primary color
- Icon in inverted container (black bg, white icon)
- Uppercase label with wide tracking
- No rounded corners

### Metrics Grid
- 4-column grid with bordered cells (`outline-variant`)
- Large display numbers (text-4xl, font-extrabold)
- Uppercase micro-labels (10px, tracking-widest)
- Shows: Total Backups, Storage Used, Latest Backup, Status

### Confirmation Modals
- Sharp corners (0px border-radius)
- 1px primary border, no shadows
- Uppercase title labels
- Button styling matches design system (primary/error variants)

### Dark Mode
- All colors use CSS custom properties that auto-invert
- Consistent `dark:` variants throughout

### Preserved Functionality
- All backup operations: create, download, delete, restore
- Loading skeletons (restyled to grid layout)
- Empty state
- Success/error banners
- Confirmation modals with focus trap and keyboard navigation
- All data-testid attributes preserved

## Files Modified

- `frontend/src/components/pages/BackupsPage.tsx` — Full restyle
