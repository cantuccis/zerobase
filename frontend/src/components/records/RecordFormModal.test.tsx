import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { RecordFormModal } from './RecordFormModal';
import type { Collection, BaseRecord, Field } from '../../lib/api/types';

// ── Test data ────────────────────────────────────────────────────────────────

function makeField(name: string, type: string = 'text', opts: Partial<Field> = {}): Field {
  const typeMap: Record<string, Field['type']> = {
    text: { type: 'text', options: { minLength: 0, maxLength: 500, pattern: null, searchable: true } },
    number: { type: 'number', options: { min: null, max: null, noDecimal: false } },
    bool: { type: 'bool', options: {} },
    email: { type: 'email', options: { exceptDomains: [], onlyDomains: [] } },
    url: { type: 'url', options: { exceptDomains: [], onlyDomains: [] } },
    dateTime: { type: 'dateTime', options: { min: '', max: '' } },
    select: { type: 'select', options: { values: ['draft', 'published', 'archived'] } },
    multiSelect: { type: 'multiSelect', options: { values: ['tag1', 'tag2', 'tag3'], maxSelect: 3 } },
    json: { type: 'json', options: { maxSize: 2097152 } },
    editor: { type: 'editor', options: { maxLength: 50000, searchable: true } },
    autoDate: { type: 'autoDate', options: { onCreate: true, onUpdate: true } },
    file: { type: 'file', options: { maxSize: 5242880, maxSelect: 1, mimeTypes: [], thumbs: [] } },
    relation: { type: 'relation', options: { collectionId: 'col_users', cascadeDelete: false, maxSelect: null } },
  };

  return {
    id: `f_${name}`,
    name,
    type: typeMap[type] ?? typeMap['text'],
    required: false,
    unique: false,
    sortOrder: 0,
    ...opts,
  };
}

const TEST_COLLECTION: Collection = {
  id: 'col_posts',
  name: 'posts',
  type: 'base',
  fields: [
    makeField('title', 'text', { required: true }),
    makeField('views', 'number'),
    makeField('published', 'bool'),
    makeField('email', 'email'),
    makeField('website', 'url'),
    makeField('status', 'select'),
    makeField('tags', 'multiSelect'),
    makeField('metadata', 'json'),
    makeField('content', 'editor'),
  ],
  rules: {
    listRule: null,
    viewRule: null,
    createRule: null,
    updateRule: null,
    deleteRule: null,
  },
  indexes: [],
};

const TEST_RECORD: BaseRecord = {
  id: 'rec_123',
  collectionId: 'col_posts',
  collectionName: 'posts',
  created: '2024-01-15T10:00:00Z',
  updated: '2024-01-15T12:00:00Z',
  title: 'Test Post',
  views: 42,
  published: true,
  email: 'test@example.com',
  website: 'https://example.com',
  status: 'published',
  tags: ['tag1', 'tag2'],
  metadata: { key: 'value' },
  content: '<p>Hello world</p>',
};

// ── Helpers ──────────────────────────────────────────────────────────────────

const defaultProps = {
  collection: TEST_COLLECTION,
  record: null as BaseRecord | null,
  onClose: vi.fn(),
  onSave: vi.fn(),
  onSubmit: vi.fn(),
};

function renderModal(props: Partial<typeof defaultProps> = {}) {
  const merged = { ...defaultProps, ...props };
  return render(<RecordFormModal {...merged} />);
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('RecordFormModal', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ── Rendering ──────────────────────────────────────────────────────────

  describe('rendering', () => {
    it('renders the modal with "New Record" title in create mode', () => {
      renderModal();
      expect(screen.getByText('New Record')).toBeInTheDocument();
    });

    it('renders the modal with "Edit Record" title in edit mode', () => {
      renderModal({ record: TEST_RECORD });
      expect(screen.getByText('Edit Record')).toBeInTheDocument();
    });

    it('shows collection name in header', () => {
      renderModal();
      expect(screen.getByText('posts')).toBeInTheDocument();
    });

    it('shows record ID in edit mode', () => {
      renderModal({ record: TEST_RECORD });
      expect(screen.getByTestId('record-id-display')).toHaveTextContent('rec_123');
    });

    it('does not show record ID in create mode', () => {
      renderModal();
      expect(screen.queryByTestId('record-id-display')).not.toBeInTheDocument();
    });

    it('renders a field input for each editable field', () => {
      renderModal();
      expect(screen.getByTestId('field-input-title')).toBeInTheDocument();
      expect(screen.getByTestId('field-input-views')).toBeInTheDocument();
      expect(screen.getByTestId('field-input-published')).toBeInTheDocument();
      expect(screen.getByTestId('field-input-email')).toBeInTheDocument();
      expect(screen.getByTestId('field-input-website')).toBeInTheDocument();
      expect(screen.getByTestId('field-input-status')).toBeInTheDocument();
      expect(screen.getByTestId('field-input-tags')).toBeInTheDocument();
      expect(screen.getByTestId('field-input-metadata')).toBeInTheDocument();
      expect(screen.getByTestId('field-input-content')).toBeInTheDocument();
    });

    it('does not render autoDate fields', () => {
      const collection: Collection = {
        ...TEST_COLLECTION,
        fields: [makeField('title'), makeField('autoCreated', 'autoDate')],
      };
      renderModal({ collection });
      expect(screen.queryByTestId('field-input-autoCreated')).not.toBeInTheDocument();
    });

    it('shows required indicator for required fields', () => {
      renderModal();
      const titleInput = screen.getByTestId('field-input-title');
      expect(within(titleInput).getByLabelText('required')).toBeInTheDocument();
    });

    it('renders "Create Record" submit button in create mode', () => {
      renderModal();
      expect(screen.getByTestId('record-form-submit')).toHaveTextContent('Create Record');
    });

    it('renders "Save Changes" submit button in edit mode', () => {
      renderModal({ record: TEST_RECORD });
      expect(screen.getByTestId('record-form-submit')).toHaveTextContent('Save Changes');
    });
  });

  // ── Field types ────────────────────────────────────────────────────────

  describe('field type inputs', () => {
    it('renders text input for text fields', () => {
      renderModal();
      const input = screen.getByLabelText(/title/i);
      expect(input).toHaveAttribute('type', 'text');
    });

    it('renders number input for number fields', () => {
      renderModal();
      const input = screen.getByLabelText(/views/i);
      expect(input).toHaveAttribute('type', 'number');
    });

    it('renders toggle switch for bool fields', () => {
      renderModal();
      expect(screen.getByTestId('bool-toggle-published')).toBeInTheDocument();
    });

    it('renders email input for email fields', () => {
      renderModal();
      const input = screen.getByLabelText(/email/i);
      expect(input).toHaveAttribute('type', 'email');
    });

    it('renders url input for url fields', () => {
      renderModal();
      const input = screen.getByLabelText(/website/i);
      expect(input).toHaveAttribute('type', 'url');
    });

    it('renders select dropdown for select fields', () => {
      renderModal();
      const select = screen.getByLabelText(/status/i);
      expect(select.tagName).toBe('SELECT');
      expect(within(select).getByText('draft')).toBeInTheDocument();
      expect(within(select).getByText('published')).toBeInTheDocument();
      expect(within(select).getByText('archived')).toBeInTheDocument();
    });

    it('renders multi-select buttons for multiSelect fields', () => {
      renderModal();
      expect(screen.getByTestId('multiselect-option-tag1')).toBeInTheDocument();
      expect(screen.getByTestId('multiselect-option-tag2')).toBeInTheDocument();
      expect(screen.getByTestId('multiselect-option-tag3')).toBeInTheDocument();
    });

    it('renders textarea for json fields', () => {
      renderModal();
      const input = screen.getByLabelText(/metadata/i);
      expect(input.tagName).toBe('TEXTAREA');
    });

    it('renders textarea for editor fields', () => {
      renderModal();
      const input = screen.getByLabelText(/content/i);
      expect(input.tagName).toBe('TEXTAREA');
    });
  });

  // ── Pre-populated values (edit mode) ───────────────────────────────────

  describe('edit mode pre-population', () => {
    it('pre-populates text field with existing value', () => {
      renderModal({ record: TEST_RECORD });
      expect(screen.getByLabelText(/title/i)).toHaveValue('Test Post');
    });

    it('pre-populates number field with existing value', () => {
      renderModal({ record: TEST_RECORD });
      expect(screen.getByLabelText(/views/i)).toHaveValue(42);
    });

    it('pre-populates bool toggle with existing value', () => {
      renderModal({ record: TEST_RECORD });
      expect(screen.getByTestId('bool-toggle-published')).toHaveAttribute('aria-checked', 'true');
    });

    it('pre-populates select with existing value', () => {
      renderModal({ record: TEST_RECORD });
      expect(screen.getByLabelText(/status/i)).toHaveValue('published');
    });

    it('pre-populates multiSelect with existing values', () => {
      renderModal({ record: TEST_RECORD });
      expect(screen.getByTestId('multiselect-option-tag1')).toHaveAttribute('aria-pressed', 'true');
      expect(screen.getByTestId('multiselect-option-tag2')).toHaveAttribute('aria-pressed', 'true');
      expect(screen.getByTestId('multiselect-option-tag3')).toHaveAttribute('aria-pressed', 'false');
    });
  });

  // ── Interactions ───────────────────────────────────────────────────────

  describe('interactions', () => {
    it('updates text field value on type', async () => {
      renderModal();
      const user = userEvent.setup();
      const input = screen.getByLabelText(/title/i);
      await user.type(input, 'New Title');
      expect(input).toHaveValue('New Title');
    });

    it('toggles bool field on click', async () => {
      renderModal();
      const user = userEvent.setup();
      const toggle = screen.getByTestId('bool-toggle-published');
      expect(toggle).toHaveAttribute('aria-checked', 'false');
      await user.click(toggle);
      expect(toggle).toHaveAttribute('aria-checked', 'true');
    });

    it('selects value in select dropdown', async () => {
      renderModal();
      const user = userEvent.setup();
      const select = screen.getByLabelText(/status/i);
      await user.selectOptions(select, 'draft');
      expect(select).toHaveValue('draft');
    });

    it('toggles multiSelect options on click', async () => {
      renderModal();
      const user = userEvent.setup();
      const tag1 = screen.getByTestId('multiselect-option-tag1');
      expect(tag1).toHaveAttribute('aria-pressed', 'false');
      await user.click(tag1);
      expect(tag1).toHaveAttribute('aria-pressed', 'true');
      await user.click(tag1);
      expect(tag1).toHaveAttribute('aria-pressed', 'false');
    });
  });

  // ── Validation ─────────────────────────────────────────────────────────

  describe('validation', () => {
    it('shows validation error for empty required field on submit', async () => {
      renderModal();
      const user = userEvent.setup();
      await user.click(screen.getByTestId('record-form-submit'));

      await waitFor(() => {
        expect(screen.getByTestId('field-error-title')).toBeInTheDocument();
      });
    });

    it('clears field error when user types in the field', async () => {
      renderModal();
      const user = userEvent.setup();

      // Trigger validation
      await user.click(screen.getByTestId('record-form-submit'));
      await waitFor(() => {
        expect(screen.getByTestId('field-error-title')).toBeInTheDocument();
      });

      // Type in the field
      await user.type(screen.getByLabelText(/title/i), 'a');
      await waitFor(() => {
        expect(screen.queryByTestId('field-error-title')).not.toBeInTheDocument();
      });
    });

    it('does not call onSubmit when validation fails', async () => {
      const onSubmit = vi.fn();
      renderModal({ onSubmit });
      const user = userEvent.setup();
      await user.click(screen.getByTestId('record-form-submit'));
      expect(onSubmit).not.toHaveBeenCalled();
    });
  });

  // ── Submission ─────────────────────────────────────────────────────────

  describe('submission', () => {
    it('calls onSubmit with form data on valid submit', async () => {
      const onSubmit = vi.fn().mockResolvedValue({ id: 'new_rec', collectionId: 'col_posts', collectionName: 'posts', created: '', updated: '', title: 'My Post' });
      const onSave = vi.fn();
      renderModal({ onSubmit, onSave });
      const user = userEvent.setup();

      await user.type(screen.getByLabelText(/title/i), 'My Post');
      await user.click(screen.getByTestId('record-form-submit'));

      await waitFor(() => {
        expect(onSubmit).toHaveBeenCalledTimes(1);
        const payload = onSubmit.mock.calls[0][0];
        expect(payload).toHaveProperty('title', 'My Post');
      });
    });

    it('calls onSave after successful submit', async () => {
      const savedRecord: BaseRecord = {
        id: 'new_rec',
        collectionId: 'col_posts',
        collectionName: 'posts',
        created: '2024-01-01',
        updated: '2024-01-01',
        title: 'My Post',
      };
      const onSubmit = vi.fn().mockResolvedValue(savedRecord);
      const onSave = vi.fn();
      renderModal({ onSubmit, onSave });
      const user = userEvent.setup();

      await user.type(screen.getByLabelText(/title/i), 'My Post');
      await user.click(screen.getByTestId('record-form-submit'));

      await waitFor(() => {
        expect(onSave).toHaveBeenCalledWith(savedRecord);
      });
    });

    it('shows server error on API failure', async () => {
      const onSubmit = vi.fn().mockRejectedValue({
        response: { message: 'Validation failed.', data: { title: { code: 'required', message: 'Title is required.' } } },
      });
      renderModal({ onSubmit });
      const user = userEvent.setup();

      await user.type(screen.getByLabelText(/title/i), 'Test');
      await user.click(screen.getByTestId('record-form-submit'));

      await waitFor(() => {
        expect(screen.getByTestId('server-error')).toHaveTextContent('Validation failed.');
        expect(screen.getByTestId('field-error-title')).toHaveTextContent('Title is required.');
      });
    });

    it('shows generic error message for non-API errors', async () => {
      const onSubmit = vi.fn().mockRejectedValue(new Error('Network error'));
      renderModal({ onSubmit });
      const user = userEvent.setup();

      await user.type(screen.getByLabelText(/title/i), 'Test');
      await user.click(screen.getByTestId('record-form-submit'));

      await waitFor(() => {
        expect(screen.getByTestId('server-error')).toHaveTextContent('Network error');
      });
    });

    it('disables submit button while submitting', async () => {
      const onSubmit = vi.fn().mockReturnValue(new Promise(() => {})); // Never resolves
      renderModal({ onSubmit });
      const user = userEvent.setup();

      await user.type(screen.getByLabelText(/title/i), 'Test');
      await user.click(screen.getByTestId('record-form-submit'));

      await waitFor(() => {
        expect(screen.getByTestId('record-form-submit')).toBeDisabled();
        expect(screen.getByTestId('record-form-submit')).toHaveTextContent('Creating…');
      });
    });

    it('shows "Saving…" text in edit mode while submitting', async () => {
      const onSubmit = vi.fn().mockReturnValue(new Promise(() => {}));
      renderModal({ record: TEST_RECORD, onSubmit });
      const user = userEvent.setup();

      await user.click(screen.getByTestId('record-form-submit'));

      await waitFor(() => {
        expect(screen.getByTestId('record-form-submit')).toHaveTextContent('Saving…');
      });
    });
  });

  // ── Modal behavior ─────────────────────────────────────────────────────

  describe('modal behavior', () => {
    it('calls onClose when clicking close button', async () => {
      const onClose = vi.fn();
      renderModal({ onClose });
      const user = userEvent.setup();

      await user.click(screen.getByLabelText('Close form'));
      expect(onClose).toHaveBeenCalledTimes(1);
    });

    it('calls onClose when clicking Cancel button', async () => {
      const onClose = vi.fn();
      renderModal({ onClose });
      const user = userEvent.setup();

      await user.click(screen.getByText('Cancel'));
      expect(onClose).toHaveBeenCalledTimes(1);
    });

    it('calls onClose when pressing Escape', async () => {
      const onClose = vi.fn();
      renderModal({ onClose });
      const user = userEvent.setup();

      await user.keyboard('{Escape}');
      expect(onClose).toHaveBeenCalledTimes(1);
    });

    it('calls onClose when clicking the backdrop', async () => {
      const onClose = vi.fn();
      renderModal({ onClose });
      const user = userEvent.setup();

      // Click the backdrop (the outer dialog container)
      const dialog = screen.getByRole('dialog');
      await user.click(dialog);
      expect(onClose).toHaveBeenCalled();
    });
  });

  // ── Edge cases ─────────────────────────────────────────────────────────

  describe('edge cases', () => {
    it('handles collection with no editable fields', () => {
      const collection: Collection = {
        ...TEST_COLLECTION,
        fields: [makeField('autoCreated', 'autoDate')],
      };
      renderModal({ collection });
      expect(screen.getByText('No editable fields in this collection.')).toBeInTheDocument();
    });

    it('handles empty values for all field types in create mode', () => {
      renderModal();
      // Should render without errors
      expect(screen.getByTestId('record-form-modal')).toBeInTheDocument();
    });

    it('submits correct payload with multiple field types', async () => {
      const onSubmit = vi.fn().mockResolvedValue({
        id: 'new',
        collectionId: 'col_posts',
        collectionName: 'posts',
        created: '',
        updated: '',
      });
      renderModal({ onSubmit });
      const user = userEvent.setup();

      // Fill in fields
      await user.type(screen.getByLabelText(/title/i), 'Post Title');
      await user.type(screen.getByLabelText(/views/i), '100');
      await user.click(screen.getByTestId('bool-toggle-published'));
      await user.selectOptions(screen.getByLabelText(/status/i), 'draft');
      await user.click(screen.getByTestId('multiselect-option-tag1'));

      await user.click(screen.getByTestId('record-form-submit'));

      await waitFor(() => {
        expect(onSubmit).toHaveBeenCalledTimes(1);
        const payload = onSubmit.mock.calls[0][0] as Record<string, unknown>;
        expect(payload.title).toBe('Post Title');
        expect(payload.views).toBe(100);
        expect(payload.published).toBe(true);
        expect(payload.status).toBe('draft');
        expect(payload.tags).toEqual(['tag1']);
      });
    });
  });
});
