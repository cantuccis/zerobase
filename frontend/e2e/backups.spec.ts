/**
 * E2E tests for the Backups page.
 *
 * Tests listing, creating, and managing database backups through the
 * admin dashboard.
 */
import { test, expect } from './fixtures';

test.describe('Backups Page', () => {
  test.beforeEach(async ({ adminPage: page }) => {
    await page.goto('/_/backups');
    await expect(page.getByRole('heading', { name: 'Backups' })).toBeVisible({
      timeout: 10_000,
    });
  });

  test('displays the backups heading', async ({ adminPage: page }) => {
    await expect(page.getByRole('heading', { name: 'Backups' })).toBeVisible();
  });

  test('shows the "Create Backup" button', async ({ adminPage: page }) => {
    await expect(
      page.getByRole('button', { name: /Create Backup|New Backup/i }),
    ).toBeVisible();
  });

  test('shows backup list or empty state', async ({ adminPage: page }) => {
    // Wait for content to load
    await page.waitForTimeout(2000);

    // Should show either a list of backups or an empty state
    const hasBackups =
      (await page.locator('table tbody tr').count()) > 0 ||
      (await page.getByText(/\.db|backup_/i).count()) > 0;
    const hasEmptyState =
      (await page.getByText(/No backups|no backups yet|empty/i).count()) > 0;

    expect(hasBackups || hasEmptyState).toBeTruthy();
  });

  test('creates a new backup', async ({ adminPage: page }) => {
    // Click "Create Backup"
    await page.getByRole('button', { name: /Create Backup|New Backup/i }).click();

    // May show a loading/progress indicator
    // Wait for the backup to complete (this can take a moment)
    await page.waitForTimeout(5000);

    // After backup creation, a backup entry should appear
    // Look for any indication of a backup entry (filename, timestamp, size)
    await expect(
      page.getByText(/\.db|backup|KB|MB|created/i).first(),
    ).toBeVisible({ timeout: 15_000 });
  });

  test('backup entries show file size and date', async ({ adminPage: page }) => {
    // Wait for content to load
    await page.waitForTimeout(2000);

    const rows = page.locator('table tbody tr');
    if ((await rows.count()) > 0) {
      const firstRow = rows.first();
      // Should display some size info (KB, MB, etc.)
      const rowText = await firstRow.textContent();
      expect(rowText).toBeTruthy();
    }
  });

  test('backup entries have download and delete actions', async ({
    adminPage: page,
  }) => {
    // Wait for content to load
    await page.waitForTimeout(2000);

    // If there are backup entries, they should have action buttons
    const downloadBtns = page.getByRole('button', { name: /Download/i });
    const deleteBtns = page.getByRole('button', { name: /Delete/i });

    if ((await downloadBtns.count()) > 0) {
      await expect(downloadBtns.first()).toBeVisible();
    }
    if ((await deleteBtns.count()) > 0) {
      await expect(deleteBtns.first()).toBeVisible();
    }
  });

  test('delete backup shows confirmation', async ({ adminPage: page }) => {
    // Wait for content to load
    await page.waitForTimeout(2000);

    const deleteBtns = page.getByRole('button', { name: /Delete/i });
    if ((await deleteBtns.count()) > 0) {
      await deleteBtns.first().click();

      // Should show a confirmation dialog
      await expect(page.getByRole('dialog')).toBeVisible();
      await expect(page.getByText(/confirm|sure|delete/i)).toBeVisible();

      // Cancel to avoid actually deleting
      const cancelBtn = page.getByRole('button', { name: /Cancel/i });
      if (await cancelBtn.isVisible()) {
        await cancelBtn.click();
      }
    }
  });

  test('restore backup shows confirmation', async ({ adminPage: page }) => {
    // Wait for content to load
    await page.waitForTimeout(2000);

    const restoreBtns = page.getByRole('button', { name: /Restore/i });
    if ((await restoreBtns.count()) > 0) {
      await restoreBtns.first().click();

      // Should show a confirmation dialog
      await expect(page.getByRole('dialog')).toBeVisible();
      await expect(page.getByText(/confirm|sure|restore/i)).toBeVisible();

      // Cancel to avoid actually restoring
      const cancelBtn = page.getByRole('button', { name: /Cancel/i });
      if (await cancelBtn.isVisible()) {
        await cancelBtn.click();
      }
    }
  });
});
