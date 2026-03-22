/**
 * E2E tests for Collection CRUD operations.
 *
 * Tests creating, viewing, editing, searching, and deleting collections
 * through the admin dashboard UI.
 */
import { test, expect } from './fixtures';

// Unique names to avoid conflicts between test runs
const TEST_COLLECTION = `e2e_test_${Date.now()}`;
const TEST_COLLECTION_AUTH = `e2e_auth_${Date.now()}`;

test.describe('Collections Page', () => {
  test('displays the collections list', async ({ adminPage: page }) => {
    await page.goto('/_/collections');

    await expect(page.getByRole('heading', { name: 'Collections' })).toBeVisible();
    // Should show either a list or the empty state
    const hasTable = await page.locator('table').count() > 0;
    const hasEmptyState = await page.getByText(/No collections|Get started/i).count() > 0;
    expect(hasTable || hasEmptyState).toBeTruthy();
  });

  test('has a "New Collection" button', async ({ adminPage: page }) => {
    await page.goto('/_/collections');

    await expect(page.getByRole('link', { name: /New Collection/i })).toBeVisible();
  });

  test('search input filters collections', async ({ adminPage: page, api }) => {
    // Setup: create a collection via API
    const token = await api.getAdminToken();
    await api.createCollection(token, { name: TEST_COLLECTION, type: 'base' });

    try {
      await page.goto('/_/collections');

      // Wait for the collection to appear
      await expect(page.getByText(TEST_COLLECTION)).toBeVisible({ timeout: 10_000 });

      // Search for the collection
      await page.getByPlaceholder('Search collections').fill(TEST_COLLECTION);
      await expect(page.getByText(TEST_COLLECTION)).toBeVisible();

      // Search for something that doesn't exist
      await page.getByPlaceholder('Search collections').fill('nonexistent_xyz');
      await expect(page.getByText(/No collections match/i)).toBeVisible();

      // Clear search
      await page.getByPlaceholder('Search collections').fill('');
      await expect(page.getByText(TEST_COLLECTION)).toBeVisible();
    } finally {
      await api.deleteCollection(token, TEST_COLLECTION);
    }
  });
});

test.describe('Collection Creation', () => {
  test('creates a new base collection', async ({ adminPage: page, api }) => {
    const collectionName = `e2e_create_${Date.now()}`;
    const token = await api.getAdminToken();

    try {
      await page.goto('/_/collections/new');

      // Should show the collection editor in create mode
      await expect(page.getByRole('heading', { name: /New Collection|Create Collection/i })).toBeVisible();

      // Fill in collection name
      const nameInput = page.getByLabel(/Collection name|Name/i).first();
      await nameInput.fill(collectionName);

      // Type should default to "Base" — verify it's selected
      await expect(page.getByText('Base').first()).toBeVisible();

      // Add a field
      const addFieldBtn = page.getByRole('button', { name: /Add Field/i });
      await addFieldBtn.click();

      // Fill in the field name
      const fieldNameInputs = page.getByPlaceholder(/field name/i);
      const lastFieldInput = fieldNameInputs.last();
      await lastFieldInput.fill('title');

      // Save the collection
      const saveBtn = page.getByRole('button', { name: /Save|Create/i }).first();
      await saveBtn.click();

      // Should redirect to the collections list or show success
      await page.waitForURL(/\/_\/collections/, { timeout: 10_000 });

      // Navigate to collections and verify it appears
      await page.goto('/_/collections');
      await expect(page.getByText(collectionName)).toBeVisible({ timeout: 10_000 });
    } finally {
      await api.deleteCollection(token, collectionName);
    }
  });

  test('creates an auth collection', async ({ adminPage: page, api }) => {
    const token = await api.getAdminToken();

    try {
      await page.goto('/_/collections/new');

      // Fill in collection name
      const nameInput = page.getByLabel(/Collection name|Name/i).first();
      await nameInput.fill(TEST_COLLECTION_AUTH);

      // Select Auth type
      const authOption = page.getByText('Auth').first();
      await authOption.click();

      // Should show auth-specific fields info
      await expect(page.getByText(/email|password|verified/i)).toBeVisible();

      // Save the collection
      const saveBtn = page.getByRole('button', { name: /Save|Create/i }).first();
      await saveBtn.click();

      await page.waitForURL(/\/_\/collections/, { timeout: 10_000 });
    } finally {
      await api.deleteCollection(token, TEST_COLLECTION_AUTH);
    }
  });

  test('validates collection name', async ({ adminPage: page }) => {
    await page.goto('/_/collections/new');

    // Try to save without a name
    const saveBtn = page.getByRole('button', { name: /Save|Create/i }).first();
    await saveBtn.click();

    // Should show validation error
    await expect(page.getByText(/name is required|required/i)).toBeVisible();
  });
});

test.describe('Collection Deletion', () => {
  test('deletes a collection through the delete dialog', async ({ adminPage: page, api }) => {
    const collectionName = `e2e_delete_${Date.now()}`;
    const token = await api.getAdminToken();

    // Create a collection to delete
    await api.createCollection(token, { name: collectionName, type: 'base' });

    await page.goto('/_/collections');

    // Wait for the collection to appear
    await expect(page.getByText(collectionName)).toBeVisible({ timeout: 10_000 });

    // Click the Delete button for this collection
    const deleteBtn = page.getByRole('button', { name: `Delete ${collectionName}` });
    await deleteBtn.click();

    // The delete confirmation dialog should appear
    await expect(page.getByRole('dialog')).toBeVisible();
    await expect(page.getByText('Delete Collection')).toBeVisible();
    await expect(page.getByText(collectionName)).toBeVisible();

    // Confirm deletion
    const confirmBtn = page.getByTestId('confirm-delete-btn');
    await confirmBtn.click();

    // The collection should be removed from the list
    await expect(page.getByText(collectionName)).not.toBeVisible({ timeout: 10_000 });

    // Should show success message
    await expect(page.getByTestId('success-message')).toBeVisible();
  });

  test('cancel button closes the delete dialog without deleting', async ({ adminPage: page, api }) => {
    const collectionName = `e2e_cancel_del_${Date.now()}`;
    const token = await api.getAdminToken();

    await api.createCollection(token, { name: collectionName, type: 'base' });

    try {
      await page.goto('/_/collections');
      await expect(page.getByText(collectionName)).toBeVisible({ timeout: 10_000 });

      // Open delete dialog
      await page.getByRole('button', { name: `Delete ${collectionName}` }).click();
      await expect(page.getByRole('dialog')).toBeVisible();

      // Cancel
      await page.getByRole('button', { name: 'Cancel' }).click();

      // Dialog should close, collection should still be there
      await expect(page.getByRole('dialog')).not.toBeVisible();
      await expect(page.getByText(collectionName)).toBeVisible();
    } finally {
      await api.deleteCollection(token, collectionName);
    }
  });
});
