import type { CollectionType } from '../../lib/api/types';

interface ApiPreviewProps {
  collectionName: string;
  collectionType: CollectionType;
}

interface Endpoint {
  method: string;
  path: string;
  description: string;
}

function getEndpoints(name: string, type: CollectionType): Endpoint[] {
  const safeName = name || ':collection';
  const base: Endpoint[] = [
    { method: 'GET', path: `/api/collections/${safeName}/records`, description: 'List records' },
    { method: 'POST', path: `/api/collections/${safeName}/records`, description: 'Create record' },
    { method: 'GET', path: `/api/collections/${safeName}/records/:id`, description: 'View record' },
    { method: 'PATCH', path: `/api/collections/${safeName}/records/:id`, description: 'Update record' },
    { method: 'DELETE', path: `/api/collections/${safeName}/records/:id`, description: 'Delete record' },
  ];

  if (type === 'auth') {
    base.push(
      { method: 'POST', path: `/api/collections/${safeName}/auth-with-password`, description: 'Auth with password' },
      { method: 'POST', path: `/api/collections/${safeName}/auth-refresh`, description: 'Refresh auth token' },
      { method: 'POST', path: `/api/collections/${safeName}/request-otp`, description: 'Request OTP' },
      { method: 'POST', path: `/api/collections/${safeName}/auth-with-otp`, description: 'Auth with OTP' },
      { method: 'POST', path: `/api/collections/${safeName}/request-verification`, description: 'Request verification' },
      { method: 'POST', path: `/api/collections/${safeName}/request-password-reset`, description: 'Request password reset' },
    );
  }

  return base;
}

function MethodBadge({ method }: { method: string }) {
  const isFilled = method === 'POST' || method === 'DELETE';
  return (
    <span
      className={`inline-flex w-16 items-center justify-center px-1.5 py-0.5 text-label-sm font-mono ${
        isFilled
          ? 'bg-primary text-on-primary'
          : 'border border-primary text-on-surface'
      }`}
    >
      {method}
    </span>
  );
}

export function ApiPreview({ collectionName, collectionType }: ApiPreviewProps) {
  const endpoints = getEndpoints(collectionName, collectionType);

  return (
    <div className="border border-primary bg-background" data-testid="api-preview">
      <div className="border-b border-primary bg-primary px-4 py-3">
        <h3 className="text-label-md text-on-primary">API Endpoints</h3>
        <p className="mt-0.5 text-xs text-on-primary opacity-80">
          Auto-generated for this collection.
        </p>
      </div>
      <div>
        {endpoints.map((ep, i) => (
          <div key={i} className={`flex items-center gap-3 px-4 py-2.5 ${i > 0 ? 'border-t border-outline-variant' : ''}`}>
            <MethodBadge method={ep.method} />
            <code className="min-w-0 flex-1 truncate font-mono text-xs text-on-surface">{ep.path}</code>
            <span className="hidden text-xs text-secondary sm:inline">{ep.description}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
