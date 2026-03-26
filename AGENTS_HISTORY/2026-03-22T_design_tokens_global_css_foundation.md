# Design Tokens and Global CSS Foundation

**Date:** 2026-03-22
**Task ID:** pyvb962awe9by7w
**Phase:** 1 — UI Style Redesign

## Summary

Implemented the foundational design system for the "Architectural Monolith" design language by completely rewriting `frontend/src/styles/global.css`. This establishes all design tokens, typography utilities, and global styling rules that subsequent component updates will build upon.

## What Was Done

### Design Tokens (CSS Custom Properties + Tailwind v4 `@theme inline`)
- **Colors:** Full palette mapped — primary (#000000), surface hierarchy (#FFFFFF through #DADADA), secondary (#5e5e5e), outline (#777777), error (#ba1a1a), plus all extended tokens from the stitch HTML samples
- **Dark mode:** Complete inverted token set (black backgrounds, white text/borders) activated via `.dark` class on `<html>`
- **Font families:** `--font-sans` (Inter), `--font-mono` (JetBrains Mono), plus `--font-headline`, `--font-body`, `--font-label` aliases
- **Border radius:** All radius values overridden to `0px` except `--radius-full` (9999px)
- **Spacing:** Custom architectural spacing tokens: `micro` (0.35rem), `micro-lg` (0.7rem), `standard` (1.4rem), `macro-sm` (4rem), `macro` (5.5rem), `macro-lg` (7rem)

### Typography Utility Classes (`@utility` directives)
- `text-display-lg` — 3.5rem/800/-0.02em
- `text-headline-lg` — 2rem/700/-0.01em
- `text-title-md` — 1.125rem/600
- `text-body-lg` — 1rem/400
- `text-label-md` — 0.75rem/700/uppercase/+0.05em
- `text-label-sm` — 0.7rem/700/uppercase/+0.06em
- `font-data` — JetBrains Mono with tabular-nums

### Global Base Styles
- Zero border-radius enforced on all elements via `@layer base`
- All transitions/animations disabled (`transition-property: none !important`)
- All box-shadows removed (`box-shadow: none !important`)
- Body defaults: background, color, font-family from tokens

### External Resources
- Google Fonts imported: Inter (400–900), JetBrains Mono (400–700)
- Material Symbols Outlined icon font imported with default variation settings

### Custom Scrollbar
- 4px width, primary-colored thumb, background-colored track
- Firefox support via `scrollbar-width: thin` and `scrollbar-color`

## Verification
- **Build:** Clean pass, 10 pages generated, no warnings
- **Tests:** All 1004 tests pass across 34 test files
- **Dark mode:** `@custom-variant dark` preserved and works with new tokens

## Files Modified
- `frontend/src/styles/global.css` — Complete rewrite with design system foundation
