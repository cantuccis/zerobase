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
    <div className="space-y-0 animate-pulse-subtle" data-testid="table-skeleton">
      {[1, 2, 3, 4, 5].map((i) => (
        <div key={i} className="animate-pulse border border-primary dark:border-on-primary bg-surface dark:bg-surface p-4">
          <div className="flex gap-4">
            <div className="h-4 w-24 bg-surface-container dark:bg-surface-container" />
            <div className="h-4 w-32 bg-surface-container dark:bg-surface-container" />
            <div className="h-4 w-20 bg-surface-container dark:bg-surface-container" />
            <div className="h-4 w-28 bg-surface-container dark:bg-surface-container" />
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
      <svg className="ml-1 inline h-3 w-3 text-on-primary/40 dark:text-primary/40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
        <path d="M8 9l4-4 4 4M8 15l4 4 4-4" />
      </svg>
    );
  }
  return sort.direction === 'asc' ? (
    <svg className="ml-1 inline h-3 w-3 text-on-primary dark:text-primary" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
      <path d="M8 15l4-4 4 4" />
    </svg>
  ) : (
    <svg className="ml-1 inline h-3 w-3 text-on-primary dark:text-primary" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
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
        className="inline-flex items-center gap-1.5 border border-primary dark:border-on-primary bg-surface dark:bg-surface px-3 py-2 text-sm font-medium text-on-surface dark:text-on-surface hover:bg-surface-container-low dark:hover:bg-surface-container-low"
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
            className="absolute right-0 z-20 mt-1 w-56 border border-primary dark:border-on-primary bg-surface dark:bg-surface py-1 animate-slide-down-in"
            role="listbox"
            aria-label="Column visibility options"
            onKeyDown={(e) => { if (e.key === 'Escape') setOpen(false); }}
          >
            {columns.map((col) => (
              <label
                key={col}
                className="flex cursor-pointer items-center gap-2 px-3 py-1.5 text-sm text-on-surface dark:text-on-surface hover:bg-surface-container-low dark:hover:bg-surface-container-low"
              >
                <input
                  type="checkbox"
                  checked={visibleColumns.has(col)}
                  onChange={() => onToggle(col)}
                  className="h-4 w-4 border-primary dark:border-on-primary text-primary dark:text-on-primary focus:ring-1 focus:ring-primary"
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
  // Generate page numbers to display
  const pageNumbers: number[] = [];
  const maxVisible = 5;
  let start = Math.max(1, page - Math.floor(maxVisible / 2));
  const end = Math.min(totalPages, start + maxVisible - 1);
  if (end - start + 1 < maxVisible) {
    start = Math.max(1, end - maxVisible + 1);
  }
  for (let i = start; i <= end; i++) {
    pageNumbers.push(i);
  }

  return (
    <div className="flex flex-col items-center justify-between gap-3 sm:flex-row" data-testid="pagination">
      <div className="flex items-center gap-2 text-sm text-secondary dark:text-secondary">
        <span>{totalItems} record{totalItems !== 1 ? 's' : ''}</span>
        <span className="text-outline-variant dark:text-outline-variant">|</span>
        <label htmlFor="per-page-select" className="sr-only">Records per page</label>
        <select
          id="per-page-select"
          value={perPage}
          onChange={(e) => onPerPageChange(Number(e.target.value))}
          className="border border-primary dark:border-on-primary bg-surface dark:bg-surface text-on-surface dark:text-on-surface py-1 pl-2 pr-6 text-sm focus:border-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary"
        >
          {PER_PAGE_OPTIONS.map((n) => (
            <option key={n} value={n}>{n} per page</option>
          ))}
        </select>
      </div>

      <div className="flex items-center gap-0">
        <button
          type="button"
          onClick={() => onPageChange(1)}
          disabled={page <= 1}
          className="border border-primary dark:border-on-primary px-2.5 py-1 text-sm text-on-surface dark:text-on-surface hover:bg-surface-container-low dark:hover:bg-surface-container-low disabled:cursor-not-allowed disabled:opacity-40"
          aria-label="First page"
        >
          ««
        </button>
        <button
          type="button"
          onClick={() => onPageChange(page - 1)}
          disabled={page <= 1}
          className="-ml-px border border-primary dark:border-on-primary px-2.5 py-1 text-sm text-on-surface dark:text-on-surface hover:bg-surface-container-low dark:hover:bg-surface-container-low disabled:cursor-not-allowed disabled:opacity-40"
          aria-label="Previous page"
        >
          «
        </button>

        {pageNumbers.map((n) => (
          <button
            key={n}
            type="button"
            onClick={() => onPageChange(n)}
            className={`-ml-px border border-primary dark:border-on-primary px-3 py-1 text-sm font-medium ${
              n === page
                ? 'bg-primary dark:bg-on-primary text-on-primary dark:text-primary'
                : 'text-on-surface dark:text-on-surface hover:bg-surface-container-low dark:hover:bg-surface-container-low'
            }`}
            aria-label={`Page ${n}`}
            aria-current={n === page ? 'page' : undefined}
          >
            {n}
          </button>
        ))}

        <button
          type="button"
          onClick={() => onPageChange(page + 1)}
          disabled={page >= totalPages}
          className="-ml-px border border-primary dark:border-on-primary px-2.5 py-1 text-sm text-on-surface dark:text-on-surface hover:bg-surface-container-low dark:hover:bg-surface-container-low disabled:cursor-not-allowed disabled:opacity-40"
          aria-label="Next page"
        >
          »
        </button>
        <button
          type="button"
          onClick={() => onPageChange(totalPages)}
          disabled={page >= totalPages}
          className="-ml-px border border-primary dark:border-on-primary px-2.5 py-1 text-sm text-on-surface dark:text-on-surface hover:bg-surface-container-low dark:hover:bg-surface-container-low disabled:cursor-not-allowed disabled:opacity-40"
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
      className="fixed inset-0 z-50 flex items-start justify-end bg-primary/30 dark:bg-on-primary/30 animate-fade-in"
      role="dialog"
      aria-modal="true"
      aria-labelledby="record-detail-title"
    >
      <div className="h-full w-full max-w-lg overflow-y-auto border-l border-primary dark:border-on-primary bg-surface dark:bg-surface sm:w-[480px] animate-slide-right-in">
        <div className="flex items-center justify-between border-b border-primary dark:border-on-primary bg-primary dark:bg-on-primary px-4 py-3">
          <h3 id="record-detail-title" className="text-sm font-semibold text-on-primary dark:text-primary">
            Record: <span className="font-mono">{record.id}</span>
          </h3>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={() => onEdit(record)}
              className="p-1.5 text-on-primary dark:text-primary hover:text-on-primary/70 dark:hover:text-primary/70"
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
                  className="bg-error px-2 py-1 text-xs font-medium text-on-error"
                  data-testid="confirm-delete-btn"
                >
                  Confirm
                </button>
                <button
                  type="button"
                  onClick={() => onDeleteConfirm(null)}
                  className="border border-on-primary dark:border-primary px-2 py-1 text-xs font-medium text-on-primary dark:text-primary"
                  data-testid="cancel-delete-btn"
                >
                  Cancel
                </button>
              </div>
            ) : (
              <button
                type="button"
                onClick={() => onDeleteConfirm(record.id)}
                className="p-1.5 text-on-primary dark:text-primary hover:text-error dark:hover:text-error"
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
              className="p-1.5 text-on-primary dark:text-primary hover:text-on-primary/70 dark:hover:text-primary/70"
              aria-label="Close record detail"
            >
              <svg className="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </div>
        </div>

        <dl className="px-4">
          {allFields.map((field) => (
            <div key={field} className="border-b border-outline-variant dark:border-outline-variant py-3">
              <dt className="text-label-sm font-bold uppercase tracking-[0.05em] text-secondary dark:text-secondary">{field}</dt>
              <dd className="mt-1 whitespace-pre-wrap break-all text-sm text-on-surface dark:text-on-surface">
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
      <nav className="mb-6 text-sm" aria-label="Breadcrumb">
        <ol className="flex items-center gap-1.5 text-secondary dark:text-secondary">
          <li>
            <a href="/_/collections" className="hover:text-on-surface dark:hover:text-on-surface underline">Collections</a>
          </li>
          <li aria-hidden="true">/</li>
          <li className="font-medium text-on-surface dark:text-on-surface">{collectionName}</li>
        </ol>
      </nav>

      {/* Toolbar */}
      <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        {/* Filter input */}
        <form onSubmit={handleFilterSubmit} className="flex gap-0 sm:max-w-md sm:flex-1">
          <label htmlFor="record-filter" className="sr-only">Filter records</label>
          <input
            id="record-filter"
            type="text"
            placeholder="Filter records… (e.g. title = 'hello')"
            value={filterInput}
            onChange={(e) => setFilterInput(e.target.value)}
            className="w-full border border-primary dark:border-on-primary bg-surface dark:bg-surface px-3 py-2 text-sm text-on-surface dark:text-on-surface placeholder-secondary dark:placeholder-secondary focus:border-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary"
          />
          <button
            type="submit"
            className="-ml-px bg-primary dark:bg-on-primary border border-primary dark:border-on-primary px-3 py-2 text-sm font-medium text-on-primary dark:text-primary"
          >
            Filter
          </button>
          {filter && (
            <button
              type="button"
              onClick={handleClearFilter}
              className="-ml-px border border-primary dark:border-on-primary bg-surface dark:bg-surface px-3 py-2 text-sm font-medium text-on-surface dark:text-on-surface hover:bg-surface-container-low dark:hover:bg-surface-container-low"
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
            className="inline-flex items-center gap-1.5 bg-primary dark:bg-on-primary px-3 py-2 text-sm font-medium text-on-primary dark:text-primary"
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
            className="inline-flex items-center gap-1.5 border border-primary dark:border-on-primary bg-surface dark:bg-surface px-3 py-2 text-sm font-medium text-on-surface dark:text-on-surface hover:bg-surface-container-low dark:hover:bg-surface-container-low"
          >
            Edit Schema
          </a>
        </div>
      </div>

      {/* Active filter indicator */}
      {filter && (
        <div className="mb-3 flex items-center gap-2 border border-primary dark:border-on-primary px-3 py-2 text-sm text-on-surface dark:text-on-surface">
          <span className="text-label-sm font-bold uppercase tracking-[0.05em]">Active filter:</span>
          <code className="border border-outline-variant dark:border-outline-variant bg-surface-container-low dark:bg-surface-container-low px-1.5 py-0.5 font-mono text-xs">{filter}</code>
          <button
            type="button"
            onClick={handleClearFilter}
            className="ml-auto text-secondary dark:text-secondary hover:text-on-surface dark:hover:text-on-surface"
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
        <div role="alert" className="mb-4 border border-error dark:border-error p-4">
          <div className="flex">
            <svg className="h-5 w-5 text-error dark:text-error" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <circle cx="12" cy="12" r="10" />
              <line x1="12" y1="8" x2="12" y2="12" />
              <line x1="12" y1="16" x2="12.01" y2="16" />
            </svg>
            <div className="ml-3">
              <p className="text-sm text-error dark:text-error">{state.error}</p>
              <button
                type="button"
                onClick={handleRetry}
                className="mt-1 text-sm font-medium text-on-surface dark:text-on-surface underline"
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
              <p className="text-sm text-secondary dark:text-secondary">
                {filter ? 'No records match the current filter.' : 'No records in this collection.'}
              </p>
              {filter && (
                <button
                  type="button"
                  onClick={handleClearFilter}
                  className="mt-2 text-sm font-medium text-on-surface dark:text-on-surface underline"
                >
                  Clear filter
                </button>
              )}
            </div>
          ) : (
            <div className="overflow-x-auto border border-primary dark:border-on-primary bg-surface dark:bg-surface">
              <table className="min-w-full divide-y divide-primary dark:divide-on-primary">
                <thead className="bg-primary dark:bg-on-primary">
                  <tr>
                    {displayedColumns.map((col) => (
                      <th
                        key={col}
                        scope="col"
                        className="cursor-pointer select-none whitespace-nowrap px-4 py-3 text-left text-label-sm font-bold uppercase tracking-[0.05em] text-on-primary dark:text-primary transition-colors-fast hover:text-on-primary/70 dark:hover:text-primary/70 sm:px-6"
                        onClick={() => handleSort(col)}
                        onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleSort(col); } }}
                        tabIndex={0}
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
                <tbody className="divide-y divide-outline-variant dark:divide-outline-variant">
                  {state.records.items.map((record) => (
                    <tr
                      key={record.id}
                      className="cursor-pointer transition-colors-fast hover:bg-surface-container-low dark:hover:bg-surface-container-low focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-primary"
                      onClick={() => setSelectedRecord(record)}
                      onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setSelectedRecord(record); } }}
                      tabIndex={0}
                      aria-label={`View record ${record.id}`}
                      data-testid={`record-row-${record.id}`}
                    >
                      {displayedColumns.map((col) => (
                        <td
                          key={col}
                          className="whitespace-nowrap px-4 py-3 text-sm text-on-surface dark:text-on-surface sm:px-6"
                        >
                          {col === 'id' ? (
                            <span className="font-mono text-xs text-on-surface dark:text-on-surface">{record.id}</span>
                          ) : col === 'created' || col === 'updated' ? (
                            <span className="font-mono text-xs">{truncate(formatCellValue(record[col]), 80)}</span>
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
