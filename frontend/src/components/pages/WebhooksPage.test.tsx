import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { WebhooksPage } from './WebhooksPage';
import { ApiError } from '../../lib/api';
import type { Webhook, WebhookDeliveryLog, Collection, ListResponse, TestWebhookResponse } from '../../lib/api/types';

// ── Test data ────────────────────────────────────────────────────────────────

function makeWebhook(overrides: Partial<Webhook> = {}): Webhook {
  return {
    id: 'wh_1',
    collection: 'posts',
    url: 'https://example.com/hook',
    events: ['create', 'update'],
    enabled: true,
    created: '2026-03-20T10:00:00Z',
    updated: '2026-03-20T10:00:00Z',
    ...overrides,
  };
}

function makeDeliveryLog(overrides: Partial<WebhookDeliveryLog> = {}): WebhookDeliveryLog {
  return {
    id: 'dl_1',
    webhookId: 'wh_1',
    event: 'create',
    collection: 'posts',
    recordId: 'rec_abc',
    url: 'https://example.com/hook',
    responseStatus: 200,
    attempt: 1,
    status: 'success',
    created: '2026-03-20T10:05:00Z',
    ...overrides,
  };
}

function makeCollection(overrides: Partial<Collection> = {}): Collection {
  return {
    id: 'col_1',
    name: 'posts',
    type: 'base',
    fields: [],
    rules: { listRule: null, viewRule: null, createRule: null, updateRule: null, deleteRule: null },
    ...overrides,
  };
}

const SAMPLE_WEBHOOKS: Webhook[] = [
  makeWebhook({ id: 'wh_1', collection: 'posts', url: 'https://example.com/hook1', events: ['create', 'update'] }),
  makeWebhook({ id: 'wh_2', collection: 'users', url: 'https://example.com/hook2', events: ['delete'], enabled: false }),
];

const SAMPLE_COLLECTIONS: Collection[] = [
  makeCollection({ id: 'col_1', name: 'posts' }),
  makeCollection({ id: 'col_2', name: 'users' }),
];

const SAMPLE_DELIVERIES: ListResponse<WebhookDeliveryLog> = {
  page: 1,
  perPage: 20,
  totalPages: 1,
  totalItems: 2,
  items: [
    makeDeliveryLog({ id: 'dl_1', event: 'create', status: 'success', responseStatus: 200 }),
    makeDeliveryLog({ id: 'dl_2', event: 'update', status: 'failed', responseStatus: 500, error: 'Internal Server Error' }),
  ],
};

// ── Mocks ────────────────────────────────────────────────────────────────────

const mockListWebhooks = vi.fn();
const mockListCollections = vi.fn();
const mockCreateWebhook = vi.fn();
const mockUpdateWebhook = vi.fn();
const mockDeleteWebhook = vi.fn();
const mockListWebhookDeliveries = vi.fn();
const mockTestWebhook = vi.fn();

vi.mock('../../lib/auth/client', () => ({
  client: {
    listWebhooks: (...args: unknown[]) => mockListWebhooks(...args),
    listCollections: (...args: unknown[]) => mockListCollections(...args),
    createWebhook: (...args: unknown[]) => mockCreateWebhook(...args),
    updateWebhook: (...args: unknown[]) => mockUpdateWebhook(...args),
    deleteWebhook: (...args: unknown[]) => mockDeleteWebhook(...args),
    listWebhookDeliveries: (...args: unknown[]) => mockListWebhookDeliveries(...args),
    testWebhook: (...args: unknown[]) => mockTestWebhook(...args),
    getWebhook: vi.fn(),
    get isAuthenticated() { return true; },
    get token() { return 'mock-token'; },
    logout: vi.fn(),
  },
}));

Object.defineProperty(window, 'location', {
  value: { href: '', pathname: '/_/webhooks', origin: 'http://localhost:8090', reload: vi.fn() },
  writable: true,
});

// ── Setup ────────────────────────────────────────────────────────────────────

beforeEach(() => {
  vi.clearAllMocks();
  mockListWebhooks.mockResolvedValue(SAMPLE_WEBHOOKS);
  mockListCollections.mockResolvedValue({ items: SAMPLE_COLLECTIONS, page: 1, perPage: 50, totalPages: 1, totalItems: 2 });
  mockListWebhookDeliveries.mockResolvedValue(SAMPLE_DELIVERIES);
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe('WebhooksPage', () => {
  describe('rendering', () => {
    it('renders the page title and description', async () => {
      render(<WebhooksPage />);
      expect(screen.getByRole('heading', { name: 'Webhooks', level: 1 })).toBeInTheDocument();
      await waitFor(() => {
        expect(screen.getByText(/Configure HTTP callbacks/)).toBeInTheDocument();
      });
    });

    it('displays loading spinner initially', () => {
      mockListWebhooks.mockReturnValue(new Promise(() => {})); // Never resolves
      render(<WebhooksPage />);
      expect(screen.getByRole('status', { name: /loading webhooks/i })).toBeInTheDocument();
    });

    it('renders webhook list after loading', async () => {
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
        expect(screen.getByText('https://example.com/hook2')).toBeInTheDocument();
      });
    });

    it('shows empty state when no webhooks exist', async () => {
      mockListWebhooks.mockResolvedValue([]);
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('No webhooks configured')).toBeInTheDocument();
      });
    });

    it('renders event badges for each webhook', async () => {
      render(<WebhooksPage />);
      await waitFor(() => {
        const table = screen.getByRole('table', { name: 'Webhooks' });
        const rows = within(table).getAllByRole('row');
        // Header + 2 data rows
        expect(rows).toHaveLength(3);
      });
    });

    it('shows active/disabled status badges', async () => {
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('Active')).toBeInTheDocument();
        expect(screen.getByText('Disabled')).toBeInTheDocument();
      });
    });

    it('displays error message on load failure', async () => {
      mockListWebhooks.mockRejectedValue(new ApiError(500, { code: 500, message: 'Server error', data: {} }));
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByRole('alert')).toHaveTextContent('Server error');
      });
    });
  });

  describe('filtering', () => {
    it('renders collection filter dropdown', async () => {
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByLabelText('Collection:')).toBeInTheDocument();
      });
    });

    it('filters webhooks by collection', async () => {
      const user = userEvent.setup();
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const select = screen.getByLabelText('Collection:');
      await user.selectOptions(select, 'posts');

      await waitFor(() => {
        expect(mockListWebhooks).toHaveBeenCalledWith('posts');
      });
    });
  });

  describe('create webhook', () => {
    it('opens create form on button click', async () => {
      const user = userEvent.setup();
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      // Click the header "Add Webhook" button
      const addButtons = screen.getAllByText('Add Webhook');
      await user.click(addButtons[0]);

      await waitFor(() => {
        expect(screen.getByRole('dialog', { name: /add webhook/i })).toBeInTheDocument();
      });
    });

    it('submits create form with correct data', async () => {
      const user = userEvent.setup();
      mockCreateWebhook.mockResolvedValue(makeWebhook({ id: 'wh_new' }));

      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      // Click the first "Add Webhook" button (header)
      const addButtons = screen.getAllByRole('button', { name: /add webhook/i });
      await user.click(addButtons[0]);

      // Fill in URL
      const urlInput = screen.getByLabelText('URL');
      await user.clear(urlInput);
      await user.type(urlInput, 'https://new-hook.com/endpoint');

      // Check delete event
      const deleteCheckbox = screen.getByRole('checkbox', { name: /delete/i });
      await user.click(deleteCheckbox);

      // Submit
      await user.click(screen.getByRole('button', { name: /create/i }));

      await waitFor(() => {
        expect(mockCreateWebhook).toHaveBeenCalledWith(
          expect.objectContaining({
            collection: 'posts',
            url: 'https://new-hook.com/endpoint',
            events: expect.arrayContaining(['create', 'delete']),
            enabled: true,
          }),
        );
      });
    });

    it('shows validation errors on create failure', async () => {
      const user = userEvent.setup();
      mockCreateWebhook.mockRejectedValue(
        new ApiError(400, {
          code: 400,
          message: 'Validation failed',
          data: { url: { code: 'invalid', message: 'Invalid URL format' } },
        }),
      );

      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const addButtons = screen.getAllByRole('button', { name: /add webhook/i });
      await user.click(addButtons[0]);

      const urlInput = screen.getByLabelText('URL');
      await user.clear(urlInput);
      await user.type(urlInput, 'https://bad-url.com/test');

      await user.click(screen.getByRole('button', { name: /create/i }));

      await waitFor(() => {
        expect(screen.getByText('Invalid URL format')).toBeInTheDocument();
      });
    });

    it('closes create form on cancel', async () => {
      const user = userEvent.setup();
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const addButtons = screen.getAllByText('Add Webhook');
      await user.click(addButtons[0]);

      expect(screen.getByRole('dialog', { name: /add webhook/i })).toBeInTheDocument();

      await user.click(screen.getByText('Cancel'));

      await waitFor(() => {
        expect(screen.queryByRole('dialog', { name: /add webhook/i })).not.toBeInTheDocument();
      });
    });
  });

  describe('edit webhook', () => {
    it('opens edit form with existing data', async () => {
      const user = userEvent.setup();
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const editButtons = screen.getAllByLabelText('Edit webhook');
      await user.click(editButtons[0]);

      await waitFor(() => {
        const dialog = screen.getByRole('dialog', { name: /edit webhook/i });
        expect(dialog).toBeInTheDocument();
        expect(within(dialog).getByLabelText('URL')).toHaveValue('https://example.com/hook1');
      });
    });

    it('submits update with changed data', async () => {
      const user = userEvent.setup();
      mockUpdateWebhook.mockResolvedValue(makeWebhook({ url: 'https://updated.com/hook' }));

      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const editButtons = screen.getAllByLabelText('Edit webhook');
      await user.click(editButtons[0]);

      const urlInput = screen.getByLabelText('URL');
      await user.clear(urlInput);
      await user.type(urlInput, 'https://updated.com/hook');

      await user.click(screen.getByRole('button', { name: /update/i }));

      await waitFor(() => {
        expect(mockUpdateWebhook).toHaveBeenCalledWith(
          'wh_1',
          expect.objectContaining({ url: 'https://updated.com/hook' }),
        );
      });
    });
  });

  describe('delete webhook', () => {
    it('shows confirmation modal before delete', async () => {
      const user = userEvent.setup();
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const deleteButtons = screen.getAllByLabelText('Delete webhook');
      await user.click(deleteButtons[0]);

      expect(screen.getByRole('dialog', { name: /confirm delete/i })).toBeInTheDocument();
      expect(screen.getByText(/Are you sure/)).toBeInTheDocument();
    });

    it('deletes webhook on confirm', async () => {
      const user = userEvent.setup();
      mockDeleteWebhook.mockResolvedValue(undefined);

      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const deleteButtons = screen.getAllByLabelText('Delete webhook');
      await user.click(deleteButtons[0]);

      // Click the "Delete" button in the confirmation modal
      const dialog = screen.getByRole('dialog', { name: /confirm delete/i });
      await user.click(within(dialog).getByText('Delete'));

      await waitFor(() => {
        expect(mockDeleteWebhook).toHaveBeenCalledWith('wh_1');
      });
    });

    it('cancels delete on cancel click', async () => {
      const user = userEvent.setup();
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const deleteButtons = screen.getAllByLabelText('Delete webhook');
      await user.click(deleteButtons[0]);

      const dialog = screen.getByRole('dialog', { name: /confirm delete/i });
      await user.click(within(dialog).getByText('Cancel'));

      await waitFor(() => {
        expect(screen.queryByRole('dialog', { name: /confirm delete/i })).not.toBeInTheDocument();
      });
      expect(mockDeleteWebhook).not.toHaveBeenCalled();
    });
  });

  describe('toggle enabled', () => {
    it('toggles webhook enabled state', async () => {
      const user = userEvent.setup();
      mockUpdateWebhook.mockResolvedValue(makeWebhook({ enabled: false }));

      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('Active')).toBeInTheDocument();
      });

      await user.click(screen.getByText('Active'));

      await waitFor(() => {
        expect(mockUpdateWebhook).toHaveBeenCalledWith('wh_1', { enabled: false });
      });
    });
  });

  describe('test webhook', () => {
    it('sends test and shows success result', async () => {
      const user = userEvent.setup();
      const testResponse: TestWebhookResponse = { success: true, statusCode: 200 };
      mockTestWebhook.mockResolvedValue(testResponse);

      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const testButtons = screen.getAllByLabelText('Test webhook');
      await user.click(testButtons[0]);

      await waitFor(() => {
        expect(mockTestWebhook).toHaveBeenCalledWith('wh_1');
        expect(screen.getByText(/Test delivery succeeded/)).toBeInTheDocument();
      });
    });

    it('sends test and shows failure result', async () => {
      const user = userEvent.setup();
      const testResponse: TestWebhookResponse = { success: false, statusCode: 500, error: 'Connection refused' };
      mockTestWebhook.mockResolvedValue(testResponse);

      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const testButtons = screen.getAllByLabelText('Test webhook');
      await user.click(testButtons[0]);

      await waitFor(() => {
        expect(screen.getByText(/Connection refused/)).toBeInTheDocument();
      });
    });

    it('handles test request error', async () => {
      const user = userEvent.setup();
      mockTestWebhook.mockRejectedValue(new ApiError(500, { code: 500, message: 'Internal error', data: {} }));

      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const testButtons = screen.getAllByLabelText('Test webhook');
      await user.click(testButtons[0]);

      await waitFor(() => {
        expect(screen.getByText('Internal error')).toBeInTheDocument();
      });
    });
  });

  describe('delivery history', () => {
    it('opens delivery history modal', async () => {
      const user = userEvent.setup();
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const historyButtons = screen.getAllByLabelText('View delivery history');
      await user.click(historyButtons[0]);

      await waitFor(() => {
        expect(screen.getByRole('dialog', { name: /delivery history/i })).toBeInTheDocument();
      });
    });

    it('displays delivery log entries', async () => {
      const user = userEvent.setup();
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const historyButtons = screen.getAllByLabelText('View delivery history');
      await user.click(historyButtons[0]);

      await waitFor(() => {
        const dialog = screen.getByRole('dialog', { name: /delivery history/i });
        const table = within(dialog).getByRole('table', { name: /delivery log/i });
        // Header + 2 entries
        expect(within(table).getAllByRole('row')).toHaveLength(3);
      });
    });

    it('shows delivery status badges', async () => {
      const user = userEvent.setup();
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const historyButtons = screen.getAllByLabelText('View delivery history');
      await user.click(historyButtons[0]);

      await waitFor(() => {
        const dialog = screen.getByRole('dialog', { name: /delivery history/i });
        expect(within(dialog).getByText('success')).toBeInTheDocument();
        expect(within(dialog).getByText('failed')).toBeInTheDocument();
      });
    });

    it('shows empty state for no deliveries', async () => {
      const user = userEvent.setup();
      mockListWebhookDeliveries.mockResolvedValue({
        page: 1, perPage: 20, totalPages: 0, totalItems: 0, items: [],
      });

      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const historyButtons = screen.getAllByLabelText('View delivery history');
      await user.click(historyButtons[0]);

      await waitFor(() => {
        expect(screen.getByText('No delivery history yet.')).toBeInTheDocument();
      });
    });

    it('closes delivery history on close button', async () => {
      const user = userEvent.setup();
      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const historyButtons = screen.getAllByLabelText('View delivery history');
      await user.click(historyButtons[0]);

      await waitFor(() => {
        expect(screen.getByRole('dialog', { name: /delivery history/i })).toBeInTheDocument();
      });

      await user.click(screen.getByLabelText('Close delivery history'));

      await waitFor(() => {
        expect(screen.queryByRole('dialog', { name: /delivery history/i })).not.toBeInTheDocument();
      });
    });

    it('handles delivery history load error', async () => {
      const user = userEvent.setup();
      mockListWebhookDeliveries.mockRejectedValue(
        new ApiError(500, { code: 500, message: 'Failed to fetch', data: {} }),
      );

      render(<WebhooksPage />);
      await waitFor(() => {
        expect(screen.getByText('https://example.com/hook1')).toBeInTheDocument();
      });

      const historyButtons = screen.getAllByLabelText('View delivery history');
      await user.click(historyButtons[0]);

      await waitFor(() => {
        const dialog = screen.getByRole('dialog', { name: /delivery history/i });
        expect(within(dialog).getByRole('alert')).toHaveTextContent('Failed to fetch');
      });
    });
  });

  describe('empty state actions', () => {
    it('opens create form from empty state button', async () => {
      const user = userEvent.setup();
      mockListWebhooks.mockResolvedValue([]);
      render(<WebhooksPage />);

      await waitFor(() => {
        expect(screen.getByText('No webhooks configured')).toBeInTheDocument();
      });

      // There are two "Add Webhook" buttons: header and empty state. Click the last one (empty state).
      const addButtons = screen.getAllByRole('button', { name: /add webhook/i });
      await user.click(addButtons[addButtons.length - 1]);

      await waitFor(() => {
        expect(screen.getByRole('dialog', { name: /add webhook/i })).toBeInTheDocument();
      });
    });
  });
});
