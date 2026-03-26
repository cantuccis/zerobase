import type { CollectionType } from '../../lib/api/types';

// ── Auth system fields ──────────────────────────────────────────────────────

interface AuthSystemField {
  name: string;
  type: string;
  description: string;
}

const AUTH_SYSTEM_FIELDS: AuthSystemField[] = [
  { name: 'email', type: 'email', description: 'User email address' },
  { name: 'emailVisibility', type: 'bool', description: 'Controls whether email is visible to other users' },
  { name: 'verified', type: 'bool', description: 'Whether the email has been verified' },
  { name: 'password', type: 'password', description: 'Hashed password (never returned in API responses)' },
  { name: 'tokenKey', type: 'text', description: 'Per-user token invalidation key' },
];

// ── Props ───────────────────────────────────────────────────────────────────

interface AuthFieldsDisplayProps {
  collectionType: CollectionType;
}

// ── Component ───────────────────────────────────────────────────────────────

export function AuthFieldsDisplay({ collectionType }: AuthFieldsDisplayProps) {
  if (collectionType !== 'auth') return null;

  return (
    <div data-testid="auth-fields-display" className="border border-primary bg-surface-container-low p-4">
      <div className="mb-3 flex items-center gap-2">
        <span className="material-symbols-outlined text-lg text-on-surface" aria-hidden="true">lock</span>
        <h4 className="text-label-md text-on-surface">Auth System Fields</h4>
        <span className="border border-primary bg-primary text-on-primary px-2 py-0.5 text-label-sm">
          AUTO-INCLUDED
        </span>
      </div>
      <p className="mb-3 text-xs text-secondary">
        These fields are automatically managed by the auth system and cannot be removed or renamed.
      </p>
      <div className="space-y-0">
        {AUTH_SYSTEM_FIELDS.map((field) => (
          <div
            key={field.name}
            className="flex items-center justify-between border border-outline-variant bg-background px-3 py-2 -mt-px first:mt-0"
            data-testid={`auth-field-${field.name}`}
          >
            <div className="flex items-center gap-3">
              <code className="text-sm font-semibold text-on-surface font-mono">{field.name}</code>
              <span className="border border-outline-variant px-1.5 py-0.5 text-label-sm text-secondary">
                {field.type}
              </span>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-xs text-secondary">{field.description}</span>
              <span className="material-symbols-outlined text-base text-outline" aria-hidden="true" data-testid={`auth-field-lock-${field.name}`}>lock</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export { AUTH_SYSTEM_FIELDS };
export type { AuthSystemField };
