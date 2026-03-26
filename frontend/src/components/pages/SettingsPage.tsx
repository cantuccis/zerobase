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
  spellCheck,
  min,
  max,
  required,
}: {
  id: string;
  name: string;
  type?: string;
  value: string | number;
  onChange: (e: React.ChangeEvent<HTMLInputElement>) => void;
  placeholder?: string;
  disabled?: boolean;
  error?: string;
  errorId?: string;
  autoComplete?: string;
  spellCheck?: boolean;
  min?: number;
  max?: number;
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
        min={min}
        max={max}
        autoComplete={autoComplete}
        spellCheck={spellCheck}
        aria-invalid={!!error}
        aria-describedby={error ? errorId : undefined}
        aria-required={required}
        className={`mono-input w-full border bg-surface text-on-surface px-4 py-3 text-sm outline-none
          focus:border-2 focus:border-primary focus:px-[15px] focus:py-[11px]
          disabled:cursor-not-allowed disabled:opacity-50
          placeholder:text-outline
          ${error
            ? 'border-error'
            : 'border-primary'
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
          ? 'bg-on-surface border-primary'
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
      className={`border border-primary px-4 py-3 text-sm
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
  return <hr className="border-t border-primary opacity-10" />;
}

// ── Spinner ─────────────────────────────────────────────────────────────────

function Spinner() {
  return (
    <svg className="h-4 w-4 animate-spin mr-2" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
      <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
    </svg>
  );
}

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
      return { label: 'Local storage', color: 'bg-surface-container text-secondary' };
    }
    if (!s3.bucket.trim() || !s3.region.trim()) {
      return { label: 'Not configured', color: 'bg-surface-container text-outline' };
    }
    return { label: 'S3', color: 'bg-on-surface text-surface' };
  }

  const storageStatusBadge = storageStatus();

  // ── Connection status ──────────────────────────────────────────────────

  function connectionStatus(): { label: string; color: string } {
    if (!smtp.enabled) {
      return { label: 'Disabled', color: 'bg-surface-container text-secondary' };
    }
    if (!smtp.host.trim()) {
      return { label: 'Not configured', color: 'bg-surface-container text-outline' };
    }
    return { label: 'Configured', color: 'bg-on-surface text-surface' };
  }

  const status = connectionStatus();

  // ── Render ─────────────────────────────────────────────────────────────

  if (loading) {
    return (
      <DashboardLayout currentPath="/_/settings" pageTitle="Settings">
        <div className="flex items-center justify-center py-24">
          <svg className="h-5 w-5 animate-spin text-outline" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
            <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
          </svg>
          <span className="ml-3 label-md text-outline">Loading settings...</span>
        </div>
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout currentPath="/_/settings" pageTitle="Settings">
      <div className="max-w-5xl mx-auto">
        {/* ── Page Header ─────────────────────────────────── */}
        <header className="mb-16">
          <h2 className="display-lg text-on-surface uppercase">Configuration</h2>
          <div className="h-1 w-24 bg-on-surface mt-2" />
        </header>

        {/* ── SMTP + Meta Form ────────────────────────────── */}
        <form onSubmit={handleSave} noValidate>
          <div className="space-y-24">
            {/* ── 01. General ──────────────────────────────── */}
            <section className="grid grid-cols-1 lg:grid-cols-12 gap-8">
              <div className="lg:col-span-4">
                <h3 className="label-md tracking-[0.2em] text-on-surface">01. General</h3>
                <p className="text-sm text-secondary mt-2 leading-relaxed">
                  Core application identifiers and public-facing URL structures.
                </p>
              </div>
              <div className="lg:col-span-8 space-y-6">
                <div>
                  <FieldLabel htmlFor="meta-app-name">Application Name</FieldLabel>
                  <MonolithInput
                    id="meta-app-name"
                    name="appName"
                    value={meta.appName}
                    onChange={(e) => updateMeta('appName', e.target.value)}
                    placeholder="Zerobase"
                    disabled={saving}
                  />
                </div>
                <div>
                  <FieldLabel htmlFor="meta-app-url">Base URL</FieldLabel>
                  <MonolithInput
                    id="meta-app-url"
                    name="appUrl"
                    type="url"
                    value={meta.appUrl}
                    onChange={(e) => updateMeta('appUrl', e.target.value)}
                    placeholder="https://api.example.com"
                    disabled={saving}
                  />
                </div>
              </div>
            </section>

            <SectionDivider />

            {/* ── 02. Mail Settings ────────────────────────── */}
            <section className="grid grid-cols-1 lg:grid-cols-12 gap-8">
              <div className="lg:col-span-4">
                <div className="flex items-center gap-3">
                  <h3 className="label-md tracking-[0.2em] text-on-surface">02. Mail Settings</h3>
                  <span
                    className={`label-sm px-2 py-0.5 ${status.color}`}
                    data-testid="smtp-status"
                  >
                    {status.label}
                  </span>
                </div>
                <p className="text-sm text-secondary mt-2 leading-relaxed">
                  SMTP configuration for verification, password reset, and OTP delivery.
                </p>
              </div>
              <div className="lg:col-span-8 space-y-6">
                {error && <MonolithAlert type="error">{error}</MonolithAlert>}
                {success && <MonolithAlert type="success">{success}</MonolithAlert>}

                {/* Enable SMTP toggle */}
                <div className="flex items-center justify-between">
                  <div>
                    <label htmlFor="smtp-enabled" className="label-md text-on-surface">
                      Enable SMTP
                    </label>
                    <p className="text-xs text-secondary mt-0.5">Turn on outgoing email delivery.</p>
                  </div>
                  <MonolithToggle
                    id="smtp-enabled"
                    checked={smtp.enabled}
                    onChange={() => updateSmtp('enabled', !smtp.enabled)}
                    label="Enable SMTP"
                  />
                </div>

                {smtp.enabled && (
                  <>
                    {/* Host */}
                    <div>
                      <FieldLabel htmlFor="smtp-host" required>SMTP Host</FieldLabel>
                      <MonolithInput
                        id="smtp-host"
                        name="host"
                        value={smtp.host}
                        onChange={(e) => updateSmtp('host', e.target.value)}
                        placeholder="smtp.example.com"
                        disabled={saving}
                        error={fieldErrors.host}
                        errorId="smtp-host-error"
                        autoComplete="off"
                        required
                      />
                    </div>

                    {/* Port + TLS */}
                    <div className="grid grid-cols-2 gap-6">
                      <div>
                        <FieldLabel htmlFor="smtp-port" required>Port</FieldLabel>
                        <MonolithInput
                          id="smtp-port"
                          name="port"
                          type="number"
                          min={1}
                          max={65535}
                          value={smtp.port}
                          onChange={(e) => updateSmtp('port', parseInt(e.target.value, 10) || 0)}
                          disabled={saving}
                          error={fieldErrors.port}
                          errorId="smtp-port-error"
                          required
                        />
                      </div>
                      <div>
                        <label htmlFor="smtp-tls" className="label-md block mb-2 text-on-surface">TLS</label>
                        <div className="flex items-center pt-2 gap-3">
                          <MonolithToggle
                            id="smtp-tls"
                            checked={smtp.tls}
                            onChange={() => updateSmtp('tls', !smtp.tls)}
                            label="Enable TLS"
                          />
                          <span className="text-sm text-secondary">
                            {smtp.tls ? 'Enabled' : 'Disabled'}
                          </span>
                        </div>
                      </div>
                    </div>

                    {/* Username */}
                    <div>
                      <FieldLabel htmlFor="smtp-username">Username</FieldLabel>
                      <MonolithInput
                        id="smtp-username"
                        name="username"
                        value={smtp.username}
                        onChange={(e) => updateSmtp('username', e.target.value)}
                        placeholder="SMTP username (optional)"
                        disabled={saving}
                        autoComplete="off"
                        spellCheck={false}
                      />
                    </div>

                    {/* Password */}
                    <div>
                      <FieldLabel htmlFor="smtp-password">Password</FieldLabel>
                      <MonolithInput
                        id="smtp-password"
                        name="password"
                        type="password"
                        value={smtp.password}
                        onChange={(e) => updateSmtp('password', e.target.value)}
                        placeholder="Leave blank to keep current password"
                        disabled={saving}
                        autoComplete="new-password"
                      />
                      <p className="text-xs text-secondary mt-1">
                        Leave empty to keep the existing password.
                      </p>
                    </div>

                    {/* Sender Name */}
                    <div>
                      <FieldLabel htmlFor="smtp-sender-name">Sender Name</FieldLabel>
                      <MonolithInput
                        id="smtp-sender-name"
                        name="senderName"
                        value={meta.senderName}
                        onChange={(e) => updateMeta('senderName', e.target.value)}
                        placeholder="Zerobase"
                        disabled={saving}
                        autoComplete="off"
                      />
                    </div>

                    {/* Sender Address */}
                    <div>
                      <FieldLabel htmlFor="smtp-sender-address" required>Sender Address</FieldLabel>
                      <MonolithInput
                        id="smtp-sender-address"
                        name="senderAddress"
                        type="email"
                        value={meta.senderAddress}
                        onChange={(e) => updateMeta('senderAddress', e.target.value)}
                        placeholder="noreply@example.com"
                        disabled={saving}
                        error={fieldErrors.senderAddress}
                        errorId="smtp-sender-error"
                        autoComplete="off"
                        spellCheck={false}
                        required
                      />
                    </div>
                  </>
                )}
              </div>
            </section>

            {/* Save SMTP button */}
            <div className="flex justify-end">
              <button
                type="submit"
                disabled={saving}
                className="label-md bg-on-surface text-surface px-12 py-4 tracking-[0.3em] uppercase
                  hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2
                  disabled:cursor-not-allowed disabled:opacity-50
                  flex items-center"
              >
                {saving ? (
                  <>
                    <Spinner />
                    Saving...
                  </>
                ) : (
                  'Save Settings'
                )}
              </button>
            </div>
          </div>
        </form>

        <SectionDivider />

        {/* ── File Storage Form ────────────────────────────── */}
        <form onSubmit={handleSaveStorage} noValidate className="mt-24">
          <div className="space-y-24">
            {/* ── 03. File Storage ─────────────────────────── */}
            <section className="grid grid-cols-1 lg:grid-cols-12 gap-8">
              <div className="lg:col-span-4">
                <div className="flex items-center gap-3">
                  <h3 className="label-md tracking-[0.2em] text-on-surface">03. File Storage</h3>
                  <span
                    className={`label-sm px-2 py-0.5 ${storageStatusBadge.color}`}
                    data-testid="storage-status"
                  >
                    {storageStatusBadge.label}
                  </span>
                </div>
                <p className="text-sm text-secondary mt-2 leading-relaxed">
                  Cloud infrastructure integration for blob and object storage.
                </p>
              </div>
              <div className="lg:col-span-8 space-y-6">
                {storageError && <MonolithAlert type="error">{storageError}</MonolithAlert>}
                {storageSuccess && <MonolithAlert type="success">{storageSuccess}</MonolithAlert>}

                {/* S3 toggle */}
                <div className="flex items-center justify-between">
                  <div>
                    <label htmlFor="s3-enabled" className="label-md text-on-surface">
                      Use S3 Storage
                    </label>
                    <p className="text-xs text-secondary mt-0.5">
                      Enable S3-compatible object storage instead of local filesystem.
                    </p>
                  </div>
                  <MonolithToggle
                    id="s3-enabled"
                    checked={s3.enabled}
                    onChange={() => updateS3('enabled', !s3.enabled)}
                    label="Use S3 Storage"
                  />
                </div>

                {s3.enabled && (
                  <>
                    {/* Bucket */}
                    <div>
                      <FieldLabel htmlFor="s3-bucket" required>Bucket</FieldLabel>
                      <MonolithInput
                        id="s3-bucket"
                        name="bucket"
                        value={s3.bucket}
                        onChange={(e) => updateS3('bucket', e.target.value)}
                        placeholder="my-bucket"
                        disabled={savingStorage}
                        error={storageFieldErrors.bucket}
                        errorId="s3-bucket-error"
                        autoComplete="off"
                        required
                      />
                    </div>

                    {/* Region + Endpoint */}
                    <div className="grid grid-cols-2 gap-6">
                      <div>
                        <FieldLabel htmlFor="s3-region" required>Region</FieldLabel>
                        <MonolithInput
                          id="s3-region"
                          name="region"
                          value={s3.region}
                          onChange={(e) => updateS3('region', e.target.value)}
                          placeholder="us-east-1"
                          disabled={savingStorage}
                          error={storageFieldErrors.region}
                          errorId="s3-region-error"
                          autoComplete="off"
                          required
                        />
                      </div>
                      <div>
                        <FieldLabel htmlFor="s3-endpoint">Endpoint</FieldLabel>
                        <MonolithInput
                          id="s3-endpoint"
                          name="endpoint"
                          value={s3.endpoint}
                          onChange={(e) => updateS3('endpoint', e.target.value)}
                          placeholder="https://s3.amazonaws.com"
                          disabled={savingStorage}
                          autoComplete="off"
                        />
                      </div>
                    </div>

                    {/* Access Key */}
                    <div>
                      <FieldLabel htmlFor="s3-access-key">Access Key</FieldLabel>
                      <MonolithInput
                        id="s3-access-key"
                        name="accessKey"
                        value={s3.accessKey}
                        onChange={(e) => updateS3('accessKey', e.target.value)}
                        placeholder="AKIAIOSFODNN7EXAMPLE"
                        disabled={savingStorage}
                        autoComplete="off"
                        spellCheck={false}
                      />
                    </div>

                    {/* Secret Key */}
                    <div>
                      <FieldLabel htmlFor="s3-secret-key">Secret Key</FieldLabel>
                      <MonolithInput
                        id="s3-secret-key"
                        name="secretKey"
                        type="password"
                        value={s3.secretKey}
                        onChange={(e) => updateS3('secretKey', e.target.value)}
                        placeholder="Leave blank to keep current secret key"
                        disabled={savingStorage}
                        autoComplete="new-password"
                      />
                      <p className="text-xs text-secondary mt-1">
                        Leave empty to keep the existing secret key.
                      </p>
                    </div>

                    {/* Force Path Style */}
                    <div className="flex items-center justify-between">
                      <div>
                        <label htmlFor="s3-force-path-style" className="label-md text-on-surface">
                          Force Path Style
                        </label>
                        <p className="text-xs text-secondary mt-0.5">
                          Use path-style URLs (required for some S3-compatible providers).
                        </p>
                      </div>
                      <MonolithToggle
                        id="s3-force-path-style"
                        checked={s3.forcePathStyle}
                        onChange={() => updateS3('forcePathStyle', !s3.forcePathStyle)}
                        label="Force path style"
                      />
                    </div>
                  </>
                )}
              </div>
            </section>

            {/* Save Storage button */}
            <div className="flex justify-end">
              <button
                type="submit"
                disabled={savingStorage}
                className="label-md bg-on-surface text-surface px-12 py-4 tracking-[0.3em] uppercase
                  hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2
                  disabled:cursor-not-allowed disabled:opacity-50
                  flex items-center"
              >
                {savingStorage ? (
                  <>
                    <Spinner />
                    Saving...
                  </>
                ) : (
                  'Save Storage Settings'
                )}
              </button>
            </div>
          </div>
        </form>

        {/* ── 04. Test Email ───────────────────────────────── */}
        {smtp.enabled && (
          <>
            <SectionDivider />
            <div className="mt-24 space-y-24">
              <section className="grid grid-cols-1 lg:grid-cols-12 gap-8">
                <div className="lg:col-span-4">
                  <h3 className="label-md tracking-[0.2em] text-on-surface">04. Test Email</h3>
                  <p className="text-sm text-secondary mt-2 leading-relaxed">
                    Verify your SMTP configuration by sending a test message.
                  </p>
                </div>
                <div className="lg:col-span-8 space-y-6">
                  {testEmailError && <MonolithAlert type="error">{testEmailError}</MonolithAlert>}
                  {testEmailSuccess && <MonolithAlert type="success">{testEmailSuccess}</MonolithAlert>}

                  <div className="flex gap-4">
                    <div className="flex-1">
                      <label htmlFor="test-email-to" className="sr-only">
                        Recipient email
                      </label>
                      <MonolithInput
                        id="test-email-to"
                        name="testEmailTo"
                        type="email"
                        value={testEmail}
                        onChange={(e) => setTestEmail(e.target.value)}
                        placeholder="recipient@example.com"
                        disabled={testingSend}
                        autoComplete="email"
                        spellCheck={false}
                      />
                    </div>
                    <button
                      type="button"
                      onClick={handleTestEmail}
                      disabled={testingSend}
                      className="label-md border border-primary text-on-surface px-6 py-3 tracking-[0.15em] uppercase
                        hover:bg-on-surface hover:text-surface
                        focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2
                        disabled:cursor-not-allowed disabled:opacity-50
                        flex shrink-0 items-center"
                    >
                      {testingSend ? (
                        <>
                          <Spinner />
                          Sending...
                        </>
                      ) : (
                        'Send Test Email'
                      )}
                    </button>
                  </div>
                </div>
              </section>
            </div>
          </>
        )}
      </div>
    </DashboardLayout>
  );
}
