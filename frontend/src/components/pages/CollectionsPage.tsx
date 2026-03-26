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
  base: 'BASE',
  auth: 'AUTH',
  view: 'VIEW',
};

function CollectionTypeBadge({ type }: { type: CollectionType }) {
  return (
    <span
      className="inline-flex items-center border border-primary dark:border-primary px-2 py-0.5 text-label-sm text-on-surface dark:text-on-surface"
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
      className="fixed inset-0 z-50 flex items-center justify-center bg-primary/30 dark:bg-primary/60 animate-fade-in"
      role="dialog"
      aria-modal="true"
      aria-labelledby="delete-dialog-title"
      ref={dialogRef}
    >
      <div className="mx-4 w-full max-w-md border border-primary dark:border-primary bg-surface-lowest dark:bg-surface-container p-6 animate-scale-in">
        <h3 id="delete-dialog-title" className="text-title-md text-on-surface dark:text-on-surface">
          DELETE COLLECTION
        </h3>

        <p className="mt-3 text-sm text-on-surface-variant dark:text-on-surface-variant">
          Are you sure you want to delete <strong className="text-on-surface dark:text-on-surface">{collection.name}</strong>? This action cannot be
          undone and all records in this collection will be permanently removed.
        </p>

        {/* Record count warning */}
        {loadingCount && (
          <p className="mt-3 text-sm text-secondary dark:text-secondary" data-testid="loading-record-count">
            Checking record count&hellip;
          </p>
        )}
        {!loadingCount && recordCount !== null && recordCount > 0 && (
          <div
            className="mt-3 border border-error dark:border-error bg-error-container dark:bg-on-error px-3 py-2 text-sm text-on-error-container dark:text-error"
            role="alert"
            data-testid="record-count-warning"
          >
            <strong>{recordCount.toLocaleString()}</strong>{' '}
            {recordCount === 1 ? 'record' : 'records'} will be permanently deleted.
          </div>
        )}
        {!loadingCount && recordCount !== null && recordCount === 0 && (
          <p className="mt-3 text-sm text-secondary dark:text-secondary" data-testid="no-records-note">
            This collection has no records.
          </p>
        )}

        {/* Name confirmation for large collections */}
        {isDangerous && (
          <div className="mt-4">
            <label htmlFor="confirm-collection-name" className="block text-label-md text-on-surface dark:text-on-surface mb-2">
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
              className="w-full border border-primary dark:border-primary bg-surface-lowest dark:bg-surface-lowest px-4 py-3 text-sm text-on-surface dark:text-on-surface placeholder:text-outline dark:placeholder:text-outline focus:border-2 focus:outline-none disabled:opacity-50 disabled:bg-surface-dim dark:disabled:bg-surface-dim"
              data-testid="confirm-name-input"
            />
          </div>
        )}

        <div className="mt-6 flex justify-end gap-3">
          <button
            type="button"
            onClick={onCancel}
            disabled={deleting}
            className="border border-primary dark:border-primary bg-transparent px-4 py-2 text-label-md text-on-surface dark:text-on-surface hover:bg-primary hover:text-on-primary dark:hover:bg-primary dark:hover:text-on-primary disabled:opacity-50 cursor-pointer transition-colors-fast"
          >
            CANCEL
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={deleting || !canDelete}
            className="bg-error dark:bg-error px-4 py-2 text-label-md text-on-error dark:text-on-error hover:opacity-90 disabled:opacity-50 cursor-pointer"
            data-testid="confirm-delete-btn"
          >
            {deleting ? 'DELETING\u2026' : 'DELETE'}
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
      <div className="py-16 text-center">
        <p className="text-sm text-secondary dark:text-secondary">No collections match your search.</p>
        <button
          type="button"
          onClick={onClear}
          className="mt-2 text-label-md text-on-surface dark:text-on-surface underline hover:no-underline cursor-pointer"
        >
          CLEAR SEARCH
        </button>
      </div>
    );
  }

  return (
    <div className="py-16 text-center">
      <p className="text-label-md text-secondary dark:text-secondary mb-2">NO COLLECTIONS</p>
      <p className="text-sm text-on-surface-variant dark:text-on-surface-variant">Get started by creating your first collection.</p>
    </div>
  );
}

// ── Loading skeleton ─────────────────────────────────────────────────────────

function LoadingSkeleton() {
  return (
    <div className="border border-primary dark:border-primary animate-pulse-subtle" data-testid="loading-skeleton">
      {/* Header skeleton */}
      <div className="bg-primary dark:bg-primary px-4 py-3 sm:px-6">
        <div className="flex gap-8">
          <div className="h-3 w-20 bg-on-primary/20 dark:bg-on-primary/20" />
          <div className="h-3 w-16 bg-on-primary/20 dark:bg-on-primary/20" />
          <div className="h-3 w-16 bg-on-primary/20 dark:bg-on-primary/20" />
          <div className="ml-auto h-3 w-20 bg-on-primary/20 dark:bg-on-primary/20" />
        </div>
      </div>
      {/* Row skeletons */}
      {[1, 2, 3].map((i) => (
        <div key={i} className="border-b border-primary dark:border-primary px-4 py-4 sm:px-6">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-6">
              <div className="h-4 w-32 bg-surface-container dark:bg-surface-container-high" />
              <div className="h-5 w-14 border border-outline-variant dark:border-outline-variant" />
            </div>
            <div className="h-4 w-20 bg-surface-container dark:bg-surface-container-high" />
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
      {/* Page heading */}
      <h1 className="text-display-lg text-on-surface dark:text-on-surface mb-2">
        Collections
      </h1>
      <p className="text-body-lg text-on-surface-variant dark:text-on-surface-variant mb-10">
        Manage your data collections and schemas.
      </p>

      {/* Toolbar */}
      <div className="mb-6 flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
        <div className="w-full sm:max-w-xs">
          <label htmlFor="collection-search" className="block text-label-md text-on-surface dark:text-on-surface mb-2">
            SEARCH
          </label>
          <div className="relative">
            <svg
              className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-outline dark:text-outline"
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
              placeholder="Search collections\u2026"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="w-full border border-primary dark:border-primary bg-surface-lowest dark:bg-surface-lowest py-3 pl-10 pr-3 text-sm text-on-surface dark:text-on-surface placeholder:text-outline dark:placeholder:text-outline focus:border-2 focus:outline-none"
            />
          </div>
        </div>

        <a
          href="/_/collections/new"
          className="inline-flex items-center justify-center gap-2 bg-primary dark:bg-primary px-6 py-3 text-label-md text-on-primary dark:text-on-primary hover:opacity-90 active:scale-[0.98] cursor-pointer"
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
          NEW COLLECTION
        </a>
      </div>

      {/* Error state */}
      {state.error && (
        <div role="alert" className="mb-4 border border-error dark:border-error bg-error-container dark:bg-on-error px-4 py-3">
          <div className="flex items-start gap-3">
            <svg
              className="h-5 w-5 text-error dark:text-error shrink-0 mt-0.5"
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
            <div>
              <p className="text-sm text-on-error-container dark:text-error">{state.error}</p>
              <button
                type="button"
                onClick={fetchCollections}
                className="mt-1 text-label-md text-on-error-container dark:text-error underline hover:no-underline cursor-pointer"
              >
                RETRY
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Success message */}
      {successMessage && (
        <div
          className="mb-4 border border-primary dark:border-primary bg-surface-container-low dark:bg-surface-container px-4 py-3"
          role="status"
          aria-live="polite"
          data-testid="success-message"
        >
          <p className="text-sm text-on-surface dark:text-on-surface">{successMessage}</p>
        </div>
      )}

      {/* Delete error */}
      {deleteError && (
        <div role="alert" className="mb-4 border border-error dark:border-error bg-error-container dark:bg-on-error px-4 py-3">
          <p className="text-sm text-on-error-container dark:text-error">{deleteError}</p>
        </div>
      )}

      {/* Loading state */}
      {state.loading && <LoadingSkeleton />}

      {/* Collection list */}
      {!state.loading && !state.error && filteredCollections.length === 0 && (
        <EmptyState hasSearch={search.trim().length > 0} onClear={() => setSearch('')} />
      )}

      {!state.loading && !state.error && filteredCollections.length > 0 && (
        <div className="overflow-x-auto border border-primary dark:border-primary">
          <table className="min-w-full">
            <thead>
              <tr className="bg-primary dark:bg-primary">
                <th
                  scope="col"
                  className="px-4 py-3 text-left text-label-md text-on-primary dark:text-on-primary sm:px-6 border-r border-on-primary/20 dark:border-on-primary/20"
                >
                  NAME
                </th>
                <th
                  scope="col"
                  className="px-4 py-3 text-left text-label-md text-on-primary dark:text-on-primary sm:px-6 border-r border-on-primary/20 dark:border-on-primary/20"
                >
                  TYPE
                </th>
                <th
                  scope="col"
                  className="px-4 py-3 text-left text-label-md text-on-primary dark:text-on-primary sm:px-6 border-r border-on-primary/20 dark:border-on-primary/20"
                >
                  FIELDS
                </th>
                <th
                  scope="col"
                  className="px-4 py-3 text-right text-label-md text-on-primary dark:text-on-primary sm:px-6"
                >
                  ACTIONS
                </th>
              </tr>
            </thead>
            <tbody>
              {filteredCollections.map((collection) => (
                <tr
                  key={collection.id}
                  className="border-b border-primary dark:border-primary hover:bg-surface-container-low dark:hover:bg-surface-container transition-colors-fast"
                >
                  <td className="whitespace-nowrap px-4 py-4 sm:px-6 border-r border-primary dark:border-primary">
                    <a
                      href={`/_/collections/${encodeURIComponent(collection.id)}`}
                      className="text-sm font-semibold text-on-surface dark:text-on-surface hover:underline"
                    >
                      {collection.name}
                    </a>
                  </td>
                  <td className="whitespace-nowrap px-4 py-4 sm:px-6 border-r border-primary dark:border-primary">
                    <CollectionTypeBadge type={collection.type} />
                  </td>
                  <td className="whitespace-nowrap px-4 py-4 text-sm text-on-surface-variant dark:text-on-surface-variant sm:px-6 border-r border-primary dark:border-primary font-data">
                    {collection.fields.length} {collection.fields.length === 1 ? 'field' : 'fields'}
                  </td>
                  <td className="whitespace-nowrap px-4 py-4 text-right sm:px-6">
                    <div className="flex items-center justify-end gap-2">
                      <a
                        href={`/_/collections/${encodeURIComponent(collection.id)}/edit`}
                        className="border border-primary dark:border-primary px-3 py-1.5 text-label-sm text-on-surface dark:text-on-surface hover:bg-primary hover:text-on-primary dark:hover:bg-primary dark:hover:text-on-primary transition-colors-fast"
                        aria-label={`Edit ${collection.name}`}
                      >
                        EDIT
                      </a>
                      <button
                        type="button"
                        onClick={() => {
                          setDeleteError(null);
                          setSuccessMessage(null);
                          setDeleteTarget(collection);
                        }}
                        className="border border-error dark:border-error px-3 py-1.5 text-label-sm text-error dark:text-error hover:bg-error hover:text-on-error dark:hover:bg-error dark:hover:text-on-error cursor-pointer transition-colors-fast"
                        aria-label={`Delete ${collection.name}`}
                      >
                        DELETE
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
        <p className="mt-4 text-label-sm text-secondary dark:text-secondary">
          {filteredCollections.length} OF {state.collections.length} COLLECTION{state.collections.length !== 1 ? 'S' : ''}
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
