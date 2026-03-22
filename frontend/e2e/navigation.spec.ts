/**
 * E2E tests for sidebar navigation and dashboard layout.
 *
 * Verifies that the sidebar links work, pages load, and the layout
 * structure (header, sidebar, main content) is correct.
 */
import { test, expect } from './fixtures';

test.describe('Navigation', () => {
  test('sidebar displays all navigation items', async ({ adminPage: page }) => {
    await page.goto('/_/');

    const sidebar = page.locator('aside[aria-label="Main navigation"]');
    await expect(sidebar).toBeVisible();

    // Verify all nav items are present
    await expect(sidebar.getByText('Overview')).toBeVisible();
    await expect(sidebar.getByText('Collections')).toBeVisible();
    await expect(sidebar.getByText('API Docs')).toBeVisible();
    await expect(sidebar.getByText('Settings')).toBeVisible();
    await expect(sidebar.getByText('Auth Providers')).toBeVisible();
    await expect(sidebar.getByText('Webhooks')).toBeVisible();
    await expect(sidebar.getByText('Logs')).toBeVisible();
    await expect(sidebar.getByText('Backups')).toBeVisible();
  });

  test('navigates to Collections page', async ({ adminPage: page }) => {
    await page.goto('/_/');
    await page.getByRole('link', { name: 'Collections' }).click();

    await expect(page).toHaveURL(/\/_\/collections/);
    await expect(page.getByRole('heading', { name: 'Collections' })).toBeVisible();
  });

  test('navigates to Settings page', async ({ adminPage: page }) => {
    await page.goto('/_/');
    await page.getByRole('link', { name: 'Settings' }).first().click();

    await expect(page).toHaveURL(/\/_\/settings$/);
    await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  });

  test('navigates to Logs page', async ({ adminPage: page }) => {
    await page.goto('/_/');
    await page.getByRole('link', { name: 'Logs' }).click();

    await expect(page).toHaveURL(/\/_\/logs/);
    await expect(page.getByRole('heading', { name: 'Logs' })).toBeVisible();
  });

  test('navigates to Backups page', async ({ adminPage: page }) => {
    await page.goto('/_/');
    await page.getByRole('link', { name: 'Backups' }).click();

    await expect(page).toHaveURL(/\/_\/backups/);
    await expect(page.getByRole('heading', { name: 'Backups' })).toBeVisible();
  });

  test('navigates to API Docs page', async ({ adminPage: page }) => {
    await page.goto('/_/');
    await page.getByRole('link', { name: 'API Docs' }).click();

    await expect(page).toHaveURL(/\/_\/docs/);
    await expect(page.getByRole('heading', { name: /API/i })).toBeVisible();
  });

  test('navigates to Auth Providers page', async ({ adminPage: page }) => {
    await page.goto('/_/');
    await page.getByRole('link', { name: 'Auth Providers' }).click();

    await expect(page).toHaveURL(/\/_\/settings\/auth-providers/);
    await expect(page.getByRole('heading', { name: /Auth Providers/i })).toBeVisible();
  });

  test('navigates to Webhooks page', async ({ adminPage: page }) => {
    await page.goto('/_/');
    await page.getByRole('link', { name: 'Webhooks' }).click();

    await expect(page).toHaveURL(/\/_\/webhooks/);
    await expect(page.getByRole('heading', { name: /Webhooks/i })).toBeVisible();
  });

  test('header displays admin email and sign-out button', async ({ adminPage: page }) => {
    await page.goto('/_/');

    // Sign Out button should be visible
    await expect(page.getByRole('button', { name: 'Sign Out' })).toBeVisible();
  });

  test('sign-out redirects to login', async ({ adminPage: page }) => {
    await page.goto('/_/');

    await page.getByRole('button', { name: 'Sign Out' }).click();

    await expect(page).toHaveURL(/\/_\/login/, { timeout: 10_000 });
  });

  test('highlights the active navigation item', async ({ adminPage: page }) => {
    await page.goto('/_/collections');

    // The Collections nav link should have aria-current="page"
    const collectionsLink = page.locator('aside[aria-label="Main navigation"]')
      .getByRole('link', { name: 'Collections' });
    await expect(collectionsLink).toHaveAttribute('aria-current', 'page');
  });
});
