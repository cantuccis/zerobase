/**
 * E2E tests for the Schema Editor (Collection Editor page).
 *
 * Tests adding fields, configuring field types, editing collection rules,
 * and the API preview panel.
 */
import { test, expect } from './fixtures';

test.describe('Schema Editor', () => {
  const COLLECTION_NAME = `e2e_schema_${Date.now()}`;
  let token: string;

  test.beforeAll(async ({ request }) => {
    const baseURL = process.env.ZEROBASE_E2E_BASE_URL ?? 'http://localhost:8090';
    const email = process.env.ADMIN_EMAIL ?? 'admin@test.com';
    const password = process.env.ADMIN_PASSWORD ?? 'admin12345678';

    const resp = await request.post(`${baseURL}/_/api/admins/auth-with-password`, {
      data: { identity: email, password },
      headers: { 'Content-Type': 'application/json' },
    });
    const body = await resp.json();
    token = body.token;

    // Create a test collection with some fields
    await request.post(`${baseURL}/api/collections`, {
      data: {
        name: COLLECTION_NAME,
        type: 'base',
        fields: [
          { name: 'title', type: { type: 'text' }, required: true, unique: false },
          { name: 'count', type: { type: 'number' }, required: false, unique: false },
        ],
      },
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${token}`,
      },
    });
  });

  test.afterAll(async ({ request }) => {
    const baseURL = process.env.ZEROBASE_E2E_BASE_URL ?? 'http://localhost:8090';
    await request.delete(`${baseURL}/api/collections/${COLLECTION_NAME}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
  });

  test('loads the schema editor in edit mode', async ({ adminPage: page, api }) => {
    // Get the collection ID
    const token = await api.getAdminToken();
    const collections = await api.listCollections(token);
    const col = collections.items.find(
      (c: { name: string }) => c.name === COLLECTION_NAME,
    );
    expect(col).toBeDefined();

    await page.goto(`/_/collections/${col.id}/edit`);

    // Should show the editor with the collection name
    await expect(page.getByRole('heading', { name: /Edit Collection/i })).toBeVisible({
      timeout: 10_000,
    });
    // The name input should have the collection name
    const nameInput = page.getByLabel(/Collection name|Name/i).first();
    await expect(nameInput).toHaveValue(COLLECTION_NAME);
  });

  test('displays existing fields', async ({ adminPage: page, api }) => {
    const token = await api.getAdminToken();
    const collections = await api.listCollections(token);
    const col = collections.items.find(
      (c: { name: string }) => c.name === COLLECTION_NAME,
    );

    await page.goto(`/_/collections/${col.id}/edit`);
    await expect(page.getByRole('heading', { name: /Edit Collection/i })).toBeVisible({
      timeout: 10_000,
    });

    // Should show the existing fields
    await expect(page.getByText('title')).toBeVisible();
    await expect(page.getByText('count')).toBeVisible();
  });

  test('can add a new field', async ({ adminPage: page, api }) => {
    const token = await api.getAdminToken();
    const collections = await api.listCollections(token);
    const col = collections.items.find(
      (c: { name: string }) => c.name === COLLECTION_NAME,
    );

    await page.goto(`/_/collections/${col.id}/edit`);
    await expect(page.getByRole('heading', { name: /Edit Collection/i })).toBeVisible({
      timeout: 10_000,
    });

    // Click "Add Field"
    await page.getByRole('button', { name: /Add Field/i }).click();

    // A new empty field row should appear
    const fieldInputs = page.getByPlaceholder(/field name/i);
    const count = await fieldInputs.count();
    expect(count).toBeGreaterThan(2); // original 2 + new one
  });

  test('shows collection type selector', async ({ adminPage: page }) => {
    await page.goto('/_/collections/new');

    // Type options should be visible
    await expect(page.getByText('Base')).toBeVisible();
    await expect(page.getByText('Auth')).toBeVisible();
    await expect(page.getByText('View')).toBeVisible();
  });

  test('shows API preview section', async ({ adminPage: page }) => {
    await page.goto('/_/collections/new');

    // Fill a name so the API preview can show endpoints
    const nameInput = page.getByLabel(/Collection name|Name/i).first();
    await nameInput.fill('test_preview');

    // Should show the API preview with endpoint information
    await expect(page.getByText(/API Preview|Endpoints/i)).toBeVisible();
  });

  test('shows rules editor', async ({ adminPage: page, api }) => {
    const token = await api.getAdminToken();
    const collections = await api.listCollections(token);
    const col = collections.items.find(
      (c: { name: string }) => c.name === COLLECTION_NAME,
    );

    await page.goto(`/_/collections/${col.id}/edit`);
    await expect(page.getByRole('heading', { name: /Edit Collection/i })).toBeVisible({
      timeout: 10_000,
    });

    // Should display the rules section
    await expect(page.getByText(/API Rules|Access Rules|Permissions/i)).toBeVisible();
  });
});
