# Redesign LoginPage and LoginForm

**Date:** 2026-03-22
**Task ID:** 8id6w28il1un4mj

## Summary

Restyled the login page and form components to match the "Architectural Monolith" design system — brutalist, zero-radius, no-shadow aesthetic with binary contrast and editorial typography.

### Changes Made

**LoginPage.tsx:**
- Replaced `bg-gray-50 dark:bg-gray-900` with `bg-background` (uses design token, auto dark mode)

**LoginForm.tsx:**
- Replaced heading with editorial `text-display-lg` ("Sign In") and `text-label-md` subtitle
- Removed all `rounded-md` classes — zero-radius enforced globally via design system
- Removed all `shadow-sm` classes
- Replaced blue accent colors (`ring-blue-500`, `bg-blue-600`, etc.) with design tokens (`bg-primary`, `text-on-primary`, `border-primary`)
- Input fields: 1px border using `border-primary`, focus state uses `ring-1 ring-primary`
- Labels: use `text-label-md` utility (12px, bold, uppercase, wide tracking)
- Submit button: `bg-primary text-on-primary`, padding `1.4rem` horizontal / `0.85rem` vertical, `active:scale-[0.98]`, `text-label-md`
- Error messages: use `text-error` / `border-error` tokens (maps to `#ba1a1a` light / `#ffb4ab` dark), no rounded containers
- Field errors use `text-error` instead of hardcoded red colors
- Dark mode handled automatically via CSS custom properties (inverted: white on black)

## Files Modified

- `frontend/src/components/LoginPage.tsx`
- `frontend/src/components/LoginForm.tsx`
