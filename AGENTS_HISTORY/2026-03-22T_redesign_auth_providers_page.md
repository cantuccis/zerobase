# Redesign AuthProvidersPage

**Date:** 2026-03-22
**Task ID:** jhfrelq6r0givjw

## Summary

Restyled the AuthProvidersPage component to match the monolith design system used by SettingsPage and the `users_auth_monolith` design reference.

### Changes Made

- Replaced card-based provider layout with numbered section layout (01. Google, 02. Microsoft) using 4/8 grid pattern
- Added page header with `display-lg` uppercase title and accent bar
- Replaced blue toggle switches with monolith-style black/white binary toggles (no border-radius)
- Replaced rounded inputs with `mono-input` styled flat inputs (1px black border, 0px radius, focus border-2 compensation)
- Replaced rounded status badges with monolith label-sm badges (inverted for active, surface-container for inactive)
- Replaced blue save button with black primary button (uppercase, wide tracking) and added bordered secondary cancel button
- Replaced rounded alerts with monolith-style bordered alerts
- Applied CSS custom properties (`--md-sys-color-*`) throughout for dark mode auto-inversion
- Removed all `rounded-*`, `shadow-*`, and blue accent classes
- Added `SectionDivider` between provider sections with macro spacing (`space-y-24`)
- Copy button for redirect URL now uses monolith border styling

### All Functionality Preserved

- OAuth2 provider enable/disable toggles
- Client ID / Client Secret configuration
- Redirect URL display and copy
- Field validation with inline errors
- Save/load settings via API
- Write-only secret field behavior

## Files Modified

- `frontend/src/components/pages/AuthProvidersPage.tsx` — Full restyle
