/**
 * Type-safe API client for the Zerobase admin dashboard.
 *
 * Covers all admin endpoints: auth, collections, records, settings, logs, backups.
 * Handles JWT token storage, automatic refresh, and structured error responses.
 */

import type {
  AdminAuthResponse,
  AuthMethodsResponse,
  AuthRecord,
  AuthResponse,
  AuthWithMfaRequest,
  AuthWithOAuth2Request,
  AuthWithOtpRequest,
  AuthWithPasswordRequest,
  BackupEntry,
  BaseRecord,
  BatchRequest,
  BatchResponse,
  Collection,
  ConfirmEmailChangeRequest,
  ConfirmMfaRequest,
  ConfirmPasswordResetRequest,
  ConfirmVerificationRequest,
  CreateWebhookInput,
  ErrorResponseBody,
  ExternalAuth,
  FileTokenResponse,
  ListLogsParams,
  ListRecordsParams,
  ListResponse,
  LogEntry,
  LogStats,
  LogStatsParams,
  MfaSetupResponse,
  RequestEmailChangeRequest,
  RequestOtpRequest,
  RequestOtpResponse,
  RequestPasswordResetRequest,
  RequestVerificationRequest,
  Settings,
  TestEmailRequest,
  TestEmailResponse,
  TestWebhookResponse,
  UpdateWebhookInput,
  Webhook,
  WebhookDeliveryLog,
} from './types';

// ── Token storage abstraction ─────────────────────────────────────────────────

/** Interface for persisting auth tokens. */
export interface TokenStore {
  getToken(): string | null;
  setToken(token: string): void;
  clearToken(): void;
}

/** In-memory token store (useful for tests and SSR). */
export class MemoryTokenStore implements TokenStore {
  private token: string | null = null;

  getToken(): string | null {
    return this.token;
  }

  setToken(token: string): void {
    this.token = token;
  }

  clearToken(): void {
    this.token = null;
  }
}

/** localStorage-based token store for browser use. */
export class LocalStorageTokenStore implements TokenStore {
  constructor(private readonly key: string = 'zerobase_auth_token') {}

  getToken(): string | null {
    try {
      return localStorage.getItem(this.key);
    } catch {
      return null;
    }
  }

  setToken(token: string): void {
    try {
      localStorage.setItem(this.key, token);
    } catch {
      // localStorage may not be available (SSR, private browsing)
    }
  }

  clearToken(): void {
    try {
      localStorage.removeItem(this.key);
    } catch {
      // Ignore
    }
  }
}

// ── API Error ─────────────────────────────────────────────────────────────────

/** Structured API error with the server's error response body. */
export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly response: ErrorResponseBody,
  ) {
    super(response.message);
    this.name = 'ApiError';
  }

  /** Whether this is a validation error (400). */
  get isValidation(): boolean {
    return this.status === 400;
  }

  /** Whether this is an auth error (401). */
  get isUnauthorized(): boolean {
    return this.status === 401;
  }

  /** Whether this is a forbidden error (403). */
  get isForbidden(): boolean {
    return this.status === 403;
  }

  /** Whether this is a not-found error (404). */
  get isNotFound(): boolean {
    return this.status === 404;
  }
}

// ── Client options ────────────────────────────────────────────────────────────

export interface ZerobaseClientOptions {
  /** Base URL of the Zerobase server (e.g., "http://localhost:8090"). */
  baseUrl: string;
  /** Token storage implementation. Defaults to MemoryTokenStore. */
  tokenStore?: TokenStore;
  /**
   * Custom fetch implementation. Defaults to global `fetch`.
   * Useful for testing or server-side usage.
   */
  fetch?: typeof globalThis.fetch;
}

// ── Client ────────────────────────────────────────────────────────────────────

export class ZerobaseClient {
  private readonly baseUrl: string;
  private readonly tokenStore: TokenStore;
  private readonly _fetch: typeof globalThis.fetch;

  constructor(options: ZerobaseClientOptions) {
    // Strip trailing slash
    this.baseUrl = options.baseUrl.replace(/\/+$/, '');
    this.tokenStore = options.tokenStore ?? new MemoryTokenStore();
    this._fetch = options.fetch ?? globalThis.fetch.bind(globalThis);
  }

  // ── Low-level request method ──────────────────────────────────────────────

  /**
   * Make an authenticated HTTP request to the API.
   *
   * Automatically attaches the stored JWT token as a Bearer header.
   * Parses error responses into `ApiError` instances.
   */
  private async request<T>(
    method: string,
    path: string,
    options: {
      body?: unknown;
      params?: Record<string, string | number | boolean | undefined>;
      headers?: Record<string, string>;
      raw?: boolean;
    } = {},
  ): Promise<T> {
    const url = new URL(path, this.baseUrl);

    // Append query params
    if (options.params) {
      for (const [key, value] of Object.entries(options.params)) {
        if (value !== undefined && value !== null) {
          url.searchParams.set(key, String(value));
        }
      }
    }

    const headers: Record<string, string> = {
      ...options.headers,
    };

    // Attach auth token
    const token = this.tokenStore.getToken();
    if (token) {
      headers['Authorization'] = `Bearer ${token}`;
    }

    // Set content-type for JSON bodies
    if (options.body !== undefined && !(options.body instanceof FormData)) {
      headers['Content-Type'] = 'application/json';
    }

    const response = await this._fetch(url.toString(), {
      method,
      headers,
      body:
        options.body !== undefined
          ? options.body instanceof FormData
            ? options.body
            : JSON.stringify(options.body)
          : undefined,
    });

    // Handle non-JSON responses (e.g., file downloads)
    if (options.raw) {
      return response as unknown as T;
    }

    // No content responses
    if (response.status === 204) {
      return undefined as T;
    }

    // Parse response
    const data = await response.json();

    if (!response.ok) {
      throw new ApiError(response.status, data as ErrorResponseBody);
    }

    return data as T;
  }

  // ── Token management ──────────────────────────────────────────────────────

  /** Get the current auth token. */
  get token(): string | null {
    return this.tokenStore.getToken();
  }

  /** Check if the client has a stored token. */
  get isAuthenticated(): boolean {
    return this.tokenStore.getToken() !== null;
  }

  /** Clear the stored auth token (logout). */
  logout(): void {
    this.tokenStore.clearToken();
  }

  // ── Admin Auth ────────────────────────────────────────────────────────────

  /** Authenticate as a superuser with email/password. */
  async adminAuthWithPassword(identity: string, password: string): Promise<AdminAuthResponse> {
    const body: AuthWithPasswordRequest = { identity, password };
    const result = await this.request<AdminAuthResponse>('POST', '/_/api/admins/auth-with-password', { body });
    this.tokenStore.setToken(result.token);
    return result;
  }

  // ── Collection Auth ───────────────────────────────────────────────────────

  /** Authenticate with email/password against an auth collection. */
  async authWithPassword(collection: string, identity: string, password: string): Promise<AuthResponse> {
    const body: AuthWithPasswordRequest = { identity, password };
    const result = await this.request<AuthResponse>('POST', `/api/collections/${encodeURIComponent(collection)}/auth-with-password`, { body });
    this.tokenStore.setToken(result.token);
    return result;
  }

  /** Refresh the current auth token. */
  async authRefresh(collection: string): Promise<AuthResponse> {
    const result = await this.request<AuthResponse>('POST', `/api/collections/${encodeURIComponent(collection)}/auth-refresh`);
    this.tokenStore.setToken(result.token);
    return result;
  }

  /** List available auth methods for a collection. */
  async authMethods(collection: string): Promise<AuthMethodsResponse> {
    return this.request('GET', `/api/collections/${encodeURIComponent(collection)}/auth-methods`);
  }

  /** Authenticate with OAuth2. */
  async authWithOAuth2(collection: string, data: AuthWithOAuth2Request): Promise<AuthResponse> {
    const result = await this.request<AuthResponse>('POST', `/api/collections/${encodeURIComponent(collection)}/auth-with-oauth2`, { body: data });
    this.tokenStore.setToken(result.token);
    return result;
  }

  // ── OTP ───────────────────────────────────────────────────────────────────

  /** Request an OTP code sent via email. */
  async requestOtp(collection: string, email: string): Promise<RequestOtpResponse> {
    const body: RequestOtpRequest = { email };
    return this.request('POST', `/api/collections/${encodeURIComponent(collection)}/request-otp`, { body });
  }

  /** Authenticate with an OTP code. */
  async authWithOtp(collection: string, data: AuthWithOtpRequest): Promise<AuthResponse> {
    const result = await this.request<AuthResponse>('POST', `/api/collections/${encodeURIComponent(collection)}/auth-with-otp`, { body: data });
    this.tokenStore.setToken(result.token);
    return result;
  }

  // ── MFA ───────────────────────────────────────────────────────────────────

  /** Begin MFA setup for a record. */
  async requestMfaSetup(collection: string, recordId: string): Promise<MfaSetupResponse> {
    return this.request('POST', `/api/collections/${encodeURIComponent(collection)}/records/${encodeURIComponent(recordId)}/request-mfa-setup`);
  }

  /** Confirm MFA setup with a TOTP code. */
  async confirmMfa(collection: string, recordId: string, code: string): Promise<void> {
    const body: ConfirmMfaRequest = { code };
    return this.request('POST', `/api/collections/${encodeURIComponent(collection)}/records/${encodeURIComponent(recordId)}/confirm-mfa`, { body });
  }

  /** Authenticate with MFA (TOTP or recovery code). */
  async authWithMfa(collection: string, data: AuthWithMfaRequest): Promise<AuthResponse> {
    const result = await this.request<AuthResponse>('POST', `/api/collections/${encodeURIComponent(collection)}/auth-with-mfa`, { body: data });
    this.tokenStore.setToken(result.token);
    return result;
  }

  // ── Passkeys ──────────────────────────────────────────────────────────────

  /** Begin passkey registration. */
  async requestPasskeyRegister(collection: string): Promise<unknown> {
    return this.request('POST', `/api/collections/${encodeURIComponent(collection)}/request-passkey-register`);
  }

  /** Complete passkey registration. */
  async confirmPasskeyRegister(collection: string, data: unknown): Promise<void> {
    return this.request('POST', `/api/collections/${encodeURIComponent(collection)}/confirm-passkey-register`, { body: data });
  }

  /** Begin passkey authentication. */
  async authWithPasskeyBegin(collection: string): Promise<unknown> {
    return this.request('POST', `/api/collections/${encodeURIComponent(collection)}/auth-with-passkey-begin`);
  }

  /** Complete passkey authentication. */
  async authWithPasskeyFinish(collection: string, data: unknown): Promise<AuthResponse> {
    const result = await this.request<AuthResponse>('POST', `/api/collections/${encodeURIComponent(collection)}/auth-with-passkey-finish`, { body: data });
    this.tokenStore.setToken(result.token);
    return result;
  }

  // ── Verification ──────────────────────────────────────────────────────────

  /** Request email verification. */
  async requestVerification(collection: string, email: string): Promise<void> {
    const body: RequestVerificationRequest = { email };
    return this.request('POST', `/api/collections/${encodeURIComponent(collection)}/request-verification`, { body });
  }

  /** Confirm email verification. */
  async confirmVerification(collection: string, token: string): Promise<void> {
    const body: ConfirmVerificationRequest = { token };
    return this.request('POST', `/api/collections/${encodeURIComponent(collection)}/confirm-verification`, { body });
  }

  // ── Password Reset ────────────────────────────────────────────────────────

  /** Request a password reset email. */
  async requestPasswordReset(collection: string, email: string): Promise<void> {
    const body: RequestPasswordResetRequest = { email };
    return this.request('POST', `/api/collections/${encodeURIComponent(collection)}/request-password-reset`, { body });
  }

  /** Confirm password reset with a token and new password. */
  async confirmPasswordReset(collection: string, data: ConfirmPasswordResetRequest): Promise<void> {
    return this.request('POST', `/api/collections/${encodeURIComponent(collection)}/confirm-password-reset`, { body: data });
  }

  // ── Email Change ──────────────────────────────────────────────────────────

  /** Request an email change. */
  async requestEmailChange(collection: string, newEmail: string): Promise<void> {
    const body: RequestEmailChangeRequest = { newEmail };
    return this.request('POST', `/api/collections/${encodeURIComponent(collection)}/request-email-change`, { body });
  }

  /** Confirm email change. */
  async confirmEmailChange(collection: string, data: ConfirmEmailChangeRequest): Promise<void> {
    return this.request('POST', `/api/collections/${encodeURIComponent(collection)}/confirm-email-change`, { body: data });
  }

  // ── External Auths ────────────────────────────────────────────────────────

  /** List external auth identities for a record. */
  async listExternalAuths(collection: string, recordId: string): Promise<ExternalAuth[]> {
    return this.request('GET', `/api/collections/${encodeURIComponent(collection)}/records/${encodeURIComponent(recordId)}/external-auths`);
  }

  /** Unlink an external auth provider from a record. */
  async unlinkExternalAuth(collection: string, recordId: string, provider: string): Promise<void> {
    return this.request('DELETE', `/api/collections/${encodeURIComponent(collection)}/records/${encodeURIComponent(recordId)}/external-auths/${encodeURIComponent(provider)}`);
  }

  // ── Collections (Superuser) ───────────────────────────────────────────────

  /** List all collections. */
  async listCollections(): Promise<ListResponse<Collection>> {
    return this.request('GET', '/api/collections');
  }

  /** Create a new collection. */
  async createCollection(collection: Partial<Collection> & { name: string; type: string }): Promise<Collection> {
    return this.request('POST', '/api/collections', { body: collection });
  }

  /** Get a single collection by ID or name. */
  async getCollection(idOrName: string): Promise<Collection> {
    return this.request('GET', `/api/collections/${encodeURIComponent(idOrName)}`);
  }

  /** Update a collection. */
  async updateCollection(idOrName: string, data: Partial<Collection>): Promise<Collection> {
    return this.request('PATCH', `/api/collections/${encodeURIComponent(idOrName)}`, { body: data });
  }

  /** Delete a collection. */
  async deleteCollection(idOrName: string): Promise<void> {
    return this.request('DELETE', `/api/collections/${encodeURIComponent(idOrName)}`);
  }

  /** Export all collection schemas. */
  async exportCollections(): Promise<Collection[]> {
    return this.request('GET', '/api/collections/export');
  }

  /** Import collection schemas (replaces all). */
  async importCollections(collections: Collection[]): Promise<void> {
    return this.request('PUT', '/api/collections/import', { body: collections });
  }

  /** List indexes for a collection. */
  async listIndexes(idOrName: string): Promise<unknown[]> {
    return this.request('GET', `/api/collections/${encodeURIComponent(idOrName)}/indexes`);
  }

  /** Add an index to a collection. */
  async addIndex(idOrName: string, index: unknown): Promise<void> {
    return this.request('POST', `/api/collections/${encodeURIComponent(idOrName)}/indexes`, { body: index });
  }

  /** Remove an index from a collection. */
  async removeIndex(idOrName: string, position: number): Promise<void> {
    return this.request('DELETE', `/api/collections/${encodeURIComponent(idOrName)}/indexes/${position}`);
  }

  // ── Records ───────────────────────────────────────────────────────────────

  /** List records in a collection with optional filtering, sorting, pagination. */
  async listRecords<T extends BaseRecord = BaseRecord>(
    collection: string,
    params?: ListRecordsParams,
  ): Promise<ListResponse<T>> {
    return this.request('GET', `/api/collections/${encodeURIComponent(collection)}/records`, {
      params: params as Record<string, string | number | boolean | undefined>,
    });
  }

  /** Get a single record by ID. */
  async getRecord<T extends BaseRecord = BaseRecord>(
    collection: string,
    id: string,
    params?: Pick<ListRecordsParams, 'fields' | 'expand'>,
  ): Promise<T> {
    return this.request('GET', `/api/collections/${encodeURIComponent(collection)}/records/${encodeURIComponent(id)}`, {
      params: params as Record<string, string | number | boolean | undefined>,
    });
  }

  /** Create a new record. Accepts JSON data or FormData for file uploads. */
  async createRecord<T extends BaseRecord = BaseRecord>(
    collection: string,
    data: Record<string, unknown> | FormData,
  ): Promise<T> {
    return this.request('POST', `/api/collections/${encodeURIComponent(collection)}/records`, { body: data });
  }

  /** Update an existing record. Accepts JSON data or FormData for file uploads. */
  async updateRecord<T extends BaseRecord = BaseRecord>(
    collection: string,
    id: string,
    data: Record<string, unknown> | FormData,
  ): Promise<T> {
    return this.request('PATCH', `/api/collections/${encodeURIComponent(collection)}/records/${encodeURIComponent(id)}`, { body: data });
  }

  /** Delete a record. */
  async deleteRecord(collection: string, id: string): Promise<void> {
    return this.request('DELETE', `/api/collections/${encodeURIComponent(collection)}/records/${encodeURIComponent(id)}`);
  }

  /** Count records in a collection with optional filter. */
  async countRecords(collection: string, filter?: string): Promise<{ count: number }> {
    return this.request('GET', `/api/collections/${encodeURIComponent(collection)}/records/count`, {
      params: filter ? { filter } : undefined,
    });
  }

  // ── Settings (Superuser) ──────────────────────────────────────────────────

  /** Get all server settings. */
  async getSettings(): Promise<Settings> {
    return this.request('GET', '/api/settings');
  }

  /** Update settings (partial merge). */
  async updateSettings(data: Partial<Settings>): Promise<Settings> {
    return this.request('PATCH', '/api/settings', { body: data });
  }

  /** Get a single setting by key. */
  async getSetting(key: string): Promise<Record<string, unknown>> {
    return this.request('GET', `/api/settings/${encodeURIComponent(key)}`);
  }

  /** Reset a single setting to its default value. */
  async resetSetting(key: string): Promise<void> {
    return this.request('DELETE', `/api/settings/${encodeURIComponent(key)}`);
  }

  /** Send a test email using the current SMTP configuration. */
  async testEmail(to: string): Promise<TestEmailResponse> {
    const body: TestEmailRequest = { to };
    return this.request('POST', '/api/settings/test-email', { body });
  }

  // ── Logs (Superuser) ──────────────────────────────────────────────────────

  /** List request logs with filtering and pagination. */
  async listLogs(params?: ListLogsParams): Promise<ListResponse<LogEntry>> {
    return this.request('GET', '/_/api/logs', {
      params: params as Record<string, string | number | boolean | undefined>,
    });
  }

  /** Get aggregated log statistics. */
  async getLogStats(params?: LogStatsParams): Promise<LogStats> {
    return this.request('GET', '/_/api/logs/stats', {
      params: params as Record<string, string | number | boolean | undefined>,
    });
  }

  /** Get a single log entry by ID. */
  async getLog(id: string): Promise<LogEntry> {
    return this.request('GET', `/_/api/logs/${encodeURIComponent(id)}`);
  }

  // ── Backups (Superuser) ───────────────────────────────────────────────────

  /** List all backups. */
  async listBackups(): Promise<BackupEntry[]> {
    return this.request('GET', '/_/api/backups');
  }

  /** Create a new backup. */
  async createBackup(): Promise<void> {
    return this.request('POST', '/_/api/backups');
  }

  /** Download a backup file. Returns the raw Response object. */
  async downloadBackup(name: string): Promise<Response> {
    return this.request('GET', `/_/api/backups/${encodeURIComponent(name)}`, { raw: true });
  }

  /** Delete a backup. */
  async deleteBackup(name: string): Promise<void> {
    return this.request('DELETE', `/_/api/backups/${encodeURIComponent(name)}`);
  }

  /** Restore from a backup. */
  async restoreBackup(name: string): Promise<void> {
    return this.request('POST', `/_/api/backups/${encodeURIComponent(name)}/restore`);
  }

  // ── Files ─────────────────────────────────────────────────────────────────

  /** Generate a short-lived file access token. */
  async getFileToken(): Promise<FileTokenResponse> {
    return this.request('GET', '/api/files/token');
  }

  /**
   * Build the URL for a file attached to a record.
   *
   * For protected files, first call `getFileToken()` and pass the token
   * as a query parameter.
   */
  getFileUrl(collectionId: string, recordId: string, filename: string, fileToken?: string): string {
    const url = new URL(`/api/files/${encodeURIComponent(collectionId)}/${encodeURIComponent(recordId)}/${encodeURIComponent(filename)}`, this.baseUrl);
    if (fileToken) {
      url.searchParams.set('token', fileToken);
    }
    return url.toString();
  }

  // ── Batch ─────────────────────────────────────────────────────────────────

  /** Execute multiple record operations atomically. */
  async batch(requests: BatchRequest): Promise<BatchResponse> {
    return this.request('POST', '/api/batch', { body: requests });
  }

  // ── Webhooks (Superuser) ─────────────────────────────────────────────

  /** List all webhooks, optionally filtered by collection. */
  async listWebhooks(collection?: string): Promise<Webhook[]> {
    return this.request('GET', '/_/api/webhooks', {
      params: collection ? { collection } : undefined,
    });
  }

  /** Get a single webhook by ID. */
  async getWebhook(id: string): Promise<Webhook> {
    return this.request('GET', `/_/api/webhooks/${encodeURIComponent(id)}`);
  }

  /** Create a new webhook. */
  async createWebhook(data: CreateWebhookInput): Promise<Webhook> {
    return this.request('POST', '/_/api/webhooks', { body: data });
  }

  /** Update an existing webhook. */
  async updateWebhook(id: string, data: UpdateWebhookInput): Promise<Webhook> {
    return this.request('PATCH', `/_/api/webhooks/${encodeURIComponent(id)}`, { body: data });
  }

  /** Delete a webhook. */
  async deleteWebhook(id: string): Promise<void> {
    return this.request('DELETE', `/_/api/webhooks/${encodeURIComponent(id)}`);
  }

  /** List delivery logs for a webhook. */
  async listWebhookDeliveries(
    webhookId: string,
    params?: { page?: number; perPage?: number },
  ): Promise<ListResponse<WebhookDeliveryLog>> {
    return this.request('GET', `/_/api/webhooks/${encodeURIComponent(webhookId)}/deliveries`, {
      params: params as Record<string, string | number | boolean | undefined>,
    });
  }

  /** Send a test delivery to a webhook URL. */
  async testWebhook(id: string): Promise<TestWebhookResponse> {
    return this.request('POST', `/_/api/webhooks/${encodeURIComponent(id)}/test`);
  }

  // ── Health ────────────────────────────────────────────────────────────────

  /** Check server health. */
  async health(): Promise<{ status: string }> {
    return this.request('GET', '/api/health');
  }
}
