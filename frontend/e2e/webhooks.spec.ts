/**
 * E2E tests for the Webhooks page.
 *
 * Tests listing, creating, and managing webhooks through the admin dashboard.
 */
import { test, expect } from './fixtures';

test.describe('Webhooks Page', () => {
  test.beforeEach(async ({ adminPage: page }) => {
    await page.goto('/_/webhooks');
    await expect(page.getByRole('heading', { name: /Webhooks/i })).toBeVisible({
      timeout: 10_000,
    });
  });

  test('displays the webhooks heading', async ({ adminPage: page }) => {
    await expect(page.getByRole('heading', { name: /Webhooks/i })).toBeVisible();
  });

  test('shows create webhook button', async ({ adminPage: page }) => {
    await expect(
      page.getByRole('button', { name: /Create|New|Add/i }),
    ).toBeVisible();
  });

  test('shows webhook list or empty state', async ({ adminPage: page }) => {
    await page.waitForTimeout(2000);

    const hasWebhooks = (await page.locator('table tbody tr').count()) > 0;
    const hasEmptyState =
      (await page.getByText(/No webhooks|no webhooks yet|empty/i).count()) > 0;

    expect(hasWebhooks || hasEmptyState).toBeTruthy();
  });

  test('opens create webhook form', async ({ adminPage: page }) => {
    await page.getByRole('button', { name: /Create|New|Add/i }).first().click();

    // Should show the webhook form (either as a modal or inline)
    await expect(page.getByLabel(/URL/i).first()).toBeVisible({ timeout: 5_000 });
  });

  test('create webhook form has event type selectors', async ({
    adminPage: page,
  }) => {
    await page.getByRole('button', { name: /Create|New|Add/i }).first().click();

    // Should show event type options (create, update, delete)
    await expect(page.getByText(/Create|Update|Delete/i).first()).toBeVisible({
      timeout: 5_000,
    });
  });

  test('create webhook form has collection selector', async ({
    adminPage: page,
  }) => {
    await page.getByRole('button', { name: /Create|New|Add/i }).first().click();

    // Should have a way to select which collection triggers the webhook
    await expect(
      page.getByText(/Collection|collection/i).first(),
    ).toBeVisible({ timeout: 5_000 });
  });
});
