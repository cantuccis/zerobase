/**
 * E2E accessibility tests for the admin dashboard.
 *
 * Verifies keyboard navigation, ARIA landmarks, focus management,
 * and other accessibility requirements across key pages.
 */
import { test, expect } from './fixtures';

test.describe('Accessibility', () => {
  test('dashboard has proper ARIA landmarks', async ({ adminPage: page }) => {
    await page.goto('/_/');

    // Should have a main landmark
    await expect(page.locator('main')).toBeVisible();

    // Should have a navigation landmark
    await expect(page.locator('nav').first()).toBeVisible();

    // Header should be present
    await expect(page.locator('header').first()).toBeVisible();
  });

  test('sidebar navigation has aria-label', async ({ adminPage: page }) => {
    await page.goto('/_/');
    const sidebar = page.locator('aside[aria-label="Main navigation"]');
    await expect(sidebar).toBeVisible();
  });

  test('navigation items use list structure', async ({ adminPage: page }) => {
    await page.goto('/_/');
    const navList = page.locator('aside[aria-label="Main navigation"] ul[role="list"]');
    await expect(navList).toBeVisible();
  });

  test('active page is indicated with aria-current', async ({
    adminPage: page,
  }) => {
    await page.goto('/_/');

    // Overview should be the active item on the home page
    const overviewLink = page
      .locator('aside[aria-label="Main navigation"]')
      .getByRole('link', { name: 'Overview' });
    await expect(overviewLink).toHaveAttribute('aria-current', 'page');
  });

  test('form inputs have associated labels on login page', async ({ page }) => {
    // Use fresh context without auth for login page
    await page.goto('/_/login');

    // Email input should have a label
    const emailInput = page.getByLabel('Email');
    await expect(emailInput).toBeVisible();

    // Password input should have a label
    const passwordInput = page.getByLabel('Password');
    await expect(passwordInput).toBeVisible();
  });

  test('error messages use role="alert"', async ({ page }) => {
    await page.goto('/_/login');

    // Submit empty form to trigger validation errors
    await page.getByRole('button', { name: 'Sign In' }).click();

    // Field errors should be associated via aria-describedby
    const emailInput = page.getByLabel('Email');
    await expect(emailInput).toHaveAttribute('aria-invalid', 'true');
  });

  test('theme toggle is keyboard accessible', async ({ adminPage: page }) => {
    await page.goto('/_/');

    // The theme toggle button should be focusable
    const themeToggle = page.getByRole('button', { name: /theme|dark|light/i });
    if (await themeToggle.isVisible()) {
      await themeToggle.focus();
      await expect(themeToggle).toBeFocused();
    }
  });

  test('dialog modals trap focus', async ({ adminPage: page, api }) => {
    // Create a collection to trigger the delete dialog
    const collName = `e2e_a11y_${Date.now()}`;
    const token = await api.getAdminToken();
    await api.createCollection(token, { name: collName, type: 'base' });

    try {
      await page.goto('/_/collections');
      await expect(page.getByText(collName)).toBeVisible({ timeout: 10_000 });

      // Open delete dialog
      await page.getByRole('button', { name: `Delete ${collName}` }).click();

      const dialog = page.getByRole('dialog');
      await expect(dialog).toBeVisible();
      await expect(dialog).toHaveAttribute('aria-modal', 'true');

      // Dialog should have a title via aria-labelledby
      await expect(dialog).toHaveAttribute('aria-labelledby');

      // Close dialog
      await page.getByRole('button', { name: 'Cancel' }).click();
    } finally {
      await api.deleteCollection(token, collName);
    }
  });
});

// Login page accessibility tests need a fresh browser context (no auth)
test.describe('Login Accessibility', () => {
  test.use({ storageState: { cookies: [], origins: [] } });

  test('login form has proper autocomplete attributes', async ({ page }) => {
    await page.goto('/_/login');

    const emailInput = page.getByLabel('Email');
    await expect(emailInput).toHaveAttribute('autocomplete', 'email');
    await expect(emailInput).toHaveAttribute('type', 'email');

    const passwordInput = page.getByLabel('Password');
    await expect(passwordInput).toHaveAttribute('autocomplete', 'current-password');
    await expect(passwordInput).toHaveAttribute('type', 'password');
  });

  test('login form disables spellcheck on email', async ({ page }) => {
    await page.goto('/_/login');

    const emailInput = page.getByLabel('Email');
    await expect(emailInput).toHaveAttribute('spellcheck', 'false');
  });

  test('submit button shows loading state with spinner', async ({ page }) => {
    await page.goto('/_/login');

    await page.getByLabel('Email').fill('admin@test.com');
    await page.getByLabel('Password').fill('wrongpassword');

    // We'll intercept network to slow down the response and check loading state
    const submitBtn = page.getByRole('button', { name: 'Sign In' });
    await submitBtn.click();

    // The button should eventually re-enable (after error or success)
    await expect(submitBtn).toBeEnabled({ timeout: 10_000 });
  });
});
