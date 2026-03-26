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

const METHOD_STYLES: Record<string, string> = {
  GET: 'border border-primary dark:border-primary bg-transparent text-on-surface dark:text-on-surface',
  POST: 'border border-primary dark:border-primary bg-primary dark:bg-primary text-on-primary dark:text-on-primary',
  PATCH: 'border border-primary dark:border-primary bg-surface-container-high dark:bg-surface-container-high text-on-surface dark:text-on-surface',
  DELETE: 'border border-error dark:border-error bg-transparent text-error dark:text-error',
};

const TYPE_LABELS: Record<CollectionType, string> = {
  base: 'BASE',
  auth: 'AUTH',
  view: 'VIEW',
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
      className="inline-flex items-center gap-1 border border-primary dark:border-primary px-2 py-1 text-label-sm text-on-surface-variant dark:text-on-surface-variant hover:bg-primary hover:text-on-primary dark:hover:bg-primary dark:hover:text-on-primary"
      aria-label={label ?? 'Copy to clipboard'}
    >
      {copied ? (
        <>
          <svg className="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <polyline points="20 6 9 17 4 12" />
          </svg>
          COPIED
        </>
      ) : (
        <>
          <svg className="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <rect x="9" y="9" width="13" height="13" rx="0" ry="0" />
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
          </svg>
          COPY
        </>
      )}
    </button>
  );
}

// ── Code block ───────────────────────────────────────────────────────────────

function CodeBlock({ code, language }: { code: string; language?: string }) {
  return (
    <div className="group relative" data-testid="code-block">
      <div className="absolute right-2 top-2 opacity-0 group-hover:opacity-100 focus-within:opacity-100 transition-opacity-fast">
        <CopyButton text={code} label={`Copy ${language ?? 'code'}`} />
      </div>
      <pre className="overflow-x-auto border border-primary dark:border-primary bg-surface-container-low dark:bg-surface-container-low p-4 font-mono text-sm leading-relaxed text-on-surface dark:text-on-surface">
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
    <div className="border border-primary dark:border-primary" data-testid="endpoint-card">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center gap-3 px-4 py-3 text-left hover:bg-surface-container-low dark:hover:bg-surface-container transition-colors-fast"
        aria-expanded={expanded}
        aria-label={`${endpoint.method} ${endpoint.path}`}
      >
        <span
          className={`inline-flex w-[4.5rem] shrink-0 items-center justify-center px-2 py-0.5 font-mono text-xs font-bold uppercase ${METHOD_STYLES[endpoint.method]}`}
        >
          {endpoint.method}
        </span>
        <code className="min-w-0 flex-1 truncate font-mono text-sm font-medium text-on-surface dark:text-on-surface">
          {endpoint.path}
        </code>
        <span className="hidden text-sm text-on-surface-variant dark:text-on-surface-variant sm:inline">{endpoint.description}</span>
        <svg
          className={`h-4 w-4 shrink-0 text-on-surface-variant dark:text-on-surface-variant ${expanded ? 'rotate-180' : ''}`}
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
        <div className="border-t border-primary dark:border-primary px-4 py-4 space-y-4 animate-fade-in" data-testid="endpoint-details">
          {/* Description */}
          <p className="text-body-lg text-on-surface-variant dark:text-on-surface-variant">{endpoint.details}</p>

          {/* Access rule */}
          <div className="flex items-center gap-2">
            <span className="text-label-sm text-on-surface-variant dark:text-on-surface-variant">ACCESS</span>
            <span className="inline-flex border border-primary dark:border-primary px-2 py-0.5 text-label-sm text-on-surface dark:text-on-surface">
              {rule.label}
            </span>
          </div>

          {/* Query parameters */}
          {endpoint.queryParams.length > 0 && (
            <div>
              <h4 className="mb-2 text-label-md text-on-surface dark:text-on-surface">
                QUERY PARAMETERS
              </h4>
              <div className="overflow-x-auto border border-primary dark:border-primary">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="bg-primary dark:bg-primary">
                      <th scope="col" className="px-4 py-2 text-left text-label-sm text-on-primary dark:text-on-primary border-r border-on-primary/20">Param</th>
                      <th scope="col" className="px-4 py-2 text-left text-label-sm text-on-primary dark:text-on-primary border-r border-on-primary/20">Type</th>
                      <th scope="col" className="px-4 py-2 text-left text-label-sm text-on-primary dark:text-on-primary">Description</th>
                    </tr>
                  </thead>
                  <tbody>
                    {endpoint.queryParams.map((param, i) => (
                      <tr key={param.name} className={`border-b border-outline-variant dark:border-outline-variant ${i % 2 === 1 ? 'bg-surface-container-low dark:bg-surface-container-low' : ''}`}>
                        <td className="px-4 py-2 border-r border-outline-variant dark:border-outline-variant">
                          <code className="font-mono text-xs font-semibold text-on-surface dark:text-on-surface">
                            {param.name}
                          </code>
                        </td>
                        <td className="px-4 py-2 border-r border-outline-variant dark:border-outline-variant font-mono text-xs text-on-surface-variant dark:text-on-surface-variant">{param.type}</td>
                        <td className="px-4 py-2 text-xs text-on-surface-variant dark:text-on-surface-variant">{param.description}</td>
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
              <h4 className="mb-2 text-label-md text-on-surface dark:text-on-surface">
                REQUEST BODY
              </h4>
              <CodeBlock code={endpoint.requestExample} language="JSON" />
            </div>
          )}

          {/* Response example */}
          <div>
            <h4 className="mb-2 text-label-md text-on-surface dark:text-on-surface">
              RESPONSE
            </h4>
            <CodeBlock code={endpoint.responseExample} language="JSON" />
          </div>

          {/* Curl example */}
          <div>
            <h4 className="mb-2 text-label-md text-on-surface dark:text-on-surface">
              CURL EXAMPLE
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
    <div className="border border-primary dark:border-primary" data-testid="filter-reference">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center justify-between px-4 py-3 text-left hover:bg-surface-container-low dark:hover:bg-surface-container transition-colors-fast"
        aria-expanded={expanded}
      >
        <div className="flex items-center gap-2">
          <svg className="h-4 w-4 text-on-surface-variant dark:text-on-surface-variant" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3" />
          </svg>
          <span className="text-title-md text-on-surface dark:text-on-surface">Filter Syntax Reference</span>
        </div>
        <svg
          className={`h-4 w-4 text-on-surface-variant dark:text-on-surface-variant ${expanded ? 'rotate-180' : ''}`}
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
        <div className="border-t border-primary dark:border-primary px-4 py-4 space-y-4 animate-fade-in" data-testid="filter-details">
          <p className="text-body-lg text-on-surface-variant dark:text-on-surface-variant">
            Use filter expressions with the <code className="font-mono text-xs border border-outline-variant dark:border-outline-variant px-1 py-0.5">filter</code> query
            parameter. Combine conditions with <code className="font-mono text-xs border border-outline-variant dark:border-outline-variant px-1 py-0.5">&amp;&amp;</code> (AND)
            and <code className="font-mono text-xs border border-outline-variant dark:border-outline-variant px-1 py-0.5">||</code> (OR). Group with parentheses.
          </p>

          <div className="overflow-x-auto border border-primary dark:border-primary">
            <table className="w-full text-sm">
              <thead>
                <tr className="bg-primary dark:bg-primary">
                  <th scope="col" className="px-4 py-2 text-left text-label-sm text-on-primary dark:text-on-primary border-r border-on-primary/20">Operator</th>
                  <th scope="col" className="px-4 py-2 text-left text-label-sm text-on-primary dark:text-on-primary border-r border-on-primary/20">Description</th>
                  <th scope="col" className="px-4 py-2 text-left text-label-sm text-on-primary dark:text-on-primary">Example</th>
                </tr>
              </thead>
              <tbody>
                {FILTER_OPERATORS.map((op, i) => (
                  <tr key={op.operator} className={`border-b border-outline-variant dark:border-outline-variant ${i % 2 === 1 ? 'bg-surface-container-low dark:bg-surface-container-low' : ''}`}>
                    <td className="px-4 py-2 border-r border-outline-variant dark:border-outline-variant">
                      <code className="font-mono text-xs font-bold text-on-surface dark:text-on-surface">
                        {op.operator}
                      </code>
                    </td>
                    <td className="px-4 py-2 border-r border-outline-variant dark:border-outline-variant text-xs text-on-surface-variant dark:text-on-surface-variant">{op.description}</td>
                    <td className="px-4 py-2">
                      <code className="font-mono text-xs text-on-surface dark:text-on-surface">{op.example}</code>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div>
            <h4 className="mb-2 text-label-md text-on-surface dark:text-on-surface">
              EXAMPLES
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
    <div className="space-y-0" data-testid="collection-selector">
      {collections.map((col) => (
        <button
          key={col.id}
          type="button"
          onClick={() => onSelect(col.id)}
          className={`flex w-full items-center gap-2 border-b border-outline-variant dark:border-outline-variant px-3 py-2.5 text-left text-sm transition-colors-fast ${
            selectedId === col.id
              ? 'bg-primary dark:bg-primary text-on-primary dark:text-on-primary font-bold'
              : 'text-on-surface dark:text-on-surface hover:bg-surface-container-low dark:hover:bg-surface-container'
          }`}
          aria-current={selectedId === col.id ? 'true' : undefined}
          aria-label={`View API docs for ${col.name}`}
        >
          <span className="flex-1 truncate">{col.name}</span>
          <span className="text-label-sm opacity-70">
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
      {/* Page heading */}
      <header className="mb-8">
        <h2 className="text-display-lg text-on-surface dark:text-on-surface">API Documentation</h2>
        <div className="mt-2 border-t border-primary dark:border-primary" />
      </header>

      {/* Loading state */}
      {state.loading && (
        <div data-testid="loading-skeleton" className="space-y-4 animate-pulse-subtle">
          <div className="h-8 w-48 animate-pulse bg-surface-container dark:bg-surface-container" />
          <div className="h-64 animate-pulse bg-surface-container dark:bg-surface-container" />
        </div>
      )}

      {/* Error state */}
      {state.error && (
        <div role="alert" className="border border-error dark:border-error bg-error-container dark:bg-on-error px-4 py-3">
          <p className="text-sm text-on-error-container dark:text-error">{state.error}</p>
          <button
            type="button"
            onClick={loadCollections}
            className="mt-2 text-label-sm text-error dark:text-error hover:underline"
          >
            RETRY
          </button>
        </div>
      )}

      {/* Empty state */}
      {!state.loading && !state.error && state.collections.length === 0 && (
        <div className="border border-primary dark:border-primary p-8 text-center" data-testid="empty-state">
          <p className="text-on-surface-variant dark:text-on-surface-variant">No collections found.</p>
          <p className="mt-1 text-sm text-on-surface-variant dark:text-on-surface-variant">
            Create a collection first to see its API documentation.
          </p>
          <a
            href="/_/collections/new"
            className="mt-4 inline-flex items-center bg-primary dark:bg-primary px-6 py-3 text-label-md text-on-primary dark:text-on-primary hover:opacity-90"
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
              <h3 className="mb-3 text-label-md text-on-surface dark:text-on-surface">
                COLLECTIONS
              </h3>
              <div className="border-t border-primary dark:border-primary">
                <CollectionSelector
                  collections={state.collections}
                  selectedId={selectedId}
                  onSelect={setSelectedId}
                />
              </div>
            </div>
          </div>

          {/* Main content area */}
          <div className="min-w-0 flex-1 space-y-6">
            {/* Mobile collection selector */}
            <div className="lg:hidden">
              <label htmlFor="collection-select" className="block text-label-md text-on-surface dark:text-on-surface">
                COLLECTION
              </label>
              <select
                id="collection-select"
                value={selectedId ?? ''}
                onChange={(e) => setSelectedId(e.target.value)}
                className="mt-1 w-full border border-primary dark:border-primary bg-surface-lowest dark:bg-surface-lowest px-3 py-2 text-sm text-on-surface dark:text-on-surface focus:border-2 focus:outline-none"
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
                    <h3 className="text-headline-lg text-on-surface dark:text-on-surface" data-testid="selected-collection-name">
                      {selectedCollection.name}
                    </h3>
                    <span className="inline-flex items-center border border-primary dark:border-primary px-2 py-0.5 text-label-sm text-on-surface dark:text-on-surface">
                      {TYPE_LABELS[selectedCollection.type]}
                    </span>
                  </div>
                  <p className="mt-1 text-sm text-on-surface-variant dark:text-on-surface-variant font-data">
                    {endpoints.length} endpoint{endpoints.length !== 1 ? 's' : ''} available
                    {' \u00b7 '}
                    {selectedCollection.fields.length} field{selectedCollection.fields.length !== 1 ? 's' : ''}
                  </p>
                  <div className="mt-3 border-t border-primary dark:border-primary" />
                </div>

                {/* Fields summary */}
                {selectedCollection.fields.length > 0 && (
                  <div className="border border-primary dark:border-primary" data-testid="fields-summary">
                    <div className="border-b border-primary dark:border-primary px-4 py-3">
                      <h4 className="text-label-md text-on-surface dark:text-on-surface">FIELDS</h4>
                    </div>
                    <div className="overflow-x-auto">
                      <table className="w-full text-sm">
                        <thead>
                          <tr className="bg-primary dark:bg-primary">
                            <th scope="col" className="px-4 py-2 text-left text-label-sm text-on-primary dark:text-on-primary border-r border-on-primary/20">Name</th>
                            <th scope="col" className="px-4 py-2 text-left text-label-sm text-on-primary dark:text-on-primary border-r border-on-primary/20">Type</th>
                            <th scope="col" className="px-4 py-2 text-left text-label-sm text-on-primary dark:text-on-primary border-r border-on-primary/20">Required</th>
                            <th scope="col" className="px-4 py-2 text-left text-label-sm text-on-primary dark:text-on-primary">Unique</th>
                          </tr>
                        </thead>
                        <tbody>
                          {selectedCollection.fields.map((field, i) => (
                            <tr key={field.id} className={`border-b border-outline-variant dark:border-outline-variant ${i % 2 === 1 ? 'bg-surface-container-low dark:bg-surface-container-low' : ''}`}>
                              <td className="px-4 py-2 border-r border-outline-variant dark:border-outline-variant">
                                <code className="font-mono text-xs font-semibold text-on-surface dark:text-on-surface">{field.name}</code>
                              </td>
                              <td className="px-4 py-2 border-r border-outline-variant dark:border-outline-variant">
                                <span className="font-mono text-xs text-on-surface-variant dark:text-on-surface-variant">
                                  {field.type.type}
                                </span>
                              </td>
                              <td className="px-4 py-2 border-r border-outline-variant dark:border-outline-variant text-xs text-on-surface-variant dark:text-on-surface-variant">{field.required ? 'Yes' : 'No'}</td>
                              <td className="px-4 py-2 text-xs text-on-surface-variant dark:text-on-surface-variant">{field.unique ? 'Yes' : 'No'}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </div>
                )}

                {/* Endpoints */}
                <div>
                  <h4 className="mb-3 text-label-md text-on-surface dark:text-on-surface">ENDPOINTS</h4>
                  <div className="space-y-3" data-testid="endpoints-list">
                    {endpoints.map((ep, i) => (
                      <EndpointCard key={i} endpoint={ep} baseUrl={baseUrl} />
                    ))}
                  </div>
                </div>

                {/* Section divider */}
                <div className="border-t border-primary dark:border-primary" />

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
