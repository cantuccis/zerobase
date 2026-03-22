/**
 * Singleton ZerobaseClient instance for the admin dashboard.
 *
 * In the browser the API base URL is the same origin that served the page.
 * The token is persisted to localStorage so it survives page reloads.
 */

import { ZerobaseClient, LocalStorageTokenStore, MemoryTokenStore } from '../api';

const isBrowser = typeof window !== 'undefined';

/** Shared client instance used across the admin dashboard. */
export const client = new ZerobaseClient({
  // The admin SPA is served from the same origin as the API, so an empty
  // base URL makes requests relative to the current host.
  baseUrl: isBrowser ? window.location.origin : 'http://localhost:8090',
  tokenStore: isBrowser ? new LocalStorageTokenStore('zerobase_admin_token') : new MemoryTokenStore(),
});
