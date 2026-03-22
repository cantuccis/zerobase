import type { AuthOptions } from '../../lib/api/types';

// ── Defaults ────────────────────────────────────────────────────────────────

export const DEFAULT_AUTH_OPTIONS: AuthOptions = {
  allowEmailAuth: true,
  allowOauth2Auth: false,
  allowOtpAuth: false,
  requireEmail: true,
  mfaEnabled: false,
  mfaDuration: 0,
  minPasswordLength: 8,
  identityFields: ['email'],
  manageRule: null,
};

// ── Styles ──────────────────────────────────────────────────────────────────

const TOGGLE_CLASS =
  'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500 focus:ring-offset-2';

const TOGGLE_KNOB_CLASS =
  'pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white dark:bg-gray-800 shadow ring-0 transition duration-200 ease-in-out';

const INPUT_CLASS =
  'w-full rounded-md border border-gray-300 dark:border-gray-600 px-3 py-1.5 text-sm placeholder-gray-400 dark:placeholder-gray-500 focus:border-blue-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-blue-500';

// ── Props ───────────────────────────────────────────────────────────────────

interface AuthSettingsEditorProps {
  authOptions: AuthOptions;
  onChange: (options: AuthOptions) => void;
}

// ── Component ───────────────────────────────────────────────────────────────

export function AuthSettingsEditor({ authOptions, onChange }: AuthSettingsEditorProps) {
  function update(partial: Partial<AuthOptions>) {
    onChange({ ...authOptions, ...partial });
  }

  return (
    <section className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-6" data-testid="auth-settings-editor">
      <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100">Auth Settings</h3>
      <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
        Configure authentication methods and security policies for this collection.
      </p>

      {/* ── Auth Methods ──────────────────────────────────────────────── */}
      <div className="mt-5">
        <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300">Authentication Methods</h4>
        <div className="mt-3 space-y-4">
          {/* Email/Password Auth */}
          <ToggleRow
            id="allow-email-auth"
            label="Email/Password"
            description="Allow users to authenticate with email and password."
            checked={authOptions.allowEmailAuth}
            onChange={(checked) => update({ allowEmailAuth: checked })}
          />

          {/* OAuth2 Auth */}
          <ToggleRow
            id="allow-oauth2-auth"
            label="OAuth2"
            description="Allow authentication via OAuth2 providers (Google, Microsoft, etc.)."
            checked={authOptions.allowOauth2Auth}
            onChange={(checked) => update({ allowOauth2Auth: checked })}
          />

          {/* OTP Auth */}
          <ToggleRow
            id="allow-otp-auth"
            label="OTP (One-Time Password)"
            description="Allow authentication via one-time passwords sent by email."
            checked={authOptions.allowOtpAuth}
            onChange={(checked) => update({ allowOtpAuth: checked })}
          />

          {/* MFA */}
          <ToggleRow
            id="mfa-enabled"
            label="Multi-Factor Authentication (MFA)"
            description="Require a second factor for authentication."
            checked={authOptions.mfaEnabled}
            onChange={(checked) => update({ mfaEnabled: checked })}
          />
        </div>
      </div>

      {/* ── MFA Duration (shown when MFA enabled) ────────────────────── */}
      {authOptions.mfaEnabled && (
        <div className="mt-4 ml-6 rounded-md border border-gray-100 dark:border-gray-700 bg-gray-50 dark:bg-gray-900 p-3" data-testid="mfa-duration-section">
          <label htmlFor="mfa-duration" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
            MFA Session Duration (seconds)
          </label>
          <p className="mt-0.5 text-xs text-gray-500 dark:text-gray-400">
            How long a partial MFA token is valid. Use 0 for system default.
          </p>
          <input
            id="mfa-duration"
            type="number"
            min={0}
            value={authOptions.mfaDuration}
            onChange={(e) => update({ mfaDuration: Math.max(0, parseInt(e.target.value, 10) || 0) })}
            className={`${INPUT_CLASS} mt-1.5 w-32`}
            data-testid="mfa-duration"
          />
        </div>
      )}

      {/* ── Password Requirements ─────────────────────────────────────── */}
      <div className="mt-6 border-t border-gray-100 dark:border-gray-700 pt-5">
        <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300">Password Requirements</h4>
        <div className="mt-3">
          <label htmlFor="min-password-length" className="block text-sm text-gray-600 dark:text-gray-400">
            Minimum Password Length
          </label>
          <input
            id="min-password-length"
            type="number"
            min={1}
            max={72}
            value={authOptions.minPasswordLength}
            onChange={(e) => update({ minPasswordLength: Math.max(1, parseInt(e.target.value, 10) || 8) })}
            className={`${INPUT_CLASS} mt-1 w-32`}
            data-testid="min-password-length"
          />
          <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">Must be at least 1. Recommended: 8 or higher.</p>
        </div>
      </div>

      {/* ── Email Policy ──────────────────────────────────────────────── */}
      <div className="mt-6 border-t border-gray-100 dark:border-gray-700 pt-5">
        <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300">Email Policy</h4>
        <div className="mt-3">
          <ToggleRow
            id="require-email"
            label="Require Email Verification"
            description="Users must verify their email before they can authenticate."
            checked={authOptions.requireEmail}
            onChange={(checked) => update({ requireEmail: checked })}
          />
        </div>
      </div>

      {/* ── Identity Fields ───────────────────────────────────────────── */}
      <div className="mt-6 border-t border-gray-100 dark:border-gray-700 pt-5">
        <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300">Identity Fields</h4>
        <p className="mt-0.5 text-xs text-gray-500 dark:text-gray-400">
          Fields that can be used as login identity (comma-separated).
        </p>
        <input
          type="text"
          value={authOptions.identityFields.join(', ')}
          onChange={(e) =>
            update({
              identityFields: e.target.value
                .split(',')
                .map((s) => s.trim())
                .filter(Boolean),
            })
          }
          placeholder="email"
          className={`${INPUT_CLASS} mt-1.5`}
          data-testid="identity-fields"
        />
      </div>
    </section>
  );
}

// ── Toggle row sub-component ────────────────────────────────────────────────

interface ToggleRowProps {
  id: string;
  label: string;
  description: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

function ToggleRow({ id, label, description, checked, onChange }: ToggleRowProps) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div className="min-w-0">
        <label htmlFor={id} className="text-sm font-medium text-gray-900 dark:text-gray-100">
          {label}
        </label>
        <p className="text-xs text-gray-500 dark:text-gray-400">{description}</p>
      </div>
      <button
        type="button"
        role="switch"
        id={id}
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={`${TOGGLE_CLASS} ${checked ? 'bg-blue-600' : 'bg-gray-200 dark:bg-gray-600'}`}
        data-testid={id}
      >
        <span className={`${TOGGLE_KNOB_CLASS} ${checked ? 'translate-x-5' : 'translate-x-0'}`} />
      </button>
    </div>
  );
}
