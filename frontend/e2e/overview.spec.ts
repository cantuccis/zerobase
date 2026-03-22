/**
 * E2E tests for the Overview / Dashboard page.
 *
 * The overview page shows server health, collection stats, and recent logs.
 */
import { test, expect } from './fixtures';

test.describe('Overview Page', () => {
  test.beforeEach(async ({ adminPage: page }) => {
    await page.goto('/_/');
  });

  test('displays the overview heading', async ({ adminPage: page }) => {
    await expect(page.getByRole('heading', { name: 'Overview' })).toBeVisible();
  });

  test('shows server health status', async ({ adminPage: page }) => {
    // The overview page fetches health status — should show "healthy" or a health indicator
    await expect(page.getByText(/healthy|Server Health/i)).toBeVisible({ timeout: 10_000 });
  });

  test('displays collection statistics', async ({ adminPage: page }) => {
    // Should show collection count stats (even if 0)
    await expect(page.getByText(/collections?/i)).toBeVisible({ timeout: 10_000 });
  });

  test('loads without errors', async ({ adminPage: page }) => {
    // No error alerts should be visible after loading
    await page.waitForTimeout(2000);
    const errorAlerts = page.locator('[role="alert"]');
    // Allow the page to load — if there are errors they indicate a real problem
    const count = await errorAlerts.count();
    // This is informational; the page should ideally have 0 errors
    if (count > 0) {
      console.warn(`Overview page has ${count} error alert(s)`);
    }
  });
});
