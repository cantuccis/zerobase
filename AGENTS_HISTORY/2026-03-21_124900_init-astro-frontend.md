# Initialize AstroJS Admin Dashboard Frontend

**Date:** 2026-03-21 12:49
**Task ID:** ypqwkao3ys5vsxk
**Phase:** 10

## Summary

Scaffolded the AstroJS project in `frontend/` for the Zerobase admin dashboard. Configured with TypeScript (strict), React for interactive components, Tailwind CSS v4, Vitest for unit testing, and Playwright for E2E testing. The build outputs static HTML/CSS/JS ready for embedding in the Rust binary.

## Key Decisions

- **Base path `/_/`**: Matches Pocketbase convention for admin UI
- **Static output mode**: Build produces `dist/` with static files for embedding via `rust-embed` or `include_dir`
- **Tailwind CSS v4**: Using `@tailwindcss/vite` plugin (no PostCSS, no `tailwind.config.ts`)
- **React 19**: For interactive island components via `client:load` / `client:idle` directives
- **Vitest + jsdom**: For fast unit/component tests with React Testing Library
- **Playwright + Chromium**: For E2E tests against the dev server

## Verification Results

- **Type check (`astro check`)**: 0 errors, 0 warnings
- **Unit tests (Vitest)**: 5/5 passing (Counter component)
- **E2E tests (Playwright)**: 2/2 passing (home page rendering)
- **Build**: Produces static output at `frontend/dist/`

## Files Created/Modified

### Created
- `frontend/` — Full AstroJS project directory
- `frontend/astro.config.mjs` — Astro config with static output, `/_/` base, React, Tailwind
- `frontend/vitest.config.ts` — Vitest config with jsdom, React JSX support
- `frontend/playwright.config.ts` — Playwright config targeting dev server
- `frontend/tsconfig.json` — TypeScript strict config with React JSX
- `frontend/src/layouts/Layout.astro` — Base layout with Tailwind global styles
- `frontend/src/pages/index.astro` — Admin dashboard home page
- `frontend/src/components/Counter.tsx` — Sample React component
- `frontend/src/components/Counter.test.tsx` — Unit tests for Counter
- `frontend/src/styles/global.css` — Tailwind v4 entry point
- `frontend/src/lib/` — Directory for shared utilities
- `frontend/tests/setup.ts` — Vitest setup with jest-dom matchers
- `frontend/e2e/home.spec.ts` — E2E tests for home page

### Modified
- `frontend/package.json` — Added test, check, and e2e scripts
- `frontend/.gitignore` — Added Playwright artifacts
