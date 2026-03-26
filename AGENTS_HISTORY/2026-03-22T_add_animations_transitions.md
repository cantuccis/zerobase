# Add Animations & Transitions (Phase 4 — UI Style Redesign)

**Task ID:** `6uxup0g5pbgtmq3`
**Date:** 2026-03-22
**Status:** Complete

## Summary

Added tasteful animations and transitions across all frontend pages and components, removing the global animation/transition blocker and implementing a consistent, accessibility-respecting animation system.

## Key Changes

### Foundation (`global.css`)
- **Removed** global `transition: none !important` and `animation-duration: 0s !important` overrides that were blocking all animations
- **Kept** `box-shadow: none !important` (intentional brutalist design principle)
- Added animation timing tokens: `--duration-fast` (120ms), `--duration-normal` (200ms), `--duration-slow` (300ms)
- Added easing tokens: `--ease-out`, `--ease-in`, `--ease-in-out`
- Added `@keyframes`: `zb-fade-in`, `zb-fade-out`, `zb-slide-up`, `zb-slide-down-in`, `zb-slide-left-in`, `zb-slide-left-out`, `zb-slide-right-in`, `zb-scale-in`, `zb-pulse`, `zb-spin`, `zb-shimmer`
- Added Tailwind v4 `@utility` classes: `animate-fade-in`, `animate-fade-out`, `animate-slide-up`, `animate-slide-down-in`, `animate-slide-left-in`, `animate-slide-left-out`, `animate-slide-right-in`, `animate-scale-in`, `animate-pulse-subtle`, `animate-spin`, `animate-shimmer`, `transition-colors-fast`, `transition-opacity-fast`, `transition-transform-fast`
- Added `prefers-reduced-motion` media query to disable animations for users who prefer reduced motion

### Page Transitions
- `DashboardLayout.tsx`: `animate-fade-in` on main content area for page transitions
- `LoginForm.tsx`: `animate-slide-up` on the form container

### Modal/Dialog Animations
- `RecordFormModal.tsx`: `animate-fade-in` backdrop, `animate-slide-up` panel
- `CollectionsPage.tsx`: `animate-fade-in` backdrop, `animate-scale-in` dialog
- `BackupsPage.tsx`: `animate-fade-in` backdrop, `animate-scale-in` dialog
- `LogsPage.tsx`: `animate-fade-in` backdrop, `animate-slide-up` panel
- `WebhooksPage.tsx`: `animate-fade-in` on all 3 modals (form, delivery history, delete confirm), `animate-scale-in`/`animate-slide-up` on panels
- `RecordsBrowserPage.tsx`: `animate-fade-in` backdrop, `animate-slide-left-in` detail panel

### Dropdown Animations
- `ThemeToggle.tsx`: `animate-slide-down-in` on dropdown
- `RelationPicker.tsx`: `animate-slide-down-in` on listbox
- `RecordsBrowserPage.tsx`: `animate-slide-down-in` on column visibility dropdown

### Sidebar & Navigation
- `Sidebar.tsx`: `animate-fade-in` backdrop, `animate-slide-left-in` mobile drawer, `transition-colors-fast` on nav items
- `DashboardLayout.tsx`: `transition-colors-fast` on sign out button

### Toast Notifications
- `ToastContainer.tsx`: `animate-slide-right-in` on individual toasts

### Hover/Focus Transitions (`transition-colors-fast`)
Applied to interactive elements across all pages:
- `OverviewPage.tsx`: retry button, table rows, quick action links
- `CollectionsPage.tsx`: cancel button, table rows, edit/delete buttons
- `LogsPage.tsx`: table rows
- `BackupsPage.tsx`: cancel button, restore/download/delete action buttons
- `WebhooksPage.tsx`: webhook table rows, delivery history rows
- `ApiDocsPage.tsx`: endpoint accordion buttons, filter reference accordion, collection selector items, code block copy reveal
- `RecordsBrowserPage.tsx`: (table rows already had inline `transition-colors`)
- `CollectionEditorPage.tsx`: type radio labels, cancel button
- `ErrorBoundary.tsx`: try again and reload buttons

### Loading State Animations
- `CollectionsPage.tsx`: `animate-pulse-subtle` on skeleton container
- `BackupsPage.tsx`: `animate-pulse-subtle` on skeleton container
- `ApiDocsPage.tsx`: `animate-pulse-subtle` on loading skeleton
- `RecordsBrowserPage.tsx`: `animate-pulse-subtle` on table skeleton

### Accordion/Expand Content
- `ApiDocsPage.tsx`: `animate-fade-in` on expanded endpoint details and filter reference details

### Error Boundary
- `ErrorBoundary.tsx`: `animate-fade-in` on fallback UI, `transition-opacity-fast` and `transition-colors-fast` on buttons

## Design Decisions
- Pure CSS animations (no animation library) — fits the brutalist "Architectural Monolith" design system
- All durations in 120–300ms range for professional, snappy feel
- Zero border-radius maintained throughout (no rounded corners on anything)
- `animation-fill-mode: both` used to prevent flicker
- `prefers-reduced-motion` fully respected — all animations collapse to 0.01ms

## Files Modified
- `frontend/src/styles/global.css`
- `frontend/src/components/DashboardLayout.tsx`
- `frontend/src/components/LoginForm.tsx`
- `frontend/src/components/Sidebar.tsx`
- `frontend/src/components/ThemeToggle.tsx`
- `frontend/src/components/records/RecordFormModal.tsx`
- `frontend/src/components/records/RelationPicker.tsx`
- `frontend/src/lib/toast/ToastContainer.tsx`
- `frontend/src/lib/error-boundary/ErrorBoundary.tsx`
- `frontend/src/components/pages/OverviewPage.tsx`
- `frontend/src/components/pages/CollectionsPage.tsx`
- `frontend/src/components/pages/CollectionEditorPage.tsx`
- `frontend/src/components/pages/RecordsBrowserPage.tsx`
- `frontend/src/components/pages/LogsPage.tsx`
- `frontend/src/components/pages/BackupsPage.tsx`
- `frontend/src/components/pages/SettingsPage.tsx` (already had inline transition-colors)
- `frontend/src/components/pages/AuthProvidersPage.tsx` (already had inline transition-colors/opacity)
- `frontend/src/components/pages/ApiDocsPage.tsx`
- `frontend/src/components/pages/WebhooksPage.tsx`
