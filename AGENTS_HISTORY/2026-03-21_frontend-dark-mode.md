# Frontend Dark Mode Implementation

**Task ID:** `4nzmhhkp9gbe98v`
**Date:** 2026-03-21
**Phase:** 10

## Objective

Implement frontend dark mode for the Zerobase admin dashboard with system preference detection, manual toggle, localStorage persistence, and correct rendering across all pages in both modes.

## Changes Made

### 1. Theme Infrastructure

- **`src/styles/global.css`** - Added Tailwind v4 class-based dark mode via `@custom-variant dark (&:where(.dark, .dark *))`.
- **`src/lib/theme/ThemeContext.tsx`** (new) - React Context providing `ThemeProvider` and `useTheme()` hook. Manages theme state (`light | dark | system`), resolves system preference via `matchMedia`, persists to localStorage (`zerobase-theme` key), and toggles `.dark` class on `<html>`.
- **`src/lib/theme/index.ts`** (new) - Re-exports from ThemeContext.
- **`src/components/ThemeToggle.tsx`** (new) - UI component with Sun/Moon icon button (cycles themes) and select dropdown (Light/Dark/System).

### 2. Layout Integration

- **`src/layouts/Layout.astro`** - Added inline `<script is:inline>` in `<head>` to prevent flash-of-incorrect-theme; added `dark:bg-gray-900 dark:text-gray-100` to body.
- **`src/layouts/DashboardLayout.astro`** - Same inline script and dark body classes.
- **`src/components/DashboardLayout.tsx`** - Wrapped with `ThemeProvider`, added `ThemeToggle` to header, added `dark:` classes throughout.
- **`src/components/LoginPage.tsx`** - Wrapped in `ThemeProvider`, added dark classes.

### 3. Component Dark Mode Styling

Applied `dark:` Tailwind variants to all UI components:

- **`src/components/LoginForm.tsx`** - Inputs, labels, errors, buttons
- **`src/components/Sidebar.tsx`** - Desktop sidebar, mobile drawer, active/inactive nav items
- **`src/lib/toast/ToastContainer.tsx`** - All toast types (success, error, warning, info)
- **All 9 page components** - OverviewPage, CollectionsPage, CollectionEditorPage, RecordsBrowserPage, ApiDocsPage, SettingsPage, AuthProvidersPage, LogsPage, BackupsPage
- **All records/schema components** - RecordFormModal, RelationPicker, FileUpload, field-inputs, FieldEditor, AuthSettingsEditor, AuthFieldsDisplay, RulesEditor, ApiPreview

### 4. Tests

- **`tests/setup.ts`** - Added localStorage polyfill (Node.js 25 compatibility) and matchMedia polyfill for test environments.
- **`src/lib/theme/ThemeContext.test.tsx`** (new) - 12 tests: default system theme, system preference detection, localStorage read/write, dark class toggling, system preference change listener, manual mode isolation, theme cycling, provider boundary error, invalid localStorage values.
- **`src/components/ThemeToggle.test.tsx`** (new) - 7 tests: renders button and select, 3 options, defaults to system, switch to dark/light, cycle via button, persistence.

## Test Results

All 889 tests pass across 30 test files. Zero TypeScript errors in any theme-related files.

## Dark Mode Color Palette

- Backgrounds: `bg-gray-900` (page), `bg-gray-800` (cards/panels)
- Text: `text-gray-100` (primary), `text-gray-300` (secondary), `text-gray-400` (muted)
- Borders: `border-gray-700`
- Active states: `bg-blue-900/30 text-blue-400`
- Hover: `hover:bg-gray-700`
