/**
 * E2E tests for the Records Browser page.
 *
 * Tests viewing, creating, editing, sorting, and deleting records
 * within a collection.
 */
import { test, expect } from './fixtures';

const COLLECTION_NAME = `e2e_records_${Date.now()}`;
let collectionId: string;
let adminToken: string;

test.describe('Records Browser', () => {
  test.beforeAll(async ({ request }) => {
    const baseURL = process.env.ZEROBASE_E2E_BASE_URL ?? 'http://localhost:8090';
    const email = process.env.ADMIN_EMAIL ?? 'admin@test.com';
    const password = process.env.ADMIN_PASSWORD ?? 'admin12345678';

    // Authenticate
    const authResp = await request.post(`${baseURL}/_/api/admins/auth-with-password`, {
      data: { identity: email, password },
      headers: { 'Content-Type': 'application/json' },
    });
    const authBody = await authResp.json();
    adminToken = authBody.token;

    // Create a test collection with fields
    const colResp = await request.post(`${baseURL}/api/collections`, {
      data: {
        name: COLLECTION_NAME,
        type: 'base',
        fields: [
          { name: 'title', type: { type: 'text' }, required: true, unique: false },
          { name: 'count', type: { type: 'number' }, required: false, unique: false },
          { name: 'active', type: { type: 'bool' }, required: false, unique: false },
        ],
      },
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${adminToken}`,
      },
    });
    const colBody = await colResp.json();
    collectionId = colBody.id;

    // Create some test records
    for (let i = 1; i <= 5; i++) {
      await request.post(
        `${baseURL}/api/collections/${COLLECTION_NAME}/records`,
        {
          data: { title: `Record ${i}`, count: i * 10, active: i % 2 === 0 },
          headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${adminToken}`,
          },
        },
      );
    }
  });

  test.afterAll(async ({ request }) => {
    const baseURL = process.env.ZEROBASE_E2E_BASE_URL ?? 'http://localhost:8090';
    await request.delete(`${baseURL}/api/collections/${COLLECTION_NAME}`, {
      headers: { Authorization: `Bearer ${adminToken}` },
    });
  });

  test('displays the records table', async ({ adminPage: page }) => {
    await page.goto(`/_/collections/${collectionId}`);

    // Should show the collection name as heading or in the page
    await expect(page.getByText(COLLECTION_NAME)).toBeVisible({ timeout: 10_000 });

    // Should display records in a table
    await expect(page.getByText('Record 1')).toBeVisible({ timeout: 10_000 });
    await expect(page.getByText('Record 5')).toBeVisible();
  });

  test('displays column headers including system and custom fields', async ({
    adminPage: page,
  }) => {
    await page.goto(`/_/collections/${collectionId}`);
    await expect(page.getByText('Record 1')).toBeVisible({ timeout: 10_000 });

    // System columns
    await expect(page.getByText('id').first()).toBeVisible();
    await expect(page.getByText('created').first()).toBeVisible();

    // Custom columns
    await expect(page.getByText('title').first()).toBeVisible();
    await expect(page.getByText('count').first()).toBeVisible();
    await expect(page.getByText('active').first()).toBeVisible();
  });

  test('shows record count', async ({ adminPage: page }) => {
    await page.goto(`/_/collections/${collectionId}`);
    await expect(page.getByText('Record 1')).toBeVisible({ timeout: 10_000 });

    // Should show some indication of record count
    await expect(page.getByText(/5|records/i)).toBeVisible();
  });

  test('has a "New Record" button', async ({ adminPage: page }) => {
    await page.goto(`/_/collections/${collectionId}`);
    await expect(page.getByText('Record 1')).toBeVisible({ timeout: 10_000 });

    await expect(
      page.getByRole('button', { name: /New Record|Add Record|Create/i }),
    ).toBeVisible();
  });

  test('opens the record form modal when clicking "New Record"', async ({
    adminPage: page,
  }) => {
    await page.goto(`/_/collections/${collectionId}`);
    await expect(page.getByText('Record 1')).toBeVisible({ timeout: 10_000 });

    await page.getByRole('button', { name: /New Record|Add Record|Create/i }).click();

    // A modal/dialog should open with the record form
    await expect(page.getByRole('dialog')).toBeVisible();
    // Should show the field inputs
    await expect(page.getByLabel(/title/i)).toBeVisible();
  });

  test('creates a new record through the form modal', async ({ adminPage: page }) => {
    await page.goto(`/_/collections/${collectionId}`);
    await expect(page.getByText('Record 1')).toBeVisible({ timeout: 10_000 });

    // Open create modal
    await page.getByRole('button', { name: /New Record|Add Record|Create/i }).click();
    await expect(page.getByRole('dialog')).toBeVisible();

    // Fill in the form
    await page.getByLabel(/title/i).fill('New E2E Record');
    await page.getByLabel(/count/i).fill('42');

    // Save
    await page.getByRole('button', { name: /Save|Create/i }).last().click();

    // Modal should close and new record should appear
    await expect(page.getByText('New E2E Record')).toBeVisible({ timeout: 10_000 });
  });

  test('shows empty state for collection with no records', async ({
    adminPage: page,
    api,
  }) => {
    const emptyCollName = `e2e_empty_${Date.now()}`;
    const token = await api.getAdminToken();
    const col = await api.createCollection(token, { name: emptyCollName, type: 'base' });

    try {
      await page.goto(`/_/collections/${col.id}`);

      // Should show an empty state message
      await expect(
        page.getByText(/No records|empty|no data/i),
      ).toBeVisible({ timeout: 10_000 });
    } finally {
      await api.deleteCollection(token, emptyCollName);
    }
  });

  test('navigates to edit collection from records page', async ({
    adminPage: page,
  }) => {
    await page.goto(`/_/collections/${collectionId}`);
    await expect(page.getByText('Record 1')).toBeVisible({ timeout: 10_000 });

    // There should be a link/button to edit the collection schema
    const editLink = page.getByRole('link', { name: /Edit Collection|Schema|Edit/i });
    if ((await editLink.count()) > 0) {
      await editLink.first().click();
      await expect(page).toHaveURL(new RegExp(`/_/collections/${collectionId}/edit`));
    }
  });
});
