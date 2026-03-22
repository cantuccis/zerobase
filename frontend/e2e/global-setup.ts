/**
 * Global setup: authenticates as a superuser and saves browser storage state
 * so that all subsequent test files start already logged in.
 */
import { test as setup, expect } from '@playwright/test';

const ADMIN_EMAIL = process.env.ADMIN_EMAIL ?? 'admin@test.com';
const ADMIN_PASSWORD = process.env.ADMIN_PASSWORD ?? 'admin12345678';

setup('authenticate as admin', async ({ page }) => {
  // Navigate to the login page
  await page.goto('/_/login');

  // Fill in credentials
  await page.getByLabel('Email').fill(ADMIN_EMAIL);
  await page.getByLabel('Password').fill(ADMIN_PASSWORD);

  // Submit the form
  await page.getByRole('button', { name: 'Sign In' }).click();

  // Wait until we're redirected to the dashboard
  await expect(page).toHaveURL(/\/_\/?$/);
  await expect(page.getByText('Overview')).toBeVisible({ timeout: 10_000 });

  // Save signed-in state for reuse by all test files
  await page.context().storageState({ path: 'e2e/.auth/admin.json' });
});
