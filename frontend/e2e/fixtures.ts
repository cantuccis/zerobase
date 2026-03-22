/**
 * Shared Playwright fixtures and helpers for Zerobase E2E tests.
 *
 * Provides a pre-authenticated `adminPage` and API helper methods
 * for programmatic setup/teardown of test data.
 */
import { test as base, expect, type Page, type APIRequestContext } from '@playwright/test';

// ── Types ────────────────────────────────────────────────────────────────────

export interface AdminFixtures {
  /** A page that is already authenticated as a superuser. */
  adminPage: Page;
  /** Helper to call the Zerobase API directly (for setup/teardown). */
  api: ApiHelper;
}

// ── API Helper ───────────────────────────────────────────────────────────────

export class ApiHelper {
  constructor(
    private request: APIRequestContext,
    private baseURL: string,
  ) {}

  private get headers() {
    return { 'Content-Type': 'application/json' };
  }

  /** Authenticate as superuser and return the JWT token. */
  async getAdminToken(): Promise<string> {
    const email = process.env.ADMIN_EMAIL ?? 'admin@test.com';
    const password = process.env.ADMIN_PASSWORD ?? 'admin12345678';

    const resp = await this.request.post(`${this.baseURL}/_/api/admins/auth-with-password`, {
      data: { identity: email, password },
      headers: this.headers,
    });
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    return body.token;
  }

  /** Create a collection via the API. Returns the created collection. */
  async createCollection(
    token: string,
    data: { name: string; type: string; fields?: unknown[] },
  ) {
    const resp = await this.request.post(`${this.baseURL}/api/collections`, {
      data: { name: data.name, type: data.type, fields: data.fields ?? [] },
      headers: {
        ...this.headers,
        Authorization: `Bearer ${token}`,
      },
    });
    expect(resp.ok(), `Failed to create collection ${data.name}: ${resp.status()}`).toBeTruthy();
    return resp.json();
  }

  /** Delete a collection by ID or name. Ignores 404. */
  async deleteCollection(token: string, idOrName: string) {
    const resp = await this.request.delete(
      `${this.baseURL}/api/collections/${encodeURIComponent(idOrName)}`,
      {
        headers: { Authorization: `Bearer ${token}` },
      },
    );
    // 204 = deleted, 404 = already gone — both are fine
    if (resp.status() !== 204 && resp.status() !== 404) {
      throw new Error(`Failed to delete collection ${idOrName}: ${resp.status()}`);
    }
  }

  /** Create a record in a collection. Returns the created record. */
  async createRecord(
    token: string,
    collection: string,
    data: Record<string, unknown>,
  ) {
    const resp = await this.request.post(
      `${this.baseURL}/api/collections/${encodeURIComponent(collection)}/records`,
      {
        data,
        headers: {
          ...this.headers,
          Authorization: `Bearer ${token}`,
        },
      },
    );
    expect(resp.ok(), `Failed to create record: ${resp.status()}`).toBeTruthy();
    return resp.json();
  }

  /** Delete a record. Ignores 404. */
  async deleteRecord(token: string, collection: string, id: string) {
    const resp = await this.request.delete(
      `${this.baseURL}/api/collections/${encodeURIComponent(collection)}/records/${encodeURIComponent(id)}`,
      {
        headers: { Authorization: `Bearer ${token}` },
      },
    );
    if (resp.status() !== 204 && resp.status() !== 404) {
      throw new Error(`Failed to delete record ${id}: ${resp.status()}`);
    }
  }

  /** List collections. */
  async listCollections(token: string) {
    const resp = await this.request.get(`${this.baseURL}/api/collections`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(resp.ok()).toBeTruthy();
    return resp.json();
  }
}

// ── Extended test fixture ────────────────────────────────────────────────────

export const test = base.extend<AdminFixtures>({
  adminPage: async ({ page }, use) => {
    // The storage state is already loaded via the project config
    await use(page);
  },

  api: async ({ request }, use) => {
    const baseURL = process.env.ZEROBASE_E2E_BASE_URL ?? 'http://localhost:8090';
    await use(new ApiHelper(request, baseURL));
  },
});

export { expect } from '@playwright/test';
