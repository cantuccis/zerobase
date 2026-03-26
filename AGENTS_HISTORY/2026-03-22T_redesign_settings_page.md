# Redesign SettingsPage to Monolith Style

**Date:** 2026-03-22
**Task ID:** ng26wr5pd2z3dru

## Summary

Restyled the SettingsPage component to match the `settings_config_monolith` design reference. The page now follows the "Architectural Monolith" design system with editorial typography, numbered sections, and a 4/8 column grid layout.

## Key Changes

- **Page header**: Large "CONFIGURATION" display heading with black underline bar
- **Section layout**: 12-column grid with 4-col labels/descriptions and 8-col form inputs
- **Numbered sections**: 01. General (app name/URL), 02. Mail Settings (SMTP), 03. File Storage (S3), 04. Test Email
- **Input styling**: 1px black border, 0px radius, focus state = 2px border with padding compensation
- **Toggle switches**: Custom monolith toggles with black checked state (no blue accents)
- **Section dividers**: 1px horizontal lines with generous 96px spacing between sections
- **Labels**: Uppercase, 12px, bold, wide tracking (label-md utility class)
- **Save buttons**: Large black background, white text, wide tracking, uppercase
- **Alert boxes**: 1px black border, no rounded corners, no colored backgrounds
- **Dark mode**: All colors use CSS custom properties that auto-invert in dark mode
- **Status badges**: Monolith-styled (surface container background or inverted black/white)
- **Responsive**: Falls back to single column on mobile (lg:grid-cols-12)

## Extracted Sub-Components

- `MonolithInput` - Reusable input with monolith styling and error handling
- `MonolithToggle` - Custom toggle switch with black checked state
- `MonolithAlert` - Alert/message box with monolith border styling
- `FieldLabel` - Uppercase label with optional required indicator
- `SectionDivider` - Horizontal rule with proper opacity
- `Spinner` - Loading spinner (extracted for reuse)

## Files Modified

- `frontend/src/components/pages/SettingsPage.tsx` - Complete restyle
- `frontend/src/components/pages/SettingsPage.test.tsx` - Updated heading text references to match new section names

## Test Results

All 42 tests passing after updating heading references in tests.
