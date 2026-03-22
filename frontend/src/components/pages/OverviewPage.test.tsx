import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { OverviewPage } from './OverviewPage';
import { ApiError } from '../../lib/api';
import type {
  Collection,
  LogEntry,
  LogStats,
  ListResponse,
  ErrorResponseBody,
} from '../../lib/api/types';

// ── Test data ────────────────────────────────────────────────────────────────

function makeCollection(name: string, type: 'base' | 'auth' | 'view' = 'base', fieldCount = 2): Collection {
  const fields = Array.from({ length: fieldCount }, (_, i) => ({
    id: `f${i}`,
    name: `field_${i}`,
    type: { type: 'text' as const, minLength: 0, maxLength: 500, pattern: null, searchable: true },
    required: false,
    unique: false,
    sortOrder: i,
  }));

  return {
    id: `col_${name}`,
    name,
    type,
    fields,
    rules: { listRule: null, viewRule: null, createRule: null, updateRule: null, deleteRule: null },
    indexes: [],
  };
}

function makeLogEntry(overrides: Partial<LogEntry> = {}): LogEntry {
  return {
    id: overrides.id ?? 'log_1',
    method: overrides.method ?? 'GET',
    url: overrides.url ?? '/api/collections',
    status: overrides.status ?? 200,
    ip: overrides.ip ?? '127.0.0.1',
    authId: overrides.authId ?? '',
    durationMs: overrides.durationMs ?? 12,
    userAgent: overrides.userAgent ?? 'test-agent',
    requestId: overrides.requestId ?? 'req_1',
    created: overrides.created ?? '2026-03-21T10:00:00Z',
  };
}

const TEST_COLLECTIONS = [
  makeCollection('posts', 'base', 3),
  makeCollection('users', 'auth', 2),
  makeCollection('post_stats', 'view', 0),
];

const TEST_LOG_STATS: LogStats = {
  totalRequests: 1500,
  statusCounts: {
    success: 1200,
    redirect: 50,
    clientError: 200,
    serverError: 50,
  },
  avgDurationMs: 45.2,
  maxDurationMs: 2300,
  timeline: [
    { date: '2026-03-20', total: 700 },
    { date: '2026-03-21', total: 800 },
  ],
};

const TEST_LOGS: LogEntry[] = [
  makeLogEntry({ id: 'log_1', method: 'GET', url: '/api/collections', status: 200, durationMs: 12 }),
  makeLogEntry({ id: 'log_2', method: 'POST', url: '/api/collections/posts/records', status: 201, durationMs: 34 }),
  makeLogEntry({ id: 'log_3', method: 'DELETE', url: '/api/collections/posts/records/r1', status: 204, durationMs: 8 }),
];

// ── Mocks ────────────────────────────────────────────────────────────────────

const mockListCollections = vi.fn();
const mockListLogs = vi.fn();
const mockGetLogStats = vi.fn();
const mockHealth = vi.fn();
const mockCountRecords = vi.fn();

vi.mock('../../lib/auth/client', () => ({
  client: {
    listCollections: (...args: unknown[]) => mockListCollections(...args),
    listLogs: (...args: unknown[]) => mockListLogs(...args),
    getLogStats: (...args: unknown[]) => mockGetLogStats(...args),
    health: (...args: unknown[]) => mockHealth(...args),
    countRecords: (...args: unknown[]) => mockCountRecords(...args),
    get isAuthenticated() {
      return true;
    },
    get token() {
      return 'mock-token';
    },
    logout: vi.fn(),
  },
}));

// ── Setup ────────────────────────────────────────────────────────────────────

function setupMocks(overrides: {
  collections?: Collection[];
  logs?: LogEntry[];
  logStats?: LogStats;
  healthStatus?: string;
  recordCounts?: Record<string, number>;
} = {}) {
  const collections = overrides.collections ?? TEST_COLLECTIONS;
  const logs = overrides.logs ?? TEST_LOGS;
  const logStats = overrides.logStats ?? TEST_LOG_STATS;
  const healthStatus = overrides.healthStatus ?? 'ok';
  const recordCounts = overrides.recordCounts ?? { posts: 42, users: 10, post_stats: 5 };

  mockListCollections.mockResolvedValue({
    page: 1,
    perPage: 30,
    totalPages: 1,
    totalItems: collections.length,
    items: collections,
  } as ListResponse<Collection>);

  mockListLogs.mockResolvedValue({
    page: 1,
    perPage: 10,
    totalPages: 1,
    totalItems: logs.length,
    items: logs,
  } as ListResponse<LogEntry>);

  mockGetLogStats.mockResolvedValue(logStats);
  mockHealth.mockResolvedValue({ status: healthStatus });
  mockCountRecords.mockImplementation((name: string) =>
    Promise.resolve({ count: recordCounts[name] ?? 0 }),
  );
}

beforeEach(() => {
  vi.clearAllMocks();
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe('OverviewPage', () => {
  describe('loading state', () => {
    it('shows loading indicator while fetching data', () => {
      // Never resolve the promises
      mockListCollections.mockReturnValue(new Promise(() => {}));
      mockListLogs.mockReturnValue(new Promise(() => {}));
      mockGetLogStats.mockReturnValue(new Promise(() => {}));
      mockHealth.mockReturnValue(new Promise(() => {}));

      render(<OverviewPage />);

      expect(screen.getByTestId('loading-state')).toBeInTheDocument();
      expect(screen.getByText(/loading dashboard data/i)).toBeInTheDocument();
    });
  });

  describe('error state', () => {
    it('shows error message when all API calls fail', async () => {
      mockListCollections.mockRejectedValue(
        new ApiError(500, { code: 500, message: 'Internal error', data: {} }),
      );
      mockListLogs.mockRejectedValue(new Error('Network error'));
      mockGetLogStats.mockRejectedValue(new Error('Network error'));
      mockHealth.mockRejectedValue(new Error('Network error'));

      render(<OverviewPage />);

      await waitFor(() => {
        expect(screen.getByTestId('error-state')).toBeInTheDocument();
      });

      expect(screen.getByText('Internal error')).toBeInTheDocument();
    });

    it('shows retry button on error', async () => {
      mockListCollections.mockRejectedValue(
        new ApiError(500, { code: 500, message: 'Server error', data: {} }),
      );
      mockListLogs.mockRejectedValue(new Error('fail'));
      mockGetLogStats.mockRejectedValue(new Error('fail'));
      mockHealth.mockRejectedValue(new Error('fail'));

      render(<OverviewPage />);

      await waitFor(() => {
        expect(screen.getByText('Retry')).toBeInTheDocument();
      });
    });

    it('retries loading data when retry button is clicked', async () => {
      const user = userEvent.setup();

      // First call fails
      mockListCollections.mockRejectedValueOnce(
        new ApiError(500, { code: 500, message: 'Server error', data: {} }),
      );
      mockListLogs.mockRejectedValueOnce(new Error('fail'));
      mockGetLogStats.mockRejectedValueOnce(new Error('fail'));
      mockHealth.mockRejectedValueOnce(new Error('fail'));

      render(<OverviewPage />);

      await waitFor(() => {
        expect(screen.getByText('Retry')).toBeInTheDocument();
      });

      // Setup success for retry
      setupMocks();

      await user.click(screen.getByText('Retry'));

      await waitFor(() => {
        expect(screen.getByText('Dashboard')).toBeInTheDocument();
      });
    });
  });

  describe('stats display', () => {
    it('displays total collections count', async () => {
      setupMocks();
      render(<OverviewPage />);

      await waitFor(() => {
        const stat = screen.getByTestId('stat-collections');
        expect(stat).toHaveTextContent('3');
      });
    });

    it('displays total records count', async () => {
      setupMocks({ recordCounts: { posts: 42, users: 10, post_stats: 5 } });
      render(<OverviewPage />);

      await waitFor(() => {
        const stat = screen.getByTestId('stat-total-records');
        expect(stat).toHaveTextContent('57');
      });
    });

    it('displays total requests count', async () => {
      setupMocks();
      render(<OverviewPage />);

      await waitFor(() => {
        const stat = screen.getByTestId('stat-total-requests');
        expect(stat).toHaveTextContent('1500');
      });
    });

    it('displays average response time', async () => {
      setupMocks();
      render(<OverviewPage />);

      await waitFor(() => {
        const stat = screen.getByTestId('stat-avg-response');
        expect(stat).toHaveTextContent('45ms');
      });
    });
  });

  describe('health status', () => {
    it('shows healthy badge when API returns ok', async () => {
      setupMocks({ healthStatus: 'ok' });
      render(<OverviewPage />);

      await waitFor(() => {
        const badge = screen.getByTestId('health-badge');
        expect(badge).toHaveTextContent('Healthy');
      });
    });

    it('shows unhealthy badge when API returns non-ok', async () => {
      setupMocks({ healthStatus: 'degraded' });
      render(<OverviewPage />);

      await waitFor(() => {
        const badge = screen.getByTestId('health-badge');
        expect(badge).toHaveTextContent('Unhealthy');
      });
    });

    it('shows unhealthy badge when health check fails', async () => {
      setupMocks();
      mockHealth.mockRejectedValue(new Error('Connection refused'));

      render(<OverviewPage />);

      await waitFor(() => {
        const badge = screen.getByTestId('health-badge');
        expect(badge).toHaveTextContent('Unhealthy');
      });
    });
  });

  describe('request status breakdown', () => {
    it('displays status counts', async () => {
      setupMocks();
      render(<OverviewPage />);

      await waitFor(() => {
        expect(screen.getByTestId('status-success')).toHaveTextContent('1200');
        expect(screen.getByTestId('status-redirect')).toHaveTextContent('50');
        expect(screen.getByTestId('status-client-error')).toHaveTextContent('200');
        expect(screen.getByTestId('status-server-error')).toHaveTextContent('50');
      });
    });
  });

  describe('collections list', () => {
    it('displays collection names with type badges', async () => {
      setupMocks();
      render(<OverviewPage />);

      await waitFor(() => {
        const list = screen.getByTestId('collections-list');
        expect(list).toBeInTheDocument();
      });

      expect(screen.getByText('posts')).toBeInTheDocument();
      expect(screen.getByText('users')).toBeInTheDocument();
      expect(screen.getByText('post_stats')).toBeInTheDocument();
    });

    it('displays field counts for each collection', async () => {
      setupMocks();
      render(<OverviewPage />);

      await waitFor(() => {
        expect(screen.getByText('3 fields')).toBeInTheDocument();
        expect(screen.getByText('2 fields')).toBeInTheDocument();
        expect(screen.getByText('0 fields')).toBeInTheDocument();
      });
    });

    it('links each collection to its detail page', async () => {
      setupMocks();
      render(<OverviewPage />);

      await waitFor(() => {
        const postsLink = screen.getByText('posts').closest('a');
        expect(postsLink).toHaveAttribute('href', '/_/collections/col_posts');
      });
    });

    it('does not render collections section when empty', async () => {
      setupMocks({ collections: [] });
      render(<OverviewPage />);

      await waitFor(() => {
        expect(screen.getByText('System Status')).toBeInTheDocument();
      });

      expect(screen.queryByTestId('collections-list')).not.toBeInTheDocument();
    });
  });

  describe('recent activity', () => {
    it('displays recent log entries in a table', async () => {
      setupMocks();
      render(<OverviewPage />);

      await waitFor(() => {
        const table = screen.getByTestId('recent-logs-table');
        expect(table).toBeInTheDocument();
      });

      // Check table headers
      expect(screen.getByText('Method')).toBeInTheDocument();
      expect(screen.getByText('URL')).toBeInTheDocument();
      expect(screen.getByText('Status')).toBeInTheDocument();
      expect(screen.getByText('Duration')).toBeInTheDocument();
      expect(screen.getByText('Time')).toBeInTheDocument();
    });

    it('displays log entry details', async () => {
      setupMocks();
      render(<OverviewPage />);

      await waitFor(() => {
        const table = screen.getByTestId('recent-logs-table');
        expect(table).toBeInTheDocument();
      });

      const table = screen.getByTestId('recent-logs-table');
      expect(table).toHaveTextContent('GET');
      expect(table).toHaveTextContent('/api/collections');
      expect(table).toHaveTextContent('200');
      expect(table).toHaveTextContent('12ms');
    });

    it('shows "No recent activity" when logs are empty', async () => {
      setupMocks({ logs: [] });
      render(<OverviewPage />);

      await waitFor(() => {
        expect(screen.getByTestId('no-logs')).toBeInTheDocument();
        expect(screen.getByText('No recent activity')).toBeInTheDocument();
      });
    });

    it('has a link to the full logs page', async () => {
      setupMocks();
      render(<OverviewPage />);

      await waitFor(() => {
        const viewAllLink = screen.getByText('View all logs');
        expect(viewAllLink.closest('a')).toHaveAttribute('href', '/_/logs');
      });
    });
  });

  describe('API calls', () => {
    it('fetches collections, logs, log stats, and health in parallel', async () => {
      setupMocks();
      render(<OverviewPage />);

      await waitFor(() => {
        expect(mockListCollections).toHaveBeenCalledOnce();
        expect(mockListLogs).toHaveBeenCalledWith({ perPage: 10, sort: '-created' });
        expect(mockGetLogStats).toHaveBeenCalledOnce();
        expect(mockHealth).toHaveBeenCalledOnce();
      });
    });

    it('fetches record counts for each collection', async () => {
      setupMocks();
      render(<OverviewPage />);

      await waitFor(() => {
        expect(mockCountRecords).toHaveBeenCalledWith('posts');
        expect(mockCountRecords).toHaveBeenCalledWith('users');
        expect(mockCountRecords).toHaveBeenCalledWith('post_stats');
      });
    });

    it('handles partial API failures gracefully', async () => {
      setupMocks();
      // Log stats fails but everything else succeeds
      mockGetLogStats.mockRejectedValue(new Error('Stats unavailable'));

      render(<OverviewPage />);

      await waitFor(() => {
        // Should still show collections and health
        expect(screen.getByTestId('stat-collections')).toHaveTextContent('3');
        expect(screen.getByTestId('health-badge')).toHaveTextContent('Healthy');
      });

      // Status breakdown should not be shown
      expect(screen.queryByTestId('status-success')).not.toBeInTheDocument();
    });
  });
});
