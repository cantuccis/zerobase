# Redesign ApiDocsPage to Monolith Design System

**Date:** 2026-03-22
**Task ID:** vcd7ewg9cn8o6ru

## Summary

Restyled `ApiDocsPage.tsx` to match the "Architectural Monolith" design system. Applied zero-radius, 1px black borders, editorial typography, JetBrains Mono code blocks, and dark mode token inversion throughout all sub-components.

## Changes Made

### Method Badges
- **GET**: Black outline on white/transparent background, monospace uppercase
- **POST**: Black fill with white text, monospace uppercase
- **PATCH**: Subtle surface fill with black border, monospace uppercase
- **DELETE**: Error-colored border with error text, monospace uppercase

### Code Blocks
- Replaced dark bg (`bg-gray-900`) with light bordered containers (`border border-primary bg-surface-container-low`)
- Font set to `font-mono` (JetBrains Mono)
- Copy button restyled with bordered monolith aesthetic

### Tables (Fields, Query Params, Filter Operators)
- Black header row (`bg-primary text-on-primary`)
- Column dividers via `border-r`
- Alternating row striping with `bg-surface-container-low`
- All borders use design token colors

### Typography
- Page heading: `text-display-lg` editorial display
- Section headings: `text-label-md` uppercase
- Collection name: `text-headline-lg`
- Body text: `text-body-lg` with `text-on-surface-variant`

### Navigation/Sidebar
- Collection selector: active item uses inverted `bg-primary text-on-primary font-bold`
- Inactive items: clean text with hover state
- Type labels shown as uppercase text (no colored pills)

### Section Dividers
- 1px horizontal black lines (`border-t border-primary`)

### Dark Mode
- All colors use paired light/dark tokens (`text-on-surface dark:text-on-surface`)
- Error states use `bg-error-container dark:bg-on-error`
- Surfaces auto-invert via CSS custom properties

## Files Modified

- `frontend/src/components/pages/ApiDocsPage.tsx`
