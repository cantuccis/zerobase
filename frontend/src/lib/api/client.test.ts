import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ZerobaseClient, ApiError, MemoryTokenStore, LocalStorageTokenStore } from './client';
import type {
  AdminAuthResponse,
  AuthResponse,
  Collection,
  ErrorResponseBody,
  ListResponse,
  BaseRecord,
  LogEntry,
  BackupEntry,
  Settings,
} from './types';

// ── Test helpers ──────────────────────────────────────────────────────────────

function createMockFetch(responses: Array<{ status: number; body: unknown; ok?: boolean }>) {
  let callIndex = 0;
  const calls: Array<{ url: string; init: RequestInit }> = [];

  const mockFetch = vi.fn(async (url: string, init?: RequestInit) => {
    calls.push({ url, init: init ?? {} });
    const resp = responses[callIndex] ?? responses[responses.length - 1];
    callIndex++;
    return {
      ok: resp.ok ?? (resp.status >= 200 && resp.status < 300),
      status: resp.status,
      json: async () => resp.body,
    } as Response;
  });

  return { mockFetch, calls: () => calls };
}

function createClient(mockFetch: typeof globalThis.fetch, tokenStore?: MemoryTokenStore) {
  return new ZerobaseClient({
    baseUrl: 'http://localhost:8090',
    fetch: mockFetch,
    tokenStore: tokenStore ?? new MemoryTokenStore(),
  });
}

const MOCK_AUTH_RECORD = {
  id: 'admin123',
  collectionId: '_pbc_superusers',
  collectionName: '_superusers',
  email: 'admin@example.com',
  emailVisibility: true,
  verified: true,
  created: '2025-01-01 00:00:00.000Z',
  updated: '2025-01-01 00:00:00.000Z',
};

const MOCK_ADMIN_AUTH: AdminAuthResponse = {
  token: 'jwt.token.here',
  admin: MOCK_AUTH_RECORD,
};

const MOCK_COLLECTION: Collection = {
  id: 'col_abc123',
  name: 'posts',
  type: 'base',
  fields: [
    {
      id: 'fld_001',
      name: 'title',
      type: { type: 'text', minLength: 0, maxLength: 255, pattern: null, searchable: false },
      required: true,
      unique: false,
      sortOrder: 0,
    },
  ],
  rules: {
    listRule: '',
    viewRule: '',
    createRule: null,
    updateRule: null,
    deleteRule: null,
  },
  indexes: [],
};

const MOCK_RECORD: BaseRecord = {
  id: 'rec_001',
  collectionId: 'col_abc123',
  collectionName: 'posts',
  created: '2025-01-01 00:00:00.000Z',
  updated: '2025-01-01 00:00:00.000Z',
  title: 'Hello World',
};

const MOCK_LIST_RESPONSE: ListResponse<BaseRecord> = {
  page: 1,
  perPage: 30,
  totalPages: 1,
  totalItems: 1,
  items: [MOCK_RECORD],
};

const MOCK_ERROR: ErrorResponseBody = {
  code: 400,
  message: 'Failed to create record.',
  data: {
    title: { code: 'validation_required', message: 'Missing required value.' },
  },
};

// ── Tests ─────────────────────────────────────────────────────────────────────

describe('ZerobaseClient', () => {
  // ── Construction ────────────────────────────────────────────────────────

  describe('construction', () => {
    it('strips trailing slash from baseUrl', () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: { status: 'ok' } }]);
      const client = new ZerobaseClient({
        baseUrl: 'http://localhost:8090/',
        fetch: mockFetch,
      });
      client.health();
      expect(calls()[0].url).toContain('http://localhost:8090/api/health');
    });

    it('defaults to MemoryTokenStore', () => {
      const { mockFetch } = createMockFetch([]);
      const client = new ZerobaseClient({ baseUrl: 'http://localhost:8090', fetch: mockFetch });
      expect(client.token).toBeNull();
      expect(client.isAuthenticated).toBe(false);
    });
  });

  // ── Token management ──────────────────────────────────────────────────

  describe('token management', () => {
    it('stores token after admin auth', async () => {
      const { mockFetch } = createMockFetch([{ status: 200, body: MOCK_ADMIN_AUTH }]);
      const client = createClient(mockFetch);

      await client.adminAuthWithPassword('admin@example.com', 'secret');
      expect(client.token).toBe('jwt.token.here');
      expect(client.isAuthenticated).toBe(true);
    });

    it('clears token on logout', async () => {
      const { mockFetch } = createMockFetch([{ status: 200, body: MOCK_ADMIN_AUTH }]);
      const client = createClient(mockFetch);

      await client.adminAuthWithPassword('admin@example.com', 'secret');
      expect(client.isAuthenticated).toBe(true);

      client.logout();
      expect(client.token).toBeNull();
      expect(client.isAuthenticated).toBe(false);
    });

    it('sends Authorization header when token is set', async () => {
      const store = new MemoryTokenStore();
      store.setToken('my-token');

      const { mockFetch, calls } = createMockFetch([{ status: 200, body: { status: 'ok' } }]);
      const client = createClient(mockFetch, store);

      await client.health();
      expect(calls()[0].init.headers).toHaveProperty('Authorization', 'Bearer my-token');
    });

    it('does not send Authorization header when no token', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: { status: 'ok' } }]);
      const client = createClient(mockFetch);

      await client.health();
      const headers = calls()[0].init.headers as Record<string, string>;
      expect(headers['Authorization']).toBeUndefined();
    });
  });

  // ── Error handling ────────────────────────────────────────────────────

  describe('error handling', () => {
    it('throws ApiError on non-ok response', async () => {
      const { mockFetch } = createMockFetch([{ status: 400, body: MOCK_ERROR, ok: false }]);
      const client = createClient(mockFetch);

      try {
        await client.createRecord('posts', { title: '' });
        expect.unreachable('should have thrown');
      } catch (err) {
        expect(err).toBeInstanceOf(ApiError);
        const apiErr = err as ApiError;
        expect(apiErr.status).toBe(400);
        expect(apiErr.response.code).toBe(400);
        expect(apiErr.response.data.title.code).toBe('validation_required');
        expect(apiErr.isValidation).toBe(true);
        expect(apiErr.isUnauthorized).toBe(false);
      }
    });

    it('throws ApiError with isUnauthorized for 401', async () => {
      const error: ErrorResponseBody = { code: 401, message: 'Invalid token', data: {} };
      const { mockFetch } = createMockFetch([{ status: 401, body: error, ok: false }]);
      const client = createClient(mockFetch);

      try {
        await client.listCollections();
        expect.unreachable('should have thrown');
      } catch (err) {
        expect((err as ApiError).isUnauthorized).toBe(true);
      }
    });

    it('throws ApiError with isForbidden for 403', async () => {
      const error: ErrorResponseBody = { code: 403, message: 'Forbidden', data: {} };
      const { mockFetch } = createMockFetch([{ status: 403, body: error, ok: false }]);
      const client = createClient(mockFetch);

      try {
        await client.getSettings();
        expect.unreachable('should have thrown');
      } catch (err) {
        expect((err as ApiError).isForbidden).toBe(true);
      }
    });

    it('throws ApiError with isNotFound for 404', async () => {
      const error: ErrorResponseBody = { code: 404, message: 'Not found', data: {} };
      const { mockFetch } = createMockFetch([{ status: 404, body: error, ok: false }]);
      const client = createClient(mockFetch);

      try {
        await client.getCollection('nonexistent');
        expect.unreachable('should have thrown');
      } catch (err) {
        expect((err as ApiError).isNotFound).toBe(true);
      }
    });

    it('ApiError message matches response message', async () => {
      const error: ErrorResponseBody = { code: 500, message: 'An internal error occurred.', data: {} };
      const { mockFetch } = createMockFetch([{ status: 500, body: error, ok: false }]);
      const client = createClient(mockFetch);

      try {
        await client.health();
        expect.unreachable('should have thrown');
      } catch (err) {
        expect((err as ApiError).message).toBe('An internal error occurred.');
      }
    });
  });

  // ── Admin Auth ────────────────────────────────────────────────────────

  describe('adminAuthWithPassword', () => {
    it('sends correct request body and stores token', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: MOCK_ADMIN_AUTH }]);
      const client = createClient(mockFetch);

      const result = await client.adminAuthWithPassword('admin@example.com', 'secret123');

      expect(calls()[0].url).toContain('/_/api/admins/auth-with-password');
      expect(calls()[0].init.method).toBe('POST');
      const body = JSON.parse(calls()[0].init.body as string);
      expect(body.identity).toBe('admin@example.com');
      expect(body.password).toBe('secret123');
      expect(result.token).toBe('jwt.token.here');
      expect(result.admin.id).toBe('admin123');
    });
  });

  // ── Collection Auth ───────────────────────────────────────────────────

  describe('authWithPassword', () => {
    it('sends request to correct collection path', async () => {
      const authResp: AuthResponse = { token: 'user-token', record: { ...MOCK_AUTH_RECORD, collectionName: 'users' } };
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: authResp }]);
      const client = createClient(mockFetch);

      const result = await client.authWithPassword('users', 'user@test.com', 'pass');

      expect(calls()[0].url).toContain('/api/collections/users/auth-with-password');
      expect(result.token).toBe('user-token');
      expect(client.token).toBe('user-token');
    });
  });

  describe('authRefresh', () => {
    it('refreshes token and stores new one', async () => {
      const authResp: AuthResponse = { token: 'refreshed-token', record: MOCK_AUTH_RECORD };
      const store = new MemoryTokenStore();
      store.setToken('old-token');

      const { mockFetch, calls } = createMockFetch([{ status: 200, body: authResp }]);
      const client = createClient(mockFetch, store);

      await client.authRefresh('users');

      expect(calls()[0].url).toContain('/api/collections/users/auth-refresh');
      expect(client.token).toBe('refreshed-token');
    });
  });

  // ── Collections ───────────────────────────────────────────────────────

  describe('collections', () => {
    it('lists collections', async () => {
      const listResp: ListResponse<Collection> = {
        page: 1, perPage: 30, totalPages: 1, totalItems: 1, items: [MOCK_COLLECTION],
      };
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: listResp }]);
      const client = createClient(mockFetch);

      const result = await client.listCollections();

      expect(calls()[0].url).toContain('/api/collections');
      expect(calls()[0].init.method).toBe('GET');
      expect(result.items).toHaveLength(1);
      expect(result.items[0].name).toBe('posts');
    });

    it('creates a collection', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: MOCK_COLLECTION }]);
      const client = createClient(mockFetch);

      const result = await client.createCollection({ name: 'posts', type: 'base', fields: [] });

      expect(calls()[0].init.method).toBe('POST');
      expect(result.name).toBe('posts');
    });

    it('gets a collection by name', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: MOCK_COLLECTION }]);
      const client = createClient(mockFetch);

      const result = await client.getCollection('posts');

      expect(calls()[0].url).toContain('/api/collections/posts');
      expect(result.id).toBe('col_abc123');
    });

    it('updates a collection', async () => {
      const updated = { ...MOCK_COLLECTION, name: 'articles' };
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: updated }]);
      const client = createClient(mockFetch);

      const result = await client.updateCollection('posts', { name: 'articles' });

      expect(calls()[0].init.method).toBe('PATCH');
      expect(result.name).toBe('articles');
    });

    it('deletes a collection', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 204, body: undefined }]);
      const client = createClient(mockFetch);

      await client.deleteCollection('posts');

      expect(calls()[0].init.method).toBe('DELETE');
      expect(calls()[0].url).toContain('/api/collections/posts');
    });

    it('exports collections', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: [MOCK_COLLECTION] }]);
      const client = createClient(mockFetch);

      const result = await client.exportCollections();

      expect(calls()[0].url).toContain('/api/collections/export');
      expect(result).toHaveLength(1);
    });

    it('imports collections', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 204, body: undefined }]);
      const client = createClient(mockFetch);

      await client.importCollections([MOCK_COLLECTION]);

      expect(calls()[0].init.method).toBe('PUT');
      expect(calls()[0].url).toContain('/api/collections/import');
    });
  });

  // ── Records ───────────────────────────────────────────────────────────

  describe('records', () => {
    it('lists records with params', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: MOCK_LIST_RESPONSE }]);
      const client = createClient(mockFetch);

      const result = await client.listRecords('posts', {
        page: 2,
        perPage: 10,
        sort: '-created',
        filter: 'title != ""',
        search: 'hello',
      });

      const url = new URL(calls()[0].url);
      expect(url.searchParams.get('page')).toBe('2');
      expect(url.searchParams.get('perPage')).toBe('10');
      expect(url.searchParams.get('sort')).toBe('-created');
      expect(url.searchParams.get('filter')).toBe('title != ""');
      expect(url.searchParams.get('search')).toBe('hello');
      expect(result.items).toHaveLength(1);
    });

    it('gets a single record', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: MOCK_RECORD }]);
      const client = createClient(mockFetch);

      const result = await client.getRecord('posts', 'rec_001');

      expect(calls()[0].url).toContain('/api/collections/posts/records/rec_001');
      expect(result.id).toBe('rec_001');
    });

    it('gets a record with expand and fields params', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: MOCK_RECORD }]);
      const client = createClient(mockFetch);

      await client.getRecord('posts', 'rec_001', { expand: 'author', fields: 'id,title' });

      const url = new URL(calls()[0].url);
      expect(url.searchParams.get('expand')).toBe('author');
      expect(url.searchParams.get('fields')).toBe('id,title');
    });

    it('creates a record with JSON', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: MOCK_RECORD }]);
      const client = createClient(mockFetch);

      const result = await client.createRecord('posts', { title: 'Hello World' });

      expect(calls()[0].init.method).toBe('POST');
      const body = JSON.parse(calls()[0].init.body as string);
      expect(body.title).toBe('Hello World');
      expect(result.id).toBe('rec_001');
    });

    it('updates a record', async () => {
      const updated = { ...MOCK_RECORD, title: 'Updated' };
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: updated }]);
      const client = createClient(mockFetch);

      const result = await client.updateRecord('posts', 'rec_001', { title: 'Updated' });

      expect(calls()[0].init.method).toBe('PATCH');
      expect(result.title).toBe('Updated');
    });

    it('deletes a record', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 204, body: undefined }]);
      const client = createClient(mockFetch);

      await client.deleteRecord('posts', 'rec_001');

      expect(calls()[0].init.method).toBe('DELETE');
      expect(calls()[0].url).toContain('/api/collections/posts/records/rec_001');
    });

    it('counts records', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: { count: 42 } }]);
      const client = createClient(mockFetch);

      const result = await client.countRecords('posts', 'status = "published"');

      expect(calls()[0].url).toContain('/api/collections/posts/records/count');
      const url = new URL(calls()[0].url);
      expect(url.searchParams.get('filter')).toBe('status = "published"');
      expect(result.count).toBe(42);
    });

    it('skips undefined params', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: MOCK_LIST_RESPONSE }]);
      const client = createClient(mockFetch);

      await client.listRecords('posts', { page: 1 });

      const url = new URL(calls()[0].url);
      expect(url.searchParams.has('sort')).toBe(false);
      expect(url.searchParams.has('filter')).toBe(false);
    });
  });

  // ── Settings ──────────────────────────────────────────────────────────

  describe('settings', () => {
    const mockSettings: Settings = {
      meta: { appName: 'MyApp', appUrl: 'http://localhost' },
      smtp: { enabled: false, host: '' },
    };

    it('gets all settings', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: mockSettings }]);
      const client = createClient(mockFetch);

      const result = await client.getSettings();

      expect(calls()[0].url).toContain('/api/settings');
      expect(result.meta).toBeDefined();
    });

    it('updates settings', async () => {
      const updated = { ...mockSettings, meta: { appName: 'Updated' } };
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: updated }]);
      const client = createClient(mockFetch);

      const result = await client.updateSettings({ meta: { appName: 'Updated' } });

      expect(calls()[0].init.method).toBe('PATCH');
    });

    it('gets a single setting', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: { appName: 'MyApp' } }]);
      const client = createClient(mockFetch);

      await client.getSetting('meta');

      expect(calls()[0].url).toContain('/api/settings/meta');
    });

    it('resets a setting', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 204, body: undefined }]);
      const client = createClient(mockFetch);

      await client.resetSetting('smtp');

      expect(calls()[0].init.method).toBe('DELETE');
      expect(calls()[0].url).toContain('/api/settings/smtp');
    });
  });

  // ── Logs ──────────────────────────────────────────────────────────────

  describe('logs', () => {
    const mockLog: LogEntry = {
      id: 'log_001',
      created: '2025-01-01 00:00:00.000Z',
      updated: '2025-01-01 00:00:00.000Z',
      method: 'GET',
      url: '/api/health',
      status: 200,
      authId: '',
      ip: '127.0.0.1',
      referer: '',
      userAgent: 'test-agent',
    };

    it('lists logs with params', async () => {
      const listResp: ListResponse<LogEntry> = {
        page: 1, perPage: 20, totalPages: 1, totalItems: 1, items: [mockLog],
      };
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: listResp }]);
      const client = createClient(mockFetch);

      const result = await client.listLogs({ method: 'GET', page: 1, perPage: 20 });

      expect(calls()[0].url).toContain('/_/api/logs');
      const url = new URL(calls()[0].url);
      expect(url.searchParams.get('method')).toBe('GET');
      expect(result.items).toHaveLength(1);
    });

    it('gets log stats', async () => {
      const stats = [{ date: '2025-01-01', total: 100 }];
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: stats }]);
      const client = createClient(mockFetch);

      const result = await client.getLogStats({ groupBy: 'day' });

      expect(calls()[0].url).toContain('/_/api/logs/stats');
      expect(result).toHaveLength(1);
    });

    it('gets a single log', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: mockLog }]);
      const client = createClient(mockFetch);

      const result = await client.getLog('log_001');

      expect(calls()[0].url).toContain('/_/api/logs/log_001');
      expect(result.id).toBe('log_001');
    });
  });

  // ── Backups ───────────────────────────────────────────────────────────

  describe('backups', () => {
    const mockBackup: BackupEntry = {
      name: 'backup_2025-01-01.zip',
      size: 1024000,
      created: '2025-01-01 00:00:00.000Z',
    };

    it('lists backups', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: [mockBackup] }]);
      const client = createClient(mockFetch);

      const result = await client.listBackups();

      expect(calls()[0].url).toContain('/_/api/backups');
      expect(result).toHaveLength(1);
      expect(result[0].name).toBe('backup_2025-01-01.zip');
    });

    it('creates a backup', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 204, body: undefined }]);
      const client = createClient(mockFetch);

      await client.createBackup();

      expect(calls()[0].init.method).toBe('POST');
      expect(calls()[0].url).toContain('/_/api/backups');
    });

    it('deletes a backup', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 204, body: undefined }]);
      const client = createClient(mockFetch);

      await client.deleteBackup('backup_2025-01-01.zip');

      expect(calls()[0].init.method).toBe('DELETE');
      expect(calls()[0].url).toContain('/_/api/backups/backup_2025-01-01.zip');
    });

    it('restores a backup', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 204, body: undefined }]);
      const client = createClient(mockFetch);

      await client.restoreBackup('backup_2025-01-01.zip');

      expect(calls()[0].init.method).toBe('POST');
      expect(calls()[0].url).toContain('/_/api/backups/backup_2025-01-01.zip/restore');
    });
  });

  // ── Files ─────────────────────────────────────────────────────────────

  describe('files', () => {
    it('gets file token', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: { token: 'file-token' } }]);
      const client = createClient(mockFetch);

      const result = await client.getFileToken();

      expect(calls()[0].url).toContain('/api/files/token');
      expect(result.token).toBe('file-token');
    });

    it('builds file URL without token', () => {
      const { mockFetch } = createMockFetch([]);
      const client = createClient(mockFetch);

      const url = client.getFileUrl('col123', 'rec456', 'image.png');

      expect(url).toBe('http://localhost:8090/api/files/col123/rec456/image.png');
    });

    it('builds file URL with token', () => {
      const { mockFetch } = createMockFetch([]);
      const client = createClient(mockFetch);

      const url = client.getFileUrl('col123', 'rec456', 'image.png', 'file-token');

      expect(url).toContain('token=file-token');
    });
  });

  // ── Batch ─────────────────────────────────────────────────────────────

  describe('batch', () => {
    it('sends batch request', async () => {
      const batchResp = {
        responses: [
          { status: 200, body: MOCK_RECORD },
          { status: 200, body: MOCK_RECORD },
        ],
      };
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: batchResp }]);
      const client = createClient(mockFetch);

      const result = await client.batch({
        requests: [
          { method: 'POST', url: '/api/collections/posts/records', body: { title: 'A' } },
          { method: 'POST', url: '/api/collections/posts/records', body: { title: 'B' } },
        ],
      });

      expect(calls()[0].url).toContain('/api/batch');
      expect(calls()[0].init.method).toBe('POST');
      expect(result.responses).toHaveLength(2);
    });
  });

  // ── Health ────────────────────────────────────────────────────────────

  describe('health', () => {
    it('checks server health', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: { status: 'ok' } }]);
      const client = createClient(mockFetch);

      const result = await client.health();

      expect(calls()[0].url).toContain('/api/health');
      expect(result.status).toBe('ok');
    });
  });

  // ── Verification, Password Reset, Email Change ────────────────────────

  describe('verification', () => {
    it('requests verification', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 204, body: undefined }]);
      const client = createClient(mockFetch);

      await client.requestVerification('users', 'user@test.com');

      expect(calls()[0].url).toContain('/api/collections/users/request-verification');
      const body = JSON.parse(calls()[0].init.body as string);
      expect(body.email).toBe('user@test.com');
    });

    it('confirms verification', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 204, body: undefined }]);
      const client = createClient(mockFetch);

      await client.confirmVerification('users', 'verify-token');

      expect(calls()[0].url).toContain('/api/collections/users/confirm-verification');
      const body = JSON.parse(calls()[0].init.body as string);
      expect(body.token).toBe('verify-token');
    });
  });

  describe('password reset', () => {
    it('requests password reset', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 204, body: undefined }]);
      const client = createClient(mockFetch);

      await client.requestPasswordReset('users', 'user@test.com');

      expect(calls()[0].url).toContain('/api/collections/users/request-password-reset');
    });

    it('confirms password reset', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 204, body: undefined }]);
      const client = createClient(mockFetch);

      await client.confirmPasswordReset('users', {
        token: 'reset-token',
        password: 'newpass123',
        passwordConfirm: 'newpass123',
      });

      expect(calls()[0].url).toContain('/api/collections/users/confirm-password-reset');
    });
  });

  describe('email change', () => {
    it('requests email change', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 204, body: undefined }]);
      const client = createClient(mockFetch);

      await client.requestEmailChange('users', 'new@test.com');

      const body = JSON.parse(calls()[0].init.body as string);
      expect(body.newEmail).toBe('new@test.com');
    });

    it('confirms email change', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 204, body: undefined }]);
      const client = createClient(mockFetch);

      await client.confirmEmailChange('users', { token: 'change-token', password: 'pass' });

      expect(calls()[0].url).toContain('/api/collections/users/confirm-email-change');
    });
  });

  // ── OTP ───────────────────────────────────────────────────────────────

  describe('OTP', () => {
    it('requests OTP', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: { otpId: 'otp-123' } }]);
      const client = createClient(mockFetch);

      const result = await client.requestOtp('users', 'user@test.com');

      expect(calls()[0].url).toContain('/api/collections/users/request-otp');
      expect(result.otpId).toBe('otp-123');
    });

    it('authenticates with OTP', async () => {
      const authResp: AuthResponse = { token: 'otp-token', record: MOCK_AUTH_RECORD };
      const { mockFetch } = createMockFetch([{ status: 200, body: authResp }]);
      const client = createClient(mockFetch);

      const result = await client.authWithOtp('users', { otpId: 'otp-123', password: '123456' });

      expect(result.token).toBe('otp-token');
      expect(client.token).toBe('otp-token');
    });
  });

  // ── MFA ───────────────────────────────────────────────────────────────

  describe('MFA', () => {
    it('requests MFA setup', async () => {
      const resp: { secret: string; qrCodeUrl: string } = { secret: 'TOTP_SECRET', qrCodeUrl: 'otpauth://...' };
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: resp }]);
      const client = createClient(mockFetch);

      const result = await client.requestMfaSetup('users', 'rec_001');

      expect(calls()[0].url).toContain('/api/collections/users/records/rec_001/request-mfa-setup');
      expect(result.secret).toBe('TOTP_SECRET');
    });

    it('authenticates with MFA', async () => {
      const authResp: AuthResponse = { token: 'mfa-token', record: MOCK_AUTH_RECORD };
      const { mockFetch } = createMockFetch([{ status: 200, body: authResp }]);
      const client = createClient(mockFetch);

      const result = await client.authWithMfa('users', { mfaId: 'mfa-123', code: '123456' });

      expect(result.token).toBe('mfa-token');
      expect(client.token).toBe('mfa-token');
    });
  });

  // ── External Auths ────────────────────────────────────────────────────

  describe('external auths', () => {
    it('lists external auths', async () => {
      const externalAuths: ExternalAuth[] = [
        {
          id: 'ea_001', collectionId: 'col_001', recordId: 'rec_001',
          provider: 'google', providerId: 'google-id-123',
          created: '2025-01-01', updated: '2025-01-01',
        },
      ];
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: externalAuths }]);
      const client = createClient(mockFetch);

      const result = await client.listExternalAuths('users', 'rec_001');

      expect(calls()[0].url).toContain('/api/collections/users/records/rec_001/external-auths');
      expect(result).toHaveLength(1);
      expect(result[0].provider).toBe('google');
    });

    it('unlinks external auth', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 204, body: undefined }]);
      const client = createClient(mockFetch);

      await client.unlinkExternalAuth('users', 'rec_001', 'google');

      expect(calls()[0].init.method).toBe('DELETE');
      expect(calls()[0].url).toContain('/api/collections/users/records/rec_001/external-auths/google');
    });
  });

  // ── URL encoding ──────────────────────────────────────────────────────

  describe('URL encoding', () => {
    it('encodes collection names with special characters', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: MOCK_LIST_RESPONSE }]);
      const client = createClient(mockFetch);

      await client.listRecords('my collection');

      expect(calls()[0].url).toContain('/api/collections/my%20collection/records');
    });

    it('encodes record IDs with special characters', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: MOCK_RECORD }]);
      const client = createClient(mockFetch);

      await client.getRecord('posts', 'id/with/slashes');

      expect(calls()[0].url).toContain('/records/id%2Fwith%2Fslashes');
    });
  });

  // ── Content-Type header ───────────────────────────────────────────────

  describe('content-type handling', () => {
    it('sets Content-Type: application/json for JSON bodies', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: MOCK_RECORD }]);
      const client = createClient(mockFetch);

      await client.createRecord('posts', { title: 'Test' });

      const headers = calls()[0].init.headers as Record<string, string>;
      expect(headers['Content-Type']).toBe('application/json');
    });

    it('does not set Content-Type for GET requests', async () => {
      const { mockFetch, calls } = createMockFetch([{ status: 200, body: { status: 'ok' } }]);
      const client = createClient(mockFetch);

      await client.health();

      const headers = calls()[0].init.headers as Record<string, string>;
      expect(headers['Content-Type']).toBeUndefined();
    });
  });
});

// ── MemoryTokenStore tests ──────────────────────────────────────────────────

describe('MemoryTokenStore', () => {
  it('starts with null token', () => {
    const store = new MemoryTokenStore();
    expect(store.getToken()).toBeNull();
  });

  it('stores and retrieves token', () => {
    const store = new MemoryTokenStore();
    store.setToken('my-token');
    expect(store.getToken()).toBe('my-token');
  });

  it('clears token', () => {
    const store = new MemoryTokenStore();
    store.setToken('my-token');
    store.clearToken();
    expect(store.getToken()).toBeNull();
  });

  it('overwrites existing token', () => {
    const store = new MemoryTokenStore();
    store.setToken('token-1');
    store.setToken('token-2');
    expect(store.getToken()).toBe('token-2');
  });
});

// ── LocalStorageTokenStore tests ────────────────────────────────────────────

describe('LocalStorageTokenStore', () => {
  let storage: Record<string, string>;

  beforeEach(() => {
    storage = {};
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => storage[key] ?? null),
      setItem: vi.fn((key: string, value: string) => { storage[key] = value; }),
      removeItem: vi.fn((key: string) => { delete storage[key]; }),
    });
  });

  it('uses default key', () => {
    const store = new LocalStorageTokenStore();
    store.setToken('test');
    expect(localStorage.setItem).toHaveBeenCalledWith('zerobase_auth_token', 'test');
  });

  it('uses custom key', () => {
    const store = new LocalStorageTokenStore('custom_key');
    store.setToken('test');
    expect(localStorage.setItem).toHaveBeenCalledWith('custom_key', 'test');
  });

  it('gets token from localStorage', () => {
    storage['zerobase_auth_token'] = 'stored-token';
    const store = new LocalStorageTokenStore();
    expect(store.getToken()).toBe('stored-token');
  });

  it('returns null when no token stored', () => {
    const store = new LocalStorageTokenStore();
    expect(store.getToken()).toBeNull();
  });

  it('clears token from localStorage', () => {
    const store = new LocalStorageTokenStore();
    store.clearToken();
    expect(localStorage.removeItem).toHaveBeenCalledWith('zerobase_auth_token');
  });
});

// ── ApiError tests ──────────────────────────────────────────────────────────

describe('ApiError', () => {
  it('has correct name', () => {
    const err = new ApiError(400, MOCK_ERROR);
    expect(err.name).toBe('ApiError');
  });

  it('message comes from response', () => {
    const err = new ApiError(400, MOCK_ERROR);
    expect(err.message).toBe('Failed to create record.');
  });

  it('is an instance of Error', () => {
    const err = new ApiError(400, MOCK_ERROR);
    expect(err).toBeInstanceOf(Error);
  });

  it('exposes status and response', () => {
    const err = new ApiError(400, MOCK_ERROR);
    expect(err.status).toBe(400);
    expect(err.response).toBe(MOCK_ERROR);
  });

  it('correctly identifies validation errors', () => {
    expect(new ApiError(400, { ...MOCK_ERROR, code: 400 }).isValidation).toBe(true);
    expect(new ApiError(401, { ...MOCK_ERROR, code: 401 }).isValidation).toBe(false);
  });

  it('correctly identifies unauthorized errors', () => {
    expect(new ApiError(401, { ...MOCK_ERROR, code: 401 }).isUnauthorized).toBe(true);
  });

  it('correctly identifies forbidden errors', () => {
    expect(new ApiError(403, { ...MOCK_ERROR, code: 403 }).isForbidden).toBe(true);
  });

  it('correctly identifies not found errors', () => {
    expect(new ApiError(404, { ...MOCK_ERROR, code: 404 }).isNotFound).toBe(true);
  });
});
