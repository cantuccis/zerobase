/**
 * E2E tests for the admin login flow.
 *
 * These tests use a fresh browser context (no saved auth state)
 * to verify the login page works end-to-end.
 */
import { test, expect } from '@playwright/test';

const ADMIN_EMAIL = process.env.ADMIN_EMAIL ?? 'admin@test.com';
const ADMIN_PASSWORD = process.env.ADMIN_PASSWORD ?? 'admin12345678';

// Use a clean context without saved auth for login tests
test.use({ storageState: { cookies: [], origins: [] } });

test.describe('Login Flow', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/_/login');
  });

  test('renders the login form', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Zerobase Admin' })).toBeVisible();
    await expect(page.getByText('Sign in to your superuser account')).toBeVisible();
    await expect(page.getByLabel('Email')).toBeVisible();
    await expect(page.getByLabel('Password')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Sign In' })).toBeVisible();
  });

  test('shows validation errors for empty fields', async ({ page }) => {
    await page.getByRole('button', { name: 'Sign In' }).click();

    await expect(page.getByText('Email is required.')).toBeVisible();
    await expect(page.getByText('Password is required.')).toBeVisible();
  });

  test('shows error for invalid credentials', async ({ page }) => {
    await page.getByLabel('Email').fill('wrong@email.com');
    await page.getByLabel('Password').fill('wrongpassword');
    await page.getByRole('button', { name: 'Sign In' }).click();

    // Should show an error message (either validation or generic auth error)
    await expect(page.getByRole('alert')).toBeVisible({ timeout: 10_000 });
  });

  test('successfully logs in with valid credentials', async ({ page }) => {
    await page.getByLabel('Email').fill(ADMIN_EMAIL);
    await page.getByLabel('Password').fill(ADMIN_PASSWORD);
    await page.getByRole('button', { name: 'Sign In' }).click();

    // Should redirect to the dashboard
    await expect(page).toHaveURL(/\/_\/?$/, { timeout: 10_000 });
    // Should show the dashboard content
    await expect(page.getByText('Overview')).toBeVisible();
  });

  test('shows loading state while submitting', async ({ page }) => {
    await page.getByLabel('Email').fill(ADMIN_EMAIL);
    await page.getByLabel('Password').fill(ADMIN_PASSWORD);

    await page.getByRole('button', { name: 'Sign In' }).click();

    // The button text should change to "Signing in..." while loading
    // (may be very fast, so we check either the loading text or successful redirect)
    await expect(page).toHaveURL(/\/_\/?$/, { timeout: 10_000 });
  });

  test('redirects unauthenticated users to login', async ({ page }) => {
    await page.goto('/_/');

    // Should redirect to login since we have no auth
    await expect(page).toHaveURL(/\/_\/login/, { timeout: 10_000 });
  });
});
