# Dark Mode Inversion Correctness Across All Pages

**Date:** 2026-03-22
**Task ID:** nrsdmzjkzzysanj
**Phase:** 5

## Summary

Audited all frontend pages and components for dark mode inversion correctness. The design system uses CSS custom properties that swap values when `.dark` class is applied to `<html>`, providing a binary black/white inversion system. Found and fixed one hardcoded color issue; all other components properly use design tokens.

## Issues Found and Fixed

### 1. Hardcoded Green Status Color in LogsPage
- **File:** `frontend/src/components/pages/LogsPage.tsx` (line 142)
- **Problem:** Used `text-[#00C853]` (hardcoded green) for system status indicator, which doesn't invert in dark mode
- **Fix:** Replaced with `text-success` design token

### 2. Added Success Color Design Tokens
- **File:** `frontend/src/styles/global.css`
- **Added:** `--color-success` and `--color-on-success` tokens for both light and dark modes
  - Light: `#1b7d3a` (dark green, readable on white)
  - Dark: `#5dda7e` (bright green, readable on black)
- **Registered** tokens in Tailwind v4 `@theme` block

## Verification Results

| Page | Background | Text | Borders | Primary Buttons | Table Headers | Status Badges | Result |
|------|-----------|------|---------|----------------|---------------|---------------|--------|
| Login | bg-background | text-on-background | border-primary | bg-primary text-on-primary | N/A | N/A | PASS |
| Overview | bg-background | text-on-surface | border-primary | bg-primary text-on-primary | bg-primary text-on-primary | bg-primary / bg-error | PASS |
| Collections | bg-background | text-on-surface | border-primary | bg-primary text-on-primary | bg-primary text-on-primary | border-error / text-error | PASS |
| CollectionEditor | bg-background | text-on-surface | border-primary | bg-primary text-on-primary | N/A | N/A | PASS |
| RecordsBrowser | bg-background | text-on-surface | border-primary | bg-primary text-on-primary | bg-primary text-on-primary | N/A | PASS |
| Logs | bg-background | text-on-background | border-primary | bg-primary text-on-primary | bg-primary text-on-primary | bg-error / bg-primary | PASS |
| Backups | bg-background | text-on-surface | border-primary | bg-primary text-on-primary | bg-primary text-on-primary | N/A | PASS |
| Settings | bg-background | text-on-surface | border-primary | bg-primary text-on-primary | N/A | N/A | PASS |
| AuthProviders | bg-background | text-on-surface | border-primary | bg-primary text-on-primary | N/A | N/A | PASS |
| ApiDocs | bg-background | text-on-surface | border-primary | bg-primary text-on-primary | bg-primary text-on-primary | Method badges | PASS |
| Webhooks | bg-background | text-on-surface | border-primary | bg-primary text-on-primary | bg-primary text-on-primary | bg-primary / bg-error | PASS |

### Additional Verifications
- **Sidebar active item:** Uses `bg-primary text-on-primary` - correctly inverts (white bg + black text in dark mode)
- **ThemeToggle dropdown:** Uses `bg-background border-primary text-primary` with hover `bg-primary text-on-primary` - correctly inverts
- **Scrollbar colors:** Uses `var(--color-primary)` thumb and `var(--color-background)` track - auto-inverts
- **System theme preference:** Listens to `matchMedia('prefers-color-scheme: dark')` changes via ThemeContext
- **No invisible elements:** No cases found where text and background would be the same color in either mode
- **Brand logos (AuthProviders):** Google/Microsoft SVG logos use hardcoded brand colors - intentional and acceptable

## Files Modified

1. `frontend/src/styles/global.css` - Added `--color-success` and `--color-on-success` design tokens (light + dark)
2. `frontend/src/components/pages/LogsPage.tsx` - Replaced `text-[#00C853]` with `text-success`

## Test Results

- All 34 test files passing
- All 999 tests passing
- Build succeeds without errors
