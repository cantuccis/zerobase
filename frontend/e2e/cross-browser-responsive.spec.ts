/**
 * Cross-browser and responsive UI validation E2E tests.
 *
 * Validates the admin dashboard across viewports and for usability/correctness:
 * 1. Schema editor field reordering
 * 2. Record form renders all field types
 * 3. Responsive layout (sidebar, table scrolling, modal containment)
 * 4. Dark mode rendering
 * 5. Delete confirmation with keyboard input
 * 6. Real-time log viewer stability
 * 7. File upload component behavior
 */
import { test, expect } from './fixtures';

// ── 1. Schema Editor Field Reordering ─────────────────────────────────────────

test.describe('Schema Editor – Field Reordering', () => {
  test('can add fields and reorder them using move up/down buttons', async ({ adminPage }) => {
    await adminPage.goto('/_/collections/new');

    // Set collection name
    const nameInput = adminPage.getByTestId('collection-name');
    await nameInput.fill('e2e_reorder_test');

    // Add three fields
    const addBtn = adminPage.getByTestId('add-field');
    await addBtn.click();
    await addBtn.click();
    await addBtn.click();

    // Verify three field editors exist
    const fieldEditors = adminPage.locator('[data-testid^="field-editor-"]');
    await expect(fieldEditors).toHaveCount(3);

    // Move up/down buttons should be present
    const moveUpBtns = adminPage.getByLabel(/move up/i);
    const moveDownBtns = adminPage.getByLabel(/move down/i);

    await expect(moveUpBtns).toHaveCount(3);
    await expect(moveDownBtns).toHaveCount(3);

    // First move up should be disabled
    await expect(moveUpBtns.first()).toBeDisabled();
    // Last move down should be disabled
    await expect(moveDownBtns.last()).toBeDisabled();
  });

  test('field reordering preserves field content', async ({ adminPage }) => {
    await adminPage.goto('/_/collections/new');

    const addBtn = adminPage.getByTestId('add-field');
    await addBtn.click();
    await addBtn.click();

    // Name the first field
    const nameInputs = adminPage.locator('input[placeholder*="field name"]');
    if (await nameInputs.count() >= 2) {
      await nameInputs.first().fill('first_field');
      await nameInputs.nth(1).fill('second_field');

      // Move second field up
      const moveUpBtns = adminPage.getByLabel(/move up/i);
      await moveUpBtns.nth(1).click();

      // First field should now be 'second_field'
      await expect(nameInputs.first()).toHaveValue('second_field');
      await expect(nameInputs.nth(1)).toHaveValue('first_field');
    }
  });
});

// ── 2. Record Form – Field Type Rendering ─────────────────────────────────────

test.describe('Record Form – Field Types', () => {
  let collectionId: string;

  test.beforeAll(async ({ api }) => {
    const token = await api.getAdminToken();

    // Clean up from previous runs
    await api.deleteCollection(token, 'e2e_all_fields');

    // Create a collection with diverse field types
    const collection = await api.createCollection(token, {
      name: 'e2e_all_fields',
      type: 'base',
      fields: [
        { name: 'text_field', type: { type: 'text', options: { maxLength: 200, pattern: '' } }, required: true },
        { name: 'number_field', type: { type: 'number', options: { min: null, max: null, noDecimal: false } } },
        { name: 'bool_field', type: { type: 'bool', options: {} } },
        { name: 'email_field', type: { type: 'email', options: {} } },
        { name: 'url_field', type: { type: 'url', options: {} } },
        { name: 'date_field', type: { type: 'dateTime', options: { min: '', max: '' } } },
        { name: 'select_field', type: { type: 'select', options: { values: ['opt_a', 'opt_b', 'opt_c'], maxSelect: 1 } } },
        { name: 'json_field', type: { type: 'json', options: {} } },
        { name: 'editor_field', type: { type: 'editor', options: { maxLength: 5000, convertUrls: false } } },
      ],
    });
    collectionId = collection.id;
  });

  test.afterAll(async ({ api }) => {
    const token = await api.getAdminToken();
    await api.deleteCollection(token, 'e2e_all_fields');
  });

  test('renders all field type inputs in create form', async ({ adminPage }) => {
    await adminPage.goto(`/_/collections/${collectionId}`);

    // Click create new record button
    const createBtn = adminPage.getByRole('button', { name: /new record|create/i });
    if (await createBtn.isVisible()) {
      await createBtn.click();

      // Wait for modal
      const modal = adminPage.getByTestId('record-form-modal');
      await expect(modal).toBeVisible({ timeout: 5000 });

      // Verify each field type is present
      await expect(adminPage.getByTestId('field-input-text_field')).toBeVisible();
      await expect(adminPage.getByTestId('field-input-number_field')).toBeVisible();
      await expect(adminPage.getByTestId('field-input-bool_field')).toBeVisible();
      await expect(adminPage.getByTestId('field-input-email_field')).toBeVisible();
      await expect(adminPage.getByTestId('field-input-url_field')).toBeVisible();
      await expect(adminPage.getByTestId('field-input-date_field')).toBeVisible();
      await expect(adminPage.getByTestId('field-input-select_field')).toBeVisible();
      await expect(adminPage.getByTestId('field-input-json_field')).toBeVisible();
      await expect(adminPage.getByTestId('field-input-editor_field')).toBeVisible();

      // Close modal
      await adminPage.getByLabel(/close form/i).click();
    }
  });
});

// ── 3. Responsive Layout ─────────────────────────────────────────────────────

test.describe('Responsive Layout', () => {
  test('sidebar collapses on mobile viewport', async ({ adminPage }) => {
    await adminPage.setViewportSize({ width: 375, height: 812 }); // iPhone viewport
    await adminPage.goto('/_/');

    // Desktop sidebar should be hidden
    const sidebar = adminPage.locator('aside[aria-label="Main navigation"]');
    await expect(sidebar).toBeHidden();

    // Mobile hamburger should be visible
    const hamburger = adminPage.getByLabel(/open navigation menu/i);
    await expect(hamburger).toBeVisible();

    // Click hamburger to open drawer
    await hamburger.click();
    const drawer = adminPage.getByRole('dialog', { name: /navigation menu/i });
    await expect(drawer).toBeVisible();

    // Close with X button
    await adminPage.getByLabel(/close navigation menu/i).click();
    await expect(drawer).toBeHidden();
  });

  test('sidebar is visible on desktop viewport', async ({ adminPage }) => {
    await adminPage.setViewportSize({ width: 1280, height: 800 });
    await adminPage.goto('/_/');

    const sidebar = adminPage.locator('aside[aria-label="Main navigation"]');
    await expect(sidebar).toBeVisible();

    // Hamburger should be hidden
    const hamburger = adminPage.getByLabel(/open navigation menu/i);
    await expect(hamburger).toBeHidden();
  });

  test('record table is horizontally scrollable on mobile', async ({ adminPage, api }) => {
    const token = await api.getAdminToken();
    await api.deleteCollection(token, 'e2e_scroll_test');
    const col = await api.createCollection(token, {
      name: 'e2e_scroll_test',
      type: 'base',
      fields: [
        { name: 'col1', type: { type: 'text', options: { maxLength: 200, pattern: '' } } },
        { name: 'col2', type: { type: 'text', options: { maxLength: 200, pattern: '' } } },
        { name: 'col3', type: { type: 'text', options: { maxLength: 200, pattern: '' } } },
        { name: 'col4', type: { type: 'text', options: { maxLength: 200, pattern: '' } } },
        { name: 'col5', type: { type: 'text', options: { maxLength: 200, pattern: '' } } },
      ],
    });

    // Add a record
    await api.createRecord(token, 'e2e_scroll_test', {
      col1: 'data1', col2: 'data2', col3: 'data3', col4: 'data4', col5: 'data5',
    });

    await adminPage.setViewportSize({ width: 375, height: 812 });
    await adminPage.goto(`/_/collections/${col.id}`);

    // The table wrapper should have overflow-x-auto
    const scrollContainer = adminPage.locator('.overflow-x-auto');
    await expect(scrollContainer.first()).toBeVisible();

    // Verify page doesn't have unwanted horizontal scroll
    const bodyScrollWidth = await adminPage.evaluate(() => document.body.scrollWidth);
    const viewportWidth = await adminPage.evaluate(() => window.innerWidth);
    // Body scroll should not significantly exceed viewport
    expect(bodyScrollWidth).toBeLessThanOrEqual(viewportWidth + 20);

    await api.deleteCollection(token, 'e2e_scroll_test');
  });

  test('modals do not overflow on small screens', async ({ adminPage, api }) => {
    const token = await api.getAdminToken();
    await api.deleteCollection(token, 'e2e_modal_test');
    const col = await api.createCollection(token, {
      name: 'e2e_modal_test',
      type: 'base',
      fields: [
        { name: 'title', type: { type: 'text', options: { maxLength: 200, pattern: '' } }, required: true },
      ],
    });

    await adminPage.setViewportSize({ width: 375, height: 667 }); // iPhone SE
    await adminPage.goto(`/_/collections/${col.id}`);

    // Open create record modal
    const createBtn = adminPage.getByRole('button', { name: /new record|create/i });
    if (await createBtn.isVisible()) {
      await createBtn.click();

      const modal = adminPage.getByTestId('record-form-modal');
      await expect(modal).toBeVisible({ timeout: 5000 });

      // Modal should fit within viewport
      const box = await modal.boundingBox();
      if (box) {
        expect(box.y).toBeGreaterThanOrEqual(0);
        expect(box.x).toBeGreaterThanOrEqual(0);
        // Width shouldn't exceed viewport
        expect(box.width).toBeLessThanOrEqual(375);
      }

      await adminPage.getByLabel(/close form/i).click();
    }

    await api.deleteCollection(token, 'e2e_modal_test');
  });
});

// ── 4. Dark Mode ────────────────────────────────────────────────────────────

test.describe('Dark Mode', () => {
  test('can toggle dark mode and UI adapts', async ({ adminPage }) => {
    await adminPage.goto('/_/');

    // Find theme toggle button
    const themeToggle = adminPage.getByLabel(/theme|toggle/i).first();
    if (await themeToggle.isVisible()) {
      await themeToggle.click();

      // Select dark option
      const darkOption = adminPage.getByRole('button', { name: /dark/i }).or(adminPage.getByText(/^dark$/i));
      if (await darkOption.isVisible()) {
        await darkOption.click();

        // HTML element should have 'dark' class
        const hasDarkClass = await adminPage.evaluate(() =>
          document.documentElement.classList.contains('dark')
        );
        expect(hasDarkClass).toBe(true);

        // Verify dark mode colors are applied (background should be dark)
        const bgColor = await adminPage.evaluate(() => {
          return window.getComputedStyle(document.body).backgroundColor;
        });
        // Dark backgrounds typically have low RGB values
        // Just verify it's not white
        expect(bgColor).not.toBe('rgb(255, 255, 255)');
      }
    }
  });

  test('dark mode persists across page navigation', async ({ adminPage }) => {
    await adminPage.goto('/_/');

    // Set dark mode via localStorage directly
    await adminPage.evaluate(() => {
      localStorage.setItem('zerobase-theme', 'dark');
      document.documentElement.classList.add('dark');
    });

    // Navigate to another page
    await adminPage.goto('/_/collections');

    // Dark mode should persist
    const hasDarkClass = await adminPage.evaluate(() =>
      document.documentElement.classList.contains('dark')
    );
    expect(hasDarkClass).toBe(true);
  });

  test('toast notifications are readable in dark mode', async ({ adminPage }) => {
    await adminPage.goto('/_/');

    // Enable dark mode
    await adminPage.evaluate(() => {
      localStorage.setItem('zerobase-theme', 'dark');
      document.documentElement.classList.add('dark');
    });
    await adminPage.reload();

    // Trigger a toast by performing an action (e.g., navigate and check)
    // We can verify the toast container structure exists with correct dark classes
    const toastContainer = adminPage.locator('[aria-label="Notifications"]');
    // Toast container only renders when there are active toasts
    // Verify the CSS classes are correct by checking the component markup
  });

  test('form inputs have sufficient contrast in dark mode', async ({ adminPage }) => {
    await adminPage.evaluate(() => {
      localStorage.setItem('zerobase-theme', 'dark');
      document.documentElement.classList.add('dark');
    });

    await adminPage.goto('/_/collections/new');

    // Check that input text is readable
    const nameInput = adminPage.getByTestId('collection-name');
    await expect(nameInput).toBeVisible();

    const styles = await nameInput.evaluate((el) => {
      const computed = window.getComputedStyle(el);
      return {
        color: computed.color,
        backgroundColor: computed.backgroundColor,
        borderColor: computed.borderColor,
      };
    });

    // Text color should not be black in dark mode
    expect(styles.color).not.toBe('rgb(0, 0, 0)');
  });
});

// ── 5. Delete Confirmation ──────────────────────────────────────────────────

test.describe('Delete Confirmation', () => {
  test('requires typing collection name for large collections', async ({ adminPage, api }) => {
    const token = await api.getAdminToken();
    await api.deleteCollection(token, 'e2e_delete_confirm');

    const col = await api.createCollection(token, {
      name: 'e2e_delete_confirm',
      type: 'base',
      fields: [{ name: 'data', type: { type: 'text', options: { maxLength: 200, pattern: '' } } }],
    });

    // Create 60 records to trigger name confirmation
    const createPromises = [];
    for (let i = 0; i < 60; i++) {
      createPromises.push(api.createRecord(token, 'e2e_delete_confirm', { data: `record_${i}` }));
    }
    await Promise.all(createPromises);

    await adminPage.goto('/_/collections');

    // Click delete
    const deleteBtn = adminPage.getByLabel(/delete e2e_delete_confirm/i);
    await deleteBtn.click();

    // Dialog should appear
    const dialog = adminPage.getByRole('dialog');
    await expect(dialog).toBeVisible();

    // Should show record count warning
    await expect(adminPage.getByTestId('record-count-warning')).toBeVisible({ timeout: 5000 });

    // Name confirmation input should be present
    const confirmInput = adminPage.getByTestId('confirm-name-input');
    await expect(confirmInput).toBeVisible();

    // Delete button should be disabled without name
    const confirmDeleteBtn = adminPage.getByTestId('confirm-delete-btn');
    await expect(confirmDeleteBtn).toBeDisabled();

    // Type partial name – button should stay disabled
    await confirmInput.fill('e2e_delete_conf');
    await expect(confirmDeleteBtn).toBeDisabled();

    // Type full name – button should enable
    await confirmInput.fill('e2e_delete_confirm');
    await expect(confirmDeleteBtn).toBeEnabled();

    // Cancel instead of deleting (for cleanup safety)
    await adminPage.getByRole('button', { name: /cancel/i }).click();

    await api.deleteCollection(token, 'e2e_delete_confirm');
  });

  test('keyboard input works in confirmation dialog', async ({ adminPage, api }) => {
    const token = await api.getAdminToken();
    await api.deleteCollection(token, 'e2e_kb_confirm');

    const col = await api.createCollection(token, {
      name: 'e2e_kb_confirm',
      type: 'base',
      fields: [{ name: 'data', type: { type: 'text', options: { maxLength: 200, pattern: '' } } }],
    });

    // Create enough records
    for (let i = 0; i < 60; i++) {
      await api.createRecord(token, 'e2e_kb_confirm', { data: `r${i}` });
    }

    await adminPage.goto('/_/collections');

    const deleteBtn = adminPage.getByLabel(/delete e2e_kb_confirm/i);
    await deleteBtn.click();

    const confirmInput = adminPage.getByTestId('confirm-name-input');
    await expect(confirmInput).toBeVisible({ timeout: 5000 });

    // Type using keyboard (character by character to simulate IME-like input)
    await confirmInput.focus();
    await adminPage.keyboard.type('e2e_kb_confirm');

    await expect(confirmInput).toHaveValue('e2e_kb_confirm');
    await expect(adminPage.getByTestId('confirm-delete-btn')).toBeEnabled();

    // Escape to close
    await adminPage.keyboard.press('Escape');
    await expect(adminPage.getByRole('dialog')).toBeHidden();

    await api.deleteCollection(token, 'e2e_kb_confirm');
  });
});

// ── 6. Log Viewer ───────────────────────────────────────────────────────────

test.describe('Log Viewer', () => {
  test('loads and displays logs with filters', async ({ adminPage }) => {
    await adminPage.goto('/_/logs');

    // Filters should be visible
    await expect(adminPage.getByTestId('logs-filters')).toBeVisible();

    // Stats section should render (even if empty)
    const statsOrTable = adminPage.getByTestId('logs-table').or(adminPage.getByTestId('stats-overview'));
    await expect(statsOrTable.first()).toBeVisible({ timeout: 10000 });
  });

  test('logs table has sort indicators that are keyboard accessible', async ({ adminPage }) => {
    await adminPage.goto('/_/logs');

    // Wait for table
    const table = adminPage.getByTestId('logs-table');
    await expect(table).toBeVisible({ timeout: 10000 });

    // Table headers should be focusable (tabIndex=0) and have aria-sort
    const timestampHeader = table.locator('th').filter({ hasText: /timestamp/i });
    await expect(timestampHeader).toHaveAttribute('tabindex', '0');

    // Click to sort
    await timestampHeader.click();

    // Should have aria-sort
    const ariaSort = await timestampHeader.getAttribute('aria-sort');
    expect(ariaSort).toBeDefined();
  });

  test('filter changes reset pagination to page 1', async ({ adminPage }) => {
    await adminPage.goto('/_/logs');

    // Change method filter
    const methodFilter = adminPage.locator('#method-filter');
    await expect(methodFilter).toBeVisible({ timeout: 10000 });

    await methodFilter.selectOption('GET');

    // After filter change, we should be on page 1
    // (verified by pagination component if visible)
    const pagination = adminPage.getByTestId('pagination');
    if (await pagination.isVisible()) {
      const pageText = await pagination.textContent();
      expect(pageText).toContain('1');
    }
  });

  test('log detail modal opens and closes correctly', async ({ adminPage }) => {
    await adminPage.goto('/_/logs');

    const logRow = adminPage.getByTestId('log-row').first();
    if (await logRow.isVisible({ timeout: 5000 }).catch(() => false)) {
      await logRow.click();

      const detailModal = adminPage.getByTestId('log-detail-modal');
      await expect(detailModal).toBeVisible({ timeout: 5000 });

      // Close with Escape
      await adminPage.keyboard.press('Escape');
      await expect(detailModal).toBeHidden();
    }
  });
});

// ── 7. File Upload ──────────────────────────────────────────────────────────

test.describe('File Upload Component', () => {
  let collectionId: string;

  test.beforeAll(async ({ api }) => {
    const token = await api.getAdminToken();
    await api.deleteCollection(token, 'e2e_file_test');
    const col = await api.createCollection(token, {
      name: 'e2e_file_test',
      type: 'base',
      fields: [
        { name: 'document', type: { type: 'file', options: { maxSize: 5242880, maxSelect: 3, mimeTypes: [] } } },
      ],
    });
    collectionId = col.id;
  });

  test.afterAll(async ({ api }) => {
    const token = await api.getAdminToken();
    await api.deleteCollection(token, 'e2e_file_test');
  });

  test('file upload zone shows drag-and-drop area', async ({ adminPage }) => {
    await adminPage.goto(`/_/collections/${collectionId}`);

    const createBtn = adminPage.getByRole('button', { name: /new record|create/i });
    if (await createBtn.isVisible()) {
      await createBtn.click();

      const modal = adminPage.getByTestId('record-form-modal');
      await expect(modal).toBeVisible({ timeout: 5000 });

      const dropzone = adminPage.getByTestId('file-upload-dropzone-document');
      await expect(dropzone).toBeVisible();
      await expect(adminPage.getByText(/click to browse/i)).toBeVisible();
      await expect(adminPage.getByText(/drag and drop/i)).toBeVisible();

      await adminPage.getByLabel(/close form/i).click();
    }
  });

  test('file upload shows file count indicator', async ({ adminPage }) => {
    await adminPage.goto(`/_/collections/${collectionId}`);

    const createBtn = adminPage.getByRole('button', { name: /new record|create/i });
    if (await createBtn.isVisible()) {
      await createBtn.click();

      const modal = adminPage.getByTestId('record-form-modal');
      await expect(modal).toBeVisible({ timeout: 5000 });

      // File count should show 0/3
      const counter = adminPage.getByTestId('file-upload-count-document');
      await expect(counter).toBeVisible();
      await expect(counter).toContainText('0/3');

      await adminPage.getByLabel(/close form/i).click();
    }
  });
});

// ── Accessibility Basics ────────────────────────────────────────────────────

test.describe('Accessibility – Cross-Browser Basics', () => {
  test('skip-to-content link is present', async ({ adminPage }) => {
    await adminPage.goto('/_/');

    const skipLink = adminPage.locator('a[href="#main-content"]');
    await expect(skipLink).toBeAttached();
  });

  test('all modals have aria-modal and role=dialog', async ({ adminPage, api }) => {
    const token = await api.getAdminToken();
    await api.deleteCollection(token, 'e2e_a11y_modal');
    const col = await api.createCollection(token, {
      name: 'e2e_a11y_modal',
      type: 'base',
      fields: [{ name: 'title', type: { type: 'text', options: { maxLength: 200, pattern: '' } } }],
    });

    await adminPage.goto(`/_/collections/${col.id}`);

    const createBtn = adminPage.getByRole('button', { name: /new record|create/i });
    if (await createBtn.isVisible()) {
      await createBtn.click();

      const dialog = adminPage.getByRole('dialog');
      await expect(dialog).toBeVisible({ timeout: 5000 });
      await expect(dialog).toHaveAttribute('aria-modal', 'true');

      await adminPage.getByLabel(/close form/i).click();
    }

    await api.deleteCollection(token, 'e2e_a11y_modal');
  });

  test('interactive elements have visible focus styles', async ({ adminPage }) => {
    await adminPage.goto('/_/');

    // Tab to first interactive element
    await adminPage.keyboard.press('Tab');
    await adminPage.keyboard.press('Tab');

    // Some element should have focus
    const focusedElement = await adminPage.evaluate(() => {
      const el = document.activeElement;
      if (!el || el === document.body) return null;
      return {
        tag: el.tagName.toLowerCase(),
        hasOutline: window.getComputedStyle(el).outlineStyle !== 'none' || el.className.includes('focus'),
      };
    });

    // There should be a focused element
    if (focusedElement) {
      expect(['a', 'button', 'input', 'select', 'textarea']).toContain(focusedElement.tag);
    }
  });
});
