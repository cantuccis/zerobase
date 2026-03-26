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
  'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer border-2 border-primary';

const TOGGLE_KNOB_CLASS =
  'pointer-events-none inline-block h-5 w-5 transform bg-on-primary ring-0';

const INPUT_CLASS =
  'w-full border border-primary px-3 py-1.5 text-sm text-on-surface bg-background placeholder-outline focus:outline-none focus:border-primary';

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
    <div data-testid="auth-settings-editor">
      {/* ── Auth Methods ──────────────────────────────────────────────── */}
      <div>
        <h4 className="text-label-md text-on-surface-variant mb-3">Authentication Methods</h4>
        <div className="space-y-4">
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
        <div className="mt-4 ml-6 border border-outline-variant bg-surface-container-low p-3" data-testid="mfa-duration-section">
          <label htmlFor="mfa-duration" className="text-label-sm text-on-surface-variant block mb-1">
            MFA Session Duration (seconds)
          </label>
          <p className="text-xs text-secondary mb-1.5">
            How long a partial MFA token is valid. Use 0 for system default.
          </p>
          <input
            id="mfa-duration"
            type="number"
            min={0}
            value={authOptions.mfaDuration}
            onChange={(e) => update({ mfaDuration: Math.max(0, parseInt(e.target.value, 10) || 0) })}
            className={`${INPUT_CLASS} w-32`}
            data-testid="mfa-duration"
          />
        </div>
      )}

      {/* ── Password Requirements ─────────────────────────────────────── */}
      <div className="mt-6 border-t border-primary pt-5">
        <h4 className="text-label-md text-on-surface-variant mb-3">Password Requirements</h4>
        <div>
          <label htmlFor="min-password-length" className="text-label-sm text-on-surface-variant block mb-1">
            Minimum Password Length
          </label>
          <input
            id="min-password-length"
            type="number"
            min={1}
            max={72}
            value={authOptions.minPasswordLength}
            onChange={(e) => update({ minPasswordLength: Math.max(1, parseInt(e.target.value, 10) || 8) })}
            className={`${INPUT_CLASS} w-32`}
            data-testid="min-password-length"
          />
          <p className="mt-1 text-xs text-secondary">Must be at least 1. Recommended: 8 or higher.</p>
        </div>
      </div>

      {/* ── Email Policy ──────────────────────────────────────────────── */}
      <div className="mt-6 border-t border-primary pt-5">
        <h4 className="text-label-md text-on-surface-variant mb-3">Email Policy</h4>
        <ToggleRow
          id="require-email"
          label="Require Email Verification"
          description="Users must verify their email before they can authenticate."
          checked={authOptions.requireEmail}
          onChange={(checked) => update({ requireEmail: checked })}
        />
      </div>

      {/* ── Identity Fields ───────────────────────────────────────────── */}
      <div className="mt-6 border-t border-primary pt-5">
        <label htmlFor="auth-identity-fields" className="text-label-md text-on-surface-variant mb-2 block">Identity Fields</label>
        <p className="text-xs text-secondary mb-1.5">
          Fields that can be used as login identity (comma-separated).
        </p>
        <input
          id="auth-identity-fields"
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
          className={INPUT_CLASS}
          data-testid="identity-fields"
        />
      </div>
    </div>
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
        <label htmlFor={id} className="text-sm font-semibold text-on-surface">
          {label}
        </label>
        <p className="text-xs text-secondary">{description}</p>
      </div>
      <button
        type="button"
        role="switch"
        id={id}
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={`${TOGGLE_CLASS} ${checked ? 'bg-primary' : 'bg-surface-container'}`}
        data-testid={id}
      >
        <span className={`${TOGGLE_KNOB_CLASS} ${checked ? 'translate-x-5' : 'translate-x-0'}`} />
      </button>
    </div>
  );
}
