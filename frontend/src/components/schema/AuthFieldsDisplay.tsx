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
    <div data-testid="auth-fields-display" className="rounded-lg border border-green-200 dark:border-green-800 bg-green-50 dark:bg-green-900/30 p-4">
      <div className="mb-3 flex items-center gap-2">
        <svg className="h-5 w-5 text-green-600 dark:text-green-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
          <path d="M7 11V7a5 5 0 0 1 10 0v4" />
        </svg>
        <h4 className="text-sm font-semibold text-green-800 dark:text-green-200">Auth System Fields</h4>
        <span className="rounded-full bg-green-200 dark:bg-green-800 px-2 py-0.5 text-xs font-medium text-green-800 dark:text-green-200">
          Auto-included
        </span>
      </div>
      <p className="mb-3 text-xs text-green-700 dark:text-green-400">
        These fields are automatically managed by the auth system and cannot be removed or renamed.
      </p>
      <div className="space-y-2">
        {AUTH_SYSTEM_FIELDS.map((field) => (
          <div
            key={field.name}
            className="flex items-center justify-between rounded-md border border-green-200 dark:border-green-800 bg-white dark:bg-gray-800 px-3 py-2"
            data-testid={`auth-field-${field.name}`}
          >
            <div className="flex items-center gap-3">
              <code className="text-sm font-medium text-gray-900 dark:text-gray-100">{field.name}</code>
              <span className="rounded bg-gray-100 dark:bg-gray-700 px-1.5 py-0.5 text-xs font-medium text-gray-600 dark:text-gray-400">
                {field.type}
              </span>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-xs text-gray-500 dark:text-gray-400">{field.description}</span>
              <svg className="h-4 w-4 text-gray-300 dark:text-gray-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" data-testid={`auth-field-lock-${field.name}`}>
                <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
                <path d="M7 11V7a5 5 0 0 1 10 0v4" />
              </svg>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export { AUTH_SYSTEM_FIELDS };
export type { AuthSystemField };
