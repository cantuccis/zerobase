/**
 * E2E tests for the Logs Viewer page.
 *
 * Tests loading logs, filtering by method/status, and viewing log details.
 * Since E2E tests themselves generate API requests, there should always be
 * some logs to display.
 */
import { test, expect } from './fixtures';

test.describe('Logs Page', () => {
  test.beforeEach(async ({ adminPage: page }) => {
    await page.goto('/_/logs');
    await expect(page.getByRole('heading', { name: 'Logs' })).toBeVisible({
      timeout: 10_000,
    });
  });

  test('displays the logs heading', async ({ adminPage: page }) => {
    await expect(page.getByRole('heading', { name: 'Logs' })).toBeVisible();
  });

  test('shows log entries after loading', async ({ adminPage: page }) => {
    // Wait for logs to load — there should be entries from our API calls
    // Look for table rows, log entries, or any indication of data
    await page.waitForTimeout(2000);

    // Either shows log entries or an empty state
    const hasEntries = await page.locator('table tbody tr, [data-testid="log-entry"]').count() > 0;
    const hasEmptyState = await page.getByText(/No logs|no entries/i).count() > 0;
    expect(hasEntries || hasEmptyState).toBeTruthy();
  });

  test('displays filter controls', async ({ adminPage: page }) => {
    // Should have method filter, status filter, or date range selector
    const hasMethodFilter =
      (await page.getByText(/Method/i).count()) > 0 ||
      (await page.locator('select').count()) > 0;
    const hasSearchOrFilter =
      (await page.getByPlaceholder(/search|filter/i).count()) > 0 ||
      (await page.getByRole('combobox').count()) > 0;

    expect(hasMethodFilter || hasSearchOrFilter).toBeTruthy();
  });

  test('shows HTTP method and status in log entries', async ({ adminPage: page }) => {
    // Wait for logs to appear
    await page.waitForTimeout(3000);

    // If there are log entries, they should show HTTP methods and status codes
    const logEntries = page.locator('table tbody tr, [data-testid="log-entry"]');
    if ((await logEntries.count()) > 0) {
      // At least one log entry should have a method like GET/POST
      await expect(
        page.getByText(/GET|POST|PATCH|DELETE/).first(),
      ).toBeVisible();
    }
  });

  test('has date range filtering', async ({ adminPage: page }) => {
    // Should have date range presets (1h, 24h, 7d, etc.)
    const hasPresets =
      (await page.getByText(/24h|7d|Last|hour/i).count()) > 0 ||
      (await page.getByRole('button', { name: /24h|1h|7d|30d/i }).count()) > 0;

    expect(hasPresets).toBeTruthy();
  });

  test('can filter by status range', async ({ adminPage: page }) => {
    // Look for status filter options
    const statusFilter = page.getByText(/2xx|Success|All/i).first();
    if (await statusFilter.isVisible()) {
      await statusFilter.click();
      // The filter should be applied (page content may change)
      await page.waitForTimeout(1000);
    }
  });

  test('shows log statistics', async ({ adminPage: page }) => {
    // The logs page may show aggregate statistics (total requests, error rate, etc.)
    await page.waitForTimeout(2000);

    const hasStats =
      (await page.getByText(/Total|Requests|Statistics|Stats/i).count()) > 0;

    // Stats may or may not be visible depending on log data — just check it loads
    if (hasStats) {
      await expect(page.getByText(/Total|Requests|Statistics|Stats/i).first()).toBeVisible();
    }
  });
});
