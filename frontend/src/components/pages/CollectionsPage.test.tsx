import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { CollectionsPage } from './CollectionsPage';
import { ApiError } from '../../lib/api';
import type { Collection, ListResponse, ErrorResponseBody } from '../../lib/api/types';

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

const COLLECTIONS: Collection[] = [
  makeCollection({
    id: 'col1',
    name: 'posts',
    type: 'base',
    fields: [
      { id: 'f1', name: 'title', type: { type: 'text', minLength: 0, maxLength: 500, pattern: null, searchable: true }, required: true, unique: false, sortOrder: 0 },
      { id: 'f2', name: 'body', type: { type: 'editor', maxLength: 50000, searchable: true }, required: false, unique: false, sortOrder: 1 },
    ],
  }),
  makeCollection({
    id: 'col2',
    name: 'users',
    type: 'auth',
    fields: [
      { id: 'f3', name: 'name', type: { type: 'text', minLength: 0, maxLength: 200, pattern: null, searchable: true }, required: true, unique: false, sortOrder: 0 },
    ],
  }),
  makeCollection({
    id: 'col3',
    name: 'post_stats',
    type: 'view',
    fields: [],
    viewQuery: 'SELECT id, COUNT(*) as total FROM posts GROUP BY id',
  }),
];

function makeListResponse(items: Collection[]): ListResponse<Collection> {
  return {
    page: 1,
    perPage: 30,
    totalPages: 1,
    totalItems: items.length,
    items,
  };
}

// ── Mocks ────────────────────────────────────────────────────────────────────

const mockListCollections = vi.fn();
const mockDeleteCollection = vi.fn();
const mockListRecords = vi.fn();

vi.mock('../../lib/auth/client', () => ({
  client: {
    listCollections: (...args: unknown[]) => mockListCollections(...args),
    deleteCollection: (...args: unknown[]) => mockDeleteCollection(...args),
    listRecords: (...args: unknown[]) => mockListRecords(...args),
    get isAuthenticated() {
      return true;
    },
    get token() {
      return 'mock-token';
    },
    logout: vi.fn(),
  },
}));

// Mock window.location for AuthProvider / DashboardLayout
Object.defineProperty(window, 'location', {
  value: { href: '', pathname: '/_/', origin: 'http://localhost:8090' },
  writable: true,
});

// ── Helpers ──────────────────────────────────────────────────────────────────

function renderPage() {
  return render(<CollectionsPage />);
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('CollectionsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockListCollections.mockResolvedValue(makeListResponse(COLLECTIONS));
    // Default: collections have 10 records
    mockListRecords.mockResolvedValue(makeListResponse([]));
    mockListRecords.mockImplementation(() =>
      Promise.resolve({ page: 1, perPage: 1, totalPages: 1, totalItems: 10, items: [] }),
    );
  });

  // ── Loading state ────────────────────────────────────────────────────────

  it('shows loading skeleton while fetching collections', () => {
    // Keep the promise pending
    mockListCollections.mockReturnValue(new Promise(() => {}));
    renderPage();

    expect(screen.getByTestId('loading-skeleton')).toBeInTheDocument();
  });

  // ── Successful data display ──────────────────────────────────────────────

  it('displays all collections with names after loading', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('posts')).toBeInTheDocument();
      expect(screen.getByText('users')).toBeInTheDocument();
      expect(screen.getByText('post_stats')).toBeInTheDocument();
    });
  });

  it('displays collection type badges correctly', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('BASE')).toBeInTheDocument();
      expect(screen.getByText('AUTH')).toBeInTheDocument();
      expect(screen.getByText('VIEW')).toBeInTheDocument();
    });
  });

  it('displays field counts for each collection', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('2 fields')).toBeInTheDocument(); // posts
      expect(screen.getByText('1 field')).toBeInTheDocument();  // users (singular)
      expect(screen.getByText('0 fields')).toBeInTheDocument(); // post_stats
    });
  });

  it('shows correct summary count', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('3 OF 3 COLLECTIONS')).toBeInTheDocument();
    });
  });

  it('renders table with column headers', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('NAME')).toBeInTheDocument();
      expect(screen.getByText('TYPE')).toBeInTheDocument();
      expect(screen.getByText('FIELDS')).toBeInTheDocument();
      expect(screen.getByText('ACTIONS')).toBeInTheDocument();
    });
  });

  it('renders edit and delete actions for each collection', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByLabelText('Edit posts')).toBeInTheDocument();
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
      expect(screen.getByLabelText('Edit users')).toBeInTheDocument();
      expect(screen.getByLabelText('Delete users')).toBeInTheDocument();
      expect(screen.getByLabelText('Edit post_stats')).toBeInTheDocument();
      expect(screen.getByLabelText('Delete post_stats')).toBeInTheDocument();
    });
  });

  it('renders edit links with correct href', async () => {
    renderPage();

    await waitFor(() => {
      const editLink = screen.getByLabelText('Edit posts');
      expect(editLink).toHaveAttribute('href', '/_/collections/col1/edit');
    });
  });

  it('renders collection name links with correct href', async () => {
    renderPage();

    await waitFor(() => {
      const link = screen.getByText('posts').closest('a');
      expect(link).toHaveAttribute('href', '/_/collections/col1');
    });
  });

  it('renders New Collection button', async () => {
    renderPage();

    const newBtn = screen.getByText('NEW COLLECTION');
    expect(newBtn.closest('a')).toHaveAttribute('href', '/_/collections/new');
  });

  // ── Empty state ──────────────────────────────────────────────────────────

  it('shows empty state when no collections exist', async () => {
    mockListCollections.mockResolvedValue(makeListResponse([]));
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('NO COLLECTIONS')).toBeInTheDocument();
      expect(screen.getByText('Get started by creating your first collection.')).toBeInTheDocument();
    });
  });

  // ── Error state ──────────────────────────────────────────────────────────

  it('shows error message when API returns an error', async () => {
    const errorBody: ErrorResponseBody = {
      code: 500,
      message: 'Internal server error.',
      data: {},
    };
    mockListCollections.mockRejectedValue(new ApiError(500, errorBody));
    renderPage();

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Internal server error.');
    });
  });

  it('shows generic error on network failure', async () => {
    mockListCollections.mockRejectedValue(new TypeError('Failed to fetch'));
    renderPage();

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent(
        'Unable to connect to the server. Please try again.',
      );
    });
  });

  it('allows retrying after an error', async () => {
    const errorBody: ErrorResponseBody = {
      code: 500,
      message: 'Server error.',
      data: {},
    };
    mockListCollections.mockRejectedValueOnce(new ApiError(500, errorBody));
    mockListCollections.mockResolvedValueOnce(makeListResponse(COLLECTIONS));

    renderPage();

    await waitFor(() => {
      expect(screen.getByText('RETRY')).toBeInTheDocument();
    });

    const user = userEvent.setup();
    await user.click(screen.getByText('RETRY'));

    await waitFor(() => {
      expect(screen.getByText('posts')).toBeInTheDocument();
    });

    expect(mockListCollections).toHaveBeenCalledTimes(2);
  });

  // ── Search / filter ────────────────────────────────────────────────────

  it('filters collections by name when searching', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('posts')).toBeInTheDocument();
    });

    await user.type(screen.getByLabelText('SEARCH'), 'users');

    expect(screen.getByText('users')).toBeInTheDocument();
    expect(screen.queryByText('posts')).not.toBeInTheDocument();
    expect(screen.queryByText('post_stats')).not.toBeInTheDocument();
  });

  it('filters collections by type when searching', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('posts')).toBeInTheDocument();
    });

    await user.type(screen.getByLabelText('SEARCH'), 'auth');

    expect(screen.getByText('users')).toBeInTheDocument();
    expect(screen.queryByText('posts')).not.toBeInTheDocument();
  });

  it('search is case-insensitive', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('posts')).toBeInTheDocument();
    });

    await user.type(screen.getByLabelText('SEARCH'), 'POSTS');

    expect(screen.getByText('posts')).toBeInTheDocument();
  });

  it('shows empty search state when no collections match', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('posts')).toBeInTheDocument();
    });

    await user.type(screen.getByLabelText('SEARCH'), 'nonexistent');

    expect(screen.getByText('No collections match your search.')).toBeInTheDocument();
  });

  it('allows clearing search from empty search state', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('posts')).toBeInTheDocument();
    });

    await user.type(screen.getByLabelText('SEARCH'), 'nonexistent');
    expect(screen.getByText('No collections match your search.')).toBeInTheDocument();

    await user.click(screen.getByText('CLEAR SEARCH'));

    expect(screen.getByText('posts')).toBeInTheDocument();
    expect(screen.getByText('users')).toBeInTheDocument();
  });

  it('updates summary count when filtering', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('3 OF 3 COLLECTIONS')).toBeInTheDocument();
    });

    await user.type(screen.getByLabelText('SEARCH'), 'post');

    // "posts" and "post_stats" match
    expect(screen.getByText('2 OF 3 COLLECTIONS')).toBeInTheDocument();
  });

  // ── Delete flow ──────────────────────────────────────────────────────────

  it('opens delete confirmation dialog when clicking delete', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    expect(screen.getByText('DELETE COLLECTION')).toBeInTheDocument();
    expect(screen.getByText(/Are you sure you want to delete/)).toBeInTheDocument();
    expect(screen.getByText('posts', { selector: 'strong' })).toBeInTheDocument();
  });

  it('closes delete dialog when clicking cancel', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));
    expect(screen.getByText('DELETE COLLECTION')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'CANCEL' }));
    expect(screen.queryByText('DELETE COLLECTION')).not.toBeInTheDocument();
  });

  it('deletes collection and removes from list on confirm', async () => {
    mockDeleteCollection.mockResolvedValue(undefined);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    // Wait for record count to load before clicking delete
    await waitFor(() => {
      expect(screen.queryByTestId('loading-record-count')).not.toBeInTheDocument();
    });

    await user.click(screen.getByTestId('confirm-delete-btn'));

    await waitFor(() => {
      expect(mockDeleteCollection).toHaveBeenCalledWith('col1');
    });

    await waitFor(() => {
      // Dialog should close and collection removed — only "posts" in table is gone
      expect(screen.queryByText('DELETE COLLECTION')).not.toBeInTheDocument();
    });

    // Other collections still present
    expect(screen.getByText('users')).toBeInTheDocument();
    expect(screen.getByText('post_stats')).toBeInTheDocument();
  });

  it('shows deleting state in dialog during deletion', async () => {
    let resolveDelete!: () => void;
    mockDeleteCollection.mockReturnValue(
      new Promise<void>((resolve) => {
        resolveDelete = resolve;
      }),
    );

    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    await waitFor(() => {
      expect(screen.queryByTestId('loading-record-count')).not.toBeInTheDocument();
    });

    await user.click(screen.getByTestId('confirm-delete-btn'));

    expect(screen.getByText(`DELETING\u2026`)).toBeInTheDocument();

    // Resolve to clean up
    resolveDelete();
    await waitFor(() => {
      expect(screen.queryByText('DELETE COLLECTION')).not.toBeInTheDocument();
    });
  });

  it('shows error when delete fails', async () => {
    const errorBody: ErrorResponseBody = {
      code: 400,
      message: 'Collection has dependent relations.',
      data: {},
    };
    mockDeleteCollection.mockRejectedValue(new ApiError(400, errorBody));

    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    await waitFor(() => {
      expect(screen.queryByTestId('loading-record-count')).not.toBeInTheDocument();
    });

    await user.click(screen.getByTestId('confirm-delete-btn'));

    await waitFor(() => {
      expect(screen.getByText('Collection has dependent relations.')).toBeInTheDocument();
    });

    // Collection should still be in the list (appears in table link and dialog strong)
    expect(screen.getAllByText('posts').length).toBeGreaterThanOrEqual(1);
  });

  // ── Record count warning ────────────────────────────────────────────────

  it('shows loading state while fetching record count', async () => {
    // Keep the record count request pending
    mockListRecords.mockReturnValue(new Promise(() => {}));

    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    expect(screen.getByTestId('loading-record-count')).toBeInTheDocument();
  });

  it('shows record count warning when collection has records', async () => {
    mockListRecords.mockResolvedValue({
      page: 1,
      perPage: 1,
      totalPages: 5,
      totalItems: 25,
      items: [],
    });

    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    await waitFor(() => {
      const warning = screen.getByTestId('record-count-warning');
      expect(warning).toHaveTextContent('25 records will be permanently deleted.');
    });
  });

  it('uses singular "record" for single record', async () => {
    mockListRecords.mockResolvedValue({
      page: 1,
      perPage: 1,
      totalPages: 1,
      totalItems: 1,
      items: [],
    });

    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    await waitFor(() => {
      const warning = screen.getByTestId('record-count-warning');
      expect(warning).toHaveTextContent('1 record will be permanently deleted.');
    });
  });

  it('shows "no records" note when collection is empty', async () => {
    mockListRecords.mockResolvedValue({
      page: 1,
      perPage: 1,
      totalPages: 0,
      totalItems: 0,
      items: [],
    });

    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    await waitFor(() => {
      expect(screen.getByTestId('no-records-note')).toHaveTextContent('This collection has no records.');
    });
  });

  it('fetches record count with perPage=1 for the target collection', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    await waitFor(() => {
      expect(mockListRecords).toHaveBeenCalledWith('posts', { perPage: 1 });
    });
  });

  // ── Name confirmation for dangerous collections ──────────────────────────

  it('requires typing collection name for collections with many records', async () => {
    mockListRecords.mockResolvedValue({
      page: 1,
      perPage: 1,
      totalPages: 100,
      totalItems: 500,
      items: [],
    });

    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    await waitFor(() => {
      expect(screen.getByTestId('confirm-name-input')).toBeInTheDocument();
    });

    // Delete button should be disabled until name is typed
    expect(screen.getByTestId('confirm-delete-btn')).toBeDisabled();
  });

  it('enables delete button after typing correct collection name', async () => {
    mockListRecords.mockResolvedValue({
      page: 1,
      perPage: 1,
      totalPages: 100,
      totalItems: 500,
      items: [],
    });

    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    await waitFor(() => {
      expect(screen.getByTestId('confirm-name-input')).toBeInTheDocument();
    });

    // Type wrong name
    await user.type(screen.getByTestId('confirm-name-input'), 'wrong');
    expect(screen.getByTestId('confirm-delete-btn')).toBeDisabled();

    // Clear and type correct name
    await user.clear(screen.getByTestId('confirm-name-input'));
    await user.type(screen.getByTestId('confirm-name-input'), 'posts');
    expect(screen.getByTestId('confirm-delete-btn')).toBeEnabled();
  });

  it('does not require name confirmation for small collections', async () => {
    mockListRecords.mockResolvedValue({
      page: 1,
      perPage: 1,
      totalPages: 1,
      totalItems: 5,
      items: [],
    });

    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    await waitFor(() => {
      expect(screen.queryByTestId('loading-record-count')).not.toBeInTheDocument();
    });

    // No name input should be present
    expect(screen.queryByTestId('confirm-name-input')).not.toBeInTheDocument();

    // Delete button should be enabled
    expect(screen.getByTestId('confirm-delete-btn')).toBeEnabled();
  });

  it('does not require name confirmation at exactly the threshold boundary', async () => {
    mockListRecords.mockResolvedValue({
      page: 1,
      perPage: 1,
      totalPages: 1,
      totalItems: 49,
      items: [],
    });

    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    await waitFor(() => {
      expect(screen.queryByTestId('loading-record-count')).not.toBeInTheDocument();
    });

    expect(screen.queryByTestId('confirm-name-input')).not.toBeInTheDocument();
  });

  it('requires name confirmation at exactly 50 records', async () => {
    mockListRecords.mockResolvedValue({
      page: 1,
      perPage: 1,
      totalPages: 2,
      totalItems: 50,
      items: [],
    });

    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    await waitFor(() => {
      expect(screen.getByTestId('confirm-name-input')).toBeInTheDocument();
    });
  });

  // ── Success feedback ────────────────────────────────────────────────────

  it('shows success message after successful deletion', async () => {
    mockDeleteCollection.mockResolvedValue(undefined);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    await waitFor(() => {
      expect(screen.queryByTestId('loading-record-count')).not.toBeInTheDocument();
    });

    await user.click(screen.getByTestId('confirm-delete-btn'));

    await waitFor(() => {
      expect(screen.getByTestId('success-message')).toHaveTextContent(
        'Collection "posts" deleted successfully.',
      );
    });
  });

  it('clears success message when opening another delete dialog', async () => {
    mockDeleteCollection.mockResolvedValue(undefined);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    // Delete posts first
    await user.click(screen.getByLabelText('Delete posts'));
    await waitFor(() => {
      expect(screen.queryByTestId('loading-record-count')).not.toBeInTheDocument();
    });
    await user.click(screen.getByTestId('confirm-delete-btn'));

    await waitFor(() => {
      expect(screen.getByTestId('success-message')).toBeInTheDocument();
    });

    // Now click delete on another collection
    await user.click(screen.getByLabelText('Delete users'));

    // Success message should be cleared
    expect(screen.queryByTestId('success-message')).not.toBeInTheDocument();
  });

  // ── View collection (no record count needed) ─────────────────────────────

  it('shows zero records for view collections without fetching', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete post_stats')).toBeInTheDocument();
    });

    mockListRecords.mockClear();

    await user.click(screen.getByLabelText('Delete post_stats'));

    await waitFor(() => {
      expect(screen.getByTestId('no-records-note')).toHaveTextContent('This collection has no records.');
    });

    // Should NOT have called listRecords for view collection
    expect(mockListRecords).not.toHaveBeenCalled();
  });

  // ── Record count fetch failure ────────────────────────────────────────────

  it('allows deletion when record count fetch fails', async () => {
    mockListRecords.mockRejectedValue(new Error('Network error'));
    mockDeleteCollection.mockResolvedValue(undefined);

    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Delete posts')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Delete posts'));

    // Wait for the count fetch to fail
    await waitFor(() => {
      expect(screen.queryByTestId('loading-record-count')).not.toBeInTheDocument();
    });

    // No name input (count is null, so not considered dangerous)
    expect(screen.queryByTestId('confirm-name-input')).not.toBeInTheDocument();

    // Delete button should be enabled
    expect(screen.getByTestId('confirm-delete-btn')).toBeEnabled();

    await user.click(screen.getByTestId('confirm-delete-btn'));

    await waitFor(() => {
      expect(mockDeleteCollection).toHaveBeenCalledWith('col1');
    });
  });

  // ── Search input has label for accessibility ─────────────────────────────

  it('search input has accessible label', () => {
    renderPage();

    const input = screen.getByLabelText('SEARCH');
    expect(input).toHaveAttribute('id', 'collection-search');
    expect(input).toHaveAttribute('type', 'search');
  });

  // ── Single collection singular summary ───────────────────────────────────

  it('uses singular form for single collection', async () => {
    mockListCollections.mockResolvedValue(
      makeListResponse([COLLECTIONS[0]]),
    );
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('1 OF 1 COLLECTION')).toBeInTheDocument();
    });
  });
});
