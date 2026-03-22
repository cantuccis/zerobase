import React, { useState, useEffect, useCallback, useMemo } from 'react';
import { DashboardLayout } from '../DashboardLayout';
import { client } from '../../lib/auth/client';
import type { Collection, CollectionType } from '../../lib/api/types';
import { ApiError } from '../../lib/api';

// ── Types ────────────────────────────────────────────────────────────────────

interface CollectionsState {
  collections: Collection[];
  loading: boolean;
  error: string | null;
}

// ── Helpers ──────────────────────────────────────────────────────────────────

const TYPE_LABELS: Record<CollectionType, string> = {
  base: 'Base',
  auth: 'Auth',
  view: 'View',
};

const TYPE_COLORS: Record<CollectionType, string> = {
  base: 'bg-blue-100 dark:bg-blue-900/20 text-blue-800 dark:text-blue-300',
  auth: 'bg-green-100 dark:bg-green-900/30 text-green-800 dark:text-green-300',
  view: 'bg-purple-100 dark:bg-purple-900/30 text-purple-800 dark:text-purple-300',
};

function CollectionTypeBadge({ type }: { type: CollectionType }) {
  return (
    <span
      className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${TYPE_COLORS[type]}`}
      data-testid={`badge-${type}`}
    >
      {TYPE_LABELS[type]}
    </span>
  );
}

// ── Constants ────────────────────────────────────────────────────────────────

/** Collections with this many records or more require typing the name to confirm deletion. */
const DANGEROUS_RECORD_THRESHOLD = 50;

// ── Delete confirmation dialog ───────────────────────────────────────────────

interface DeleteDialogProps {
  collection: Collection;
  recordCount: number | null;
  loadingCount: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  deleting: boolean;
}

function DeleteConfirmDialog({
  collection,
  recordCount,
  loadingCount,
  onConfirm,
  onCancel,
  deleting,
}: DeleteDialogProps) {
  const [confirmName, setConfirmName] = useState('');

  const dialogRef = React.useRef<HTMLDivElement>(null);
  const isDangerous = recordCount !== null && recordCount >= DANGEROUS_RECORD_THRESHOLD;
  const nameMatches = confirmName === collection.name;
  const canDelete = !loadingCount && (!isDangerous || nameMatches);

  React.useEffect(() => {
    const firstFocusable = dialogRef.current?.querySelector<HTMLElement>('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])');
    firstFocusable?.focus();
  }, []);

  React.useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape' && !deleting) {
        onCancel();
        return;
      }
      if (e.key === 'Tab' && dialogRef.current) {
        const focusable = dialogRef.current.querySelectorAll<HTMLElement>('button:not(:disabled), [href], input:not(:disabled), select, textarea, [tabindex]:not([tabindex="-1"])');
        if (focusable.length === 0) return;
        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    }
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [deleting, onCancel]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/30"
      role="dialog"
      aria-modal="true"
      aria-labelledby="delete-dialog-title"
      ref={dialogRef}
    >
      <div className="mx-4 w-full max-w-md rounded-lg bg-white dark:bg-gray-800 p-6 shadow-xl dark:shadow-gray-900/20">
        <h3 id="delete-dialog-title" className="text-lg font-semibold text-gray-900 dark:text-gray-100">
          Delete Collection
        </h3>

        <p className="mt-2 text-sm text-gray-600 dark:text-gray-400">
          Are you sure you want to delete <strong>{collection.name}</strong>? This action cannot be
          undone and all records in this collection will be permanently removed.
        </p>

        {/* Record count warning */}
        {loadingCount && (
          <p className="mt-3 text-sm text-gray-500 dark:text-gray-400" data-testid="loading-record-count">
            Checking record count&hellip;
          </p>
        )}
        {!loadingCount && recordCount !== null && recordCount > 0 && (
          <div
            className="mt-3 rounded-md bg-amber-50 dark:bg-amber-900/30 px-3 py-2 text-sm text-amber-800 dark:text-amber-300"
            role="alert"
            data-testid="record-count-warning"
          >
            <strong>{recordCount.toLocaleString()}</strong>{' '}
            {recordCount === 1 ? 'record' : 'records'} will be permanently deleted.
          </div>
        )}
        {!loadingCount && recordCount !== null && recordCount === 0 && (
          <p className="mt-3 text-sm text-gray-500 dark:text-gray-400" data-testid="no-records-note">
            This collection has no records.
          </p>
        )}

        {/* Name confirmation for large collections */}
        {isDangerous && (
          <div className="mt-4">
            <label htmlFor="confirm-collection-name" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
              Type <strong>{collection.name}</strong> to confirm
            </label>
            <input
              id="confirm-collection-name"
              type="text"
              value={confirmName}
              onChange={(e) => setConfirmName(e.target.value)}
              placeholder={collection.name}
              disabled={deleting}
              autoComplete="off"
              spellCheck={false}
              className="mt-1 w-full rounded-md border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 px-3 py-2 text-sm text-gray-900 dark:text-gray-100 placeholder-gray-400 dark:placeholder-gray-500 focus:border-red-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-red-500 disabled:opacity-50"
              data-testid="confirm-name-input"
            />
          </div>
        )}

        <div className="mt-6 flex justify-end gap-3">
          <button
            type="button"
            onClick={onCancel}
            disabled={deleting}
            className="rounded-md border border-gray-300 dark:border-gray-600 px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 transition-colors hover:bg-gray-50 dark:hover:bg-gray-700 disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={deleting || !canDelete}
            className="rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-red-700 dark:hover:bg-red-600 disabled:opacity-50"
            data-testid="confirm-delete-btn"
          >
            {deleting ? 'Deleting...' : 'Delete'}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Empty state ──────────────────────────────────────────────────────────────

function EmptyState({ hasSearch, onClear }: { hasSearch: boolean; onClear: () => void }) {
  if (hasSearch) {
    return (
      <div className="py-12 text-center">
        <p className="text-sm text-gray-500 dark:text-gray-400">No collections match your search.</p>
        <button
          type="button"
          onClick={onClear}
          className="mt-2 text-sm font-medium text-blue-600 dark:text-blue-400 hover:text-blue-700 dark:hover:text-blue-300"
        >
          Clear search
        </button>
      </div>
    );
  }

  return (
    <div className="py-12 text-center">
      <svg
        className="mx-auto h-12 w-12 text-gray-400 dark:text-gray-500"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <rect x="3" y="3" width="7" height="7" rx="1" />
        <rect x="14" y="3" width="7" height="7" rx="1" />
        <rect x="3" y="14" width="7" height="7" rx="1" />
        <rect x="14" y="14" width="7" height="7" rx="1" />
      </svg>
      <h3 className="mt-2 text-sm font-semibold text-gray-900 dark:text-gray-100">No collections</h3>
      <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">Get started by creating your first collection.</p>
    </div>
  );
}

// ── Loading skeleton ─────────────────────────────────────────────────────────

function LoadingSkeleton() {
  return (
    <div className="space-y-3" data-testid="loading-skeleton">
      {[1, 2, 3].map((i) => (
        <div key={i} className="animate-pulse rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="h-5 w-32 rounded bg-gray-200 dark:bg-gray-600" />
              <div className="h-5 w-14 rounded-full bg-gray-200 dark:bg-gray-600" />
            </div>
            <div className="h-5 w-20 rounded bg-gray-200 dark:bg-gray-600" />
          </div>
        </div>
      ))}
    </div>
  );
}

// ── Main component ───────────────────────────────────────────────────────────

export function CollectionsPage() {
  const [state, setState] = useState<CollectionsState>({
    collections: [],
    loading: true,
    error: null,
  });
  const [search, setSearch] = useState('');
  const [deleteTarget, setDeleteTarget] = useState<Collection | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [deleteRecordCount, setDeleteRecordCount] = useState<number | null>(null);
  const [loadingRecordCount, setLoadingRecordCount] = useState(false);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);

  const fetchCollections = useCallback(async () => {
    setState((prev) => ({ ...prev, loading: true, error: null }));
    try {
      const response = await client.listCollections();
      setState({ collections: response.items, loading: false, error: null });
    } catch (err) {
      const message =
        err instanceof ApiError
          ? err.message
          : 'Unable to connect to the server. Please try again.';
      setState((prev) => ({ ...prev, loading: false, error: message }));
    }
  }, []);

  useEffect(() => {
    fetchCollections();
  }, [fetchCollections]);

  // Fetch record count when a delete target is selected
  useEffect(() => {
    if (!deleteTarget) {
      setDeleteRecordCount(null);
      setLoadingRecordCount(false);
      return;
    }

    // View collections have no records to count
    if (deleteTarget.type === 'view') {
      setDeleteRecordCount(0);
      return;
    }

    let cancelled = false;
    setLoadingRecordCount(true);
    setDeleteRecordCount(null);

    client
      .listRecords(deleteTarget.name, { perPage: 1 })
      .then((response) => {
        if (!cancelled) {
          setDeleteRecordCount(response.totalItems);
          setLoadingRecordCount(false);
        }
      })
      .catch(() => {
        if (!cancelled) {
          // If we can't get the count, allow deletion without the count warning
          setDeleteRecordCount(null);
          setLoadingRecordCount(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [deleteTarget]);

  // Auto-dismiss success message
  useEffect(() => {
    if (!successMessage) return;
    const timer = setTimeout(() => setSuccessMessage(null), 5000);
    return () => clearTimeout(timer);
  }, [successMessage]);

  const filteredCollections = useMemo(() => {
    if (!search.trim()) return state.collections;
    const query = search.trim().toLowerCase();
    return state.collections.filter(
      (c) =>
        c.name.toLowerCase().includes(query) ||
        c.type.toLowerCase().includes(query),
    );
  }, [state.collections, search]);

  const handleDelete = useCallback(async () => {
    if (!deleteTarget) return;
    setDeleting(true);
    setDeleteError(null);
    try {
      await client.deleteCollection(deleteTarget.id);
      const deletedName = deleteTarget.name;
      setState((prev) => ({
        ...prev,
        collections: prev.collections.filter((c) => c.id !== deleteTarget.id),
      }));
      setDeleteTarget(null);
      setSuccessMessage(`Collection "${deletedName}" deleted successfully.`);
    } catch (err) {
      const message =
        err instanceof ApiError ? err.message : 'Failed to delete collection.';
      setDeleteError(message);
    } finally {
      setDeleting(false);
    }
  }, [deleteTarget]);

  return (
    <DashboardLayout currentPath="/_/collections" pageTitle="Collections">
      {/* Toolbar */}
      <div className="mb-6 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="relative w-full sm:max-w-xs">
          <label htmlFor="collection-search" className="sr-only">
            Search collections
          </label>
          <svg
            className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400 dark:text-gray-500"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <circle cx="11" cy="11" r="8" />
            <line x1="21" y1="21" x2="16.65" y2="16.65" />
          </svg>
          <input
            id="collection-search"
            type="search"
            placeholder="Search collections..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="w-full rounded-md border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 py-2 pl-10 pr-3 text-sm text-gray-900 dark:text-gray-100 placeholder-gray-400 dark:placeholder-gray-500 focus:border-blue-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-blue-500"
          />
        </div>

        <a
          href="/_/collections/new"
          className="inline-flex items-center justify-center gap-2 rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-700 dark:hover:bg-blue-600"
        >
          <svg
            className="h-4 w-4"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <line x1="12" y1="5" x2="12" y2="19" />
            <line x1="5" y1="12" x2="19" y2="12" />
          </svg>
          New Collection
        </a>
      </div>

      {/* Error state */}
      {state.error && (
        <div role="alert" className="mb-4 rounded-md bg-red-50 dark:bg-red-900/30 p-4">
          <div className="flex">
            <svg
              className="h-5 w-5 text-red-400 dark:text-red-500"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden="true"
            >
              <circle cx="12" cy="12" r="10" />
              <line x1="12" y1="8" x2="12" y2="12" />
              <line x1="12" y1="16" x2="12.01" y2="16" />
            </svg>
            <div className="ml-3">
              <p className="text-sm text-red-700 dark:text-red-400">{state.error}</p>
              <button
                type="button"
                onClick={fetchCollections}
                className="mt-1 text-sm font-medium text-red-700 dark:text-red-400 underline hover:text-red-800 dark:hover:text-red-300"
              >
                Retry
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Success message */}
      {successMessage && (
        <div
          className="mb-4 rounded-md bg-green-50 dark:bg-green-900/30 p-4"
          role="status"
          aria-live="polite"
          data-testid="success-message"
        >
          <p className="text-sm text-green-700 dark:text-green-400">{successMessage}</p>
        </div>
      )}

      {/* Delete error */}
      {deleteError && (
        <div role="alert" className="mb-4 rounded-md bg-red-50 dark:bg-red-900/30 p-4">
          <p className="text-sm text-red-700 dark:text-red-400">{deleteError}</p>
        </div>
      )}

      {/* Loading state */}
      {state.loading && <LoadingSkeleton />}

      {/* Collection list */}
      {!state.loading && !state.error && filteredCollections.length === 0 && (
        <EmptyState hasSearch={search.trim().length > 0} onClear={() => setSearch('')} />
      )}

      {!state.loading && !state.error && filteredCollections.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800">
          <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-700">
            <thead className="bg-gray-50 dark:bg-gray-900">
              <tr>
                <th
                  scope="col"
                  className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 sm:px-6"
                >
                  Name
                </th>
                <th
                  scope="col"
                  className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 sm:px-6"
                >
                  Type
                </th>
                <th
                  scope="col"
                  className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 sm:px-6"
                >
                  Fields
                </th>
                <th
                  scope="col"
                  className="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 sm:px-6"
                >
                  Actions
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 dark:divide-gray-700">
              {filteredCollections.map((collection) => (
                <tr key={collection.id} className="transition-colors hover:bg-gray-50 dark:hover:bg-gray-700">
                  <td className="whitespace-nowrap px-4 py-4 sm:px-6">
                    <a
                      href={`/_/collections/${encodeURIComponent(collection.id)}`}
                      className="text-sm font-medium text-gray-900 dark:text-gray-100 hover:text-blue-600 dark:hover:text-blue-400"
                    >
                      {collection.name}
                    </a>
                  </td>
                  <td className="whitespace-nowrap px-4 py-4 sm:px-6">
                    <CollectionTypeBadge type={collection.type} />
                  </td>
                  <td className="whitespace-nowrap px-4 py-4 text-sm text-gray-500 dark:text-gray-400 sm:px-6">
                    {collection.fields.length} {collection.fields.length === 1 ? 'field' : 'fields'}
                  </td>
                  <td className="whitespace-nowrap px-4 py-4 text-right sm:px-6">
                    <div className="flex items-center justify-end gap-2">
                      <a
                        href={`/_/collections/${encodeURIComponent(collection.id)}/edit`}
                        className="rounded-md px-2.5 py-1.5 text-sm font-medium text-gray-600 dark:text-gray-400 transition-colors hover:bg-gray-100 dark:hover:bg-gray-700 hover:text-gray-900 dark:hover:text-gray-100"
                        aria-label={`Edit ${collection.name}`}
                      >
                        Edit
                      </a>
                      <button
                        type="button"
                        onClick={() => {
                          setDeleteError(null);
                          setSuccessMessage(null);
                          setDeleteTarget(collection);
                        }}
                        className="rounded-md px-2.5 py-1.5 text-sm font-medium text-red-600 dark:text-red-400 transition-colors hover:bg-red-50 dark:hover:bg-red-900/30 hover:text-red-700 dark:hover:text-red-300"
                        aria-label={`Delete ${collection.name}`}
                      >
                        Delete
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Summary */}
      {!state.loading && !state.error && state.collections.length > 0 && (
        <p className="mt-4 text-xs text-gray-400 dark:text-gray-500">
          {filteredCollections.length} of {state.collections.length} collection{state.collections.length !== 1 ? 's' : ''}
        </p>
      )}

      {/* Delete confirmation dialog */}
      {deleteTarget && (
        <DeleteConfirmDialog
          collection={deleteTarget}
          recordCount={deleteRecordCount}
          loadingCount={loadingRecordCount}
          onConfirm={handleDelete}
          onCancel={() => {
            setDeleteTarget(null);
            setDeleteError(null);
          }}
          deleting={deleting}
        />
      )}
    </DashboardLayout>
  );
}
