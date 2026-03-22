import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright E2E configuration for the Zerobase admin dashboard.
 *
 * Tests run against a real Zerobase backend. The backend must be started
 * before running E2E tests — typically via `cargo run -- serve` or by
 * pointing to an already running instance.
 *
 * Environment variables:
 *   ZEROBASE_E2E_BASE_URL  — backend URL (default: http://localhost:8090)
 *   ADMIN_EMAIL             — superuser email (default: admin@test.com)
 *   ADMIN_PASSWORD          — superuser password (default: admin12345678)
 */
export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: 1, // Serial — tests share backend state
  reporter: process.env.CI ? 'github' : 'html',
  timeout: 30_000,

  use: {
    baseURL: process.env.ZEROBASE_E2E_BASE_URL ?? 'http://localhost:8090',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },

  projects: [
    // Global setup: authenticates once and saves storage state
    {
      name: 'setup',
      testMatch: /global-setup\.ts/,
    },
    {
      name: 'chromium',
      use: {
        ...devices['Desktop Chrome'],
        storageState: 'e2e/.auth/admin.json',
      },
      dependencies: ['setup'],
    },
    {
      name: 'firefox',
      use: {
        ...devices['Desktop Firefox'],
        storageState: 'e2e/.auth/admin.json',
      },
      dependencies: ['setup'],
    },
    {
      name: 'webkit',
      use: {
        ...devices['Desktop Safari'],
        storageState: 'e2e/.auth/admin.json',
      },
      dependencies: ['setup'],
    },
    {
      name: 'mobile-chrome',
      use: {
        ...devices['Pixel 5'],
        storageState: 'e2e/.auth/admin.json',
      },
      dependencies: ['setup'],
    },
    {
      name: 'mobile-safari',
      use: {
        ...devices['iPhone 13'],
        storageState: 'e2e/.auth/admin.json',
      },
      dependencies: ['setup'],
    },
  ],
});
