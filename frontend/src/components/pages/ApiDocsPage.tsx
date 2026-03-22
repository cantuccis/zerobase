import { useState, useEffect, useCallback, useMemo } from 'react';
import { DashboardLayout } from '../DashboardLayout';
import { client } from '../../lib/auth/client';
import { ApiError } from '../../lib/api';
import {
  generateEndpointDocs,
  generateCurlExample,
  formatAccessRule,
  FILTER_OPERATORS,
} from '../../lib/api-docs';
import type { Collection, CollectionType } from '../../lib/api/types';
import type { EndpointDoc, QueryParamDoc } from '../../lib/api-docs';

// ── Types ────────────────────────────────────────────────────────────────────

interface PageState {
  collections: Collection[];
  loading: boolean;
  error: string | null;
}

// ── Constants ────────────────────────────────────────────────────────────────

const METHOD_COLORS: Record<string, string> = {
  GET: 'bg-green-100 dark:bg-green-900/20 text-green-700 dark:text-green-400',
  POST: 'bg-blue-100 dark:bg-blue-900/20 text-blue-700 dark:text-blue-400',
  PATCH: 'bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400',
  DELETE: 'bg-red-100 dark:bg-red-900/20 text-red-700 dark:text-red-400',
};

const TYPE_LABELS: Record<CollectionType, string> = {
  base: 'Base',
  auth: 'Auth',
  view: 'View',
};

const TYPE_COLORS: Record<CollectionType, string> = {
  base: 'bg-blue-100 dark:bg-blue-900/20 text-blue-800 dark:text-blue-300',
  auth: 'bg-green-100 dark:bg-green-900/20 text-green-800 dark:text-green-300',
  view: 'bg-purple-100 dark:bg-purple-900/20 text-purple-800 dark:text-purple-300',
};

// ── Copy button ──────────────────────────────────────────────────────────────

function CopyButton({ text, label }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Fallback for environments without clipboard API
      const textarea = document.createElement('textarea');
      textarea.value = text;
      textarea.style.position = 'fixed';
      textarea.style.opacity = '0';
      document.body.appendChild(textarea);
      textarea.select();
      document.execCommand('copy');
      document.body.removeChild(textarea);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  }, [text]);

  return (
    <button
      type="button"
      onClick={handleCopy}
      className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs font-medium text-gray-500 dark:text-gray-400 transition-colors hover:bg-gray-100 dark:hover:bg-gray-700 hover:text-gray-700 dark:hover:text-gray-300"
      aria-label={label ?? 'Copy to clipboard'}
    >
      {copied ? (
        <>
          <svg className="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <polyline points="20 6 9 17 4 12" />
          </svg>
          Copied
        </>
      ) : (
        <>
          <svg className="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
          </svg>
          Copy
        </>
      )}
    </button>
  );
}

// ── Code block ───────────────────────────────────────────────────────────────

function CodeBlock({ code, language }: { code: string; language?: string }) {
  return (
    <div className="group relative" data-testid="code-block">
      <div className="absolute right-2 top-2 opacity-0 transition-opacity group-hover:opacity-100">
        <CopyButton text={code} label={`Copy ${language ?? 'code'}`} />
      </div>
      <pre className="overflow-x-auto rounded-md bg-gray-900 p-4 text-sm leading-relaxed text-gray-100">
        <code>{code}</code>
      </pre>
    </div>
  );
}

// ── Endpoint card ────────────────────────────────────────────────────────────

function EndpointCard({
  endpoint,
  baseUrl,
  defaultExpanded,
}: {
  endpoint: EndpointDoc;
  baseUrl: string;
  defaultExpanded?: boolean;
}) {
  const [expanded, setExpanded] = useState(defaultExpanded ?? false);
  const rule = formatAccessRule(endpoint.accessRule);
  const curl = generateCurlExample(endpoint, baseUrl);

  return (
    <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800" data-testid="endpoint-card">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-gray-50 dark:hover:bg-gray-700"
        aria-expanded={expanded}
        aria-label={`${endpoint.method} ${endpoint.path}`}
      >
        <span
          className={`inline-flex w-16 shrink-0 items-center justify-center rounded px-1.5 py-0.5 text-xs font-bold ${METHOD_COLORS[endpoint.method]}`}
        >
          {endpoint.method}
        </span>
        <code className="min-w-0 flex-1 truncate text-sm font-medium text-gray-800 dark:text-gray-200">
          {endpoint.path}
        </code>
        <span className="hidden text-sm text-gray-500 dark:text-gray-400 sm:inline">{endpoint.description}</span>
        <svg
          className={`h-4 w-4 shrink-0 text-gray-400 dark:text-gray-500 transition-transform ${expanded ? 'rotate-180' : ''}`}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>

      {expanded && (
        <div className="border-t border-gray-100 dark:border-gray-700 px-4 py-4 space-y-4" data-testid="endpoint-details">
          {/* Description */}
          <p className="text-sm text-gray-600 dark:text-gray-400">{endpoint.details}</p>

          {/* Access rule */}
          <div className="flex items-center gap-2">
            <span className="text-xs font-medium text-gray-500 dark:text-gray-400">Access:</span>
            <span className={`inline-flex rounded px-2 py-0.5 text-xs font-medium ${rule.color}`}>
              {rule.label}
            </span>
          </div>

          {/* Query parameters */}
          {endpoint.queryParams.length > 0 && (
            <div>
              <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">
                Query Parameters
              </h4>
              <div className="overflow-x-auto rounded-md border border-gray-200 dark:border-gray-700">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-900">
                      <th scope="col" className="px-3 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400">Param</th>
                      <th scope="col" className="px-3 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400">Type</th>
                      <th scope="col" className="px-3 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400">Description</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-gray-100 dark:divide-gray-700">
                    {endpoint.queryParams.map((param) => (
                      <tr key={param.name}>
                        <td className="px-3 py-2">
                          <code className="rounded bg-gray-100 dark:bg-gray-700 px-1.5 py-0.5 text-xs font-medium text-gray-800 dark:text-gray-200">
                            {param.name}
                          </code>
                        </td>
                        <td className="px-3 py-2 text-xs text-gray-500 dark:text-gray-400">{param.type}</td>
                        <td className="px-3 py-2 text-xs text-gray-600 dark:text-gray-400">{param.description}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {/* Request example */}
          {endpoint.requestExample && (
            <div>
              <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">
                Request Body
              </h4>
              <CodeBlock code={endpoint.requestExample} language="JSON" />
            </div>
          )}

          {/* Response example */}
          <div>
            <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">
              Response
            </h4>
            <CodeBlock code={endpoint.responseExample} language="JSON" />
          </div>

          {/* Curl example */}
          <div>
            <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">
              cURL Example
            </h4>
            <CodeBlock code={curl} language="bash" />
          </div>
        </div>
      )}
    </div>
  );
}

// ── Filter reference ─────────────────────────────────────────────────────────

function FilterReference() {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800" data-testid="filter-reference">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center justify-between px-4 py-3 text-left transition-colors hover:bg-gray-50 dark:hover:bg-gray-700"
        aria-expanded={expanded}
      >
        <div className="flex items-center gap-2">
          <svg className="h-4 w-4 text-gray-500 dark:text-gray-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3" />
          </svg>
          <span className="text-sm font-semibold text-gray-900 dark:text-gray-100">Filter Syntax Reference</span>
        </div>
        <svg
          className={`h-4 w-4 text-gray-400 dark:text-gray-500 transition-transform ${expanded ? 'rotate-180' : ''}`}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>

      {expanded && (
        <div className="border-t border-gray-100 dark:border-gray-700 px-4 py-4 space-y-4" data-testid="filter-details">
          <p className="text-sm text-gray-600 dark:text-gray-400">
            Use filter expressions with the <code className="rounded bg-gray-100 dark:bg-gray-700 px-1 py-0.5 text-xs">filter</code> query
            parameter. Combine conditions with <code className="rounded bg-gray-100 dark:bg-gray-700 px-1 py-0.5 text-xs">&&</code> (AND)
            and <code className="rounded bg-gray-100 dark:bg-gray-700 px-1 py-0.5 text-xs">||</code> (OR). Group with parentheses.
          </p>

          <div className="overflow-x-auto rounded-md border border-gray-200 dark:border-gray-700">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-900">
                  <th scope="col" className="px-3 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400">Operator</th>
                  <th scope="col" className="px-3 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400">Description</th>
                  <th scope="col" className="px-3 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400">Example</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100 dark:divide-gray-700">
                {FILTER_OPERATORS.map((op) => (
                  <tr key={op.operator}>
                    <td className="px-3 py-2">
                      <code className="rounded bg-gray-100 dark:bg-gray-700 px-1.5 py-0.5 text-xs font-bold text-gray-800 dark:text-gray-200">
                        {op.operator}
                      </code>
                    </td>
                    <td className="px-3 py-2 text-xs text-gray-600 dark:text-gray-400">{op.description}</td>
                    <td className="px-3 py-2">
                      <code className="text-xs text-gray-700 dark:text-gray-300">{op.example}</code>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div>
            <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">
              Examples
            </h4>
            <CodeBlock
              code={[
                '# Simple equality',
                '?filter=(status="active")',
                '',
                '# Multiple conditions (AND)',
                '?filter=(status="active" && created > "2024-01-01")',
                '',
                '# Multiple conditions (OR)',
                '?filter=(role="admin" || role="editor")',
                '',
                '# Nested groups',
                '?filter=(status="active" && (role="admin" || role="editor"))',
                '',
                '# Contains (like)',
                '?filter=(title ~ "hello")',
              ].join('\n')}
              language="text"
            />
          </div>
        </div>
      )}
    </div>
  );
}

// ── Collection selector ──────────────────────────────────────────────────────

function CollectionSelector({
  collections,
  selectedId,
  onSelect,
}: {
  collections: Collection[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}) {
  return (
    <div className="space-y-1" data-testid="collection-selector">
      {collections.map((col) => (
        <button
          key={col.id}
          type="button"
          onClick={() => onSelect(col.id)}
          className={`flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm transition-colors ${
            selectedId === col.id
              ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-400 font-medium'
              : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'
          }`}
          aria-current={selectedId === col.id ? 'true' : undefined}
          aria-label={`View API docs for ${col.name}`}
        >
          <span className="flex-1 truncate">{col.name}</span>
          <span
            className={`inline-flex shrink-0 items-center rounded-full px-2 py-0.5 text-xs font-medium ${TYPE_COLORS[col.type]}`}
          >
            {TYPE_LABELS[col.type]}
          </span>
        </button>
      ))}
    </div>
  );
}

// ── Main page ────────────────────────────────────────────────────────────────

export function ApiDocsPage() {
  const [state, setState] = useState<PageState>({
    collections: [],
    loading: true,
    error: null,
  });
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const baseUrl = typeof window !== 'undefined' ? window.location.origin : 'http://localhost:8090';

  const loadCollections = useCallback(async () => {
    setState((s) => ({ ...s, loading: true, error: null }));
    try {
      const resp = await client.listCollections();
      const collections = resp.items;
      setState({ collections, loading: false, error: null });
      // Auto-select first collection
      if (collections.length > 0 && !selectedId) {
        setSelectedId(collections[0].id);
      }
    } catch (err) {
      const message =
        err instanceof ApiError
          ? err.response.message
          : 'Unable to connect to the server. Please try again.';
      setState({ collections: [], loading: false, error: message });
    }
  }, []);

  useEffect(() => {
    loadCollections();
  }, [loadCollections]);

  const selectedCollection = useMemo(
    () => state.collections.find((c) => c.id === selectedId) ?? null,
    [state.collections, selectedId],
  );

  const endpoints = useMemo(
    () => (selectedCollection ? generateEndpointDocs(selectedCollection) : []),
    [selectedCollection],
  );

  return (
    <DashboardLayout currentPath="/_/docs" pageTitle="API Documentation">
      {/* Loading state */}
      {state.loading && (
        <div data-testid="loading-skeleton" className="space-y-4">
          <div className="h-8 w-48 animate-pulse rounded bg-gray-200 dark:bg-gray-600" />
          <div className="h-64 animate-pulse rounded-lg bg-gray-200 dark:bg-gray-600" />
        </div>
      )}

      {/* Error state */}
      {state.error && (
        <div role="alert" className="rounded-lg border border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30 p-4">
          <p className="text-sm text-red-700 dark:text-red-400">{state.error}</p>
          <button
            type="button"
            onClick={loadCollections}
            className="mt-2 text-sm font-medium text-red-600 dark:text-red-400 hover:text-red-500 dark:hover:text-red-400"
          >
            Retry
          </button>
        </div>
      )}

      {/* Empty state */}
      {!state.loading && !state.error && state.collections.length === 0 && (
        <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-8 text-center" data-testid="empty-state">
          <p className="text-gray-500 dark:text-gray-400">No collections found.</p>
          <p className="mt-1 text-sm text-gray-400 dark:text-gray-500">
            Create a collection first to see its API documentation.
          </p>
          <a
            href="/_/collections/new"
            className="mt-4 inline-flex items-center rounded-md bg-blue-600 px-3 py-2 text-sm font-medium text-white hover:bg-blue-700 dark:hover:bg-blue-600"
          >
            Create Collection
          </a>
        </div>
      )}

      {/* Main content */}
      {!state.loading && !state.error && state.collections.length > 0 && (
        <div className="flex gap-6">
          {/* Sidebar - collection list */}
          <div className="hidden w-56 shrink-0 lg:block">
            <div className="sticky top-0">
              <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">
                Collections
              </h3>
              <CollectionSelector
                collections={state.collections}
                selectedId={selectedId}
                onSelect={setSelectedId}
              />
            </div>
          </div>

          {/* Main content area */}
          <div className="min-w-0 flex-1 space-y-6">
            {/* Mobile collection selector */}
            <div className="lg:hidden">
              <label htmlFor="collection-select" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                Collection
              </label>
              <select
                id="collection-select"
                value={selectedId ?? ''}
                onChange={(e) => setSelectedId(e.target.value)}
                className="mt-1 w-full rounded-md border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 px-3 py-2 text-sm focus:border-blue-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-blue-500"
              >
                {state.collections.map((col) => (
                  <option key={col.id} value={col.id}>
                    {col.name} ({TYPE_LABELS[col.type]})
                  </option>
                ))}
              </select>
            </div>

            {selectedCollection && (
              <>
                {/* Header */}
                <div>
                  <div className="flex items-center gap-3">
                    <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100" data-testid="selected-collection-name">
                      {selectedCollection.name}
                    </h3>
                    <span
                      className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${TYPE_COLORS[selectedCollection.type]}`}
                    >
                      {TYPE_LABELS[selectedCollection.type]}
                    </span>
                  </div>
                  <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
                    {endpoints.length} endpoint{endpoints.length !== 1 ? 's' : ''} available
                    {' \u00b7 '}
                    {selectedCollection.fields.length} field{selectedCollection.fields.length !== 1 ? 's' : ''}
                  </p>
                </div>

                {/* Fields summary */}
                {selectedCollection.fields.length > 0 && (
                  <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800" data-testid="fields-summary">
                    <div className="border-b border-gray-100 dark:border-gray-700 px-4 py-3">
                      <h4 className="text-sm font-semibold text-gray-900 dark:text-gray-100">Fields</h4>
                    </div>
                    <div className="overflow-x-auto">
                      <table className="w-full text-sm">
                        <thead>
                          <tr className="border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-900">
                            <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400">Name</th>
                            <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400">Type</th>
                            <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400">Required</th>
                            <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400">Unique</th>
                          </tr>
                        </thead>
                        <tbody className="divide-y divide-gray-100 dark:divide-gray-700">
                          {selectedCollection.fields.map((field) => (
                            <tr key={field.id}>
                              <td className="px-4 py-2">
                                <code className="text-xs font-medium text-gray-800 dark:text-gray-200">{field.name}</code>
                              </td>
                              <td className="px-4 py-2">
                                <span className="inline-flex rounded bg-gray-100 dark:bg-gray-700 px-1.5 py-0.5 text-xs text-gray-600 dark:text-gray-400">
                                  {field.type.type}
                                </span>
                              </td>
                              <td className="px-4 py-2 text-xs text-gray-500 dark:text-gray-400">{field.required ? 'Yes' : 'No'}</td>
                              <td className="px-4 py-2 text-xs text-gray-500 dark:text-gray-400">{field.unique ? 'Yes' : 'No'}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </div>
                )}

                {/* Endpoints */}
                <div>
                  <h4 className="mb-3 text-sm font-semibold text-gray-900 dark:text-gray-100">Endpoints</h4>
                  <div className="space-y-3" data-testid="endpoints-list">
                    {endpoints.map((ep, i) => (
                      <EndpointCard key={i} endpoint={ep} baseUrl={baseUrl} />
                    ))}
                  </div>
                </div>

                {/* Filter reference */}
                <FilterReference />
              </>
            )}
          </div>
        </div>
      )}
    </DashboardLayout>
  );
}
