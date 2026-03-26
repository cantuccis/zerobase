import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { RecordsBrowserPage } from './RecordsBrowserPage';
import { ApiError } from '../../lib/api';
import type {
  Collection,
  BaseRecord,
  ListResponse,
  ErrorResponseBody,
  Field,
} from '../../lib/api/types';

// ── Test data ────────────────────────────────────────────────────────────────

function makeField(name: string, type: string = 'text'): Field {
  switch (type) {
    case 'number':
      return { id: `f_${name}`, name, type: { type: 'number', min: null, max: null, noDecimal: false }, required: false, unique: false, sortOrder: 0 };
    case 'bool':
      return { id: `f_${name}`, name, type: { type: 'bool' }, required: false, unique: false, sortOrder: 0 };
    default:
      return { id: `f_${name}`, name, type: { type: 'text', minLength: 0, maxLength: 500, pattern: null, searchable: true }, required: false, unique: false, sortOrder: 0 };
  }
}

const TEST_COLLECTION: Collection = {
  id: 'col_posts',
  name: 'posts',
  type: 'base',
  fields: [
    makeField('title'),
    makeField('views', 'number'),
    makeField('published', 'bool'),
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

function makeRecord(id: string, data: Record<string, unknown> = {}): BaseRecord {
  return {
    id,
    collectionId: 'col_posts',
    collectionName: 'posts',
    created: '2024-01-15 10:00:00.000Z',
    updated: '2024-01-15 12:00:00.000Z',
    title: `Post ${id}`,
    views: 42,
    published: true,
    ...data,
  };
}

function makeRecordsResponse(
  items: BaseRecord[],
  opts: { page?: number; perPage?: number; totalItems?: number; totalPages?: number } = {},
): ListResponse<BaseRecord> {
  return {
    page: opts.page ?? 1,
    perPage: opts.perPage ?? 20,
    totalPages: opts.totalPages ?? 1,
    totalItems: opts.totalItems ?? items.length,
    items,
  };
}

const RECORDS = [
  makeRecord('rec1', { title: 'First Post', views: 100, published: true }),
  makeRecord('rec2', { title: 'Second Post', views: 50, published: false }),
  makeRecord('rec3', { title: 'Third Post', views: 200, published: true }),
];

// ── Mocks ────────────────────────────────────────────────────────────────────

const mockGetCollection = vi.fn();
const mockListRecords = vi.fn();
const mockListCollections = vi.fn();

vi.mock('../../lib/auth/client', () => ({
  client: {
    getCollection: (...args: unknown[]) => mockGetCollection(...args),
    listRecords: (...args: unknown[]) => mockListRecords(...args),
    listCollections: (...args: unknown[]) => mockListCollections(...args),
    get isAuthenticated() { return true; },
    get token() { return 'mock-token'; },
    logout: vi.fn(),
  },
}));

Object.defineProperty(window, 'location', {
  value: { href: '', pathname: '/_/collections/col_posts', origin: 'http://localhost:8090' },
  writable: true,
});

// ── Helpers ──────────────────────────────────────────────────────────────────

function renderPage(collectionId = 'col_posts') {
  return render(<RecordsBrowserPage collectionId={collectionId} />);
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('RecordsBrowserPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetCollection.mockResolvedValue(TEST_COLLECTION);
    mockListRecords.mockResolvedValue(makeRecordsResponse(RECORDS));
    mockListCollections.mockResolvedValue({ page: 1, perPage: 50, totalPages: 1, totalItems: 1, items: [TEST_COLLECTION] });
  });

  // ── Loading state ──────────────────────────────────────────────────────

  it('shows loading skeleton while fetching', () => {
    mockGetCollection.mockReturnValue(new Promise(() => {}));
    renderPage();
    expect(screen.getByTestId('table-skeleton')).toBeInTheDocument();
  });

  // ── Data display ───────────────────────────────────────────────────────

  it('displays collection name as page title after loading', async () => {
    renderPage();
    await waitFor(() => {
      // Page title in h2 and breadcrumb both show collection name
      const headings = screen.getAllByText('posts');
      expect(headings.length).toBeGreaterThanOrEqual(1);
      // The h2 page title
      const h2 = headings.find((el) => el.tagName === 'H2');
      expect(h2).toBeDefined();
    });
  });

  it('displays records in a table after loading', async () => {
    renderPage();
    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
      expect(screen.getByText('Second Post')).toBeInTheDocument();
      expect(screen.getByText('Third Post')).toBeInTheDocument();
    });
  });

  it('displays system columns (id, created, updated)', async () => {
    renderPage();
    await waitFor(() => {
      // Column headers
      const headers = screen.getAllByRole('columnheader');
      const headerTexts = headers.map((h) => h.textContent?.trim().toLowerCase());
      expect(headerTexts).toContain('id');
      expect(headerTexts).toContain('created');
      expect(headerTexts).toContain('updated');
    });
  });

  it('displays custom field columns', async () => {
    renderPage();
    await waitFor(() => {
      const headers = screen.getAllByRole('columnheader');
      const headerTexts = headers.map((h) => h.textContent?.trim().toLowerCase());
      expect(headerTexts).toContain('title');
      expect(headerTexts).toContain('views');
      expect(headerTexts).toContain('published');
    });
  });

  it('displays record IDs as monospace text', async () => {
    renderPage();
    await waitFor(() => {
      const idCell = screen.getByText('rec1');
      expect(idCell.tagName).toBe('SPAN');
      expect(idCell.className).toContain('font-mono');
    });
  });

  it('renders breadcrumb with link to collections', async () => {
    renderPage();
    await waitFor(() => {
      const breadcrumb = screen.getByLabelText('Breadcrumb');
      const collectionsLink = within(breadcrumb).getByText('Collections');
      expect(collectionsLink.closest('a')).toHaveAttribute('href', '/_/collections');
    });
  });

  it('renders Edit Schema link', async () => {
    renderPage();
    await waitFor(() => {
      const editLink = screen.getByText('Edit Schema');
      expect(editLink.closest('a')).toHaveAttribute('href', '/_/collections/col_posts/edit');
    });
  });

  // ── Empty state ────────────────────────────────────────────────────────

  it('shows empty state when no records exist', async () => {
    mockListRecords.mockResolvedValue(makeRecordsResponse([]));
    renderPage();
    await waitFor(() => {
      expect(screen.getByText('No records in this collection.')).toBeInTheDocument();
    });
  });

  it('shows filtered empty state when filter yields no results', async () => {
    // First load with records
    mockListRecords.mockResolvedValueOnce(makeRecordsResponse(RECORDS));
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    // Apply filter that yields no results
    mockListRecords.mockResolvedValueOnce(makeRecordsResponse([]));
    await user.type(screen.getByPlaceholderText(/Filter records/), "title = 'nonexistent'");
    await user.click(screen.getByRole('button', { name: 'Filter' }));

    await waitFor(() => {
      expect(screen.getByText('No records match the current filter.')).toBeInTheDocument();
    });
  });

  // ── Error state ────────────────────────────────────────────────────────

  it('shows error when collection fetch fails', async () => {
    const errorBody: ErrorResponseBody = {
      code: 404,
      message: 'Collection not found.',
      data: {},
    };
    mockGetCollection.mockRejectedValue(new ApiError(404, errorBody));
    renderPage();

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Collection not found.');
    });
  });

  it('shows error when records fetch fails', async () => {
    const errorBody: ErrorResponseBody = {
      code: 500,
      message: 'Internal server error.',
      data: {},
    };
    mockListRecords.mockRejectedValue(new ApiError(500, errorBody));
    renderPage();

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Internal server error.');
    });
  });

  it('shows generic error on network failure', async () => {
    mockListRecords.mockRejectedValue(new TypeError('Failed to fetch'));
    renderPage();

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Failed to load records.');
    });
  });

  it('allows retrying after an error', async () => {
    const errorBody: ErrorResponseBody = {
      code: 500,
      message: 'Server error.',
      data: {},
    };
    mockListRecords
      .mockRejectedValueOnce(new ApiError(500, errorBody))
      .mockResolvedValueOnce(makeRecordsResponse(RECORDS));
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('Retry')).toBeInTheDocument();
    });

    const user = userEvent.setup();
    await user.click(screen.getByText('Retry'));

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });
  });

  // ── Sorting ────────────────────────────────────────────────────────────

  it('calls API with sort parameter when clicking column header', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    mockListRecords.mockResolvedValue(makeRecordsResponse(RECORDS));
    const titleHeader = screen.getAllByRole('columnheader').find(
      (h) => h.textContent?.trim().toLowerCase().startsWith('title'),
    )!;
    await user.click(titleHeader);

    await waitFor(() => {
      expect(mockListRecords).toHaveBeenCalledWith(
        'posts',
        expect.objectContaining({ sort: 'title' }),
      );
    });
  });

  it('toggles sort direction on second click', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    function getTitleHeader() {
      return screen.getAllByRole('columnheader').find(
        (h) => h.textContent?.trim().toLowerCase().startsWith('title'),
      )!;
    }

    // First click: asc
    mockListRecords.mockResolvedValue(makeRecordsResponse(RECORDS));
    await user.click(getTitleHeader());
    await waitFor(() => {
      expect(mockListRecords).toHaveBeenCalledWith(
        'posts',
        expect.objectContaining({ sort: 'title' }),
      );
    });

    // Wait for table to re-render after loading
    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    // Second click: desc (re-query header since DOM was rebuilt)
    mockListRecords.mockResolvedValue(makeRecordsResponse(RECORDS));
    await user.click(getTitleHeader());
    await waitFor(() => {
      expect(mockListRecords).toHaveBeenCalledWith(
        'posts',
        expect.objectContaining({ sort: '-title' }),
      );
    });
  });

  it('removes sort on third click', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    function getTitleHeader() {
      return screen.getAllByRole('columnheader').find(
        (h) => h.textContent?.trim().toLowerCase().startsWith('title'),
      )!;
    }

    // Click 1: asc
    mockListRecords.mockResolvedValue(makeRecordsResponse(RECORDS));
    await user.click(getTitleHeader());
    await waitFor(() => {
      expect(mockListRecords).toHaveBeenCalledWith('posts', expect.objectContaining({ sort: 'title' }));
    });
    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    // Click 2: desc
    mockListRecords.mockResolvedValue(makeRecordsResponse(RECORDS));
    await user.click(getTitleHeader());
    await waitFor(() => {
      expect(mockListRecords).toHaveBeenCalledWith('posts', expect.objectContaining({ sort: '-title' }));
    });
    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    // Click 3: none
    const callCountBefore = mockListRecords.mock.calls.length;
    mockListRecords.mockResolvedValue(makeRecordsResponse(RECORDS));
    await user.click(getTitleHeader());

    // Third click should NOT include sort param
    await waitFor(() => {
      expect(mockListRecords.mock.calls.length).toBeGreaterThan(callCountBefore);
      const lastCall = mockListRecords.mock.calls[mockListRecords.mock.calls.length - 1];
      expect(lastCall[1]).not.toHaveProperty('sort');
    });
  });

  it('sets aria-sort on sorted column', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    const titleHeader = screen.getAllByRole('columnheader').find(
      (h) => h.textContent?.trim().toLowerCase().startsWith('title'),
    )!;

    expect(titleHeader).toHaveAttribute('aria-sort', 'none');

    mockListRecords.mockResolvedValue(makeRecordsResponse(RECORDS));
    await user.click(titleHeader);

    await waitFor(() => {
      expect(titleHeader).toHaveAttribute('aria-sort', 'ascending');
    });
  });

  // ── Filtering ──────────────────────────────────────────────────────────

  it('calls API with filter parameter on form submit', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    mockListRecords.mockResolvedValue(makeRecordsResponse([RECORDS[0]]));
    await user.type(screen.getByPlaceholderText(/Filter records/), "title = 'First Post'");
    await user.click(screen.getByRole('button', { name: 'Filter' }));

    await waitFor(() => {
      expect(mockListRecords).toHaveBeenCalledWith(
        'posts',
        expect.objectContaining({ filter: "title = 'First Post'" }),
      );
    });
  });

  it('shows active filter indicator when filter is applied', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    mockListRecords.mockResolvedValue(makeRecordsResponse([RECORDS[0]]));
    await user.type(screen.getByPlaceholderText(/Filter records/), "views > 50");
    await user.click(screen.getByRole('button', { name: 'Filter' }));

    await waitFor(() => {
      expect(screen.getByText('Active filter:')).toBeInTheDocument();
      expect(screen.getByText('views > 50')).toBeInTheDocument();
    });
  });

  it('clears filter when clicking Clear button', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    // Apply filter
    mockListRecords.mockResolvedValue(makeRecordsResponse([RECORDS[0]]));
    await user.type(screen.getByPlaceholderText(/Filter records/), "views > 50");
    await user.click(screen.getByRole('button', { name: 'Filter' }));

    await waitFor(() => {
      expect(screen.getByText('Active filter:')).toBeInTheDocument();
    });

    // Clear filter
    mockListRecords.mockResolvedValue(makeRecordsResponse(RECORDS));
    await user.click(screen.getByRole('button', { name: 'Clear' }));

    await waitFor(() => {
      expect(screen.queryByText('Active filter:')).not.toBeInTheDocument();
    });
  });

  it('resets to page 1 when applying a filter', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    mockListRecords.mockResolvedValue(makeRecordsResponse([RECORDS[0]]));
    await user.type(screen.getByPlaceholderText(/Filter records/), "views > 50");
    await user.click(screen.getByRole('button', { name: 'Filter' }));

    await waitFor(() => {
      expect(mockListRecords).toHaveBeenCalledWith(
        'posts',
        expect.objectContaining({ page: 1 }),
      );
    });
  });

  // ── Pagination ─────────────────────────────────────────────────────────

  it('displays pagination controls', async () => {
    mockListRecords.mockResolvedValue(
      makeRecordsResponse(RECORDS, { totalItems: 50, totalPages: 3 }),
    );
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('pagination')).toBeInTheDocument();
      expect(screen.getByText('50 records')).toBeInTheDocument();
      // Page buttons are rendered instead of "Page X of Y" text
      expect(screen.getByLabelText('Page 1')).toBeInTheDocument();
    });
  });

  it('navigates to next page on click', async () => {
    mockListRecords.mockResolvedValue(
      makeRecordsResponse(RECORDS, { totalItems: 50, totalPages: 3, page: 1 }),
    );
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Page 1')).toBeInTheDocument();
    });

    mockListRecords.mockResolvedValue(
      makeRecordsResponse(RECORDS, { totalItems: 50, totalPages: 3, page: 2 }),
    );
    await user.click(screen.getByLabelText('Next page'));

    await waitFor(() => {
      expect(mockListRecords).toHaveBeenCalledWith(
        'posts',
        expect.objectContaining({ page: 2 }),
      );
    });
  });

  it('navigates to previous page on click', async () => {
    // Start on page 2
    mockListRecords.mockResolvedValue(
      makeRecordsResponse(RECORDS, { totalItems: 50, totalPages: 3, page: 2 }),
    );
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText('Page 1')).toBeInTheDocument();
    });

    // Need to first navigate to page 2
    mockListRecords.mockResolvedValue(
      makeRecordsResponse(RECORDS, { totalItems: 50, totalPages: 3, page: 2 }),
    );
    await user.click(screen.getByLabelText('Next page'));

    await waitFor(() => {
      expect(screen.getByTestId('pagination')).toBeInTheDocument();
    });

    mockListRecords.mockResolvedValue(
      makeRecordsResponse(RECORDS, { totalItems: 50, totalPages: 3, page: 1 }),
    );
    await user.click(screen.getByLabelText('Previous page'));

    await waitFor(() => {
      expect(mockListRecords).toHaveBeenCalledWith(
        'posts',
        expect.objectContaining({ page: 1 }),
      );
    });
  });

  it('disables previous/first buttons on page 1', async () => {
    mockListRecords.mockResolvedValue(
      makeRecordsResponse(RECORDS, { totalItems: 50, totalPages: 3, page: 1 }),
    );
    renderPage();

    await waitFor(() => {
      expect(screen.getByLabelText('Previous page')).toBeDisabled();
      expect(screen.getByLabelText('First page')).toBeDisabled();
      expect(screen.getByLabelText('Next page')).not.toBeDisabled();
      expect(screen.getByLabelText('Last page')).not.toBeDisabled();
    });
  });

  it('disables next/last buttons on last page', async () => {
    mockListRecords.mockResolvedValue(
      makeRecordsResponse(RECORDS, { totalItems: 50, totalPages: 3, page: 3 }),
    );
    renderPage();

    // We need to set internal page state to 3 to trigger the disabled check
    // The component checks `page >= totalPages`, but internal `page` starts at 1
    // Let's navigate to page 3 by clicking last page
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByTestId('pagination')).toBeInTheDocument();
    });

    mockListRecords.mockResolvedValue(
      makeRecordsResponse(RECORDS, { totalItems: 50, totalPages: 3, page: 3 }),
    );
    await user.click(screen.getByLabelText('Last page'));

    await waitFor(() => {
      expect(screen.getByLabelText('Next page')).toBeDisabled();
      expect(screen.getByLabelText('Last page')).toBeDisabled();
    });
  });

  it('changes per-page count', async () => {
    mockListRecords.mockResolvedValue(
      makeRecordsResponse(RECORDS, { totalItems: 50, totalPages: 3 }),
    );
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByTestId('pagination')).toBeInTheDocument();
    });

    mockListRecords.mockResolvedValue(
      makeRecordsResponse(RECORDS, { totalItems: 50, totalPages: 1, perPage: 50 }),
    );
    await user.selectOptions(screen.getByLabelText('Records per page'), '50');

    await waitFor(() => {
      expect(mockListRecords).toHaveBeenCalledWith(
        'posts',
        expect.objectContaining({ perPage: 50, page: 1 }),
      );
    });
  });

  it('uses singular form for single record', async () => {
    mockListRecords.mockResolvedValue(
      makeRecordsResponse([RECORDS[0]], { totalItems: 1 }),
    );
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('1 record')).toBeInTheDocument();
    });
  });

  // ── Column visibility ──────────────────────────────────────────────────

  it('renders column toggle button', async () => {
    renderPage();
    await waitFor(() => {
      expect(screen.getByLabelText('Toggle column visibility')).toBeInTheDocument();
    });
  });

  it('opens column visibility dropdown on click', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Toggle column visibility'));

    // Should see checkboxes for each column
    const menu = screen.getByRole('listbox');
    expect(menu).toBeInTheDocument();

    const checkboxes = within(menu).getAllByRole('checkbox');
    expect(checkboxes.length).toBe(6); // id, created, updated, title, views, published
  });

  it('hides column when unchecking in dropdown', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    // Verify 'views' column is visible
    const headers = screen.getAllByRole('columnheader');
    expect(headers.some((h) => h.textContent?.trim().toLowerCase().startsWith('views'))).toBe(true);

    // Open column toggle and uncheck 'views'
    await user.click(screen.getByLabelText('Toggle column visibility'));
    const menu = screen.getByRole('listbox');
    const viewsCheckbox = within(menu).getAllByRole('checkbox').find(
      (cb) => cb.closest('label')?.textContent?.includes('views'),
    )!;
    await user.click(viewsCheckbox);

    // Close dropdown
    await user.click(screen.getByLabelText('Toggle column visibility'));

    // 'views' column header should be gone
    const updatedHeaders = screen.getAllByRole('columnheader');
    expect(updatedHeaders.some((h) => h.textContent?.trim().toLowerCase().startsWith('views'))).toBe(false);
  });

  it('shows column when re-checking in dropdown', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    // Hide 'views'
    await user.click(screen.getByLabelText('Toggle column visibility'));
    let menu = screen.getByRole('listbox');
    let viewsCheckbox = within(menu).getAllByRole('checkbox').find(
      (cb) => cb.closest('label')?.textContent?.includes('views'),
    )!;
    await user.click(viewsCheckbox);
    await user.click(screen.getByLabelText('Toggle column visibility'));

    // Verify hidden
    expect(screen.getAllByRole('columnheader').some((h) => h.textContent?.trim().toLowerCase().startsWith('views'))).toBe(false);

    // Re-show 'views'
    await user.click(screen.getByLabelText('Toggle column visibility'));
    menu = screen.getByRole('listbox');
    viewsCheckbox = within(menu).getAllByRole('checkbox').find(
      (cb) => cb.closest('label')?.textContent?.includes('views'),
    )!;
    await user.click(viewsCheckbox);
    await user.click(screen.getByLabelText('Toggle column visibility'));

    // Verify visible again
    expect(screen.getAllByRole('columnheader').some((h) => h.textContent?.trim().toLowerCase().startsWith('views'))).toBe(true);
  });

  it('prevents hiding all columns (last checkbox stays checked)', async () => {
    // Collection with only one custom field
    const singleFieldCollection: Collection = {
      ...TEST_COLLECTION,
      fields: [makeField('title')],
    };
    mockGetCollection.mockResolvedValue(singleFieldCollection);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    // Hide all columns except one
    await user.click(screen.getByLabelText('Toggle column visibility'));
    const menu = screen.getByRole('listbox');
    const checkboxes = within(menu).getAllByRole('checkbox');

    // Uncheck all but one - the 4 columns are id, created, updated, title
    // Uncheck first 3
    for (let i = 0; i < 3; i++) {
      await user.click(checkboxes[i]);
    }

    // Try to uncheck the last one — should stay checked
    await user.click(checkboxes[3]);

    // Should still have at least 1 column header visible
    await user.click(screen.getByLabelText('Toggle column visibility'));
    const visibleHeaders = screen.getAllByRole('columnheader');
    expect(visibleHeaders.length).toBeGreaterThanOrEqual(1);
  });

  // ── Record detail ──────────────────────────────────────────────────────

  it('opens record detail panel when clicking a row', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId('record-row-rec1'));

    await waitFor(() => {
      expect(screen.getByText((_content, element) => element?.id === 'record-detail-title' && element?.textContent === 'Record: rec1')).toBeInTheDocument();
    });
  });

  it('displays all field values in record detail', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId('record-row-rec1'));

    await waitFor(() => {
      const dialog = screen.getByRole('dialog');
      expect(within(dialog).getAllByText('rec1').length).toBeGreaterThanOrEqual(1);
      expect(within(dialog).getByText('First Post')).toBeInTheDocument();
      expect(within(dialog).getByText('100')).toBeInTheDocument();
      expect(within(dialog).getByText('true')).toBeInTheDocument();
    });
  });

  it('closes record detail panel when clicking close button', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('First Post')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId('record-row-rec1'));

    await waitFor(() => {
      expect(screen.getByText((_content, element) => element?.id === 'record-detail-title' && element?.textContent === 'Record: rec1')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('Close record detail'));

    await waitFor(() => {
      expect(screen.queryByText((_content, element) => element?.id === 'record-detail-title' && element?.textContent === 'Record: rec1')).not.toBeInTheDocument();
    });
  });

  // ── API interaction ────────────────────────────────────────────────────

  it('fetches collection schema by ID on mount', async () => {
    renderPage();

    await waitFor(() => {
      expect(mockGetCollection).toHaveBeenCalledWith('col_posts');
    });
  });

  it('fetches records using collection name', async () => {
    renderPage();

    await waitFor(() => {
      expect(mockListRecords).toHaveBeenCalledWith(
        'posts',
        expect.objectContaining({ page: 1, perPage: 20 }),
      );
    });
  });

  // ── Formatting ─────────────────────────────────────────────────────────

  it('displays boolean values as true/false text', async () => {
    renderPage();

    await waitFor(() => {
      // First record has published=true, second has published=false
      const rows = screen.getAllByRole('row');
      // Row 0 is header, rows 1-3 are data
      expect(within(rows[1]).getByText('true')).toBeInTheDocument();
      expect(within(rows[2]).getByText('false')).toBeInTheDocument();
    });
  });

  it('displays null values as em-dash', async () => {
    mockListRecords.mockResolvedValue(
      makeRecordsResponse([makeRecord('rec_null', { title: null, views: null, published: null })]),
    );
    renderPage();

    await waitFor(() => {
      const emDashes = screen.getAllByText('—');
      expect(emDashes.length).toBeGreaterThanOrEqual(1);
    });
  });

  // ── Accessibility ──────────────────────────────────────────────────────

  it('filter input has accessible label', async () => {
    renderPage();
    await waitFor(() => {
      const input = screen.getByPlaceholderText(/Filter records/);
      expect(input).toHaveAttribute('id', 'record-filter');
    });
  });

  it('breadcrumb has navigation landmark', async () => {
    renderPage();
    await waitFor(() => {
      expect(screen.getByLabelText('Breadcrumb')).toBeInTheDocument();
    });
  });
});
