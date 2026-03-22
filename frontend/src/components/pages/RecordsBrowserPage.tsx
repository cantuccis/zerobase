import { useState, useEffect, useCallback, useMemo } from 'react';
import { DashboardLayout } from '../DashboardLayout';
import { client } from '../../lib/auth/client';
import { ApiError } from '../../lib/api';
import { RecordFormModal } from '../records/RecordFormModal';
import type { RelationOption } from '../records/field-inputs';
import type {
  Collection,
  BaseRecord,
  Field,
  ListResponse,
  ListRecordsParams,
} from '../../lib/api/types';

// ── Types ────────────────────────────────────────────────────────────────────

interface RecordsBrowserState {
  collection: Collection | null;
  records: ListResponse<BaseRecord> | null;
  loading: boolean;
  error: string | null;
}

interface SortConfig {
  field: string;
  direction: 'asc' | 'desc';
}

export interface RecordsBrowserPageProps {
  collectionId: string;
}

// ── Constants ────────────────────────────────────────────────────────────────

const SYSTEM_FIELDS = ['id', 'created', 'updated'];
const DEFAULT_PER_PAGE = 20;
const PER_PAGE_OPTIONS = [10, 20, 50, 100];

/** Try to derive a human-readable label for a record. */
function getRecordLabel(record: BaseRecord): string {
  // Use common label fields if available
  for (const key of ['title', 'name', 'label', 'email', 'username', 'slug']) {
    if (typeof record[key] === 'string' && record[key]) return record[key] as string;
  }
  return record.id;
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function getDisplayColumns(collection: Collection): string[] {
  return [...SYSTEM_FIELDS, ...collection.fields.map((f) => f.name)];
}

function formatCellValue(value: unknown): string {
  if (value === null || value === undefined) return '—';
  if (typeof value === 'boolean') return value ? 'true' : 'false';
  if (typeof value === 'object') return JSON.stringify(value);
  return String(value);
}

function truncate(text: string, maxLen: number): string {
  if (text.length <= maxLen) return text;
  return text.slice(0, maxLen) + '…';
}

// ── Loading skeleton ─────────────────────────────────────────────────────────

function TableSkeleton() {
  return (
    <div className="space-y-2" data-testid="table-skeleton">
      {[1, 2, 3, 4, 5].map((i) => (
        <div key={i} className="animate-pulse rounded border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4">
          <div className="flex gap-4">
            <div className="h-4 w-24 rounded bg-gray-200 dark:bg-gray-600" />
            <div className="h-4 w-32 rounded bg-gray-200 dark:bg-gray-600" />
            <div className="h-4 w-20 rounded bg-gray-200 dark:bg-gray-600" />
            <div className="h-4 w-28 rounded bg-gray-200 dark:bg-gray-600" />
          </div>
        </div>
      ))}
    </div>
  );
}

// ── Sort indicator ───────────────────────────────────────────────────────────

function SortIndicator({ field, sort }: { field: string; sort: SortConfig | null }) {
  if (!sort || sort.field !== field) {
    return (
      <svg className="ml-1 inline h-3 w-3 text-gray-300 dark:text-gray-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
        <path d="M8 9l4-4 4 4M8 15l4 4 4-4" />
      </svg>
    );
  }
  return sort.direction === 'asc' ? (
    <svg className="ml-1 inline h-3 w-3 text-blue-600 dark:text-blue-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
      <path d="M8 15l4-4 4 4" />
    </svg>
  ) : (
    <svg className="ml-1 inline h-3 w-3 text-blue-600 dark:text-blue-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
      <path d="M8 9l4 4 4-4" />
    </svg>
  );
}

// ── Column visibility dropdown ───────────────────────────────────────────────

interface ColumnToggleProps {
  columns: string[];
  visibleColumns: Set<string>;
  onToggle: (column: string) => void;
}

function ColumnToggle({ columns, visibleColumns, onToggle }: ColumnToggleProps) {
  const [open, setOpen] = useState(false);

  return (
    <div className="relative">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="inline-flex items-center gap-1.5 rounded-md border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 px-3 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 transition-colors hover:bg-gray-50 dark:hover:bg-gray-700"
        aria-label="Toggle column visibility"
        aria-expanded={open}
      >
        <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
          <circle cx="12" cy="12" r="3" />
        </svg>
        Columns
      </button>

      {open && (
        <>
          <div
            className="fixed inset-0 z-10"
            onClick={() => setOpen(false)}
            aria-hidden="true"
          />
          <div
            className="absolute right-0 z-20 mt-1 w-56 rounded-md border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 py-1 shadow-lg dark:shadow-gray-900/20"
            role="menu"
          >
            {columns.map((col) => (
              <label
                key={col}
                className="flex cursor-pointer items-center gap-2 px-3 py-1.5 text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700"
              >
                <input
                  type="checkbox"
                  checked={visibleColumns.has(col)}
                  onChange={() => onToggle(col)}
                  className="h-4 w-4 rounded border-gray-300 dark:border-gray-600 text-blue-600 focus:ring-2 focus:ring-blue-500"
                />
                {col}
              </label>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

// ── Pagination ───────────────────────────────────────────────────────────────

interface PaginationProps {
  page: number;
  totalPages: number;
  totalItems: number;
  perPage: number;
  onPageChange: (page: number) => void;
  onPerPageChange: (perPage: number) => void;
}

function Pagination({ page, totalPages, totalItems, perPage, onPageChange, onPerPageChange }: PaginationProps) {
  return (
    <div className="flex flex-col items-center justify-between gap-3 sm:flex-row" data-testid="pagination">
      <div className="flex items-center gap-2 text-sm text-gray-500 dark:text-gray-400">
        <span>{totalItems} record{totalItems !== 1 ? 's' : ''}</span>
        <span className="text-gray-300 dark:text-gray-600">|</span>
        <label htmlFor="per-page-select" className="sr-only">Records per page</label>
        <select
          id="per-page-select"
          value={perPage}
          onChange={(e) => onPerPageChange(Number(e.target.value))}
          className="rounded border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100 py-1 pl-2 pr-6 text-sm focus:border-blue-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-blue-500"
        >
          {PER_PAGE_OPTIONS.map((n) => (
            <option key={n} value={n}>{n} per page</option>
          ))}
        </select>
      </div>

      <div className="flex items-center gap-1">
        <button
          type="button"
          onClick={() => onPageChange(1)}
          disabled={page <= 1}
          className="rounded-md px-2 py-1 text-sm text-gray-600 dark:text-gray-400 transition-colors hover:bg-gray-100 dark:hover:bg-gray-700 disabled:cursor-not-allowed disabled:opacity-40"
          aria-label="First page"
        >
          ««
        </button>
        <button
          type="button"
          onClick={() => onPageChange(page - 1)}
          disabled={page <= 1}
          className="rounded-md px-2 py-1 text-sm text-gray-600 dark:text-gray-400 transition-colors hover:bg-gray-100 dark:hover:bg-gray-700 disabled:cursor-not-allowed disabled:opacity-40"
          aria-label="Previous page"
        >
          «
        </button>

        <span className="px-3 py-1 text-sm text-gray-700 dark:text-gray-300">
          Page {page} of {totalPages || 1}
        </span>

        <button
          type="button"
          onClick={() => onPageChange(page + 1)}
          disabled={page >= totalPages}
          className="rounded-md px-2 py-1 text-sm text-gray-600 dark:text-gray-400 transition-colors hover:bg-gray-100 dark:hover:bg-gray-700 disabled:cursor-not-allowed disabled:opacity-40"
          aria-label="Next page"
        >
          »
        </button>
        <button
          type="button"
          onClick={() => onPageChange(totalPages)}
          disabled={page >= totalPages}
          className="rounded-md px-2 py-1 text-sm text-gray-600 dark:text-gray-400 transition-colors hover:bg-gray-100 dark:hover:bg-gray-700 disabled:cursor-not-allowed disabled:opacity-40"
          aria-label="Last page"
        >
          »»
        </button>
      </div>
    </div>
  );
}

// ── Record detail panel ──────────────────────────────────────────────────────

interface RecordDetailProps {
  record: BaseRecord;
  collection: Collection;
  onClose: () => void;
}

interface RecordDetailExtraProps {
  onEdit: (record: BaseRecord) => void;
  onDelete: (recordId: string) => void;
  deleteConfirmId: string | null;
  onDeleteConfirm: (recordId: string | null) => void;
}

function RecordDetail({
  record,
  collection,
  onClose,
  onEdit,
  onDelete,
  deleteConfirmId,
  onDeleteConfirm,
}: RecordDetailProps & RecordDetailExtraProps) {
  const allFields = getDisplayColumns(collection);

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-end bg-black/30"
      role="dialog"
      aria-modal="true"
      aria-labelledby="record-detail-title"
    >
      <div className="h-full w-full max-w-lg overflow-y-auto bg-white dark:bg-gray-800 shadow-xl dark:shadow-gray-900/20 sm:w-[480px]">
        <div className="flex items-center justify-between border-b border-gray-200 dark:border-gray-700 px-4 py-3">
          <h3 id="record-detail-title" className="text-sm font-semibold text-gray-900 dark:text-gray-100">
            Record: {record.id}
          </h3>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={() => onEdit(record)}
              className="rounded-md p-1.5 text-blue-600 dark:text-blue-400 transition-colors hover:bg-blue-50 dark:hover:bg-blue-900/30"
              aria-label="Edit record"
              data-testid="edit-record-btn"
            >
              <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
                <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
              </svg>
            </button>
            {deleteConfirmId === record.id ? (
              <div className="flex items-center gap-1">
                <button
                  type="button"
                  onClick={() => onDelete(record.id)}
                  className="rounded-md bg-red-600 px-2 py-1 text-xs font-medium text-white hover:bg-red-700 dark:hover:bg-red-600"
                  data-testid="confirm-delete-btn"
                >
                  Confirm
                </button>
                <button
                  type="button"
                  onClick={() => onDeleteConfirm(null)}
                  className="rounded-md border border-gray-300 dark:border-gray-600 px-2 py-1 text-xs font-medium text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-700"
                  data-testid="cancel-delete-btn"
                >
                  Cancel
                </button>
              </div>
            ) : (
              <button
                type="button"
                onClick={() => onDeleteConfirm(record.id)}
                className="rounded-md p-1.5 text-red-500 dark:text-red-400 transition-colors hover:bg-red-50 dark:hover:bg-red-900/30"
                aria-label="Delete record"
                data-testid="delete-record-btn"
              >
                <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <polyline points="3 6 5 6 21 6" />
                  <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                </svg>
              </button>
            )}
            <button
              type="button"
              onClick={onClose}
              className="rounded-md p-1.5 text-gray-500 dark:text-gray-400 transition-colors hover:bg-gray-100 dark:hover:bg-gray-700 hover:text-gray-900 dark:hover:text-gray-100"
              aria-label="Close record detail"
            >
              <svg className="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </div>
        </div>

        <dl className="divide-y divide-gray-100 dark:divide-gray-700 px-4">
          {allFields.map((field) => (
            <div key={field} className="py-3">
              <dt className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">{field}</dt>
              <dd className="mt-1 whitespace-pre-wrap break-all text-sm text-gray-900 dark:text-gray-100">
                {formatCellValue(record[field])}
              </dd>
            </div>
          ))}
        </dl>
      </div>
    </div>
  );
}

// ── Main component ───────────────────────────────────────────────────────────

export function RecordsBrowserPage({ collectionId }: RecordsBrowserPageProps) {
  const [state, setState] = useState<RecordsBrowserState>({
    collection: null,
    records: null,
    loading: true,
    error: null,
  });

  const [page, setPage] = useState(1);
  const [perPage, setPerPage] = useState(DEFAULT_PER_PAGE);
  const [sort, setSort] = useState<SortConfig | null>(null);
  const [filter, setFilter] = useState('');
  const [filterInput, setFilterInput] = useState('');
  const [visibleColumns, setVisibleColumns] = useState<Set<string>>(new Set());
  const [selectedRecord, setSelectedRecord] = useState<BaseRecord | null>(null);
  const [formMode, setFormMode] = useState<'create' | 'edit' | null>(null);
  const [editingRecord, setEditingRecord] = useState<BaseRecord | null>(null);
  const [allCollections, setAllCollections] = useState<Collection[]>([]);

  // Fetch collection schema
  const fetchCollection = useCallback(async () => {
    try {
      const col = await client.getCollection(collectionId);
      setState((prev) => ({ ...prev, collection: col }));
      // Initialize visible columns with all columns
      setVisibleColumns(new Set(getDisplayColumns(col)));
      return col;
    } catch (err) {
      const message = err instanceof ApiError ? err.message : 'Failed to load collection.';
      setState((prev) => ({ ...prev, loading: false, error: message }));
      return null;
    }
  }, [collectionId]);

  // Fetch records
  const fetchRecords = useCallback(async (col: Collection | null) => {
    if (!col) return;
    setState((prev) => ({ ...prev, loading: true, error: null }));
    try {
      const params: ListRecordsParams = {
        page,
        perPage,
      };
      if (sort) {
        params.sort = `${sort.direction === 'desc' ? '-' : ''}${sort.field}`;
      }
      if (filter.trim()) {
        params.filter = filter.trim();
      }
      const response = await client.listRecords(col.name, params);
      setState((prev) => ({ ...prev, records: response, loading: false }));
    } catch (err) {
      const message = err instanceof ApiError ? err.message : 'Failed to load records.';
      setState((prev) => ({ ...prev, loading: false, error: message }));
    }
  }, [page, perPage, sort, filter]);

  // Initial load
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const col = await fetchCollection();
      if (!cancelled && col) {
        await fetchRecords(col);
      }
    })();
    return () => { cancelled = true; };
  }, [fetchCollection]); // eslint-disable-line react-hooks/exhaustive-deps

  // Refetch records when pagination/sort/filter changes
  useEffect(() => {
    if (state.collection) {
      fetchRecords(state.collection);
    }
  }, [page, perPage, sort, filter]); // eslint-disable-line react-hooks/exhaustive-deps

  // All displayable columns
  const allColumns = useMemo(() => {
    if (!state.collection) return [];
    return getDisplayColumns(state.collection);
  }, [state.collection]);

  // Displayed columns (filtered by visibility)
  const displayedColumns = useMemo(() => {
    return allColumns.filter((col) => visibleColumns.has(col));
  }, [allColumns, visibleColumns]);

  // Handlers
  const handleSort = useCallback((field: string) => {
    setSort((prev) => {
      if (prev?.field === field) {
        if (prev.direction === 'asc') return { field, direction: 'desc' };
        return null; // Third click removes sort
      }
      return { field, direction: 'asc' };
    });
    setPage(1);
  }, []);

  const handleColumnToggle = useCallback((column: string) => {
    setVisibleColumns((prev) => {
      const next = new Set(prev);
      if (next.has(column)) {
        // Don't allow hiding all columns
        if (next.size <= 1) return prev;
        next.delete(column);
      } else {
        next.add(column);
      }
      return next;
    });
  }, []);

  const handleFilterSubmit = useCallback((e: React.FormEvent) => {
    e.preventDefault();
    setFilter(filterInput);
    setPage(1);
  }, [filterInput]);

  const handleClearFilter = useCallback(() => {
    setFilterInput('');
    setFilter('');
    setPage(1);
  }, []);

  const handlePerPageChange = useCallback((newPerPage: number) => {
    setPerPage(newPerPage);
    setPage(1);
  }, []);

  const handleRetry = useCallback(() => {
    (async () => {
      const col = state.collection ?? await fetchCollection();
      if (col) await fetchRecords(col);
    })();
  }, [state.collection, fetchCollection, fetchRecords]);

  // ── Record form handlers ──────────────────────────────────────────────

  const handleOpenCreate = useCallback(() => {
    setFormMode('create');
    setEditingRecord(null);
  }, []);

  const handleOpenEdit = useCallback((record: BaseRecord) => {
    setFormMode('edit');
    setEditingRecord(record);
    setSelectedRecord(null);
  }, []);

  const handleCloseForm = useCallback(() => {
    setFormMode(null);
    setEditingRecord(null);
  }, []);

  const handleFormSubmit = useCallback(
    async (data: Record<string, unknown> | FormData): Promise<BaseRecord> => {
      if (!state.collection) throw new Error('Collection not loaded');
      if (formMode === 'edit' && editingRecord) {
        return client.updateRecord(state.collection.name, editingRecord.id, data);
      }
      return client.createRecord(state.collection.name, data);
    },
    [state.collection, formMode, editingRecord],
  );

  const handleFormSave = useCallback(
    (_savedRecord: BaseRecord) => {
      handleCloseForm();
      // Refresh records list
      if (state.collection) {
        fetchRecords(state.collection);
      }
    },
    [handleCloseForm, state.collection, fetchRecords],
  );

  const handleSearchRelation = useCallback(
    async (collectionId: string, query: string): Promise<RelationOption[]> => {
      try {
        const targetCollection = allCollections.find((c) => c.id === collectionId);
        const colName = targetCollection?.name ?? collectionId;
        const result = await client.listRecords(colName, {
          perPage: 20,
          filter: query ? `id ~ '${query}'` : undefined,
        });
        return result.items.map((r) => ({
          id: r.id,
          label: getRecordLabel(r),
        }));
      } catch {
        return [];
      }
    },
    [allCollections],
  );

  // Fetch all collections for relation pickers
  useEffect(() => {
    client.listCollections().then((resp) => {
      setAllCollections(resp.items);
    }).catch(() => {
      // Non-critical, relation pickers will show IDs only
    });
  }, []);

  // ── Delete record handler ─────────────────────────────────────────────

  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);

  const handleDeleteRecord = useCallback(
    async (recordId: string) => {
      if (!state.collection) return;
      try {
        await client.deleteRecord(state.collection.name, recordId);
        setDeleteConfirmId(null);
        setSelectedRecord(null);
        fetchRecords(state.collection);
      } catch (err) {
        const message = err instanceof ApiError ? err.message : 'Failed to delete record.';
        setState((prev) => ({ ...prev, error: message }));
      }
    },
    [state.collection, fetchRecords],
  );

  const collectionName = state.collection?.name ?? 'Loading…';

  return (
    <DashboardLayout currentPath={`/_/collections/${collectionId}`} pageTitle={collectionName}>
      {/* Breadcrumb */}
      <nav className="mb-4 text-sm" aria-label="Breadcrumb">
        <ol className="flex items-center gap-1.5 text-gray-500 dark:text-gray-400">
          <li>
            <a href="/_/collections" className="hover:text-blue-600 dark:hover:text-blue-400">Collections</a>
          </li>
          <li aria-hidden="true">/</li>
          <li className="font-medium text-gray-900 dark:text-gray-100">{collectionName}</li>
        </ol>
      </nav>

      {/* Toolbar */}
      <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        {/* Filter input */}
        <form onSubmit={handleFilterSubmit} className="flex gap-2 sm:max-w-md sm:flex-1">
          <label htmlFor="record-filter" className="sr-only">Filter records</label>
          <input
            id="record-filter"
            type="text"
            placeholder="Filter records… (e.g. title = 'hello')"
            value={filterInput}
            onChange={(e) => setFilterInput(e.target.value)}
            className="w-full rounded-md border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 px-3 py-2 text-sm text-gray-900 dark:text-gray-100 placeholder-gray-400 dark:placeholder-gray-500 focus:border-blue-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-blue-500"
          />
          <button
            type="submit"
            className="rounded-md bg-blue-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-700 dark:hover:bg-blue-600"
          >
            Filter
          </button>
          {filter && (
            <button
              type="button"
              onClick={handleClearFilter}
              className="rounded-md border border-gray-300 dark:border-gray-600 px-3 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 transition-colors hover:bg-gray-50 dark:hover:bg-gray-700"
            >
              Clear
            </button>
          )}
        </form>

        {/* Column toggle + edit schema link */}
        <div className="flex items-center gap-2">
          {state.collection && (
            <ColumnToggle
              columns={allColumns}
              visibleColumns={visibleColumns}
              onToggle={handleColumnToggle}
            />
          )}
          <button
            type="button"
            onClick={handleOpenCreate}
            className="inline-flex items-center gap-1.5 rounded-md bg-blue-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-700 dark:hover:bg-blue-600"
            data-testid="new-record-btn"
          >
            <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
            New Record
          </button>
          <a
            href={`/_/collections/${encodeURIComponent(collectionId)}/edit`}
            className="inline-flex items-center gap-1.5 rounded-md border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 px-3 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 transition-colors hover:bg-gray-50 dark:hover:bg-gray-700"
          >
            Edit Schema
          </a>
        </div>
      </div>

      {/* Active filter indicator */}
      {filter && (
        <div className="mb-3 flex items-center gap-2 rounded-md bg-blue-50 dark:bg-blue-900/30 px-3 py-2 text-sm text-blue-700 dark:text-blue-400">
          <span>Active filter:</span>
          <code className="rounded bg-blue-100 dark:bg-blue-900/20 px-1.5 py-0.5 font-mono text-xs">{filter}</code>
          <button
            type="button"
            onClick={handleClearFilter}
            className="ml-auto text-blue-600 dark:text-blue-400 hover:text-blue-800 dark:hover:text-blue-300"
            aria-label="Remove filter"
          >
            <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>
      )}

      {/* Error state */}
      {state.error && (
        <div role="alert" className="mb-4 rounded-md bg-red-50 dark:bg-red-900/30 p-4">
          <div className="flex">
            <svg className="h-5 w-5 text-red-400 dark:text-red-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <circle cx="12" cy="12" r="10" />
              <line x1="12" y1="8" x2="12" y2="12" />
              <line x1="12" y1="16" x2="12.01" y2="16" />
            </svg>
            <div className="ml-3">
              <p className="text-sm text-red-700 dark:text-red-400">{state.error}</p>
              <button
                type="button"
                onClick={handleRetry}
                className="mt-1 text-sm font-medium text-red-700 dark:text-red-400 underline hover:text-red-800 dark:hover:text-red-300"
              >
                Retry
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Loading state */}
      {state.loading && <TableSkeleton />}

      {/* Records table */}
      {!state.loading && !state.error && state.records && (
        <>
          {state.records.items.length === 0 ? (
            <div className="py-12 text-center">
              <p className="text-sm text-gray-500 dark:text-gray-400">
                {filter ? 'No records match the current filter.' : 'No records in this collection.'}
              </p>
              {filter && (
                <button
                  type="button"
                  onClick={handleClearFilter}
                  className="mt-2 text-sm font-medium text-blue-600 dark:text-blue-400 hover:text-blue-700 dark:hover:text-blue-300"
                >
                  Clear filter
                </button>
              )}
            </div>
          ) : (
            <div className="overflow-x-auto rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800">
              <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-700">
                <thead className="bg-gray-50 dark:bg-gray-900">
                  <tr>
                    {displayedColumns.map((col) => (
                      <th
                        key={col}
                        scope="col"
                        className="cursor-pointer select-none whitespace-nowrap px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 transition-colors hover:text-gray-700 dark:hover:text-gray-300 sm:px-6"
                        onClick={() => handleSort(col)}
                        aria-sort={
                          sort?.field === col
                            ? sort.direction === 'asc' ? 'ascending' : 'descending'
                            : 'none'
                        }
                      >
                        {col}
                        <SortIndicator field={col} sort={sort} />
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 dark:divide-gray-700">
                  {state.records.items.map((record) => (
                    <tr
                      key={record.id}
                      className="cursor-pointer transition-colors hover:bg-gray-50 dark:hover:bg-gray-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-blue-500"
                      onClick={() => setSelectedRecord(record)}
                      onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setSelectedRecord(record); } }}
                      tabIndex={0}
                      aria-label={`View record ${record.id}`}
                      data-testid={`record-row-${record.id}`}
                    >
                      {displayedColumns.map((col) => (
                        <td
                          key={col}
                          className="whitespace-nowrap px-4 py-3 text-sm text-gray-700 dark:text-gray-300 sm:px-6"
                        >
                          {col === 'id' ? (
                            <span className="font-mono text-xs text-blue-600 dark:text-blue-400">{record.id}</span>
                          ) : (
                            truncate(formatCellValue(record[col]), 80)
                          )}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          {/* Pagination */}
          {state.records.totalItems > 0 && (
            <div className="mt-4">
              <Pagination
                page={state.records.page}
                totalPages={state.records.totalPages}
                totalItems={state.records.totalItems}
                perPage={perPage}
                onPageChange={setPage}
                onPerPageChange={handlePerPageChange}
              />
            </div>
          )}
        </>
      )}

      {/* Record detail panel */}
      {selectedRecord && state.collection && (
        <RecordDetail
          record={selectedRecord}
          collection={state.collection}
          onClose={() => setSelectedRecord(null)}
          onEdit={handleOpenEdit}
          onDelete={handleDeleteRecord}
          deleteConfirmId={deleteConfirmId}
          onDeleteConfirm={setDeleteConfirmId}
        />
      )}

      {/* Record create/edit form modal */}
      {formMode && state.collection && (
        <RecordFormModal
          collection={state.collection}
          record={editingRecord}
          onClose={handleCloseForm}
          onSave={handleFormSave}
          onSubmit={handleFormSubmit}
          collections={allCollections}
          onSearchRelation={handleSearchRelation}
        />
      )}
    </DashboardLayout>
  );
}
