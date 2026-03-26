# Redesign Sidebar Navigation

**Date:** 2026-03-22
**Task ID:** 6qkw86vv158awhs

## Summary

Restyled the Sidebar component to match the "Architectural Monolith" design system — a brutalist, high-precision black/white aesthetic with zero-radius corners, no shadows, and instant transitions.

### Changes Made

**Nav item styling:**
- Default state: `text-outline` (#777777 gray), no background, transparent left border
- Hover state: `text-on-surface` (black/white), `bg-surface-container-low` (#F3F3F4) — instant transition (global CSS disables all transitions)
- Active state: `bg-primary` (black) + `text-on-primary` (white) + 4px left `border-primary` accent

**Typography:**
- Applied `text-label-md` utility: 12px, bold (700), uppercase, 0.05em letter-spacing

**Spacing:**
- Horizontal padding: `px-8` (32px) on nav items and header
- Vertical padding: `py-3` (12px) on nav items

**Header:**
- Logo text changed to "ADMIN" with `text-label-md` and `tracking-widest` for editorial feel

**Icons:**
- Kept existing SVG icons at consistent 20px (`h-5 w-5`) with `shrink-0`
- Removed rounded `rx` values from overview icon rectangles (zero-radius principle)
- Gap between icon and text: `gap-3` (12px)

**Mobile drawer:**
- Same styling as desktop sidebar
- Backdrop opacity increased to `bg-black/50`
- Hover states use `surface-container-low` instead of `surface-container-high`

**Dark mode:**
- Works automatically via CSS custom properties (black bg, white text/borders, inverted active state)

**Code refactoring:**
- Extracted shared `NavList` component to eliminate duplication between desktop and mobile

## Files Modified

- `frontend/src/components/Sidebar.tsx` — Full restyle of Sidebar, MobileSidebar, and nav item rendering
