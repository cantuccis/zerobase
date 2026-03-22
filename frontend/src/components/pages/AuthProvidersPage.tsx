import { useState, useEffect, useCallback } from 'react';
import { DashboardLayout } from '../DashboardLayout';
import { client } from '../../lib/auth/client';
import { ApiError } from '../../lib/api';
import type { OAuth2ProviderSettings } from '../../lib/api/types';

// ── Known providers ─────────────────────────────────────────────────────────

interface ProviderMeta {
  key: string;
  displayName: string;
  /** Base redirect URL path for this provider. */
  redirectPath: string;
}

const KNOWN_PROVIDERS: ProviderMeta[] = [
  { key: 'google', displayName: 'Google', redirectPath: '/api/oauth2/redirect' },
  { key: 'microsoft', displayName: 'Microsoft', redirectPath: '/api/oauth2/redirect' },
];

const DEFAULT_PROVIDER: OAuth2ProviderSettings = {
  enabled: false,
  clientId: '',
  clientSecret: '',
  displayName: '',
};

// ── Component ───────────────────────────────────────────────────────────────

export function AuthProvidersPage() {
  const [providers, setProviders] = useState<Record<string, OAuth2ProviderSettings>>({});
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  // ── Load settings ──────────────────────────────────────────────────────

  const loadSettings = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const settings = await client.getSettings();
      const auth = (settings.auth ?? {}) as Record<string, unknown>;
      const oauth2Providers = (auth.oauth2Providers ?? {}) as Record<string, Partial<OAuth2ProviderSettings>>;

      const loaded: Record<string, OAuth2ProviderSettings> = {};
      for (const meta of KNOWN_PROVIDERS) {
        const stored = oauth2Providers[meta.key] ?? {};
        loaded[meta.key] = {
          enabled: stored.enabled ?? DEFAULT_PROVIDER.enabled,
          clientId: stored.clientId ?? DEFAULT_PROVIDER.clientId,
          clientSecret: '', // Write-only
          displayName: stored.displayName || meta.displayName,
        };
      }
      setProviders(loaded);
    } catch (err) {
      if (err instanceof ApiError) {
        setError(err.response.message || 'Failed to load auth provider settings.');
      } else {
        setError('Unable to connect to the server.');
      }
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  // ── Helpers ────────────────────────────────────────────────────────────

  function updateProvider(key: string, field: keyof OAuth2ProviderSettings, value: OAuth2ProviderSettings[keyof OAuth2ProviderSettings]) {
    setProviders((prev) => ({
      ...prev,
      [key]: { ...prev[key], [field]: value },
    }));
    const errorKey = `${key}.${field}`;
    if (fieldErrors[errorKey]) {
      setFieldErrors((prev) => {
        const next = { ...prev };
        delete next[errorKey];
        return next;
      });
    }
  }

  function getRedirectUrl(providerKey: string): string {
    const origin = typeof window !== 'undefined' ? window.location.origin : '';
    return `${origin}/api/oauth2/redirect/${providerKey}`;
  }

  // ── Validate ───────────────────────────────────────────────────────────

  function validate(): boolean {
    const errors: Record<string, string> = {};

    for (const meta of KNOWN_PROVIDERS) {
      const p = providers[meta.key];
      if (p?.enabled) {
        if (!p.clientId.trim()) {
          errors[`${meta.key}.clientId`] = 'Client ID is required when provider is enabled.';
        }
      }
    }

    setFieldErrors(errors);
    return Object.keys(errors).length === 0;
  }

  // ── Save ───────────────────────────────────────────────────────────────

  async function handleSave(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setSuccess(null);

    if (!validate()) return;

    setSaving(true);
    try {
      const oauth2Providers: Record<string, Record<string, unknown>> = {};

      for (const meta of KNOWN_PROVIDERS) {
        const p = providers[meta.key];
        if (!p) continue;

        const update: Record<string, unknown> = {
          enabled: p.enabled,
          clientId: p.clientId.trim(),
          displayName: p.displayName.trim() || meta.displayName,
        };
        // Only send clientSecret if non-empty (write-only field)
        if (p.clientSecret) {
          update.clientSecret = p.clientSecret;
        }

        oauth2Providers[meta.key] = update;
      }

      await client.updateSettings({
        auth: { oauth2Providers },
      });

      setSuccess('Auth provider settings saved successfully.');
      // Clear secrets after save (write-only)
      setProviders((prev) => {
        const next = { ...prev };
        for (const key of Object.keys(next)) {
          next[key] = { ...next[key], clientSecret: '' };
        }
        return next;
      });
    } catch (err) {
      if (err instanceof ApiError) {
        setError(err.response.message || 'Failed to save auth provider settings.');
      } else {
        setError('Unable to connect to the server.');
      }
    } finally {
      setSaving(false);
    }
  }

  // ── Render ─────────────────────────────────────────────────────────────

  if (loading) {
    return (
      <DashboardLayout currentPath="/_/settings/auth-providers" pageTitle="Auth Providers">
        <div className="flex items-center justify-center py-12">
          <svg className="h-6 w-6 animate-spin text-gray-400 dark:text-gray-500" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
            <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
          </svg>
          <span className="ml-2 text-sm text-gray-500 dark:text-gray-400">Loading auth providers...</span>
        </div>
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout currentPath="/_/settings/auth-providers" pageTitle="Auth Providers">
      <div className="mx-auto max-w-2xl space-y-8">
        <form onSubmit={handleSave} noValidate>
          {/* Global messages */}
          {error && (
            <div role="alert" className="mb-6 rounded-md border border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30 px-4 py-3 text-sm text-red-700 dark:text-red-400">
              {error}
            </div>
          )}
          {success && (
            <div role="status" className="mb-6 rounded-md border border-green-200 dark:border-green-800 bg-green-50 dark:bg-green-900/30 px-4 py-3 text-sm text-green-700 dark:text-green-400">
              {success}
            </div>
          )}

          {/* Provider cards */}
          {KNOWN_PROVIDERS.map((meta) => {
            const p = providers[meta.key];
            if (!p) return null;

            const statusBadge = providerStatus(p);

            return (
              <div key={meta.key} className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 shadow-sm" data-testid={`provider-${meta.key}`}>
                {/* Header */}
                <div className="flex items-center justify-between border-b border-gray-200 dark:border-gray-700 px-6 py-4">
                  <div className="flex items-center gap-3">
                    <ProviderIcon name={meta.key} />
                    <div>
                      <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">{meta.displayName}</h3>
                      <p className="text-sm text-gray-500 dark:text-gray-400">OAuth2 authentication provider</p>
                    </div>
                  </div>
                  <span
                    className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${statusBadge.color}`}
                    data-testid={`${meta.key}-status`}
                  >
                    {statusBadge.label}
                  </span>
                </div>

                {/* Body */}
                <div className="space-y-5 px-6 py-5">
                  {/* Enable toggle */}
                  <div className="flex items-center justify-between">
                    <div>
                      <label htmlFor={`${meta.key}-enabled`} className="text-sm font-medium text-gray-700 dark:text-gray-300">
                        Enable {meta.displayName}
                      </label>
                      <p className="text-xs text-gray-500 dark:text-gray-400">Allow users to sign in with {meta.displayName}.</p>
                    </div>
                    <button
                      id={`${meta.key}-enabled`}
                      type="button"
                      role="switch"
                      aria-checked={p.enabled}
                      onClick={() => updateProvider(meta.key, 'enabled', !p.enabled)}
                      className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors
                        focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2
                        ${p.enabled ? 'bg-blue-600' : 'bg-gray-200 dark:bg-gray-600'}`}
                    >
                      <span
                        aria-hidden="true"
                        className={`pointer-events-none inline-block h-5 w-5 rounded-full bg-white dark:bg-gray-800 shadow ring-0 transition-transform
                          ${p.enabled ? 'translate-x-5' : 'translate-x-0'}`}
                      />
                    </button>
                  </div>

                  {/* Fields — only show when enabled */}
                  {p.enabled && (
                    <>
                      {/* Client ID */}
                      <div className="space-y-1.5">
                        <label htmlFor={`${meta.key}-client-id`} className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                          Client ID <span className="text-red-500 dark:text-red-400">*</span>
                        </label>
                        <input
                          id={`${meta.key}-client-id`}
                          name={`${meta.key}-clientId`}
                          type="text"
                          autoComplete="off"
                          value={p.clientId}
                          onChange={(e) => updateProvider(meta.key, 'clientId', e.target.value)}
                          disabled={saving}
                          aria-invalid={!!fieldErrors[`${meta.key}.clientId`]}
                          aria-describedby={fieldErrors[`${meta.key}.clientId`] ? `${meta.key}-client-id-error` : undefined}
                          className={`block w-full rounded-md border px-3 py-2 text-sm shadow-sm transition-colors
                            focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
                            disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700
                            ${fieldErrors[`${meta.key}.clientId`] ? 'border-red-400 dark:border-red-700' : 'border-gray-300 dark:border-gray-600'}`}
                          placeholder={`${meta.displayName} OAuth2 client ID`}
                        />
                        {fieldErrors[`${meta.key}.clientId`] && (
                          <p id={`${meta.key}-client-id-error`} className="text-xs text-red-600 dark:text-red-400">
                            {fieldErrors[`${meta.key}.clientId`]}
                          </p>
                        )}
                      </div>

                      {/* Client Secret */}
                      <div className="space-y-1.5">
                        <label htmlFor={`${meta.key}-client-secret`} className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                          Client Secret
                        </label>
                        <input
                          id={`${meta.key}-client-secret`}
                          name={`${meta.key}-clientSecret`}
                          type="password"
                          autoComplete="off"
                          value={p.clientSecret}
                          onChange={(e) => updateProvider(meta.key, 'clientSecret', e.target.value)}
                          disabled={saving}
                          className="block w-full rounded-md border border-gray-300 dark:border-gray-600 px-3 py-2 text-sm shadow-sm transition-colors
                            focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
                            disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700"
                          placeholder="Leave blank to keep existing secret"
                        />
                        <p className="text-xs text-gray-500 dark:text-gray-400">
                          Write-only. Leave blank to keep the current secret.
                        </p>
                      </div>

                      {/* Redirect URL (read-only) */}
                      <div className="space-y-1.5">
                        <label htmlFor={`${meta.key}-redirect-url`} className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                          Redirect URL
                        </label>
                        <div className="flex gap-2">
                          <input
                            id={`${meta.key}-redirect-url`}
                            type="text"
                            readOnly
                            value={getRedirectUrl(meta.key)}
                            className="block w-full rounded-md border border-gray-300 dark:border-gray-600 bg-gray-50 dark:bg-gray-900 px-3 py-2 text-sm text-gray-600 dark:text-gray-400 shadow-sm"
                            data-testid={`${meta.key}-redirect-url`}
                          />
                          <button
                            type="button"
                            onClick={() => {
                              navigator.clipboard.writeText(getRedirectUrl(meta.key));
                            }}
                            className="shrink-0 rounded-md border border-gray-300 dark:border-gray-600 px-3 py-2 text-sm font-medium text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
                            aria-label={`Copy ${meta.displayName} redirect URL`}
                          >
                            <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                              <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
                              <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                            </svg>
                          </button>
                        </div>
                        <p className="text-xs text-gray-500 dark:text-gray-400">
                          Add this URL to your {meta.displayName} OAuth2 app's authorized redirect URIs.
                        </p>
                      </div>
                    </>
                  )}
                </div>
              </div>
            );
          })}

          {/* Save button */}
          <div className="flex justify-end pt-2">
            <button
              type="submit"
              disabled={saving}
              className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-blue-700 dark:hover:bg-blue-600 transition-colors
                focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2
                disabled:cursor-not-allowed disabled:bg-blue-400"
            >
              {saving ? 'Saving...' : 'Save Providers'}
            </button>
          </div>
        </form>
      </div>
    </DashboardLayout>
  );
}

// ── Provider status badge ────────────────────────────────────────────────────

function providerStatus(p: OAuth2ProviderSettings): { label: string; color: string } {
  if (!p.enabled) {
    return { label: 'Disabled', color: 'bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400' };
  }
  if (!p.clientId.trim()) {
    return { label: 'Not configured', color: 'bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400' };
  }
  return { label: 'Enabled', color: 'bg-green-100 dark:bg-green-900/20 text-green-700 dark:text-green-400' };
}

// ── Provider icons ───────────────────────────────────────────────────────────

function ProviderIcon({ name }: { name: string }) {
  switch (name) {
    case 'google':
      return (
        <svg className="h-8 w-8" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 0 1-2.2 3.32v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.1z" fill="#4285F4" />
          <path d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z" fill="#34A853" />
          <path d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z" fill="#FBBC05" />
          <path d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z" fill="#EA4335" />
        </svg>
      );
    case 'microsoft':
      return (
        <svg className="h-8 w-8" viewBox="0 0 24 24" aria-hidden="true">
          <rect x="1" y="1" width="10" height="10" fill="#F25022" />
          <rect x="13" y="1" width="10" height="10" fill="#7FBA00" />
          <rect x="1" y="13" width="10" height="10" fill="#00A4EF" />
          <rect x="13" y="13" width="10" height="10" fill="#FFB900" />
        </svg>
      );
    default:
      return (
        <svg className="h-8 w-8 text-gray-400 dark:text-gray-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <circle cx="12" cy="12" r="10" />
          <path d="M12 16v-4M12 8h.01" />
        </svg>
      );
  }
}
