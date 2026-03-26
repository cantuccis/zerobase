# Design Token Fidelity Validation Against Stitch References

**Date:** 2026-03-22
**Task ID:** 81zb4wqj935c2c2

## Summary

Performed comprehensive design token fidelity validation comparing all restyled frontend pages against the DESIGN.md specification from `docs/design/stitch/monolith_grid/DESIGN.md`. Audited 6 categories and fixed all violations found.

## Audit Results

### Passing (No Violations Found)
- **Border-radius**: Zero-radius enforced globally via `* { border-radius: 0; }` in global.css. No `rounded-*` classes found in any component.
- **Box-shadows**: No shadow classes found. Global `* { box-shadow: none !important; }` enforced.
- **Font families**: Only Inter and JetBrains Mono are used. No foreign font references.

### Violations Found & Fixed

#### 1. Wrong Token Namespace (`--md-sys-color-*`)
**AuthProvidersPage.tsx** and **SettingsPage.tsx** used Material Design 3 token namespace (`--md-sys-color-*`) instead of the Zerobase design system (`--color-*` / Tailwind token classes).
- Replaced 24+ instances in AuthProvidersPage.tsx
- Replaced 20+ instances in SettingsPage.tsx
- Mapped to proper Tailwind tokens: `bg-surface`, `text-on-surface`, `border-primary`, `border-error`, `text-error`, `text-outline`, `bg-surface-container`, etc.

#### 2. Hardcoded Hex Colors
**RulesEditor.tsx**: `#059669` and `#34d399` for syntax highlighting string tokens.
- Replaced with `var(--color-secondary)` to stay within the monolith grayscale palette.

#### 3. Raw Tailwind Colors (Modal Backdrops)
Four files used `bg-black/*` instead of design token `bg-primary/*`:
- **Sidebar.tsx**: `bg-black/50` → `bg-primary/50`
- **BackupsPage.tsx**: `bg-black/50` → `bg-primary/50`
- **LogsPage.tsx**: `bg-black/40 dark:bg-black/60` → `bg-primary/40 dark:bg-primary/60`
- **CollectionsPage.tsx**: `bg-black/30 dark:bg-black/60` → `bg-primary/30 dark:bg-primary/60`

#### 4. CSS Variable Wrappers Instead of Tailwind Tokens
**ErrorBoundary.tsx** and **ToastContainer.tsx** used `border-[var(--color-primary)]` pattern instead of direct `border-primary`.
- Replaced all `*-[var(--color-*)]` patterns with direct Tailwind token classes.

#### 5. Test File Updates
- **Sidebar.test.tsx**: Updated backdrop selector from `bg-black` to `bg-primary`
- **ToastContainer.test.tsx**: Updated border assertions to use direct token classes
- **cross-browser-responsive.test.tsx**: Updated toast className assertion

## Design Token Compliance Summary

| Token Category | Spec Value | Implementation | Status |
|---|---|---|---|
| Primary | #000000 (light) / #FFFFFF (dark) | `--color-primary` in global.css | PASS |
| Surface | #FFFFFF | `--color-surface-lowest: #ffffff` | PASS |
| Secondary | #5e5e5e | `--color-secondary: #5e5e5e` | PASS |
| Outline | #777777 | `--color-outline: #777777` | PASS |
| Surface hierarchy | #F3F3F4/#EEEEEE/#DADADA | Correctly mapped | PASS |
| Error | #ba1a1a | `--color-error: #ba1a1a` | PASS |
| Display-LG | 3.5rem/800 | `text-display-lg` utility | PASS |
| Headline-LG | 2rem/700 | `text-headline-lg` utility | PASS |
| Title-MD | 1.125rem/600 | `text-title-md` utility | PASS |
| Body-LG | 1rem/400 | `text-body-lg` utility | PASS |
| Label-MD | 0.75rem/700/uppercase/+0.05em | `text-label-md` utility | PASS |
| Inter font | All UI text | `--font-sans` globally applied | PASS |
| JetBrains Mono | Technical data | `font-data` / `font-mono` utilities | PASS |
| Material Symbols | 20px outlined | Configured in global.css | PASS |
| Zero border-radius | Everywhere (except 9999px circles) | Global reset `* { border-radius: 0; }` | PASS |
| No box-shadows | Everywhere | Global `* { box-shadow: none !important; }` | PASS |
| Borders | 1px solid primary | `border-primary` tokens | PASS |
| Spacing scale | micro/standard/macro | Custom properties in @theme | PASS |

## Test Results
**999/999 tests passing** after all fixes.

## Files Modified
- `frontend/src/components/pages/AuthProvidersPage.tsx`
- `frontend/src/components/pages/SettingsPage.tsx`
- `frontend/src/components/schema/RulesEditor.tsx`
- `frontend/src/components/Sidebar.tsx`
- `frontend/src/components/Sidebar.test.tsx`
- `frontend/src/components/pages/BackupsPage.tsx`
- `frontend/src/components/pages/LogsPage.tsx`
- `frontend/src/components/pages/CollectionsPage.tsx`
- `frontend/src/lib/error-boundary/ErrorBoundary.tsx`
- `frontend/src/lib/toast/ToastContainer.tsx`
- `frontend/src/lib/toast/ToastContainer.test.tsx`
- `frontend/src/components/__tests__/cross-browser-responsive.test.tsx`
