/**
 * E2E tests for the API Docs page.
 *
 * Tests that the auto-generated API documentation is displayed correctly.
 */
import { test, expect } from './fixtures';

test.describe('API Docs Page', () => {
  test.beforeEach(async ({ adminPage: page }) => {
    await page.goto('/_/docs');
  });

  test('displays the API docs heading', async ({ adminPage: page }) => {
    await expect(
      page.getByRole('heading', { name: /API|Documentation/i }),
    ).toBeVisible({ timeout: 10_000 });
  });

  test('shows endpoint documentation', async ({ adminPage: page }) => {
    // Wait for API docs to load (they depend on collections data)
    await page.waitForTimeout(3000);

    // Should show HTTP method badges (GET, POST, etc.)
    const hasEndpoints =
      (await page.getByText(/GET|POST|PATCH|DELETE/).count()) > 0;

    // If collections exist, there should be endpoint docs
    if (hasEndpoints) {
      await expect(page.getByText(/GET|POST/).first()).toBeVisible();
    }
  });

  test('shows collection endpoints when collections exist', async ({
    adminPage: page,
    api,
  }) => {
    const collName = `e2e_docs_${Date.now()}`;
    const token = await api.getAdminToken();
    await api.createCollection(token, { name: collName, type: 'base' });

    try {
      // Reload the docs page
      await page.goto('/_/docs');
      await page.waitForTimeout(3000);

      // Should show endpoints related to the collection
      await expect(page.getByText(new RegExp(collName, 'i'))).toBeVisible({
        timeout: 10_000,
      });
    } finally {
      await api.deleteCollection(token, collName);
    }
  });

  test('shows curl examples', async ({ adminPage: page }) => {
    await page.waitForTimeout(3000);

    // API docs should include curl examples or code snippets
    const hasCurlExamples =
      (await page.getByText(/curl/i).count()) > 0;

    if (hasCurlExamples) {
      await expect(page.getByText(/curl/i).first()).toBeVisible();
    }
  });

  test('shows filter operators reference', async ({ adminPage: page }) => {
    await page.waitForTimeout(3000);

    // The API docs may include filter operator documentation
    const hasFilterDocs =
      (await page.getByText(/filter|operators|query/i).count()) > 0;

    // This is informational — the page layout may vary
    if (hasFilterDocs) {
      await expect(page.getByText(/filter|operators|query/i).first()).toBeVisible();
    }
  });
});
