# Redesign LogsPage — Brutalist Monolith Style

**Date:** 2026-03-22
**Task ID:** q6yag46a5f93i0m

## Summary

Completely restyled the LogsPage component to match the `logs_activity_monolith` design reference. The page now uses the brutalist monolith design system with zero-radius corners, 1px borders, binary contrast, and uppercase labels throughout.

## Changes Made

### Files Modified

- `frontend/src/components/pages/LogsPage.tsx` — Full redesign

### Design Changes

1. **System Health Metrics Bar**: 4-column bordered grid at the top with large numbers (total requests, avg latency, error rate, system status) and small uppercase labels. Uses `border-primary` with cells separated by right borders.

2. **Filter/Search Bar**: Replaced dropdown selects with grouped toggle buttons in bordered containers:
   - STATUS toggle group (All, 2xx, 3xx, 4xx, 5xx)
   - Date preset toggles (1H, 24H, 7D, 30D)
   - METHOD toggle group (ALL, GET, POST, PATCH, PUT, DELETE)
   - URL path filter input with monospace font
   - Active state: black bg/white text; inactive: white bg/black text; all uppercase bold

3. **Logs Table**:
   - Black header row with white uppercase text at 11px
   - 1px borders throughout using design tokens
   - Monospace font for timestamps, IPs, paths, and latency values
   - Removed timeline chart and status breakdown bar (not in design reference)

4. **Status Badges**:
   - Success (2xx): green `#00C853` bg with black text
   - Error (5xx): red `#BA1A1A` bg with white text
   - Client error (4xx): orange `#E65100` bg with white text
   - Redirect (3xx): bordered, transparent
   - All uppercase, 11px font

5. **Method Badges**: Write methods (POST/PUT/PATCH/DELETE) get filled black; GET gets outlined style

6. **Error Rows**: Subtle `bg-error-container/30` highlight on rows with status >= 400

7. **Pagination**: Numbered page buttons in a bordered container at bottom, with arrow navigation

8. **Modal**: Black header bar, bordered cells for detail rows, uppercase labels

9. **Dark Mode**: Full inversion using design system tokens (`dark:bg-primary`, `dark:text-on-primary`, etc.)

10. **Zero Radius**: All corners are 0px per the design system's global CSS rule

11. **No Shadows**: Enforced by the global CSS `box-shadow: none !important`

### Removed Components

- `TimelineChart` — Not present in the monolith design reference
- `StatusBreakdown` — Not present in the monolith design reference
- Color-coded method classes (replaced with binary MethodBadge)
- Rounded corners on all elements
- All shadow effects
