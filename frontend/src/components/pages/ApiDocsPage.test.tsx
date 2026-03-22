import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ApiDocsPage } from './ApiDocsPage';
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

vi.mock('../../lib/auth/client', () => ({
  client: {
    listCollections: (...args: unknown[]) => mockListCollections(...args),
    get isAuthenticated() {
      return true;
    },
    get token() {
      return 'mock-token';
    },
    logout: vi.fn(),
  },
}));

Object.defineProperty(window, 'location', {
  value: { href: '', pathname: '/_/docs', origin: 'http://localhost:8090' },
  writable: true,
});

// ── Helpers ──────────────────────────────────────────────────────────────────

function renderPage() {
  return render(<ApiDocsPage />);
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('ApiDocsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockListCollections.mockResolvedValue(makeListResponse(COLLECTIONS));
  });

  // ── Loading state ────────────────────────────────────────────────────────

  it('shows loading skeleton while fetching collections', () => {
    mockListCollections.mockReturnValue(new Promise(() => {}));
    renderPage();

    expect(screen.getByTestId('loading-skeleton')).toBeInTheDocument();
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
      expect(screen.getByText('Retry')).toBeInTheDocument();
    });

    const user = userEvent.setup();
    await user.click(screen.getByText('Retry'));

    await waitFor(() => {
      expect(screen.getByTestId('selected-collection-name')).toHaveTextContent('posts');
    });

    expect(mockListCollections).toHaveBeenCalledTimes(2);
  });

  // ── Empty state ──────────────────────────────────────────────────────────

  it('shows empty state when no collections exist', async () => {
    mockListCollections.mockResolvedValue(makeListResponse([]));
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('empty-state')).toBeInTheDocument();
      expect(screen.getByText('No collections found.')).toBeInTheDocument();
    });
  });

  it('shows link to create collection in empty state', async () => {
    mockListCollections.mockResolvedValue(makeListResponse([]));
    renderPage();

    await waitFor(() => {
      const link = screen.getByText('Create Collection');
      expect(link.closest('a')).toHaveAttribute('href', '/_/collections/new');
    });
  });

  // ── Collection selector ──────────────────────────────────────────────────

  it('shows all collections in the selector', async () => {
    renderPage();

    await waitFor(() => {
      const selector = screen.getByTestId('collection-selector');
      expect(within(selector).getByText('posts')).toBeInTheDocument();
      expect(within(selector).getByText('users')).toBeInTheDocument();
      expect(within(selector).getByText('post_stats')).toBeInTheDocument();
    });
  });

  it('auto-selects the first collection', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('selected-collection-name')).toHaveTextContent('posts');
    });
  });

  it('switches collection when clicking a different one', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByTestId('selected-collection-name')).toHaveTextContent('posts');
    });

    await user.click(screen.getByLabelText('View API docs for users'));

    expect(screen.getByTestId('selected-collection-name')).toHaveTextContent('users');
  });

  it('shows collection type badge next to selected collection name', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('selected-collection-name')).toHaveTextContent('posts');
      // "Base" appears in both the selector and the header badge — at least 2
      expect(screen.getAllByText('Base').length).toBeGreaterThanOrEqual(2);
    });
  });

  // ── Endpoint display ─────────────────────────────────────────────────────

  it('shows endpoints list for selected collection', async () => {
    renderPage();

    await waitFor(() => {
      const endpointsList = screen.getByTestId('endpoints-list');
      const cards = within(endpointsList).getAllByTestId('endpoint-card');
      // base collection: 5 CRUD endpoints
      expect(cards).toHaveLength(5);
    });
  });

  it('shows 11 endpoints for auth collection', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByTestId('selected-collection-name')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('View API docs for users'));

    await waitFor(() => {
      const endpointsList = screen.getByTestId('endpoints-list');
      const cards = within(endpointsList).getAllByTestId('endpoint-card');
      // auth: 5 CRUD + 6 auth = 11
      expect(cards).toHaveLength(11);
    });
  });

  it('displays endpoint count in header', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText(/5 endpoints? available/)).toBeInTheDocument();
    });
  });

  it('displays field count in header', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText(/2 fields/)).toBeInTheDocument();
    });
  });

  // ── Endpoint expansion ───────────────────────────────────────────────────

  it('expands endpoint card to show details', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByTestId('endpoints-list')).toBeInTheDocument();
    });

    // Click the first endpoint card button
    const cards = screen.getAllByTestId('endpoint-card');
    const firstCardButton = within(cards[0]).getByRole('button');
    await user.click(firstCardButton);

    expect(within(cards[0]).getByTestId('endpoint-details')).toBeInTheDocument();
  });

  it('shows response example in expanded endpoint', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByTestId('endpoints-list')).toBeInTheDocument();
    });

    const cards = screen.getAllByTestId('endpoint-card');
    await user.click(within(cards[0]).getByRole('button'));

    const details = within(cards[0]).getByTestId('endpoint-details');
    expect(within(details).getByText('Response')).toBeInTheDocument();
    expect(within(details).getAllByTestId('code-block').length).toBeGreaterThanOrEqual(1);
  });

  it('shows curl example in expanded endpoint', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByTestId('endpoints-list')).toBeInTheDocument();
    });

    const cards = screen.getAllByTestId('endpoint-card');
    await user.click(within(cards[0]).getByRole('button'));

    const details = within(cards[0]).getByTestId('endpoint-details');
    expect(within(details).getByText('cURL Example')).toBeInTheDocument();
  });

  it('shows query parameters table for list endpoint', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByTestId('endpoints-list')).toBeInTheDocument();
    });

    const cards = screen.getAllByTestId('endpoint-card');
    // First card is the list endpoint
    await user.click(within(cards[0]).getByRole('button'));

    const details = within(cards[0]).getByTestId('endpoint-details');
    expect(within(details).getByText('Query Parameters')).toBeInTheDocument();
    expect(within(details).getByText('page')).toBeInTheDocument();
    expect(within(details).getByText('perPage')).toBeInTheDocument();
  });

  it('shows access rule badge in expanded endpoint', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByTestId('endpoints-list')).toBeInTheDocument();
    });

    const cards = screen.getAllByTestId('endpoint-card');
    await user.click(within(cards[0]).getByRole('button'));

    const details = within(cards[0]).getByTestId('endpoint-details');
    expect(within(details).getByText('Access:')).toBeInTheDocument();
    // Default rules are null = "Superusers only"
    expect(within(details).getByText('Superusers only')).toBeInTheDocument();
  });

  // ── Fields summary ───────────────────────────────────────────────────────

  it('shows fields summary table', async () => {
    renderPage();

    await waitFor(() => {
      const fieldsSummary = screen.getByTestId('fields-summary');
      expect(within(fieldsSummary).getByText('title')).toBeInTheDocument();
      expect(within(fieldsSummary).getByText('body')).toBeInTheDocument();
      expect(within(fieldsSummary).getByText('text')).toBeInTheDocument();
      expect(within(fieldsSummary).getByText('editor')).toBeInTheDocument();
    });
  });

  it('shows required and unique columns in fields summary', async () => {
    renderPage();

    await waitFor(() => {
      const fieldsSummary = screen.getByTestId('fields-summary');
      expect(within(fieldsSummary).getByText('Required')).toBeInTheDocument();
      expect(within(fieldsSummary).getByText('Unique')).toBeInTheDocument();
    });
  });

  it('does not show fields summary for collection with no fields', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByTestId('selected-collection-name')).toBeInTheDocument();
    });

    await user.click(screen.getByLabelText('View API docs for post_stats'));

    // post_stats has 0 fields
    expect(screen.queryByTestId('fields-summary')).not.toBeInTheDocument();
  });

  // ── Filter reference ─────────────────────────────────────────────────────

  it('shows filter reference section', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('filter-reference')).toBeInTheDocument();
    });
  });

  it('expands filter reference to show operators', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByTestId('filter-reference')).toBeInTheDocument();
    });

    const filterRef = screen.getByTestId('filter-reference');
    await user.click(within(filterRef).getByRole('button'));

    expect(screen.getByTestId('filter-details')).toBeInTheDocument();
  });

  // ── Access rules with custom values ───────────────────────────────────────

  it('shows "Public" for empty string rule', async () => {
    mockListCollections.mockResolvedValue(
      makeListResponse([
        makeCollection({
          id: 'col_public',
          name: 'public_posts',
          fields: [
            { id: 'f1', name: 'title', type: { type: 'text', minLength: 0, maxLength: 500, pattern: null, searchable: true }, required: false, unique: false, sortOrder: 0 },
          ],
          rules: {
            listRule: '',
            viewRule: '',
            createRule: '@request.auth.id != ""',
            updateRule: '@request.auth.id != ""',
            deleteRule: null,
          },
        }),
      ]),
    );

    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByTestId('endpoints-list')).toBeInTheDocument();
    });

    // Expand the list endpoint (first card)
    const cards = screen.getAllByTestId('endpoint-card');
    await user.click(within(cards[0]).getByRole('button'));

    const details = within(cards[0]).getByTestId('endpoint-details');
    expect(within(details).getByText('Public')).toBeInTheDocument();
  });

  // ── Page title ───────────────────────────────────────────────────────────

  it('renders the page title', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('API Documentation')).toBeInTheDocument();
    });
  });

  // ── Mobile selector ──────────────────────────────────────────────────────

  it('renders mobile collection select', async () => {
    renderPage();

    await waitFor(() => {
      const select = screen.getByLabelText('Collection');
      expect(select).toBeInTheDocument();
      expect(select.tagName).toBe('SELECT');
    });
  });
});
