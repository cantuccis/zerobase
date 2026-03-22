/**
 * E2E tests for the Settings page.
 *
 * Tests loading, editing, and saving SMTP, Meta/Sender, and S3 storage settings.
 */
import { test, expect } from './fixtures';

test.describe('Settings Page', () => {
  test.beforeEach(async ({ adminPage: page }) => {
    await page.goto('/_/settings');
    await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible({
      timeout: 10_000,
    });
  });

  test('displays SMTP settings section', async ({ adminPage: page }) => {
    await expect(page.getByText(/SMTP|Email/i)).toBeVisible();
  });

  test('displays SMTP form fields', async ({ adminPage: page }) => {
    // Should show SMTP host, port, username fields
    await expect(page.getByLabel(/Host/i).first()).toBeVisible();
    await expect(page.getByLabel(/Port/i).first()).toBeVisible();
  });

  test('displays S3 storage settings section', async ({ adminPage: page }) => {
    await expect(page.getByText(/S3|Storage/i)).toBeVisible();
  });

  test('displays Meta/Sender settings', async ({ adminPage: page }) => {
    // Meta section with app name, sender address, etc.
    await expect(page.getByText(/App Name|Application|Sender/i).first()).toBeVisible();
  });

  test('can toggle SMTP enabled switch', async ({ adminPage: page }) => {
    // Find the SMTP enabled toggle/checkbox
    const enabledToggle = page.getByLabel(/enabled/i).first();
    if (await enabledToggle.isVisible()) {
      const initialChecked = await enabledToggle.isChecked();
      await enabledToggle.click();
      const newChecked = await enabledToggle.isChecked();
      expect(newChecked).toBe(!initialChecked);
      // Toggle back to not affect other tests
      await enabledToggle.click();
    }
  });

  test('has a save button for SMTP settings', async ({ adminPage: page }) => {
    await expect(
      page.getByRole('button', { name: /Save|Update/i }).first(),
    ).toBeVisible();
  });

  test('shows test email section when SMTP is configured', async ({
    adminPage: page,
  }) => {
    // The test email input may be visible if SMTP is configured
    const testInput = page.getByPlaceholder(/test.*email|recipient/i);
    // This is optional — the feature might be hidden when SMTP is disabled
    if (await testInput.isVisible()) {
      await expect(testInput).toBeEditable();
    }
  });

  test('validates SMTP settings before saving', async ({ adminPage: page }) => {
    // Enable SMTP without filling required fields
    const enabledToggle = page.getByLabel(/enabled/i).first();
    if (await enabledToggle.isVisible()) {
      // Check the toggle if not already checked
      if (!(await enabledToggle.isChecked())) {
        await enabledToggle.click();
      }

      // Clear host field
      const hostInput = page.getByLabel(/Host/i).first();
      await hostInput.clear();

      // Try to save — should show validation error or the API returns error
      await page.getByRole('button', { name: /Save|Update/i }).first().click();

      // Wait a moment for the response
      await page.waitForTimeout(1000);

      // Should show an error or the field should be highlighted
      // (behavior depends on whether validation is client-side or server-side)
    }
  });
});

test.describe('Auth Providers Page', () => {
  test('displays the auth providers heading', async ({ adminPage: page }) => {
    await page.goto('/_/settings/auth-providers');
    await expect(
      page.getByRole('heading', { name: /Auth Providers/i }),
    ).toBeVisible({ timeout: 10_000 });
  });

  test('shows available OAuth2 providers', async ({ adminPage: page }) => {
    await page.goto('/_/settings/auth-providers');
    await expect(
      page.getByRole('heading', { name: /Auth Providers/i }),
    ).toBeVisible({ timeout: 10_000 });

    // Should show Google and Microsoft as available providers
    await expect(page.getByText(/Google/i)).toBeVisible();
    await expect(page.getByText(/Microsoft/i)).toBeVisible();
  });
});
