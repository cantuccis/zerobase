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

// ── Reusable sub-components ─────────────────────────────────────────────────

function MonolithInput({
  id,
  name,
  type = 'text',
  value,
  onChange,
  placeholder,
  disabled,
  error,
  errorId,
  autoComplete,
  readOnly,
  required,
}: {
  id: string;
  name: string;
  type?: string;
  value: string;
  onChange?: (e: React.ChangeEvent<HTMLInputElement>) => void;
  placeholder?: string;
  disabled?: boolean;
  error?: string;
  errorId?: string;
  autoComplete?: string;
  readOnly?: boolean;
  required?: boolean;
}) {
  return (
    <>
      <input
        id={id}
        name={name}
        type={type}
        value={value}
        onChange={onChange}
        placeholder={placeholder}
        disabled={disabled}
        readOnly={readOnly}
        autoComplete={autoComplete}
        aria-invalid={!!error}
        aria-describedby={error ? errorId : undefined}
        aria-required={required}
        className={`mono-input w-full border bg-surface text-on-surface px-4 py-3 text-sm outline-none
          focus:border-2 focus:border-on-surface focus:px-[15px] focus:py-[11px]
          disabled:cursor-not-allowed disabled:opacity-50
          placeholder:text-outline
          ${readOnly ? 'bg-surface-container text-secondary cursor-default' : ''}
          ${error
            ? 'border-error'
            : 'border-on-surface'
          }`}
      />
      {error && (
        <p id={errorId} className="label-sm text-error mt-1">{error}</p>
      )}
    </>
  );
}

function MonolithToggle({
  id,
  checked,
  onChange,
  label,
}: {
  id: string;
  checked: boolean;
  onChange: () => void;
  label: string;
}) {
  return (
    <button
      id={id}
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={onChange}
      className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer border
        focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2
        ${checked
          ? 'bg-on-surface border-on-surface'
          : 'bg-surface-container border-outline'
        }`}
    >
      <span
        aria-hidden="true"
        className={`pointer-events-none inline-block h-4 w-4 mt-[3px]
          ${checked
            ? 'translate-x-[22px] bg-surface'
            : 'translate-x-[3px] bg-outline'
          }`}
      />
    </button>
  );
}

function MonolithAlert({
  type,
  children,
}: {
  type: 'error' | 'success';
  children: React.ReactNode;
}) {
  return (
    <div
      role={type === 'error' ? 'alert' : 'status'}
      aria-live={type === 'success' ? 'polite' : undefined}
      className={`border border-on-surface px-4 py-3 text-sm
        ${type === 'error'
          ? 'text-error'
          : 'text-on-surface'
        }`}
    >
      {children}
    </div>
  );
}

function FieldLabel({
  htmlFor,
  required,
  children,
}: {
  htmlFor: string;
  required?: boolean;
  children: React.ReactNode;
}) {
  return (
    <label htmlFor={htmlFor} className="label-md block mb-2 text-on-surface">
      {children}
      {required && <span className="text-error ml-1">*</span>}
    </label>
  );
}

function SectionDivider() {
  return <hr className="border-t border-on-surface opacity-10" />;
}

function Spinner() {
  return (
    <svg className="h-4 w-4 animate-spin mr-2" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
      <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
    </svg>
  );
}

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

  // ── Provider status ────────────────────────────────────────────────────

  function providerStatusBadge(p: OAuth2ProviderSettings): { label: string; color: string } {
    if (!p.enabled) {
      return { label: 'Disabled', color: 'bg-surface-container text-secondary' };
    }
    if (!p.clientId.trim()) {
      return { label: 'Not configured', color: 'bg-surface-container text-outline' };
    }
    return { label: 'Active', color: 'bg-on-surface text-surface' };
  }

  // ── Render ─────────────────────────────────────────────────────────────

  if (loading) {
    return (
      <DashboardLayout currentPath="/_/settings/auth-providers" pageTitle="Auth Providers">
        <div className="flex items-center justify-center py-24">
          <svg className="h-5 w-5 animate-spin text-outline" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
            <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
          </svg>
          <span className="ml-3 label-md text-outline">Loading auth providers...</span>
        </div>
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout currentPath="/_/settings/auth-providers" pageTitle="Auth Providers">
      <div className="max-w-5xl mx-auto">
        {/* ── Page Header ─────────────────────────────────── */}
        <header className="mb-16">
          <h2 className="display-lg text-on-surface uppercase">Auth Providers</h2>
          <div className="h-1 w-24 bg-on-surface mt-2" />
        </header>

        <form onSubmit={handleSave} noValidate>
          <div className="space-y-24">
            {/* ── Global messages ────────────────────────────── */}
            {(error || success) && (
              <div className="space-y-3">
                {error && <MonolithAlert type="error">{error}</MonolithAlert>}
                {success && <MonolithAlert type="success">{success}</MonolithAlert>}
              </div>
            )}

            {/* ── Provider sections ─────────────────────────── */}
            {KNOWN_PROVIDERS.map((meta, idx) => {
              const p = providers[meta.key];
              if (!p) return null;

              const status = providerStatusBadge(p);
              const sectionNum = String(idx + 1).padStart(2, '0');

              return (
                <div key={meta.key}>
                  {idx > 0 && <div className="mb-24"><SectionDivider /></div>}

                  <section className="grid grid-cols-1 lg:grid-cols-12 gap-8" data-testid={`provider-${meta.key}`}>
                    {/* Left column — section header */}
                    <div className="lg:col-span-4">
                      <div className="flex items-center gap-3">
                        <h3 className="label-md tracking-[0.2em] text-on-surface">
                          {sectionNum}. {meta.displayName}
                        </h3>
                        <span
                          className={`label-sm px-2 py-0.5 ${status.color}`}
                          data-testid={`${meta.key}-status`}
                        >
                          {status.label}
                        </span>
                      </div>
                      <p className="text-sm text-secondary mt-2 leading-relaxed">
                        OAuth2 authentication via {meta.displayName}. Users can sign in with their {meta.displayName} account.
                      </p>
                      <div className="mt-4 flex items-center gap-2">
                        <ProviderIcon name={meta.key} />
                      </div>
                    </div>

                    {/* Right column — form fields */}
                    <div className="lg:col-span-8 space-y-6">
                      {/* Enable toggle */}
                      <div className="flex items-center justify-between">
                        <div>
                          <label htmlFor={`${meta.key}-enabled`} className="label-md text-on-surface">
                            Enable {meta.displayName}
                          </label>
                          <p className="text-xs text-secondary mt-0.5">
                            Allow users to sign in with {meta.displayName}.
                          </p>
                        </div>
                        <MonolithToggle
                          id={`${meta.key}-enabled`}
                          checked={p.enabled}
                          onChange={() => updateProvider(meta.key, 'enabled', !p.enabled)}
                          label={`Enable ${meta.displayName}`}
                        />
                      </div>

                      {/* Fields — only show when enabled */}
                      {p.enabled && (
                        <>
                          {/* Client ID */}
                          <div>
                            <FieldLabel htmlFor={`${meta.key}-client-id`} required>Client ID</FieldLabel>
                            <MonolithInput
                              id={`${meta.key}-client-id`}
                              name={`${meta.key}-clientId`}
                              value={p.clientId}
                              onChange={(e) => updateProvider(meta.key, 'clientId', e.target.value)}
                              placeholder={`${meta.displayName} OAuth2 client ID`}
                              disabled={saving}
                              error={fieldErrors[`${meta.key}.clientId`]}
                              errorId={`${meta.key}-client-id-error`}
                              autoComplete="off"
                              required
                            />
                          </div>

                          {/* Client Secret */}
                          <div>
                            <FieldLabel htmlFor={`${meta.key}-client-secret`}>Client Secret</FieldLabel>
                            <MonolithInput
                              id={`${meta.key}-client-secret`}
                              name={`${meta.key}-clientSecret`}
                              type="password"
                              value={p.clientSecret}
                              onChange={(e) => updateProvider(meta.key, 'clientSecret', e.target.value)}
                              placeholder="Leave blank to keep existing secret"
                              disabled={saving}
                              autoComplete="off"
                            />
                            <p className="text-xs text-secondary mt-1">
                              Write-only. Leave blank to keep the current secret.
                            </p>
                          </div>

                          {/* Redirect URL (read-only) */}
                          <div>
                            <FieldLabel htmlFor={`${meta.key}-redirect-url`}>Redirect URL</FieldLabel>
                            <div className="flex gap-2">
                              <MonolithInput
                                id={`${meta.key}-redirect-url`}
                                name={`${meta.key}-redirectUrl`}
                                value={getRedirectUrl(meta.key)}
                                readOnly
                              />
                              <button
                                type="button"
                                onClick={() => {
                                  navigator.clipboard.writeText(getRedirectUrl(meta.key));
                                }}
                                className="shrink-0 border border-on-surface px-3 py-3 text-on-surface hover:bg-surface-container transition-colors-fast"
                                aria-label={`Copy ${meta.displayName} redirect URL`}
                              >
                                <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                                  <rect x="9" y="9" width="13" height="13" />
                                  <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                                </svg>
                              </button>
                            </div>
                            <p className="text-xs text-secondary mt-1">
                              Add this URL to your {meta.displayName} OAuth2 app's authorized redirect URIs.
                            </p>
                          </div>
                        </>
                      )}
                    </div>
                  </section>
                </div>
              );
            })}

            <SectionDivider />

            {/* ── Save / Cancel ──────────────────────────────── */}
            <div className="flex justify-end gap-4">
              <button
                type="button"
                onClick={() => loadSettings()}
                disabled={saving}
                className="border border-on-surface bg-surface text-on-surface px-8 py-3 label-md tracking-[0.15em] uppercase hover:bg-surface-container transition-colors
                  focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2
                  disabled:cursor-not-allowed disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={saving}
                className="bg-on-surface text-surface px-8 py-3 label-md tracking-[0.15em] uppercase hover:opacity-90 transition-opacity
                  focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2
                  disabled:cursor-not-allowed disabled:opacity-50 inline-flex items-center"
              >
                {saving && <Spinner />}
                {saving ? 'Saving...' : 'Save Providers'}
              </button>
            </div>
          </div>
        </form>
      </div>
    </DashboardLayout>
  );
}

// ── Provider icons ───────────────────────────────────────────────────────────

function ProviderIcon({ name }: { name: string }) {
  switch (name) {
    case 'google':
      return (
        <svg className="h-6 w-6" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 0 1-2.2 3.32v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.1z" fill="#4285F4" />
          <path d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z" fill="#34A853" />
          <path d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z" fill="#FBBC05" />
          <path d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z" fill="#EA4335" />
        </svg>
      );
    case 'microsoft':
      return (
        <svg className="h-6 w-6" viewBox="0 0 24 24" aria-hidden="true">
          <rect x="1" y="1" width="10" height="10" fill="#F25022" />
          <rect x="13" y="1" width="10" height="10" fill="#7FBA00" />
          <rect x="1" y="13" width="10" height="10" fill="#00A4EF" />
          <rect x="13" y="13" width="10" height="10" fill="#FFB900" />
        </svg>
      );
    default:
      return (
        <svg className="h-6 w-6 text-outline" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <circle cx="12" cy="12" r="10" />
          <path d="M12 16v-4M12 8h.01" />
        </svg>
      );
  }
}
