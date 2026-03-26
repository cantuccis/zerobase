# Redesign DashboardLayout and Header

**Date:** 2026-03-22
**Task ID:** mfdtjhrn9keu90f

## Summary

Restyled the DashboardLayout, Sidebar, MobileSidebar, and ThemeToggle components to match the "Architectural Monolith" design system. Key changes:

- **Layout structure:** Changed from flexbox sidebar+content to fixed-position sidebar (w-64, full height) + fixed header (h-16, offset by sidebar width on desktop) + main content with `pt-16 md:ml-64`
- **Header:** Removed rounded corners, shadows, gray color scheme. Now uses 1px `border-primary` bottom border, `bg-background`, black text. Sign Out button uses `text-label-md` (uppercase, bold, tracked). Dark mode auto-inverts via CSS custom properties.
- **Sidebar:** Fixed position, 1px right `border-primary`. Navigation items use `text-label-md` (uppercase). Active state: black background + white text + 4px left accent border. Removed rounded corners and blue accent colors.
- **Mobile drawer:** Matching monolith styling — no shadows, 1px borders, black/white binary colors, uppercase labels
- **ThemeToggle:** Removed rounded corners, gray colors. Uses `border-primary`, `text-primary`, uppercase select dropdown
- **Skip-to-main-content:** Preserved with monolith-appropriate styling (bg-primary, text-on-primary)
- **Provider wrapping:** All providers (AuthProvider, ThemeProvider, ToastProvider, ErrorBoundary) preserved unchanged

## Files Modified

1. `frontend/src/components/DashboardLayout.tsx` — Layout structure, header, skip link, content area
2. `frontend/src/components/Sidebar.tsx` — Desktop sidebar and mobile drawer
3. `frontend/src/components/ThemeToggle.tsx` — Theme toggle button and select styling
