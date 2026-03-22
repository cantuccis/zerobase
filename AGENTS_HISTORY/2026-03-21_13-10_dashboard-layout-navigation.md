# Admin Dashboard Layout and Navigation

**Date**: 2026-03-21 13:10
**Task ID**: 4nox8lp5u8jkfg9

## Summary

Implemented the main admin dashboard layout with sidebar navigation, header with user info and logout, responsive mobile drawer, auth guard, and comprehensive tests. The dashboard now has a proper sidebar-based layout matching PocketBase's admin UI pattern.

### Key decisions:
- **Sidebar-based layout**: Desktop shows a persistent 240px sidebar; mobile uses a slide-out drawer activated by a hamburger menu button.
- **Astro multi-page routing**: Each section (Collections, Settings, Logs, Backups) is a separate Astro page with its own React island, rather than client-side routing. This keeps the static-first approach.
- **Auth guard**: Wraps all dashboard pages via `DashboardLayout`, redirecting to `/_/login` if not authenticated.
- **Accessibility**: Navigation uses semantic HTML (`<nav>`, `<ul>`, `<li>`), `aria-current="page"` for active items, `aria-label` on icon-only buttons, `aria-hidden` on decorative SVGs, and proper dialog attributes on the mobile drawer.

## Files Modified

- `frontend/src/components/Sidebar.tsx` — **New**. Desktop sidebar and mobile drawer components with navigation items and SVG icons.
- `frontend/src/components/DashboardLayout.tsx` — **New**. Main layout component composing AuthProvider, AuthGuard, Sidebar, header, and content area.
- `frontend/src/components/Dashboard.tsx` — **Updated**. Refactored to use DashboardLayout (marked deprecated in favor of page-specific components).
- `frontend/src/components/pages/CollectionsPage.tsx` — **New**. Collections page component.
- `frontend/src/components/pages/SettingsPage.tsx` — **New**. Settings page component.
- `frontend/src/components/pages/LogsPage.tsx` — **New**. Logs page component.
- `frontend/src/components/pages/BackupsPage.tsx` — **New**. Backups page component.
- `frontend/src/layouts/DashboardLayout.astro` — **New**. Astro layout for dashboard pages (h-screen, overflow-hidden body).
- `frontend/src/pages/index.astro` — **Updated**. Now uses CollectionsPage with DashboardLayout.
- `frontend/src/pages/settings.astro` — **New**. Settings page route.
- `frontend/src/pages/logs.astro` — **New**. Logs page route.
- `frontend/src/pages/backups.astro` — **New**. Backups page route.
- `frontend/src/components/Sidebar.test.tsx` — **New**. 27 tests covering isNavItemActive logic, sidebar rendering, active states, accessibility, and mobile drawer open/close behavior.
- `frontend/src/components/DashboardLayout.test.tsx` — **New**. 12 tests covering layout rendering, page title, Sign Out, active sidebar item, auth guard redirect.
- `frontend/src/components/AuthGuard.test.tsx` — **New**. 4 tests covering auth guard behavior.

## Test Results

- **149 tests passing** across 7 test files
- **Astro build**: 5 pages built successfully (index, login, settings, logs, backups)
