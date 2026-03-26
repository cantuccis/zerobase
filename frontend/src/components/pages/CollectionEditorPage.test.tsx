import { render, screen, waitFor, within, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { CollectionEditorPage } from './CollectionEditorPage';
import { ApiError } from '../../lib/api';
import type { AuthOptions, Collection, ListResponse, ErrorResponseBody } from '../../lib/api/types';

// ── Test data ────────────────────────────────────────────────────────────────

function makeCollection(overrides: Partial<Collection> & { name: string }): Collection {
  return {
    id: overrides.id ?? `col_${overrides.name}`,
    name: overrides.name,
    type: overrides.type ?? 'base',
    fields: overrides.fields ?? [],
    rules: overrides.rules ?? {
      listRule: null,
      viewRule: null,
      createRule: null,
      updateRule: null,
      deleteRule: null,
    },
    indexes: overrides.indexes ?? [],
    ...overrides,
  };
}

const EXISTING_COLLECTION: Collection = makeCollection({
  id: 'col_posts',
  name: 'posts',
  type: 'base',
  fields: [
    { id: 'f1', name: 'title', type: { type: 'text', options: { minLength: 0, maxLength: 500, pattern: null, searchable: true } }, required: true, unique: false, sortOrder: 0 },
    { id: 'f2', name: 'body', type: { type: 'editor', options: { maxLength: 50000, searchable: true } }, required: false, unique: false, sortOrder: 1 },
  ],
});

const ALL_COLLECTIONS: Collection[] = [
  EXISTING_COLLECTION,
  makeCollection({ id: 'col_users', name: 'users', type: 'auth' }),
];

function makeListResponse(items: Collection[]): ListResponse<Collection> {
  return { page: 1, perPage: 30, totalPages: 1, totalItems: items.length, items };
}

// ── Mocks ────────────────────────────────────────────────────────────────────

const mockGetCollection = vi.fn();
const mockCreateCollection = vi.fn();
const mockUpdateCollection = vi.fn();
const mockListCollections = vi.fn();

vi.mock('../../lib/auth/client', () => ({
  client: {
    getCollection: (...args: unknown[]) => mockGetCollection(...args),
    createCollection: (...args: unknown[]) => mockCreateCollection(...args),
    updateCollection: (...args: unknown[]) => mockUpdateCollection(...args),
    listCollections: (...args: unknown[]) => mockListCollections(...args),
    get isAuthenticated() { return true; },
    get token() { return 'mock-token'; },
    logout: vi.fn(),
  },
}));

Object.defineProperty(window, 'location', {
  value: { href: '', pathname: '/_/collections/new', origin: 'http://localhost:8090' },
  writable: true,
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe('CollectionEditorPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockListCollections.mockResolvedValue(makeListResponse(ALL_COLLECTIONS));
  });

  // ── Create mode ───────────────────────────────────────────────────────

  describe('create mode', () => {
    function renderCreate() {
      return render(<CollectionEditorPage mode="create" />);
    }

    it('renders the create form with empty fields', () => {
      renderCreate();
      expect(screen.getByText('New Collection')).toBeInTheDocument();
      expect(screen.getByTestId('collection-name')).toHaveValue('');
      expect(screen.getByText('Create Collection')).toBeInTheDocument();
    });

    it('renders collection type selection with Base selected by default', () => {
      renderCreate();
      const baseRadio = screen.getByTestId('type-base').querySelector('input');
      expect(baseRadio).toBeChecked();
    });

    it('shows all three collection types', () => {
      renderCreate();
      expect(screen.getByTestId('type-base')).toBeInTheDocument();
      expect(screen.getByTestId('type-auth')).toBeInTheDocument();
      expect(screen.getByTestId('type-view')).toBeInTheDocument();
    });

    it('allows switching collection type', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('type-auth'));
      const authRadio = screen.getByTestId('type-auth').querySelector('input');
      expect(authRadio).toBeChecked();
    });

    it('shows empty fields state initially', () => {
      renderCreate();
      expect(screen.getByText(/No fields yet\. Click \u201CAdd Field\u201D to start\./)).toBeInTheDocument();
    });

    it('adds a new field when clicking Add Field', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('add-field'));
      expect(screen.getByTestId('field-editor-0')).toBeInTheDocument();
    });

    it('adds multiple fields sequentially', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('add-field'));
      await user.click(screen.getByTestId('add-field'));
      await user.click(screen.getByTestId('add-field'));

      expect(screen.getByTestId('field-editor-0')).toBeInTheDocument();
      expect(screen.getByTestId('field-editor-1')).toBeInTheDocument();
      expect(screen.getByTestId('field-editor-2')).toBeInTheDocument();
    });

    it('removes a field when clicking remove', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('add-field'));
      expect(screen.getByTestId('field-editor-0')).toBeInTheDocument();

      await user.click(screen.getByTestId('field-remove-0'));
      expect(screen.queryByTestId('field-editor-0')).not.toBeInTheDocument();
    });

    it('shows API preview panel', () => {
      renderCreate();
      expect(screen.getByTestId('api-preview')).toBeInTheDocument();
      expect(screen.getByText('API Endpoints')).toBeInTheDocument();
    });

    it('API preview shows auth endpoints when auth type is selected', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('type-auth'));
      expect(screen.getByText(/auth-with-password/)).toBeInTheDocument();
    });

    it('renders Cancel link back to collections list', () => {
      renderCreate();
      const cancelLink = screen.getByText('Cancel');
      expect(cancelLink.closest('a')).toHaveAttribute('href', '/_/collections');
    });

    // ── Validation ────────────────────────────────────────────────────

    it('shows validation error when saving without collection name', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('save-collection'));
      expect(screen.getByText('Collection name is required.')).toBeInTheDocument();
    });

    it('shows validation error for invalid collection name', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.type(screen.getByTestId('collection-name'), '123invalid');
      await user.click(screen.getByTestId('save-collection'));

      expect(screen.getByText(/must start with a letter or underscore/)).toBeInTheDocument();
    });

    it('shows validation error for empty field name', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.type(screen.getByTestId('collection-name'), 'test');
      await user.click(screen.getByTestId('add-field'));
      // Field name is empty by default
      await user.click(screen.getByTestId('save-collection'));

      expect(screen.getByText('Field name is required.')).toBeInTheDocument();
    });

    it('shows validation error for duplicate field names', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.type(screen.getByTestId('collection-name'), 'test');
      await user.click(screen.getByTestId('add-field'));
      await user.click(screen.getByTestId('add-field'));

      await user.type(screen.getByTestId('field-name-0'), 'title');
      await user.type(screen.getByTestId('field-name-1'), 'title');

      await user.click(screen.getByTestId('save-collection'));

      expect(screen.getByText('Duplicate field name.')).toBeInTheDocument();
    });

    it('clears validation error when user edits the name', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('save-collection'));
      expect(screen.getByText('Collection name is required.')).toBeInTheDocument();

      await user.type(screen.getByTestId('collection-name'), 'valid_name');
      expect(screen.queryByText('Collection name is required.')).not.toBeInTheDocument();
    });

    // ── Successful save ───────────────────────────────────────────────

    it('calls createCollection API on save in create mode', async () => {
      mockCreateCollection.mockResolvedValue({ ...EXISTING_COLLECTION, id: 'new_id' });
      renderCreate();
      const user = userEvent.setup();

      await user.type(screen.getByTestId('collection-name'), 'articles');
      await user.click(screen.getByTestId('save-collection'));

      await waitFor(() => {
        expect(mockCreateCollection).toHaveBeenCalledWith(
          expect.objectContaining({
            name: 'articles',
            type: 'base',
            fields: [],
          }),
        );
      });
    });

    it('shows saving state while API call is in progress', async () => {
      let resolveCreate!: (value: Collection) => void;
      mockCreateCollection.mockReturnValue(
        new Promise<Collection>((resolve) => { resolveCreate = resolve; }),
      );
      renderCreate();
      const user = userEvent.setup();

      await user.type(screen.getByTestId('collection-name'), 'test');
      await user.click(screen.getByTestId('save-collection'));

      expect(screen.getByText('Saving\u2026')).toBeInTheDocument();
      expect(screen.getByTestId('save-collection')).toBeDisabled();

      resolveCreate({ ...EXISTING_COLLECTION, id: 'new_id' });
    });

    it('shows save error on API failure', async () => {
      const errorBody: ErrorResponseBody = {
        code: 400,
        message: 'Collection name already exists.',
        data: { name: { code: 'unique', message: 'Name is taken.' } },
      };
      mockCreateCollection.mockRejectedValue(new ApiError(400, errorBody));
      renderCreate();
      const user = userEvent.setup();

      await user.type(screen.getByTestId('collection-name'), 'posts');
      await user.click(screen.getByTestId('save-collection'));

      await waitFor(() => {
        // General error from the API
        expect(screen.getByText('Collection name already exists.')).toBeInTheDocument();
        // Field-level error mapped from the response data
        expect(screen.getByText('Name is taken.')).toBeInTheDocument();
      });
    });

    // ── Field type selection ──────────────────────────────────────────

    it('new fields default to text type', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('add-field'));
      expect(screen.getByTestId('field-type-0')).toHaveValue('text');
    });

    it('allows changing field type', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('add-field'));
      await user.selectOptions(screen.getByTestId('field-type-0'), 'number');
      expect(screen.getByTestId('field-type-0')).toHaveValue('number');
    });

    it('shows type-specific options when type is number', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('add-field'));
      await user.selectOptions(screen.getByTestId('field-type-0'), 'number');

      expect(screen.getByTestId('number-min')).toBeInTheDocument();
      expect(screen.getByTestId('number-max')).toBeInTheDocument();
    });

    it('shows type-specific options when type is select', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('add-field'));
      await user.selectOptions(screen.getByTestId('field-type-0'), 'select');

      expect(screen.getByTestId('select-values')).toBeInTheDocument();
    });

    it('shows no additional options for bool type', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('add-field'));
      await user.selectOptions(screen.getByTestId('field-type-0'), 'bool');

      expect(screen.getByText(/No additional options/)).toBeInTheDocument();
    });

    // ── Required and Unique toggles ─────────────────────────────────

    it('allows toggling field required', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('add-field'));
      const requiredCheckbox = screen.getByTestId('field-required-0');
      expect(requiredCheckbox).not.toBeChecked();

      await user.click(requiredCheckbox);
      expect(requiredCheckbox).toBeChecked();
    });

    it('allows toggling field unique', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('add-field'));
      const uniqueCheckbox = screen.getByTestId('field-unique-0');
      expect(uniqueCheckbox).not.toBeChecked();

      await user.click(uniqueCheckbox);
      expect(uniqueCheckbox).toBeChecked();
    });

    // ── Field reorder ───────────────────────────────────────────────

    it('allows reordering fields up and down', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('add-field'));
      await user.click(screen.getByTestId('add-field'));

      await user.type(screen.getByTestId('field-name-0'), 'first');
      await user.type(screen.getByTestId('field-name-1'), 'second');

      // Move second field up
      await user.click(screen.getByTestId('field-move-up-1'));

      // After move, the first field editor should now have the name "second"
      expect(screen.getByTestId('field-name-0')).toHaveValue('second');
      expect(screen.getByTestId('field-name-1')).toHaveValue('first');
    });

    it('disables move up for first field and move down for last field', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('add-field'));
      await user.click(screen.getByTestId('add-field'));

      expect(screen.getByTestId('field-move-up-0')).toBeDisabled();
      expect(screen.getByTestId('field-move-down-1')).toBeDisabled();
    });

    // ── Full create flow ────────────────────────────────────────────

    it('creates a collection with fields and correct payload', async () => {
      mockCreateCollection.mockResolvedValue({ ...EXISTING_COLLECTION, id: 'new_col' });
      renderCreate();
      const user = userEvent.setup();

      // Set name and type
      await user.type(screen.getByTestId('collection-name'), 'articles');
      await user.click(screen.getByTestId('type-auth'));

      // Add a text field
      await user.click(screen.getByTestId('add-field'));
      await user.type(screen.getByTestId('field-name-0'), 'title');
      await user.click(screen.getByTestId('field-required-0'));

      // Add a number field
      await user.click(screen.getByTestId('add-field'));
      await user.type(screen.getByTestId('field-name-1'), 'views');
      await user.selectOptions(screen.getByTestId('field-type-1'), 'number');

      // Save
      await user.click(screen.getByTestId('save-collection'));

      await waitFor(() => {
        expect(mockCreateCollection).toHaveBeenCalledWith(
          expect.objectContaining({
            name: 'articles',
            type: 'auth',
            fields: expect.arrayContaining([
              expect.objectContaining({
                name: 'title',
                type: expect.objectContaining({ type: 'text' }),
                required: true,
                sortOrder: 0,
              }),
              expect.objectContaining({
                name: 'views',
                type: expect.objectContaining({ type: 'number' }),
                required: false,
                sortOrder: 1,
              }),
            ]),
          }),
        );
      });
    });
  });

  // ── Edit mode ─────────────────────────────────────────────────────────

  describe('edit mode', () => {
    function renderEdit() {
      return render(<CollectionEditorPage mode="edit" collectionId="col_posts" />);
    }

    it('shows loading state while fetching collection', () => {
      mockGetCollection.mockReturnValue(new Promise(() => {}));
      renderEdit();
      expect(screen.getByTestId('editor-loading')).toBeInTheDocument();
    });

    it('loads and displays existing collection data', async () => {
      mockGetCollection.mockResolvedValue(EXISTING_COLLECTION);
      renderEdit();

      await waitFor(() => {
        expect(screen.getByTestId('collection-name')).toHaveValue('posts');
      });

      // Check fields are loaded
      expect(screen.getByTestId('field-name-0')).toHaveValue('title');
      expect(screen.getByTestId('field-name-1')).toHaveValue('body');
    });

    it('shows error when collection fails to load', async () => {
      const errorBody: ErrorResponseBody = {
        code: 404,
        message: 'Collection not found.',
        data: {},
      };
      mockGetCollection.mockRejectedValue(new ApiError(404, errorBody));
      renderEdit();

      await waitFor(() => {
        expect(screen.getByText('Collection not found.')).toBeInTheDocument();
      });

      expect(screen.getByText('Back to collections')).toBeInTheDocument();
    });

    it('shows "Save Changes" button in edit mode', async () => {
      mockGetCollection.mockResolvedValue(EXISTING_COLLECTION);
      renderEdit();

      await waitFor(() => {
        expect(screen.getByText('Save Changes')).toBeInTheDocument();
      });
    });

    it('calls updateCollection API on save in edit mode', async () => {
      mockGetCollection.mockResolvedValue(EXISTING_COLLECTION);
      mockUpdateCollection.mockResolvedValue(EXISTING_COLLECTION);
      renderEdit();

      await waitFor(() => {
        expect(screen.getByTestId('collection-name')).toHaveValue('posts');
      });

      const user = userEvent.setup();
      await user.click(screen.getByTestId('save-collection'));

      await waitFor(() => {
        expect(mockUpdateCollection).toHaveBeenCalledWith(
          'col_posts',
          expect.objectContaining({ name: 'posts', type: 'base' }),
        );
      });
    });

    it('shows success message after saving changes', async () => {
      mockGetCollection.mockResolvedValue(EXISTING_COLLECTION);
      mockUpdateCollection.mockResolvedValue(EXISTING_COLLECTION);
      renderEdit();

      await waitFor(() => {
        expect(screen.getByTestId('collection-name')).toHaveValue('posts');
      });

      const user = userEvent.setup();
      await user.click(screen.getByTestId('save-collection'));

      await waitFor(() => {
        expect(screen.getByText('Collection saved successfully.')).toBeInTheDocument();
      });
    });

    it('allows modifying and saving existing fields', async () => {
      mockGetCollection.mockResolvedValue(EXISTING_COLLECTION);
      mockUpdateCollection.mockResolvedValue(EXISTING_COLLECTION);
      renderEdit();

      await waitFor(() => {
        expect(screen.getByTestId('field-name-0')).toHaveValue('title');
      });

      const user = userEvent.setup();

      // Clear and type new name
      await user.clear(screen.getByTestId('field-name-0'));
      await user.type(screen.getByTestId('field-name-0'), 'headline');

      await user.click(screen.getByTestId('save-collection'));

      await waitFor(() => {
        expect(mockUpdateCollection).toHaveBeenCalledWith(
          'col_posts',
          expect.objectContaining({
            fields: expect.arrayContaining([
              expect.objectContaining({ name: 'headline' }),
            ]),
          }),
        );
      });
    });

    it('can add fields to existing collection', async () => {
      mockGetCollection.mockResolvedValue(EXISTING_COLLECTION);
      mockUpdateCollection.mockResolvedValue(EXISTING_COLLECTION);
      renderEdit();

      await waitFor(() => {
        expect(screen.getByTestId('field-editor-0')).toBeInTheDocument();
        expect(screen.getByTestId('field-editor-1')).toBeInTheDocument();
      });

      const user = userEvent.setup();
      await user.click(screen.getByTestId('add-field'));
      expect(screen.getByTestId('field-editor-2')).toBeInTheDocument();
    });

    it('can remove fields from existing collection', async () => {
      mockGetCollection.mockResolvedValue(EXISTING_COLLECTION);
      renderEdit();

      await waitFor(() => {
        expect(screen.getByTestId('field-editor-1')).toBeInTheDocument();
      });

      const user = userEvent.setup();
      await user.click(screen.getByTestId('field-remove-1'));

      expect(screen.queryByTestId('field-editor-1')).not.toBeInTheDocument();
    });
  });

  // ── API Preview ───────────────────────────────────────────────────────

  describe('API Preview', () => {
    it('updates preview when collection name changes', async () => {
      render(<CollectionEditorPage mode="create" />);
      const user = userEvent.setup();

      await user.type(screen.getByTestId('collection-name'), 'articles');

      const preview = screen.getByTestId('api-preview');
      const endpoints = within(preview).getAllByText(/\/api\/collections\/articles\/records/);
      expect(endpoints.length).toBeGreaterThan(0);
    });

    it('shows CRUD endpoints for base type', () => {
      render(<CollectionEditorPage mode="create" />);
      const preview = screen.getByTestId('api-preview');
      expect(within(preview).getAllByText('GET').length).toBeGreaterThanOrEqual(2);
      expect(within(preview).getByText('POST')).toBeInTheDocument();
      expect(within(preview).getByText('PATCH')).toBeInTheDocument();
      expect(within(preview).getByText('DELETE')).toBeInTheDocument();
    });

    it('shows auth-specific endpoints for auth type', async () => {
      render(<CollectionEditorPage mode="create" />);
      const user = userEvent.setup();

      await user.click(screen.getByTestId('type-auth'));

      const preview = screen.getByTestId('api-preview');
      expect(within(preview).getByText(/auth-with-password/)).toBeInTheDocument();
      expect(within(preview).getByText(/auth-refresh/)).toBeInTheDocument();
    });
  });

  // ── Rules Editor Integration ────────────────────────────────────────────

  describe('Rules Editor', () => {
    it('renders the rules editor section in create mode', () => {
      render(<CollectionEditorPage mode="create" />);
      expect(screen.getByTestId('rules-editor')).toBeInTheDocument();
      expect(screen.getByText('API Rules')).toBeInTheDocument();
    });

    it('shows all rules as locked by default in create mode', () => {
      render(<CollectionEditorPage mode="create" />);
      expect(screen.getByTestId('rule-locked-listRule')).toBeInTheDocument();
      expect(screen.getByTestId('rule-locked-viewRule')).toBeInTheDocument();
      expect(screen.getByTestId('rule-locked-createRule')).toBeInTheDocument();
      expect(screen.getByTestId('rule-locked-updateRule')).toBeInTheDocument();
      expect(screen.getByTestId('rule-locked-deleteRule')).toBeInTheDocument();
    });

    it('can unlock a rule and type an expression', async () => {
      render(<CollectionEditorPage mode="create" />);
      const user = userEvent.setup();

      // Unlock listRule
      await user.click(screen.getByTestId('rule-toggle-listRule'));

      // Now the input should be visible
      const input = screen.getByTestId('rule-input-listRule');
      expect(input).toBeInTheDocument();

      // Type an expression
      await user.type(input, '@request.auth.id != ""');
      expect(input).toHaveValue('@request.auth.id != ""');
    });

    it('includes rules in the save payload for create', async () => {
      mockCreateCollection.mockResolvedValue({ ...EXISTING_COLLECTION, id: 'new_id' });
      render(<CollectionEditorPage mode="create" />);
      const user = userEvent.setup();

      // Set collection name
      await user.type(screen.getByTestId('collection-name'), 'articles');

      // Unlock and set listRule
      await user.click(screen.getByTestId('rule-toggle-listRule'));
      await user.type(screen.getByTestId('rule-input-listRule'), '@request.auth.id != ""');

      // Save
      await user.click(screen.getByTestId('save-collection'));

      await waitFor(() => {
        expect(mockCreateCollection).toHaveBeenCalledWith(
          expect.objectContaining({
            name: 'articles',
            rules: expect.objectContaining({
              listRule: '@request.auth.id != ""',
              viewRule: null,
              createRule: null,
              updateRule: null,
              deleteRule: null,
            }),
          }),
        );
      });
    });

    it('loads existing rules in edit mode', async () => {
      const collectionWithRules = makeCollection({
        id: 'col_posts',
        name: 'posts',
        rules: {
          listRule: '',
          viewRule: '@request.auth.id != ""',
          createRule: '@request.auth.id != ""',
          updateRule: '@request.auth.id = id',
          deleteRule: null,
        },
      });
      mockGetCollection.mockResolvedValue(collectionWithRules);
      render(<CollectionEditorPage mode="edit" collectionId="col_posts" />);

      await waitFor(() => {
        expect(screen.getByTestId('collection-name')).toHaveValue('posts');
      });

      // listRule should be unlocked and empty (public)
      expect(screen.getByTestId('rule-input-listRule')).toBeInTheDocument();
      const listBadge = within(screen.getByTestId('rule-field-listRule')).getByTestId('rule-badge-listRule');
      expect(listBadge).toHaveTextContent(/PUBLIC/i);

      // viewRule should show the expression
      const viewInput = screen.getByTestId('rule-input-viewRule') as HTMLTextAreaElement;
      expect(viewInput.value).toBe('@request.auth.id != ""');

      // deleteRule should be locked
      expect(screen.getByTestId('rule-locked-deleteRule')).toBeInTheDocument();
    });

    it('includes updated rules in the save payload for edit', async () => {
      const collectionWithRules = makeCollection({
        id: 'col_posts',
        name: 'posts',
        rules: {
          listRule: '',
          viewRule: '@request.auth.id != ""',
          createRule: null,
          updateRule: null,
          deleteRule: null,
        },
      });
      mockGetCollection.mockResolvedValue(collectionWithRules);
      mockUpdateCollection.mockResolvedValue(collectionWithRules);
      render(<CollectionEditorPage mode="edit" collectionId="col_posts" />);

      await waitFor(() => {
        expect(screen.getByTestId('collection-name')).toHaveValue('posts');
      });

      const user = userEvent.setup();

      // Unlock createRule and set expression
      await user.click(screen.getByTestId('rule-toggle-createRule'));
      await user.type(screen.getByTestId('rule-input-createRule'), '@request.auth.id != ""');

      // Save
      await user.click(screen.getByTestId('save-collection'));

      await waitFor(() => {
        expect(mockUpdateCollection).toHaveBeenCalledWith(
          'col_posts',
          expect.objectContaining({
            rules: expect.objectContaining({
              listRule: '',
              viewRule: '@request.auth.id != ""',
              createRule: '@request.auth.id != ""',
              updateRule: null,
              deleteRule: null,
            }),
          }),
        );
      });
    });

    it('shows helper docs toggle button', () => {
      render(<CollectionEditorPage mode="create" />);
      expect(screen.getByTestId('rules-helper-toggle')).toBeInTheDocument();
    });

    it('can toggle helper documentation', async () => {
      render(<CollectionEditorPage mode="create" />);
      const user = userEvent.setup();

      await user.click(screen.getByTestId('rules-helper-toggle'));
      expect(screen.getByTestId('rules-helper-docs')).toBeInTheDocument();
      expect(screen.getByText('Rule Expression Reference')).toBeInTheDocument();
    });
  });

  // ── Auth Collection UI ──────────────────────────────────────────────────

  describe('Auth Collection UI', () => {
    function renderCreate() {
      return render(<CollectionEditorPage mode="create" />);
    }

    // ── Auth system fields display ──────────────────────────────────

    it('does not show auth fields display for base type', () => {
      renderCreate();
      expect(screen.queryByTestId('auth-fields-display')).not.toBeInTheDocument();
    });

    it('shows auth fields display when auth type is selected', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('type-auth'));
      expect(screen.getByTestId('auth-fields-display')).toBeInTheDocument();
    });

    it('shows all auth system fields as read-only when type is auth', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('type-auth'));

      expect(screen.getByTestId('auth-field-email')).toBeInTheDocument();
      expect(screen.getByTestId('auth-field-emailVisibility')).toBeInTheDocument();
      expect(screen.getByTestId('auth-field-verified')).toBeInTheDocument();
      expect(screen.getByTestId('auth-field-password')).toBeInTheDocument();
      expect(screen.getByTestId('auth-field-tokenKey')).toBeInTheDocument();
    });

    it('shows lock icons on auth system fields', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('type-auth'));

      expect(screen.getByTestId('auth-field-lock-email')).toBeInTheDocument();
      expect(screen.getByTestId('auth-field-lock-password')).toBeInTheDocument();
    });

    it('hides auth fields when switching back to base type', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('type-auth'));
      expect(screen.getByTestId('auth-fields-display')).toBeInTheDocument();

      await user.click(screen.getByTestId('type-base'));
      expect(screen.queryByTestId('auth-fields-display')).not.toBeInTheDocument();
    });

    it('shows "Additional Fields" heading when type is auth', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('type-auth'));
      expect(screen.getByText('Additional Fields')).toBeInTheDocument();
    });

    it('shows "Fields" heading when type is base', () => {
      renderCreate();
      expect(screen.getByText('Fields')).toBeInTheDocument();
    });

    // ── Auth settings editor ────────────────────────────────────────

    it('does not show auth settings for base type', () => {
      renderCreate();
      expect(screen.queryByTestId('auth-settings-editor')).not.toBeInTheDocument();
    });

    it('shows auth settings editor when auth type is selected', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('type-auth'));
      expect(screen.getByTestId('auth-settings-editor')).toBeInTheDocument();
    });

    it('shows auth method toggles in auth settings', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('type-auth'));

      expect(screen.getByTestId('allow-email-auth')).toBeInTheDocument();
      expect(screen.getByTestId('allow-oauth2-auth')).toBeInTheDocument();
      expect(screen.getByTestId('allow-otp-auth')).toBeInTheDocument();
      expect(screen.getByTestId('mfa-enabled')).toBeInTheDocument();
    });

    it('shows password requirements in auth settings', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('type-auth'));
      expect(screen.getByTestId('min-password-length')).toBeInTheDocument();
    });

    it('hides auth settings when switching back to base', async () => {
      renderCreate();
      const user = userEvent.setup();

      await user.click(screen.getByTestId('type-auth'));
      expect(screen.getByTestId('auth-settings-editor')).toBeInTheDocument();

      await user.click(screen.getByTestId('type-base'));
      expect(screen.queryByTestId('auth-settings-editor')).not.toBeInTheDocument();
    });

    // ── Auth settings in save payload ───────────────────────────────

    it('includes authOptions in save payload for auth collections', async () => {
      mockCreateCollection.mockResolvedValue({ ...EXISTING_COLLECTION, id: 'new_auth' });
      renderCreate();
      const user = userEvent.setup();

      await user.type(screen.getByTestId('collection-name'), 'users');
      await user.click(screen.getByTestId('type-auth'));

      await user.click(screen.getByTestId('save-collection'));

      await waitFor(() => {
        expect(mockCreateCollection).toHaveBeenCalledWith(
          expect.objectContaining({
            name: 'users',
            type: 'auth',
            authOptions: expect.objectContaining({
              allowEmailAuth: true,
              allowOauth2Auth: false,
              allowOtpAuth: false,
              mfaEnabled: false,
              minPasswordLength: 8,
              requireEmail: true,
              identityFields: ['email'],
            }),
          }),
        );
      });
    });

    it('does not include authOptions in save payload for base collections', async () => {
      mockCreateCollection.mockResolvedValue({ ...EXISTING_COLLECTION, id: 'new_base' });
      renderCreate();
      const user = userEvent.setup();

      await user.type(screen.getByTestId('collection-name'), 'posts');
      await user.click(screen.getByTestId('save-collection'));

      await waitFor(() => {
        const payload = mockCreateCollection.mock.calls[0][0];
        expect(payload).not.toHaveProperty('authOptions');
      });
    });

    it('includes modified auth settings in save payload', async () => {
      mockCreateCollection.mockResolvedValue({ ...EXISTING_COLLECTION, id: 'new_auth' });
      renderCreate();
      const user = userEvent.setup();

      await user.type(screen.getByTestId('collection-name'), 'users');
      await user.click(screen.getByTestId('type-auth'));

      // Enable OAuth2
      await user.click(screen.getByTestId('allow-oauth2-auth'));
      // Change password length via fireEvent (avoids clear/type issues with number inputs)
      fireEvent.change(screen.getByTestId('min-password-length'), { target: { value: '12' } });

      await user.click(screen.getByTestId('save-collection'));

      await waitFor(() => {
        expect(mockCreateCollection).toHaveBeenCalledWith(
          expect.objectContaining({
            authOptions: expect.objectContaining({
              allowOauth2Auth: true,
              minPasswordLength: 12,
            }),
          }),
        );
      });
    });

    // ── Edit mode with auth collection ──────────────────────────────

    it('loads and displays auth options from existing auth collection', async () => {
      const authCollection = makeCollection({
        id: 'col_users',
        name: 'users',
        type: 'auth',
        authOptions: {
          allowEmailAuth: true,
          allowOauth2Auth: true,
          allowOtpAuth: false,
          requireEmail: true,
          mfaEnabled: false,
          mfaDuration: 0,
          minPasswordLength: 10,
          identityFields: ['email', 'username'],
          manageRule: null,
        },
      });
      mockGetCollection.mockResolvedValue(authCollection);
      render(<CollectionEditorPage mode="edit" collectionId="col_users" />);

      await waitFor(() => {
        expect(screen.getByTestId('collection-name')).toHaveValue('users');
      });

      // Auth fields display should be visible
      expect(screen.getByTestId('auth-fields-display')).toBeInTheDocument();
      // Auth settings editor should be visible
      expect(screen.getByTestId('auth-settings-editor')).toBeInTheDocument();

      // Auth options should reflect loaded values
      expect(screen.getByTestId('allow-email-auth')).toHaveAttribute('aria-checked', 'true');
      expect(screen.getByTestId('allow-oauth2-auth')).toHaveAttribute('aria-checked', 'true');
      expect(screen.getByTestId('allow-otp-auth')).toHaveAttribute('aria-checked', 'false');
      expect(screen.getByTestId('min-password-length')).toHaveValue(10);
      expect(screen.getByTestId('identity-fields')).toHaveValue('email, username');
    });

    it('saves updated auth options in edit mode', async () => {
      const authCollection = makeCollection({
        id: 'col_users',
        name: 'users',
        type: 'auth',
        authOptions: {
          allowEmailAuth: true,
          allowOauth2Auth: false,
          allowOtpAuth: false,
          requireEmail: true,
          mfaEnabled: false,
          mfaDuration: 0,
          minPasswordLength: 8,
          identityFields: ['email'],
          manageRule: null,
        },
      });
      mockGetCollection.mockResolvedValue(authCollection);
      mockUpdateCollection.mockResolvedValue(authCollection);
      render(<CollectionEditorPage mode="edit" collectionId="col_users" />);

      await waitFor(() => {
        expect(screen.getByTestId('collection-name')).toHaveValue('users');
      });

      const user = userEvent.setup();

      // Enable OTP
      await user.click(screen.getByTestId('allow-otp-auth'));

      // Save
      await user.click(screen.getByTestId('save-collection'));

      await waitFor(() => {
        expect(mockUpdateCollection).toHaveBeenCalledWith(
          'col_users',
          expect.objectContaining({
            authOptions: expect.objectContaining({
              allowOtpAuth: true,
            }),
          }),
        );
      });
    });
  });
});
