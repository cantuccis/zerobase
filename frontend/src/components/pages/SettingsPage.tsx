import { useState, useEffect, useCallback } from 'react';
import { DashboardLayout } from '../DashboardLayout';
import { client } from '../../lib/auth/client';
import { ApiError } from '../../lib/api';
import type { SmtpSettings, MetaSenderSettings, S3Settings } from '../../lib/api/types';

// ── Default values ──────────────────────────────────────────────────────────

const DEFAULT_SMTP: SmtpSettings = {
  enabled: false,
  host: '',
  port: 587,
  username: '',
  password: '',
  tls: true,
};

const DEFAULT_META: MetaSenderSettings = {
  appName: '',
  appUrl: '',
  senderName: 'Zerobase',
  senderAddress: '',
};

const DEFAULT_S3: S3Settings = {
  enabled: false,
  bucket: '',
  region: '',
  endpoint: '',
  accessKey: '',
  secretKey: '',
  forcePathStyle: false,
};

// ── Component ───────────────────────────────────────────────────────────────

export function SettingsPage() {
  // SMTP settings
  const [smtp, setSmtp] = useState<SmtpSettings>(DEFAULT_SMTP);
  // Meta sender settings
  const [meta, setMeta] = useState<MetaSenderSettings>(DEFAULT_META);
  // S3 storage settings
  const [s3, setS3] = useState<S3Settings>(DEFAULT_S3);

  // UI state
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [savingStorage, setSavingStorage] = useState(false);
  const [testingSend, setTestingSend] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [success, setSuccess] = useState<string | null>(null);
  const [testEmail, setTestEmail] = useState('');
  const [testEmailError, setTestEmailError] = useState<string | null>(null);
  const [testEmailSuccess, setTestEmailSuccess] = useState<string | null>(null);
  const [storageError, setStorageError] = useState<string | null>(null);
  const [storageFieldErrors, setStorageFieldErrors] = useState<Record<string, string>>({});
  const [storageSuccess, setStorageSuccess] = useState<string | null>(null);

  // ── Load settings ──────────────────────────────────────────────────────

  const loadSettings = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const settings = await client.getSettings();
      const smtpData = (settings.smtp ?? {}) as Partial<SmtpSettings>;
      const metaData = (settings.meta ?? {}) as Partial<MetaSenderSettings>;

      setSmtp({
        enabled: smtpData.enabled ?? DEFAULT_SMTP.enabled,
        host: smtpData.host ?? DEFAULT_SMTP.host,
        port: smtpData.port ?? DEFAULT_SMTP.port,
        username: smtpData.username ?? DEFAULT_SMTP.username,
        password: '', // Password is write-only
        tls: smtpData.tls ?? DEFAULT_SMTP.tls,
      });

      setMeta({
        appName: metaData.appName ?? DEFAULT_META.appName,
        appUrl: metaData.appUrl ?? DEFAULT_META.appUrl,
        senderName: metaData.senderName ?? DEFAULT_META.senderName,
        senderAddress: metaData.senderAddress ?? DEFAULT_META.senderAddress,
      });

      const s3Data = (settings.s3 ?? {}) as Partial<S3Settings>;
      setS3({
        enabled: s3Data.enabled ?? DEFAULT_S3.enabled,
        bucket: s3Data.bucket ?? DEFAULT_S3.bucket,
        region: s3Data.region ?? DEFAULT_S3.region,
        endpoint: s3Data.endpoint ?? DEFAULT_S3.endpoint,
        accessKey: s3Data.accessKey ?? DEFAULT_S3.accessKey,
        secretKey: '', // Secret key is write-only
        forcePathStyle: s3Data.forcePathStyle ?? DEFAULT_S3.forcePathStyle,
      });
    } catch (err) {
      if (err instanceof ApiError) {
        setError(err.response.message || 'Failed to load settings.');
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

  // ── Validate ───────────────────────────────────────────────────────────

  function validate(): boolean {
    const errors: Record<string, string> = {};

    if (smtp.enabled) {
      if (!smtp.host.trim()) {
        errors.host = 'SMTP host is required when enabled.';
      }
      if (!smtp.port || smtp.port < 1 || smtp.port > 65535) {
        errors.port = 'Port must be between 1 and 65535.';
      }
      if (!meta.senderAddress.trim()) {
        errors.senderAddress = 'Sender address is required when SMTP is enabled.';
      }
    }

    setFieldErrors(errors);
    return Object.keys(errors).length === 0;
  }

  // ── Save settings ──────────────────────────────────────────────────────

  async function handleSave(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setSuccess(null);

    if (!validate()) return;

    setSaving(true);
    try {
      // Build the SMTP update — only include password if non-empty
      const smtpUpdate: Record<string, unknown> = {
        enabled: smtp.enabled,
        host: smtp.host.trim(),
        port: smtp.port,
        username: smtp.username.trim(),
        tls: smtp.tls,
      };
      if (smtp.password) {
        smtpUpdate.password = smtp.password;
      }

      await client.updateSettings({
        smtp: smtpUpdate,
        meta: {
          senderName: meta.senderName.trim(),
          senderAddress: meta.senderAddress.trim(),
        },
      });

      setSuccess('Settings saved successfully.');
      // Clear password field after save (it's write-only)
      setSmtp((prev) => ({ ...prev, password: '' }));
    } catch (err) {
      if (err instanceof ApiError) {
        setError(err.response.message || 'Failed to save settings.');
      } else {
        setError('Unable to connect to the server.');
      }
    } finally {
      setSaving(false);
    }
  }

  // ── Send test email ────────────────────────────────────────────────────

  async function handleTestEmail() {
    setTestEmailError(null);
    setTestEmailSuccess(null);

    if (!testEmail.trim()) {
      setTestEmailError('Please enter a recipient email address.');
      return;
    }

    if (!smtp.enabled) {
      setTestEmailError('SMTP must be enabled before sending a test email.');
      return;
    }

    setTestingSend(true);
    try {
      await client.testEmail(testEmail.trim());
      setTestEmailSuccess(`Test email sent to ${testEmail.trim()}.`);
    } catch (err) {
      if (err instanceof ApiError) {
        setTestEmailError(err.response.message || 'Failed to send test email.');
      } else {
        setTestEmailError('Unable to connect to the server.');
      }
    } finally {
      setTestingSend(false);
    }
  }

  // ── Helpers ────────────────────────────────────────────────────────────

  function updateSmtp<K extends keyof SmtpSettings>(key: K, value: SmtpSettings[K]) {
    setSmtp((prev) => ({ ...prev, [key]: value }));
    // Clear field error on change
    if (fieldErrors[key]) {
      setFieldErrors((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
    }
  }

  function updateMeta<K extends keyof MetaSenderSettings>(key: K, value: MetaSenderSettings[K]) {
    setMeta((prev) => ({ ...prev, [key]: value }));
    if (fieldErrors[key]) {
      setFieldErrors((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
    }
  }

  // ── S3 helpers ────────────────────────────────────────────────────────

  function updateS3<K extends keyof S3Settings>(key: K, value: S3Settings[K]) {
    setS3((prev) => ({ ...prev, [key]: value }));
    if (storageFieldErrors[key]) {
      setStorageFieldErrors((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
    }
  }

  function validateStorage(): boolean {
    const errors: Record<string, string> = {};

    if (s3.enabled) {
      if (!s3.bucket.trim()) {
        errors.bucket = 'Bucket is required when S3 is enabled.';
      }
      if (!s3.region.trim()) {
        errors.region = 'Region is required when S3 is enabled.';
      }
    }

    setStorageFieldErrors(errors);
    return Object.keys(errors).length === 0;
  }

  async function handleSaveStorage(e: React.FormEvent) {
    e.preventDefault();
    setStorageError(null);
    setStorageSuccess(null);

    if (!validateStorage()) return;

    setSavingStorage(true);
    try {
      const s3Update: Record<string, unknown> = {
        enabled: s3.enabled,
        bucket: s3.bucket.trim(),
        region: s3.region.trim(),
        endpoint: s3.endpoint.trim(),
        accessKey: s3.accessKey.trim(),
        forcePathStyle: s3.forcePathStyle,
      };
      if (s3.secretKey) {
        s3Update.secretKey = s3.secretKey;
      }

      await client.updateSettings({ s3: s3Update });

      setStorageSuccess('Storage settings saved successfully.');
      // Clear secret key field after save (it's write-only)
      setS3((prev) => ({ ...prev, secretKey: '' }));
    } catch (err) {
      if (err instanceof ApiError) {
        setStorageError(err.response.message || 'Failed to save storage settings.');
      } else {
        setStorageError('Unable to connect to the server.');
      }
    } finally {
      setSavingStorage(false);
    }
  }

  function storageStatus(): { label: string; color: string } {
    if (!s3.enabled) {
      return { label: 'Local storage', color: 'bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400' };
    }
    if (!s3.bucket.trim() || !s3.region.trim()) {
      return { label: 'Not configured', color: 'bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400' };
    }
    return { label: 'S3', color: 'bg-green-100 dark:bg-green-900/20 text-green-700 dark:text-green-400' };
  }

  const storageStatusBadge = storageStatus();

  // ── Connection status ──────────────────────────────────────────────────

  function connectionStatus(): { label: string; color: string } {
    if (!smtp.enabled) {
      return { label: 'Disabled', color: 'bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400' };
    }
    if (!smtp.host.trim()) {
      return { label: 'Not configured', color: 'bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400' };
    }
    return { label: 'Configured', color: 'bg-green-100 dark:bg-green-900/20 text-green-700 dark:text-green-400' };
  }

  const status = connectionStatus();

  // ── Render ─────────────────────────────────────────────────────────────

  if (loading) {
    return (
      <DashboardLayout currentPath="/_/settings" pageTitle="Settings">
        <div className="flex items-center justify-center py-12">
          <svg className="h-6 w-6 animate-spin text-gray-400 dark:text-gray-500" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
            <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
          </svg>
          <span className="ml-2 text-sm text-gray-500 dark:text-gray-400">Loading settings...</span>
        </div>
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout currentPath="/_/settings" pageTitle="Settings">
      <div className="mx-auto max-w-2xl space-y-8">
        {/* ── SMTP Configuration ─────────────────────────────── */}
        <form onSubmit={handleSave} noValidate>
          <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 shadow-sm">
            <div className="flex items-center justify-between border-b border-gray-200 dark:border-gray-700 px-6 py-4">
              <div>
                <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">SMTP Configuration</h3>
                <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
                  Configure outgoing email for verification, password reset, and OTP.
                </p>
              </div>
              <span
                className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${status.color}`}
                data-testid="smtp-status"
              >
                {status.label}
              </span>
            </div>

            <div className="space-y-5 px-6 py-5">
              {/* Global messages */}
              {error && (
                <div role="alert" className="rounded-md border border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30 px-4 py-3 text-sm text-red-700 dark:text-red-400">
                  {error}
                </div>
              )}
              {success && (
                <div role="status" aria-live="polite" className="rounded-md border border-green-200 dark:border-green-800 bg-green-50 dark:bg-green-900/30 px-4 py-3 text-sm text-green-700 dark:text-green-400">
                  {success}
                </div>
              )}

              {/* Enabled toggle */}
              <div className="flex items-center justify-between">
                <div>
                  <label htmlFor="smtp-enabled" className="text-sm font-medium text-gray-700 dark:text-gray-300">
                    Enable SMTP
                  </label>
                  <p className="text-xs text-gray-500 dark:text-gray-400">Turn on outgoing email delivery.</p>
                </div>
                <button
                  id="smtp-enabled"
                  type="button"
                  role="switch"
                  aria-checked={smtp.enabled}
                  onClick={() => updateSmtp('enabled', !smtp.enabled)}
                  className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors
                    focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2
                    ${smtp.enabled ? 'bg-blue-600' : 'bg-gray-200 dark:bg-gray-600'}`}
                >
                  <span
                    aria-hidden="true"
                    className={`pointer-events-none inline-block h-5 w-5 rounded-full bg-white dark:bg-gray-800 shadow ring-0 transition-transform
                      ${smtp.enabled ? 'translate-x-5' : 'translate-x-0'}`}
                  />
                </button>
              </div>

              {/* SMTP fields — only show when enabled */}
              {smtp.enabled && (
                <>
                  {/* Host */}
                  <div className="space-y-1.5">
                    <label htmlFor="smtp-host" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                      SMTP Host <span className="text-red-500 dark:text-red-400">*</span>
                    </label>
                    <input
                      id="smtp-host"
                      name="host"
                      type="text"
                      autoComplete="off"
                      value={smtp.host}
                      onChange={(e) => updateSmtp('host', e.target.value)}
                      disabled={saving}
                      aria-invalid={!!fieldErrors.host}
                      aria-describedby={fieldErrors.host ? 'smtp-host-error' : undefined}
                      className={`block w-full rounded-md border px-3 py-2 text-sm shadow-sm transition-colors
                        focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
                        disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700
                        ${fieldErrors.host ? 'border-red-400 dark:border-red-700' : 'border-gray-300 dark:border-gray-600'}`}
                      placeholder="smtp.example.com"
                    />
                    {fieldErrors.host && (
                      <p id="smtp-host-error" className="text-xs text-red-600 dark:text-red-400">{fieldErrors.host}</p>
                    )}
                  </div>

                  {/* Port + TLS */}
                  <div className="grid grid-cols-2 gap-4">
                    <div className="space-y-1.5">
                      <label htmlFor="smtp-port" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                        Port <span className="text-red-500 dark:text-red-400">*</span>
                      </label>
                      <input
                        id="smtp-port"
                        name="port"
                        type="number"
                        min={1}
                        max={65535}
                        value={smtp.port}
                        onChange={(e) => updateSmtp('port', parseInt(e.target.value, 10) || 0)}
                        disabled={saving}
                        aria-invalid={!!fieldErrors.port}
                        aria-describedby={fieldErrors.port ? 'smtp-port-error' : undefined}
                        className={`block w-full rounded-md border px-3 py-2 text-sm shadow-sm transition-colors
                          focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
                          disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700
                          ${fieldErrors.port ? 'border-red-400 dark:border-red-700' : 'border-gray-300 dark:border-gray-600'}`}
                      />
                      {fieldErrors.port && (
                        <p id="smtp-port-error" className="text-xs text-red-600 dark:text-red-400">{fieldErrors.port}</p>
                      )}
                    </div>

                    <div className="space-y-1.5">
                      <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">TLS</label>
                      <div className="flex items-center pt-2">
                        <button
                          id="smtp-tls"
                          type="button"
                          role="switch"
                          aria-checked={smtp.tls}
                          aria-label="Enable TLS"
                          onClick={() => updateSmtp('tls', !smtp.tls)}
                          className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors
                            focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2
                            ${smtp.tls ? 'bg-blue-600' : 'bg-gray-200 dark:bg-gray-600'}`}
                        >
                          <span
                            aria-hidden="true"
                            className={`pointer-events-none inline-block h-5 w-5 rounded-full bg-white dark:bg-gray-800 shadow ring-0 transition-transform
                              ${smtp.tls ? 'translate-x-5' : 'translate-x-0'}`}
                          />
                        </button>
                        <span className="ml-2 text-sm text-gray-600 dark:text-gray-400">
                          {smtp.tls ? 'Enabled' : 'Disabled'}
                        </span>
                      </div>
                    </div>
                  </div>

                  {/* Username */}
                  <div className="space-y-1.5">
                    <label htmlFor="smtp-username" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                      Username
                    </label>
                    <input
                      id="smtp-username"
                      name="username"
                      type="text"
                      autoComplete="off"
                      spellCheck={false}
                      value={smtp.username}
                      onChange={(e) => updateSmtp('username', e.target.value)}
                      disabled={saving}
                      className="block w-full rounded-md border border-gray-300 dark:border-gray-600 px-3 py-2 text-sm shadow-sm transition-colors
                        focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
                        disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700"
                      placeholder="SMTP username (optional)"
                    />
                  </div>

                  {/* Password */}
                  <div className="space-y-1.5">
                    <label htmlFor="smtp-password" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                      Password
                    </label>
                    <input
                      id="smtp-password"
                      name="password"
                      type="password"
                      autoComplete="new-password"
                      value={smtp.password}
                      onChange={(e) => updateSmtp('password', e.target.value)}
                      disabled={saving}
                      className="block w-full rounded-md border border-gray-300 dark:border-gray-600 px-3 py-2 text-sm shadow-sm transition-colors
                        focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
                        disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700"
                      placeholder="Leave blank to keep current password"
                    />
                    <p className="text-xs text-gray-500 dark:text-gray-400">
                      Leave empty to keep the existing password.
                    </p>
                  </div>

                  {/* Sender Name */}
                  <div className="space-y-1.5">
                    <label htmlFor="smtp-sender-name" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                      Sender Name
                    </label>
                    <input
                      id="smtp-sender-name"
                      name="senderName"
                      type="text"
                      autoComplete="off"
                      value={meta.senderName}
                      onChange={(e) => updateMeta('senderName', e.target.value)}
                      disabled={saving}
                      className="block w-full rounded-md border border-gray-300 dark:border-gray-600 px-3 py-2 text-sm shadow-sm transition-colors
                        focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
                        disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700"
                      placeholder="Zerobase"
                    />
                  </div>

                  {/* Sender Address */}
                  <div className="space-y-1.5">
                    <label htmlFor="smtp-sender-address" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                      Sender Address <span className="text-red-500 dark:text-red-400">*</span>
                    </label>
                    <input
                      id="smtp-sender-address"
                      name="senderAddress"
                      type="email"
                      autoComplete="off"
                      spellCheck={false}
                      value={meta.senderAddress}
                      onChange={(e) => updateMeta('senderAddress', e.target.value)}
                      disabled={saving}
                      aria-invalid={!!fieldErrors.senderAddress}
                      aria-describedby={fieldErrors.senderAddress ? 'smtp-sender-error' : undefined}
                      className={`block w-full rounded-md border px-3 py-2 text-sm shadow-sm transition-colors
                        focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
                        disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700
                        ${fieldErrors.senderAddress ? 'border-red-400 dark:border-red-700' : 'border-gray-300 dark:border-gray-600'}`}
                      placeholder="noreply@example.com"
                    />
                    {fieldErrors.senderAddress && (
                      <p id="smtp-sender-error" className="text-xs text-red-600 dark:text-red-400">{fieldErrors.senderAddress}</p>
                    )}
                  </div>
                </>
              )}
            </div>

            {/* Save button */}
            <div className="flex items-center justify-end border-t border-gray-200 dark:border-gray-700 px-6 py-4">
              <button
                type="submit"
                disabled={saving}
                className="flex items-center rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white shadow-sm
                  transition-colors hover:bg-blue-700 dark:hover:bg-blue-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2
                  disabled:cursor-not-allowed disabled:opacity-60"
              >
                {saving ? (
                  <>
                    <svg className="mr-2 h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                      <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                    </svg>
                    Saving...
                  </>
                ) : (
                  'Save Settings'
                )}
              </button>
            </div>
          </div>
        </form>

        {/* ── Send Test Email ─────────────────────────────────── */}
        {/* ── File Storage ──────────────────────────────────────── */}
        <form onSubmit={handleSaveStorage} noValidate>
          <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 shadow-sm">
            <div className="flex items-center justify-between border-b border-gray-200 dark:border-gray-700 px-6 py-4">
              <div>
                <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">File Storage</h3>
                <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
                  Configure where uploaded files are stored.
                </p>
              </div>
              <span
                className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${storageStatusBadge.color}`}
                data-testid="storage-status"
              >
                {storageStatusBadge.label}
              </span>
            </div>

            <div className="space-y-5 px-6 py-5">
              {/* Storage messages */}
              {storageError && (
                <div role="alert" className="rounded-md border border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30 px-4 py-3 text-sm text-red-700 dark:text-red-400">
                  {storageError}
                </div>
              )}
              {storageSuccess && (
                <div role="status" aria-live="polite" className="rounded-md border border-green-200 dark:border-green-800 bg-green-50 dark:bg-green-900/30 px-4 py-3 text-sm text-green-700 dark:text-green-400">
                  {storageSuccess}
                </div>
              )}

              {/* S3 toggle */}
              <div className="flex items-center justify-between">
                <div>
                  <label htmlFor="s3-enabled" className="text-sm font-medium text-gray-700 dark:text-gray-300">
                    Use S3 Storage
                  </label>
                  <p className="text-xs text-gray-500 dark:text-gray-400">Enable S3-compatible object storage instead of local filesystem.</p>
                </div>
                <button
                  id="s3-enabled"
                  type="button"
                  role="switch"
                  aria-checked={s3.enabled}
                  onClick={() => updateS3('enabled', !s3.enabled)}
                  className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors
                    focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2
                    ${s3.enabled ? 'bg-blue-600' : 'bg-gray-200 dark:bg-gray-600'}`}
                >
                  <span
                    aria-hidden="true"
                    className={`pointer-events-none inline-block h-5 w-5 rounded-full bg-white dark:bg-gray-800 shadow ring-0 transition-transform
                      ${s3.enabled ? 'translate-x-5' : 'translate-x-0'}`}
                  />
                </button>
              </div>

              {/* S3 fields — only show when enabled */}
              {s3.enabled && (
                <>
                  {/* Bucket */}
                  <div className="space-y-1.5">
                    <label htmlFor="s3-bucket" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                      Bucket <span className="text-red-500 dark:text-red-400">*</span>
                    </label>
                    <input
                      id="s3-bucket"
                      name="bucket"
                      type="text"
                      autoComplete="off"
                      value={s3.bucket}
                      onChange={(e) => updateS3('bucket', e.target.value)}
                      disabled={savingStorage}
                      aria-invalid={!!storageFieldErrors.bucket}
                      aria-describedby={storageFieldErrors.bucket ? 's3-bucket-error' : undefined}
                      className={`block w-full rounded-md border px-3 py-2 text-sm shadow-sm transition-colors
                        focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
                        disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700
                        ${storageFieldErrors.bucket ? 'border-red-400 dark:border-red-700' : 'border-gray-300 dark:border-gray-600'}`}
                      placeholder="my-bucket"
                    />
                    {storageFieldErrors.bucket && (
                      <p id="s3-bucket-error" className="text-xs text-red-600 dark:text-red-400">{storageFieldErrors.bucket}</p>
                    )}
                  </div>

                  {/* Region + Endpoint */}
                  <div className="grid grid-cols-2 gap-4">
                    <div className="space-y-1.5">
                      <label htmlFor="s3-region" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                        Region <span className="text-red-500 dark:text-red-400">*</span>
                      </label>
                      <input
                        id="s3-region"
                        name="region"
                        type="text"
                        autoComplete="off"
                        value={s3.region}
                        onChange={(e) => updateS3('region', e.target.value)}
                        disabled={savingStorage}
                        aria-invalid={!!storageFieldErrors.region}
                        aria-describedby={storageFieldErrors.region ? 's3-region-error' : undefined}
                        className={`block w-full rounded-md border px-3 py-2 text-sm shadow-sm transition-colors
                          focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
                          disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700
                          ${storageFieldErrors.region ? 'border-red-400 dark:border-red-700' : 'border-gray-300 dark:border-gray-600'}`}
                        placeholder="us-east-1"
                      />
                      {storageFieldErrors.region && (
                        <p id="s3-region-error" className="text-xs text-red-600 dark:text-red-400">{storageFieldErrors.region}</p>
                      )}
                    </div>

                    <div className="space-y-1.5">
                      <label htmlFor="s3-endpoint" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                        Endpoint
                      </label>
                      <input
                        id="s3-endpoint"
                        name="endpoint"
                        type="text"
                        autoComplete="off"
                        value={s3.endpoint}
                        onChange={(e) => updateS3('endpoint', e.target.value)}
                        disabled={savingStorage}
                        className="block w-full rounded-md border border-gray-300 dark:border-gray-600 px-3 py-2 text-sm shadow-sm transition-colors
                          focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
                          disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700"
                        placeholder="https://s3.amazonaws.com"
                      />
                    </div>
                  </div>

                  {/* Access Key */}
                  <div className="space-y-1.5">
                    <label htmlFor="s3-access-key" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                      Access Key
                    </label>
                    <input
                      id="s3-access-key"
                      name="accessKey"
                      type="text"
                      autoComplete="off"
                      spellCheck={false}
                      value={s3.accessKey}
                      onChange={(e) => updateS3('accessKey', e.target.value)}
                      disabled={savingStorage}
                      className="block w-full rounded-md border border-gray-300 dark:border-gray-600 px-3 py-2 text-sm shadow-sm transition-colors
                        focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
                        disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700"
                      placeholder="AKIAIOSFODNN7EXAMPLE"
                    />
                  </div>

                  {/* Secret Key */}
                  <div className="space-y-1.5">
                    <label htmlFor="s3-secret-key" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                      Secret Key
                    </label>
                    <input
                      id="s3-secret-key"
                      name="secretKey"
                      type="password"
                      autoComplete="new-password"
                      value={s3.secretKey}
                      onChange={(e) => updateS3('secretKey', e.target.value)}
                      disabled={savingStorage}
                      className="block w-full rounded-md border border-gray-300 dark:border-gray-600 px-3 py-2 text-sm shadow-sm transition-colors
                        focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
                        disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700"
                      placeholder="Leave blank to keep current secret key"
                    />
                    <p className="text-xs text-gray-500 dark:text-gray-400">
                      Leave empty to keep the existing secret key.
                    </p>
                  </div>

                  {/* Force Path Style */}
                  <div className="flex items-center justify-between">
                    <div>
                      <label htmlFor="s3-force-path-style" className="text-sm font-medium text-gray-700 dark:text-gray-300">
                        Force Path Style
                      </label>
                      <p className="text-xs text-gray-500 dark:text-gray-400">Use path-style URLs (required for some S3-compatible providers).</p>
                    </div>
                    <button
                      id="s3-force-path-style"
                      type="button"
                      role="switch"
                      aria-checked={s3.forcePathStyle}
                      aria-label="Force path style"
                      onClick={() => updateS3('forcePathStyle', !s3.forcePathStyle)}
                      className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors
                        focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2
                        ${s3.forcePathStyle ? 'bg-blue-600' : 'bg-gray-200 dark:bg-gray-600'}`}
                    >
                      <span
                        aria-hidden="true"
                        className={`pointer-events-none inline-block h-5 w-5 rounded-full bg-white dark:bg-gray-800 shadow ring-0 transition-transform
                          ${s3.forcePathStyle ? 'translate-x-5' : 'translate-x-0'}`}
                      />
                    </button>
                  </div>
                </>
              )}
            </div>

            {/* Save button */}
            <div className="flex items-center justify-end border-t border-gray-200 dark:border-gray-700 px-6 py-4">
              <button
                type="submit"
                disabled={savingStorage}
                className="flex items-center rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white shadow-sm
                  transition-colors hover:bg-blue-700 dark:hover:bg-blue-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2
                  disabled:cursor-not-allowed disabled:opacity-60"
              >
                {savingStorage ? (
                  <>
                    <svg className="mr-2 h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                      <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                    </svg>
                    Saving...
                  </>
                ) : (
                  'Save Storage Settings'
                )}
              </button>
            </div>
          </div>
        </form>

        {smtp.enabled && (
          <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 shadow-sm">
            <div className="border-b border-gray-200 dark:border-gray-700 px-6 py-4">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">Send Test Email</h3>
              <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
                Verify your SMTP configuration by sending a test message.
              </p>
            </div>

            <div className="space-y-4 px-6 py-5">
              {testEmailError && (
                <div role="alert" className="rounded-md border border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30 px-4 py-3 text-sm text-red-700 dark:text-red-400">
                  {testEmailError}
                </div>
              )}
              {testEmailSuccess && (
                <div role="status" aria-live="polite" className="rounded-md border border-green-200 dark:border-green-800 bg-green-50 dark:bg-green-900/30 px-4 py-3 text-sm text-green-700 dark:text-green-400">
                  {testEmailSuccess}
                </div>
              )}

              <div className="flex gap-3">
                <div className="flex-1">
                  <label htmlFor="test-email-to" className="sr-only">
                    Recipient email
                  </label>
                  <input
                    id="test-email-to"
                    name="testEmailTo"
                    type="email"
                    autoComplete="email"
                    spellCheck={false}
                    value={testEmail}
                    onChange={(e) => setTestEmail(e.target.value)}
                    disabled={testingSend}
                    className="block w-full rounded-md border border-gray-300 dark:border-gray-600 px-3 py-2 text-sm shadow-sm transition-colors
                      focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
                      disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700"
                    placeholder="recipient@example.com"
                  />
                </div>
                <button
                  type="button"
                  onClick={handleTestEmail}
                  disabled={testingSend}
                  className="flex shrink-0 items-center rounded-md border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 shadow-sm
                    transition-colors hover:bg-gray-50 dark:hover:bg-gray-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2
                    disabled:cursor-not-allowed disabled:opacity-60"
                >
                  {testingSend ? (
                    <>
                      <svg className="mr-2 h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                        <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                        <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                      </svg>
                      Sending...
                    </>
                  ) : (
                    'Send Test Email'
                  )}
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </DashboardLayout>
  );
}
