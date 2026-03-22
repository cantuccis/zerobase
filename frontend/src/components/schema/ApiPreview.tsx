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

const METHOD_COLORS: Record<string, string> = {
  GET: 'bg-green-100 dark:bg-green-900/20 text-green-700 dark:text-green-400',
  POST: 'bg-blue-100 dark:bg-blue-900/20 text-blue-700 dark:text-blue-400',
  PATCH: 'bg-yellow-100 dark:bg-yellow-900/20 text-yellow-700 dark:text-yellow-400',
  DELETE: 'bg-red-100 dark:bg-red-900/20 text-red-700 dark:text-red-400',
};

export function ApiPreview({ collectionName, collectionType }: ApiPreviewProps) {
  const endpoints = getEndpoints(collectionName, collectionType);

  return (
    <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800" data-testid="api-preview">
      <div className="border-b border-gray-200 dark:border-gray-700 px-4 py-3">
        <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">API Endpoints Preview</h3>
        <p className="mt-0.5 text-xs text-gray-500 dark:text-gray-400">
          These endpoints will be auto-generated for this collection.
        </p>
      </div>
      <div className="divide-y divide-gray-100 dark:divide-gray-700">
        {endpoints.map((ep, i) => (
          <div key={i} className="flex items-center gap-3 px-4 py-2">
            <span
              className={`inline-flex w-16 items-center justify-center rounded px-1.5 py-0.5 text-xs font-semibold ${METHOD_COLORS[ep.method]}`}
            >
              {ep.method}
            </span>
            <code className="min-w-0 flex-1 truncate text-xs text-gray-700 dark:text-gray-300">{ep.path}</code>
            <span className="hidden text-xs text-gray-400 dark:text-gray-500 sm:inline">{ep.description}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
