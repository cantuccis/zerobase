import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { LogsPage } from './LogsPage';
import { ApiError } from '../../lib/api';
import type { LogEntry, LogStats, ListResponse, ErrorResponseBody } from '../../lib/api/types';

// ── Test data ────────────────────────────────────────────────────────────────

function makeLogEntry(overrides: Partial<LogEntry> = {}): LogEntry {
  return {
    id: 'log-001',
    method: 'GET',
    url: '/api/collections/posts/records',
    status: 200,
    ip: '127.0.0.1',
    authId: '',
    durationMs: 12,
    userAgent: 'Mozilla/5.0',
    requestId: 'req-abc123',
    created: '2026-03-21T10:30:00Z',
    ...overrides,
  };
}

const SAMPLE_LOGS: ListResponse<LogEntry> = {
  page: 1,
  perPage: 30,
  totalPages: 2,
  totalItems: 35,
  items: [
    makeLogEntry({ id: 'log-001', method: 'GET', url: '/api/health', status: 200, durationMs: 3 }),
    makeLogEntry({ id: 'log-002', method: 'POST', url: '/api/collections', status: 201, durationMs: 45 }),
    makeLogEntry({ id: 'log-003', method: 'GET', url: '/api/collections/users/records', status: 404, durationMs: 8 }),
    makeLogEntry({ id: 'log-004', method: 'DELETE', url: '/api/collections/posts/records/abc', status: 500, durationMs: 2500, ip: '192.168.1.1' }),
    makeLogEntry({ id: 'log-005', method: 'PATCH', url: '/api/collections/posts/records/def', status: 200, durationMs: 120, authId: 'user-123' }),
  ],
};

const EMPTY_LOGS: ListResponse<LogEntry> = {
  page: 1,
  perPage: 30,
  totalPages: 0,
  totalItems: 0,
  items: [],
};

const SAMPLE_STATS: LogStats = {
  totalRequests: 1250,
  statusCounts: {
    success: 1100,
    redirect: 20,
    clientError: 100,
    serverError: 30,
  },
  avgDurationMs: 42.5,
  maxDurationMs: 3200,
  timeline: [
    { date: '2026-03-21T08:00:00Z', total: 45 },
    { date: '2026-03-21T09:00:00Z', total: 78 },
    { date: '2026-03-21T10:00:00Z', total: 120 },
    { date: '2026-03-21T11:00:00Z', total: 95 },
  ],
};

const DETAIL_LOG = makeLogEntry({
  id: 'log-001',
  method: 'GET',
  url: '/api/health',
  status: 200,
  durationMs: 3,
  ip: '10.0.0.1',
  authId: 'admin-user',
  userAgent: 'TestAgent/1.0',
  requestId: 'req-detail-001',
  created: '2026-03-21T10:30:15Z',
});

// ── Mocks ────────────────────────────────────────────────────────────────────

const mockListLogs = vi.fn();
const mockGetLogStats = vi.fn();
const mockGetLog = vi.fn();

vi.mock('../../lib/auth/client', () => ({
  client: {
    listLogs: (...args: unknown[]) => mockListLogs(...args),
    getLogStats: (...args: unknown[]) => mockGetLogStats(...args),
    getLog: (...args: unknown[]) => mockGetLog(...args),
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
  value: { href: '', pathname: '/_/logs', origin: 'http://localhost:8090' },
  writable: true,
});

// ── Helpers ──────────────────────────────────────────────────────────────────

function renderPage() {
  return render(<LogsPage />);
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('LogsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockListLogs.mockResolvedValue(SAMPLE_LOGS);
    mockGetLogStats.mockResolvedValue(SAMPLE_STATS);
    mockGetLog.mockResolvedValue(DETAIL_LOG);
  });

  // ── Loading states ──────────────────────────────────────────────────────

  it('shows loading skeletons while fetching data', () => {
    mockListLogs.mockReturnValue(new Promise(() => {}));
    mockGetLogStats.mockReturnValue(new Promise(() => {}));
    renderPage();

    // Stats skeleton cards
    const pulseElements = document.querySelectorAll('.animate-pulse');
    expect(pulseElements.length).toBeGreaterThan(0);
  });

  // ── Stats overview ──────────────────────────────────────────────────────

  it('renders stats overview cards with correct values', async () => {
    renderPage();

    await waitFor(() => {
      const statsOverview = screen.getByTestId('stats-overview');
      expect(within(statsOverview).getByText('Total Requests')).toBeInTheDocument();
      expect(within(statsOverview).getByText('1,250')).toBeInTheDocument();
      expect(within(statsOverview).getByText('Error Rate')).toBeInTheDocument();
      expect(within(statsOverview).getByText('10.4%')).toBeInTheDocument();
      expect(within(statsOverview).getByText('Avg Duration')).toBeInTheDocument();
      expect(within(statsOverview).getByText('43ms')).toBeInTheDocument();
    });
  });

  it('renders timeline chart with bars', async () => {
    renderPage();

    await waitFor(() => {
      const chart = screen.getByTestId('timeline-chart');
      expect(chart).toBeInTheDocument();
      const bars = within(chart).getAllByTestId('timeline-bar');
      expect(bars).toHaveLength(4);
    });
  });

  it('renders status breakdown', async () => {
    renderPage();

    await waitFor(() => {
      const breakdown = screen.getByTestId('status-breakdown');
      expect(breakdown).toBeInTheDocument();
      expect(within(breakdown).getByText(/2xx/)).toBeInTheDocument();
      expect(within(breakdown).getByText(/4xx/)).toBeInTheDocument();
      expect(within(breakdown).getByText(/5xx/)).toBeInTheDocument();
    });
  });

  // ── Logs table ──────────────────────────────────────────────────────────

  it('renders the logs table with all entries', async () => {
    renderPage();

    await waitFor(() => {
      const table = screen.getByTestId('logs-table');
      expect(table).toBeInTheDocument();

      const rows = screen.getAllByTestId('log-row');
      expect(rows).toHaveLength(5);
    });
  });

  it('displays log entry details in table rows', async () => {
    renderPage();

    await waitFor(() => {
      const table = screen.getByTestId('logs-table');

      // Check method badges exist in table
      expect(within(table).getAllByText('GET').length).toBeGreaterThanOrEqual(1);
      expect(within(table).getByText('POST')).toBeInTheDocument();
      expect(within(table).getByText('DELETE')).toBeInTheDocument();

      // Check URLs
      expect(within(table).getByText('/api/health')).toBeInTheDocument();

      // Check status codes
      expect(within(table).getAllByText('200').length).toBeGreaterThanOrEqual(1);
      expect(within(table).getByText('404')).toBeInTheDocument();
      expect(within(table).getByText('500')).toBeInTheDocument();

      // Check IPs (multiple rows may share the same IP)
      expect(within(table).getAllByText('127.0.0.1').length).toBeGreaterThanOrEqual(1);
      expect(within(table).getByText('192.168.1.1')).toBeInTheDocument();
    });
  });

  it('shows empty state when no logs match filters', async () => {
    mockListLogs.mockResolvedValue(EMPTY_LOGS);
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('No logs found matching the current filters.')).toBeInTheDocument();
    });
  });

  // ── Pagination ──────────────────────────────────────────────────────────

  it('renders pagination when there are multiple pages', async () => {
    renderPage();

    await waitFor(() => {
      const pagination = screen.getByTestId('pagination');
      expect(pagination).toBeInTheDocument();
      expect(within(pagination).getByText('35 total entries')).toBeInTheDocument();
      expect(within(pagination).getByText('1 / 2')).toBeInTheDocument();
    });
  });

  it('navigates to next page when clicking Next', async () => {
    const user = userEvent.setup();
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('pagination')).toBeInTheDocument();
    });

    const nextButton = screen.getByRole('button', { name: 'Next page' });
    await user.click(nextButton);

    await waitFor(() => {
      // Should have been called with page: 2
      const lastCall = mockListLogs.mock.calls[mockListLogs.mock.calls.length - 1];
      expect(lastCall[0]).toMatchObject({ page: 2 });
    });
  });

  it('hides pagination when only one page', async () => {
    mockListLogs.mockResolvedValue({ ...SAMPLE_LOGS, totalPages: 1, totalItems: 5 });
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('logs-table')).toBeInTheDocument();
    });

    expect(screen.queryByTestId('pagination')).not.toBeInTheDocument();
  });

  // ── Filters ─────────────────────────────────────────────────────────────

  it('renders filter controls', async () => {
    renderPage();

    await waitFor(() => {
      const filters = screen.getByTestId('logs-filters');
      expect(filters).toBeInTheDocument();
      expect(screen.getByLabelText('Method')).toBeInTheDocument();
      expect(screen.getByLabelText('Status')).toBeInTheDocument();
      expect(screen.getByLabelText('Time Range')).toBeInTheDocument();
      expect(screen.getByLabelText('URL')).toBeInTheDocument();
    });
  });

  it('applies method filter when changed', async () => {
    const user = userEvent.setup();
    renderPage();

    await waitFor(() => {
      expect(screen.getByLabelText('Method')).toBeInTheDocument();
    });

    const methodSelect = screen.getByLabelText('Method');
    await user.selectOptions(methodSelect, 'POST');

    await waitFor(() => {
      const lastCall = mockListLogs.mock.calls[mockListLogs.mock.calls.length - 1];
      expect(lastCall[0]).toMatchObject({ method: 'POST' });
    });
  });

  it('applies status filter when changed', async () => {
    const user = userEvent.setup();
    renderPage();

    await waitFor(() => {
      expect(screen.getByLabelText('Status')).toBeInTheDocument();
    });

    const statusSelect = screen.getByLabelText('Status');
    await user.selectOptions(statusSelect, '4'); // 5xx Server Error (index 4)

    await waitFor(() => {
      const lastCall = mockListLogs.mock.calls[mockListLogs.mock.calls.length - 1];
      expect(lastCall[0]).toMatchObject({ statusMin: 500, statusMax: 599 });
    });
  });

  it('applies date range filter when changed', async () => {
    const user = userEvent.setup();
    renderPage();

    await waitFor(() => {
      expect(screen.getByLabelText('Time Range')).toBeInTheDocument();
    });

    const dateSelect = screen.getByLabelText('Time Range');
    await user.selectOptions(dateSelect, '7d');

    await waitFor(() => {
      const lastCall = mockListLogs.mock.calls[mockListLogs.mock.calls.length - 1];
      expect(lastCall[0].createdAfter).toBeDefined();
    });
  });

  // ── Sorting ─────────────────────────────────────────────────────────────

  it('sorts by column when clicking header', async () => {
    const user = userEvent.setup();
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('logs-table')).toBeInTheDocument();
    });

    // Click Status header to sort (in table header)
    const table = screen.getByTestId('logs-table');
    const thead = within(table).getAllByRole('columnheader');
    const statusHeader = thead.find(th => th.textContent?.startsWith('Status'))!;
    await user.click(statusHeader);

    await waitFor(() => {
      const lastCall = mockListLogs.mock.calls[mockListLogs.mock.calls.length - 1];
      expect(lastCall[0].sort).toBe('-status');
    });
  });

  // ── Log detail modal ───────────────────────────────────────────────────

  it('opens log detail modal when clicking a row', async () => {
    const user = userEvent.setup();
    renderPage();

    await waitFor(() => {
      expect(screen.getAllByTestId('log-row')).toHaveLength(5);
    });

    const firstRow = screen.getAllByTestId('log-row')[0];
    await user.click(firstRow);

    await waitFor(() => {
      const modal = screen.getByTestId('log-detail-modal');
      expect(modal).toBeInTheDocument();
      expect(within(modal).getByText('Request Log Detail')).toBeInTheDocument();
    });
  });

  it('shows log details in the modal', async () => {
    const user = userEvent.setup();
    renderPage();

    await waitFor(() => {
      expect(screen.getAllByTestId('log-row')).toHaveLength(5);
    });

    const firstRow = screen.getAllByTestId('log-row')[0];
    await user.click(firstRow);

    await waitFor(() => {
      const modal = screen.getByTestId('log-detail-modal');
      expect(within(modal).getByText('log-001')).toBeInTheDocument();
      expect(within(modal).getByText('/api/health')).toBeInTheDocument();
      expect(within(modal).getByText('10.0.0.1')).toBeInTheDocument();
      expect(within(modal).getByText('admin-user')).toBeInTheDocument();
      expect(within(modal).getByText('TestAgent/1.0')).toBeInTheDocument();
      expect(within(modal).getByText('req-detail-001')).toBeInTheDocument();
    });
  });

  it('closes the detail modal when clicking Close', async () => {
    const user = userEvent.setup();
    renderPage();

    await waitFor(() => {
      expect(screen.getAllByTestId('log-row')).toHaveLength(5);
    });

    await user.click(screen.getAllByTestId('log-row')[0]);

    await waitFor(() => {
      expect(screen.getByTestId('log-detail-modal')).toBeInTheDocument();
    });

    const closeButton = screen.getByRole('button', { name: 'Close detail' });
    await user.click(closeButton);

    await waitFor(() => {
      expect(screen.queryByTestId('log-detail-modal')).not.toBeInTheDocument();
    });
  });

  it('closes the detail modal when clicking the backdrop', async () => {
    const user = userEvent.setup();
    renderPage();

    await waitFor(() => {
      expect(screen.getAllByTestId('log-row')).toHaveLength(5);
    });

    await user.click(screen.getAllByTestId('log-row')[0]);

    await waitFor(() => {
      expect(screen.getByTestId('log-detail-modal')).toBeInTheDocument();
    });

    // Click the backdrop (the outer div of the modal)
    const backdrop = screen.getByTestId('log-detail-modal');
    await user.click(backdrop);

    await waitFor(() => {
      expect(screen.queryByTestId('log-detail-modal')).not.toBeInTheDocument();
    });
  });

  // ── Error handling ──────────────────────────────────────────────────────

  it('shows error message when log fetch fails', async () => {
    const errorBody: ErrorResponseBody = {
      code: 500,
      message: 'Internal server error',
      data: {},
    };
    mockListLogs.mockRejectedValue(new ApiError(500, errorBody));
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('Internal server error')).toBeInTheDocument();
    });
  });

  it('shows connection error for non-API errors', async () => {
    mockListLogs.mockRejectedValue(new Error('fetch failed'));
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('Unable to connect to the server.')).toBeInTheDocument();
    });
  });

  it('retries fetching when clicking Retry', async () => {
    const errorBody: ErrorResponseBody = {
      code: 500,
      message: 'Server error',
      data: {},
    };
    mockListLogs.mockRejectedValueOnce(new ApiError(500, errorBody));
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('Server error')).toBeInTheDocument();
    });

    // Now mock successful response
    mockListLogs.mockResolvedValue(SAMPLE_LOGS);
    const user = userEvent.setup();
    await user.click(screen.getByText('Retry'));

    await waitFor(() => {
      expect(screen.queryByText('Server error')).not.toBeInTheDocument();
      expect(screen.getByTestId('logs-table')).toBeInTheDocument();
    });
  });

  // ── API call parameters ────────────────────────────────────────────────

  it('fetches logs with default parameters on mount', async () => {
    renderPage();

    await waitFor(() => {
      expect(mockListLogs).toHaveBeenCalled();
    });

    const call = mockListLogs.mock.calls[0][0];
    expect(call.page).toBe(1);
    expect(call.perPage).toBe(30);
    expect(call.sort).toBe('-created');
    expect(call.createdAfter).toBeDefined();
    expect(call.createdBefore).toBeDefined();
  });

  it('fetches stats on mount', async () => {
    renderPage();

    await waitFor(() => {
      expect(mockGetLogStats).toHaveBeenCalled();
    });

    const call = mockGetLogStats.mock.calls[0][0];
    expect(call.createdAfter).toBeDefined();
    expect(call.createdBefore).toBeDefined();
    expect(call.groupBy).toBeDefined();
  });

  it('fetches log detail when row is clicked', async () => {
    const user = userEvent.setup();
    renderPage();

    await waitFor(() => {
      expect(screen.getAllByTestId('log-row')).toHaveLength(5);
    });

    await user.click(screen.getAllByTestId('log-row')[0]);

    await waitFor(() => {
      expect(mockGetLog).toHaveBeenCalledWith('log-001');
    });
  });

  // ── Stats display edge cases ────────────────────────────────────────────

  it('handles zero total requests in stats', async () => {
    mockGetLogStats.mockResolvedValue({
      ...SAMPLE_STATS,
      totalRequests: 0,
      statusCounts: { success: 0, redirect: 0, clientError: 0, serverError: 0 },
      avgDurationMs: 0,
      maxDurationMs: 0,
      timeline: [],
    });
    renderPage();

    await waitFor(() => {
      const statsOverview = screen.getByTestId('stats-overview');
      expect(within(statsOverview).getByText('0')).toBeInTheDocument();
      expect(within(statsOverview).getByText('0.0%')).toBeInTheDocument();
    });
  });

  it('does not render timeline chart when timeline is empty', async () => {
    mockGetLogStats.mockResolvedValue({
      ...SAMPLE_STATS,
      timeline: [],
    });
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('stats-overview')).toBeInTheDocument();
    });

    expect(screen.queryByTestId('timeline-chart')).not.toBeInTheDocument();
  });

  it('does not render status breakdown when total requests is 0', async () => {
    mockGetLogStats.mockResolvedValue({
      ...SAMPLE_STATS,
      totalRequests: 0,
      statusCounts: { success: 0, redirect: 0, clientError: 0, serverError: 0 },
    });
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('stats-overview')).toBeInTheDocument();
    });

    expect(screen.queryByTestId('status-breakdown')).not.toBeInTheDocument();
  });

  // ── Duration formatting ────────────────────────────────────────────────

  it('displays duration in seconds for slow requests', async () => {
    mockListLogs.mockResolvedValue({
      ...SAMPLE_LOGS,
      items: [makeLogEntry({ id: 'slow-1', durationMs: 2500 })],
    });
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('2.50s')).toBeInTheDocument();
    });
  });

  // ── Auth ID display ────────────────────────────────────────────────────

  it('shows em dash for anonymous requests in table', async () => {
    mockListLogs.mockResolvedValue({
      ...SAMPLE_LOGS,
      items: [makeLogEntry({ id: 'anon-1', authId: '' })],
    });
    renderPage();

    await waitFor(() => {
      const row = screen.getByTestId('log-row');
      expect(within(row).getByText('\u2014')).toBeInTheDocument();
    });
  });

  it('shows (anonymous) in detail modal for empty authId', async () => {
    mockGetLog.mockResolvedValue(makeLogEntry({ authId: '' }));
    const user = userEvent.setup();
    renderPage();

    await waitFor(() => {
      expect(screen.getAllByTestId('log-row')).toHaveLength(5);
    });

    await user.click(screen.getAllByTestId('log-row')[0]);

    await waitFor(() => {
      const modal = screen.getByTestId('log-detail-modal');
      expect(within(modal).getByText('(anonymous)')).toBeInTheDocument();
    });
  });
});
