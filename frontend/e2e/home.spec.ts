/**
 * E2E tests for the admin dashboard home page.
 *
 * Basic smoke tests to verify the dashboard loads correctly.
 */
import { test, expect } from './fixtures';

test.describe('Admin Dashboard Home', () => {
  test('renders the dashboard heading', async ({ adminPage: page }) => {
    await page.goto('/_/');
    await expect(page.getByRole('heading', { name: 'Overview' })).toBeVisible();
  });

  test('has correct page title', async ({ adminPage: page }) => {
    await page.goto('/_/');
    await expect(page).toHaveTitle(/Zerobase/);
  });

  test('shows the sidebar branding', async ({ adminPage: page }) => {
    await page.goto('/_/');
    await expect(page.getByText('Zerobase').first()).toBeVisible();
  });

  test('has skip-to-content link for accessibility', async ({ adminPage: page }) => {
    await page.goto('/_/');
    const skipLink = page.getByText('Skip to main content');
    // Skip link is sr-only by default, but should exist in the DOM
    await expect(skipLink).toBeAttached();
  });

  test('main content area has correct landmark', async ({ adminPage: page }) => {
    await page.goto('/_/');
    const main = page.locator('main#main-content');
    await expect(main).toBeVisible();
  });
});
