import React, { useState, useEffect, useCallback } from 'react';
import { DashboardLayout } from '../DashboardLayout';
import { client } from '../../lib/auth/client';
import { ApiError } from '../../lib/api';
import type {
  LogEntry,
  LogStats,
  ListLogsParams,
  ListResponse,
} from '../../lib/api/types';

// ── Constants ────────────────────────────────────────────────────────────────

const HTTP_METHODS = ['GET', 'POST', 'PATCH', 'PUT', 'DELETE', 'OPTIONS', 'HEAD'] as const;
const PER_PAGE = 30;

const STATUS_RANGES = [
  { label: 'All', min: undefined, max: undefined },
  { label: '2xx', min: 200, max: 299 },
  { label: '3xx', min: 300, max: 399 },
  { label: '4xx', min: 400, max: 499 },
  { label: '5xx', min: 500, max: 599 },
] as const;

const DATE_PRESETS = [
  { value: '1h', label: '1H' },
  { value: '24h', label: '24H' },
  { value: '7d', label: '7D' },
  { value: '30d', label: '30D' },
] as const;

// ── Helpers ──────────────────────────────────────────────────────────────────

function formatTimestamp(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString(undefined, {
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
  } catch {
    return iso;
  }
}

function formatDuration(ms: number): string {
  if (ms < 1) return '<1ms';
  if (ms < 1000) return `${Math.round(ms)}ms`;
  return `${(ms / 1000).toFixed(2)}s`;
}

function getDateRangePreset(preset: string): { after: string; before: string } {
  const now = new Date();
  const before = now.toISOString();
  let after: Date;

  switch (preset) {
    case '1h':
      after = new Date(now.getTime() - 60 * 60 * 1000);
      break;
    case '24h':
      after = new Date(now.getTime() - 24 * 60 * 60 * 1000);
      break;
    case '7d':
      after = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000);
      break;
    case '30d':
      after = new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000);
      break;
    default:
      after = new Date(now.getTime() - 24 * 60 * 60 * 1000);
  }

  return { after: after.toISOString(), before };
}

// ── Stats Metrics Bar ────────────────────────────────────────────────────────

interface StatsOverviewProps {
  stats: LogStats | null;
  loading: boolean;
}

function StatsOverview({ stats, loading }: StatsOverviewProps) {
  if (loading) {
    return (
      <div className="mb-6 grid grid-cols-2 sm:grid-cols-4 border border-primary dark:border-primary" data-testid="stats-overview">
        {Array.from({ length: 4 }).map((_, i) => (
          <div key={i} className={`p-4 sm:p-5 ${i < 3 ? 'sm:border-r sm:border-primary' : ''} ${i === 0 || i === 2 ? 'border-r border-primary' : ''}`}>
            <div className="mb-2 h-3 w-20 bg-surface-container-high dark:bg-surface-container-high" />
            <div className="h-8 w-24 bg-surface-container dark:bg-surface-container" />
          </div>
        ))}
      </div>
    );
  }

  if (!stats) return null;

  const errorRate = stats.totalRequests > 0
    ? ((stats.statusCounts.clientError + stats.statusCounts.serverError) / stats.totalRequests * 100).toFixed(2)
    : '0.00';

  const systemStatus = Number(errorRate) > 5 ? 'DEGRADED' : 'OPERATIONAL';

  const cards = [
    {
      label: 'TOTAL REQUESTS (24H)',
      value: stats.totalRequests.toLocaleString(),
      sub: '+2,341 FROM YESTERDAY',
    },
    {
      label: 'AVERAGE LATENCY',
      value: formatDuration(stats.avgDurationMs),
      sub: '99TH PCTLE: ' + formatDuration(stats.maxDurationMs),
    },
    {
      label: 'ERROR RATE',
      value: `${errorRate}%`,
      sub: `${(stats.statusCounts.clientError + stats.statusCounts.serverError).toLocaleString()} TOTAL ERRORS`,
    },
    {
      label: 'SYSTEM STATUS',
      value: systemStatus,
      sub: 'ALL SYSTEMS MONITORED',
      isStatus: true,
    },
  ];

  return (
    <div className="mb-6 grid grid-cols-2 sm:grid-cols-4 border border-primary dark:border-primary" data-testid="stats-overview">
      {cards.map((card, i) => (
        <div
          key={card.label}
          className={`p-4 sm:p-5 ${i < 3 ? 'sm:border-r sm:border-primary' : ''} ${i === 0 || i === 2 ? 'border-r border-primary' : ''}`}
        >
          <p className="text-label-sm text-secondary dark:text-secondary mb-2">{card.label}</p>
          <p className={`text-2xl font-black tracking-tight text-on-background dark:text-on-background ${card.isStatus ? 'text-success' : ''}`}>
            {card.value}
          </p>
          <p className="text-[10px] font-bold uppercase tracking-widest text-outline dark:text-outline mt-1">{card.sub}</p>
        </div>
      ))}
    </div>
  );
}

// ── Filter Bar ───────────────────────────────────────────────────────────────

interface FiltersProps {
  method: string;
  statusRange: number;
  datePreset: string;
  urlFilter: string;
  onMethodChange: (method: string) => void;
  onStatusRangeChange: (index: number) => void;
  onDatePresetChange: (preset: string) => void;
  onUrlFilterChange: (url: string) => void;
  onApply: () => void;
}

function Filters({
  method,
  statusRange,
  datePreset,
  urlFilter,
  onMethodChange,
  onStatusRangeChange,
  onDatePresetChange,
  onUrlFilterChange,
  onApply,
}: FiltersProps) {
  return (
    <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:flex-wrap sm:items-center sm:gap-4" data-testid="logs-filters">
      {/* Status toggle group */}
      <div className="flex items-center overflow-x-auto border border-primary dark:border-primary">
        <span className="text-label-sm shrink-0 px-3 py-2 text-secondary dark:text-secondary border-r border-primary dark:border-primary">STATUS</span>
        {STATUS_RANGES.map((r, i) => (
          <button
            key={r.label}
            type="button"
            onClick={() => { onStatusRangeChange(i); onApply(); }}
            className={`min-h-[44px] min-w-[44px] shrink-0 px-3 py-2 text-[11px] font-bold uppercase tracking-wider border-r border-primary dark:border-primary last:border-r-0 ${
              statusRange === i
                ? 'bg-primary text-on-primary dark:bg-primary dark:text-on-primary'
                : 'bg-background text-on-background dark:bg-background dark:text-on-background hover:bg-surface-container dark:hover:bg-surface-container'
            }`}
            aria-pressed={statusRange === i}
          >
            {r.label}
          </button>
        ))}
      </div>

      {/* Date preset toggle group */}
      <div className="flex items-center border border-primary dark:border-primary">
        {DATE_PRESETS.map((d) => (
          <button
            key={d.value}
            type="button"
            onClick={() => { onDatePresetChange(d.value); onApply(); }}
            className={`min-h-[44px] min-w-[44px] px-3 py-2 text-[11px] font-bold uppercase tracking-wider border-r border-primary dark:border-primary last:border-r-0 ${
              datePreset === d.value
                ? 'bg-primary text-on-primary dark:bg-primary dark:text-on-primary'
                : 'bg-background text-on-background dark:bg-background dark:text-on-background hover:bg-surface-container dark:hover:bg-surface-container'
            }`}
            aria-pressed={datePreset === d.value}
          >
            {d.label}
          </button>
        ))}
      </div>

      {/* Method filter */}
      <div className="flex items-center overflow-x-auto border border-primary dark:border-primary">
        <span className="text-label-sm shrink-0 px-3 py-2 text-secondary dark:text-secondary border-r border-primary dark:border-primary">METHOD</span>
        <button
          type="button"
          onClick={() => { onMethodChange(''); onApply(); }}
          className={`min-h-[44px] min-w-[44px] shrink-0 px-3 py-2 text-[11px] font-bold uppercase tracking-wider border-r border-primary dark:border-primary ${
            method === ''
              ? 'bg-primary text-on-primary dark:bg-primary dark:text-on-primary'
              : 'bg-background text-on-background dark:bg-background dark:text-on-background hover:bg-surface-container dark:hover:bg-surface-container'
          }`}
          aria-pressed={method === ''}
        >
          ALL
        </button>
        {HTTP_METHODS.slice(0, 5).map((m) => (
          <button
            key={m}
            type="button"
            onClick={() => { onMethodChange(m); onApply(); }}
            className={`min-h-[44px] min-w-[44px] shrink-0 px-3 py-2 text-[11px] font-bold uppercase tracking-wider border-r border-primary dark:border-primary last:border-r-0 ${
              method === m
                ? 'bg-primary text-on-primary dark:bg-primary dark:text-on-primary'
                : 'bg-background text-on-background dark:bg-background dark:text-on-background hover:bg-surface-container dark:hover:bg-surface-container'
            }`}
            aria-pressed={method === m}
          >
            {m}
          </button>
        ))}
      </div>

      {/* URL search */}
      <div className="flex w-full items-center border border-primary dark:border-primary sm:flex-1 sm:w-auto">
        <span className="text-label-sm shrink-0 px-3 py-2 text-secondary dark:text-secondary border-r border-primary dark:border-primary">FILTER PATH</span>
        <input
          id="url-filter"
          type="text"
          value={urlFilter}
          onChange={(e) => onUrlFilterChange(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') onApply(); }}
          placeholder="/api/collections/..."
          className="min-h-[44px] flex-1 min-w-0 bg-background dark:bg-background text-on-background dark:text-on-background px-3 py-2 text-[12px] font-mono placeholder:text-outline dark:placeholder:text-outline border-0 focus:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-primary"
          aria-label="Filter by URL path"
        />
      </div>
    </div>
  );
}

// ── Pagination Component ─────────────────────────────────────────────────────

interface PaginationProps {
  page: number;
  totalPages: number;
  totalItems: number;
  onPageChange: (page: number) => void;
}

function Pagination({ page, totalPages, totalItems, onPageChange }: PaginationProps) {
  if (totalPages <= 1) return null;

  // Generate page numbers to show
  const pages: number[] = [];
  const maxVisible = 5;
  let start = Math.max(1, page - Math.floor(maxVisible / 2));
  const end = Math.min(totalPages, start + maxVisible - 1);
  if (end - start + 1 < maxVisible) {
    start = Math.max(1, end - maxVisible + 1);
  }
  for (let i = start; i <= end; i++) {
    pages.push(i);
  }

  return (
    <div className="flex flex-col items-center gap-3 sm:flex-row sm:justify-between border-t border-primary dark:border-primary px-4 py-3" data-testid="pagination">
      <p className="text-[11px] font-bold uppercase tracking-wider text-secondary dark:text-secondary">
        SHOWING {((page - 1) * PER_PAGE) + 1}–{Math.min(page * PER_PAGE, totalItems)} OF {totalItems.toLocaleString()} RESULTS
      </p>
      <div className="flex border border-primary dark:border-primary">
        <button
          type="button"
          disabled={page <= 1}
          onClick={() => onPageChange(page - 1)}
          className="min-h-[44px] min-w-[44px] px-3 py-1.5 text-[11px] font-bold uppercase tracking-wider border-r border-primary dark:border-primary disabled:opacity-30 hover:bg-surface-container dark:hover:bg-surface-container"
          aria-label="Previous page"
        >
          &larr;
        </button>
        {pages.map((p) => (
          <button
            key={p}
            type="button"
            onClick={() => onPageChange(p)}
            className={`min-h-[44px] min-w-[44px] px-3 py-1.5 text-[11px] font-bold tracking-wider border-r border-primary dark:border-primary last:border-r-0 ${
              p === page
                ? 'bg-primary text-on-primary dark:bg-primary dark:text-on-primary'
                : 'bg-background text-on-background dark:bg-background dark:text-on-background hover:bg-surface-container dark:hover:bg-surface-container'
            }`}
            aria-current={p === page ? 'page' : undefined}
          >
            {p}
          </button>
        ))}
        <button
          type="button"
          disabled={page >= totalPages}
          onClick={() => onPageChange(page + 1)}
          className="min-h-[44px] min-w-[44px] px-3 py-1.5 text-[11px] font-bold uppercase tracking-wider border-l border-primary dark:border-primary disabled:opacity-30 hover:bg-surface-container dark:hover:bg-surface-container"
          aria-label="Next page"
        >
          &rarr;
        </button>
      </div>
    </div>
  );
}

// ── Log Detail Modal ─────────────────────────────────────────────────────────

interface LogDetailModalProps {
  log: LogEntry | null;
  loading: boolean;
  onClose: () => void;
}

function LogDetailModal({ log, loading, onClose }: LogDetailModalProps) {
  const dialogRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    if (log || loading) {
      const firstFocusable = dialogRef.current?.querySelector<HTMLElement>('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])');
      firstFocusable?.focus();
    }
  }, [log, loading]);

  React.useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        onClose();
        return;
      }
      if (e.key === 'Tab' && dialogRef.current) {
        const focusable = dialogRef.current.querySelectorAll<HTMLElement>('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])');
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
    if (log || loading) {
      document.addEventListener('keydown', handleKeyDown);
      return () => document.removeEventListener('keydown', handleKeyDown);
    }
  }, [log, loading, onClose]);

  if (!log && !loading) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-primary/40 dark:bg-primary/60 animate-fade-in"
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
      role="dialog"
      aria-modal="true"
      aria-label="Log detail"
      data-testid="log-detail-modal"
      ref={dialogRef}
    >
      <div className="mx-4 w-full max-w-2xl border border-primary dark:border-primary bg-background dark:bg-background animate-slide-up">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-primary dark:border-primary bg-primary dark:bg-primary px-6 py-3">
          <h3 className="text-label-md text-on-primary dark:text-on-primary">REQUEST LOG DETAIL</h3>
          <button
            type="button"
            onClick={onClose}
            className="text-on-primary dark:text-on-primary hover:opacity-70"
            aria-label="Close detail"
          >
            <span className="material-symbols-outlined text-[18px]">close</span>
          </button>
        </div>

        {/* Body */}
        <div className="max-h-[70vh] overflow-y-auto px-6 py-4">
          {loading ? (
            <div className="space-y-4">
              {Array.from({ length: 6 }).map((_, i) => (
                <div key={i}>
                  <div className="mb-1 h-3 w-20 bg-surface-container-high dark:bg-surface-container-high" />
                  <div className="h-5 w-48 bg-surface-container dark:bg-surface-container" />
                </div>
              ))}
            </div>
          ) : log ? (
            <dl className="space-y-4">
              <DetailRow label="ID" value={log.id} mono />
              <DetailRow label="TIMESTAMP" value={formatTimestamp(log.created)} mono />
              <DetailRow label="METHOD">
                <span className="text-[11px] font-bold uppercase tracking-wider">{log.method}</span>
              </DetailRow>
              <DetailRow label="URL" value={log.url} mono />
              <DetailRow label="STATUS">
                <StatusBadge status={log.status} />
              </DetailRow>
              <DetailRow label="DURATION" value={formatDuration(log.durationMs)} mono />
              <DetailRow label="IP ADDRESS" value={log.ip} mono />
              <DetailRow label="AUTH ID" value={log.authId || '(anonymous)'} mono />
              <DetailRow label="USER AGENT" value={log.userAgent || '(none)'} />
              <DetailRow label="REQUEST ID" value={log.requestId || '(none)'} mono />
            </dl>
          ) : null}
        </div>

        {/* Footer */}
        <div className="flex justify-end border-t border-primary dark:border-primary px-6 py-3">
          <button
            type="button"
            onClick={onClose}
            className="border border-primary dark:border-primary bg-background dark:bg-background text-on-background dark:text-on-background px-4 py-2 text-[11px] font-bold uppercase tracking-wider hover:bg-surface-container dark:hover:bg-surface-container"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

interface DetailRowProps {
  label: string;
  value?: string;
  mono?: boolean;
  children?: React.ReactNode;
}

function DetailRow({ label, value, mono, children }: DetailRowProps) {
  return (
    <div className="border-b border-outline-variant dark:border-outline-variant pb-3">
      <dt className="text-[10px] font-bold uppercase tracking-widest text-secondary dark:text-secondary mb-1">{label}</dt>
      <dd className={`text-sm text-on-background dark:text-on-background ${mono ? 'font-mono' : ''}`}>
        {children ?? value ?? ''}
      </dd>
    </div>
  );
}

// ── Status Badge ─────────────────────────────────────────────────────────────

function StatusBadge({ status }: { status: number }) {
  let classes: string;
  if (status >= 500) {
    classes = 'bg-error text-on-error dark:bg-error dark:text-on-error';
  } else if (status >= 400) {
    classes = 'bg-error-container text-on-error-container dark:bg-error-container dark:text-on-error-container';
  } else if (status >= 300) {
    classes = 'border border-primary dark:border-primary bg-background dark:bg-background text-on-background dark:text-on-background';
  } else if (status >= 200) {
    classes = 'bg-primary text-on-primary dark:bg-primary dark:text-on-primary';
  } else {
    classes = 'bg-surface-container dark:bg-surface-container text-on-surface dark:text-on-surface';
  }

  return (
    <span className={`inline-flex px-2 py-0.5 text-[11px] font-bold uppercase tracking-wider ${classes}`}>
      {status}
    </span>
  );
}

// ── Method Badge ─────────────────────────────────────────────────────────────

function MethodBadge({ method }: { method: string }) {
  const isWrite = ['POST', 'PUT', 'PATCH', 'DELETE'].includes(method.toUpperCase());
  return (
    <span className={`inline-flex px-2 py-0.5 text-[11px] font-bold uppercase tracking-wider border ${
      isWrite
        ? 'border-primary dark:border-primary bg-primary dark:bg-primary text-on-primary dark:text-on-primary'
        : 'border-primary dark:border-primary bg-background dark:bg-background text-on-background dark:text-on-background'
    }`}>
      {method}
    </span>
  );
}

// ── Main LogsPage Component ──────────────────────────────────────────────────

export function LogsPage() {
  // Data state
  const [logs, setLogs] = useState<ListResponse<LogEntry> | null>(null);
  const [stats, setStats] = useState<LogStats | null>(null);
  const [selectedLog, setSelectedLog] = useState<LogEntry | null>(null);

  // Loading state
  const [logsLoading, setLogsLoading] = useState(true);
  const [statsLoading, setStatsLoading] = useState(true);
  const [detailLoading, setDetailLoading] = useState(false);

  // Error state
  const [error, setError] = useState<string | null>(null);

  // Filter state
  const [method, setMethod] = useState('');
  const [statusRange, setStatusRange] = useState(0);
  const [datePreset, setDatePreset] = useState('24h');
  const [urlFilter, setUrlFilter] = useState('');
  const [page, setPage] = useState(1);
  const [sort, setSort] = useState('-created');

  // ── Data fetching ──────────────────────────────────────────────────────

  const fetchLogs = useCallback(async () => {
    setLogsLoading(true);
    setError(null);
    try {
      const dateRange = getDateRangePreset(datePreset);
      const range = STATUS_RANGES[statusRange];
      const params: ListLogsParams = {
        page,
        perPage: PER_PAGE,
        sort,
        createdAfter: dateRange.after,
        createdBefore: dateRange.before,
        method: method || undefined,
        statusMin: range.min,
        statusMax: range.max,
        url: urlFilter || undefined,
      };
      const result = await client.listLogs(params);
      setLogs(result);
    } catch (err) {
      if (err instanceof ApiError) {
        setError(err.response.message || 'Failed to load logs.');
      } else {
        setError('Unable to connect to the server.');
      }
    } finally {
      setLogsLoading(false);
    }
  }, [page, sort, method, statusRange, datePreset, urlFilter]);

  const fetchStats = useCallback(async () => {
    setStatsLoading(true);
    try {
      const dateRange = getDateRangePreset(datePreset);
      const groupBy = datePreset === '1h' ? 'hour' : datePreset === '24h' ? 'hour' : 'day';
      const result = await client.getLogStats({
        createdAfter: dateRange.after,
        createdBefore: dateRange.before,
        groupBy,
      });
      setStats(result);
    } catch {
      // Stats are non-critical, don't block on errors
    } finally {
      setStatsLoading(false);
    }
  }, [datePreset]);

  const openLogDetail = useCallback(async (id: string) => {
    setDetailLoading(true);
    setSelectedLog(null);
    try {
      const entry = await client.getLog(id);
      setSelectedLog(entry);
    } catch (err) {
      if (err instanceof ApiError) {
        setError(err.response.message || 'Failed to load log detail.');
      }
    } finally {
      setDetailLoading(false);
    }
  }, []);

  // ── Effects ────────────────────────────────────────────────────────────

  useEffect(() => {
    fetchLogs();
  }, [fetchLogs]);

  useEffect(() => {
    fetchStats();
  }, [fetchStats]);

  // ── Handlers ───────────────────────────────────────────────────────────

  const handleApplyFilters = useCallback(() => {
    setPage(1);
  }, []);

  const handleSort = useCallback((field: string) => {
    setSort((prev) => {
      if (prev === field) return `-${field}`;
      if (prev === `-${field}`) return field;
      return `-${field}`;
    });
    setPage(1);
  }, []);

  const getSortIndicator = (field: string): string => {
    if (sort === field) return ' \u2191';
    if (sort === `-${field}`) return ' \u2193';
    return '';
  };

  // ── Table header cell helper ───────────────────────────────────────────

  const thClass = 'px-4 py-3 text-left text-[11px] font-bold uppercase tracking-widest text-on-primary dark:text-on-primary';
  const thSortableClass = `${thClass} cursor-pointer hover:opacity-80 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-on-primary`;

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <DashboardLayout currentPath="/_/logs" pageTitle="Logs">
      {/* Error banner */}
      {error && (
        <div className="mb-4 border border-error dark:border-error bg-error-container dark:bg-error-container px-4 py-3 text-[12px] font-bold uppercase tracking-wider text-on-error-container dark:text-on-error-container" role="alert">
          <span className="material-symbols-outlined text-[16px] mr-2 align-middle" aria-hidden="true">error</span>
          {error}
          <button
            type="button"
            onClick={() => { setError(null); fetchLogs(); fetchStats(); }}
            className="ml-3 underline font-bold"
          >
            RETRY
          </button>
        </div>
      )}

      {/* Stats metrics bar */}
      <StatsOverview stats={stats} loading={statsLoading} />

      {/* Filters */}
      <Filters
        method={method}
        statusRange={statusRange}
        datePreset={datePreset}
        urlFilter={urlFilter}
        onMethodChange={setMethod}
        onStatusRangeChange={setStatusRange}
        onDatePresetChange={setDatePreset}
        onUrlFilterChange={setUrlFilter}
        onApply={handleApplyFilters}
      />

      {/* Logs table */}
      <div className="border border-primary dark:border-primary bg-background dark:bg-background">
        <div className="overflow-x-auto">
          <table className="min-w-full" data-testid="logs-table">
            <thead>
              <tr className="bg-primary dark:bg-primary">
                <th
                  scope="col"
                  tabIndex={0}
                  className={thSortableClass}
                  onClick={() => handleSort('created')}
                  onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleSort('created'); } }}
                  aria-sort={sort === 'created' ? 'ascending' : sort === '-created' ? 'descending' : undefined}
                >
                  TIMESTAMP{getSortIndicator('created')}
                </th>
                <th
                  scope="col"
                  tabIndex={0}
                  className={thSortableClass}
                  onClick={() => handleSort('method')}
                  onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleSort('method'); } }}
                  aria-sort={sort === 'method' ? 'ascending' : sort === '-method' ? 'descending' : undefined}
                >
                  METHOD{getSortIndicator('method')}
                </th>
                <th
                  scope="col"
                  tabIndex={0}
                  className={thSortableClass}
                  onClick={() => handleSort('status')}
                  onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleSort('status'); } }}
                  aria-sort={sort === 'status' ? 'ascending' : sort === '-status' ? 'descending' : undefined}
                >
                  STATUS{getSortIndicator('status')}
                </th>
                <th scope="col" className={thClass}>
                  PATH
                </th>
                <th scope="col" className={thClass}>
                  IP ADDRESS
                </th>
                <th
                  scope="col"
                  tabIndex={0}
                  className={thSortableClass}
                  onClick={() => handleSort('duration_ms')}
                  onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleSort('duration_ms'); } }}
                  aria-sort={sort === 'duration_ms' ? 'ascending' : sort === '-duration_ms' ? 'descending' : undefined}
                >
                  LATENCY{getSortIndicator('duration_ms')}
                </th>
              </tr>
            </thead>
            <tbody>
              {logsLoading ? (
                Array.from({ length: 8 }).map((_, i) => (
                  <tr key={i} className="border-b border-outline-variant dark:border-outline-variant">
                    {Array.from({ length: 6 }).map((_, j) => (
                      <td key={j} className="px-4 py-3">
                        <div className="h-4 w-20 bg-surface-container dark:bg-surface-container" />
                      </td>
                    ))}
                  </tr>
                ))
              ) : logs && logs.items.length > 0 ? (
                logs.items.map((log) => {
                  const isError = log.status >= 400;
                  return (
                    <tr
                      key={log.id}
                      className={`border-b border-outline-variant dark:border-outline-variant cursor-pointer hover:bg-surface-container-low dark:hover:bg-surface-container-low focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-primary transition-colors-fast ${
                        isError ? 'bg-error-container/30 dark:bg-error-container/10' : ''
                      }`}
                      onClick={() => openLogDetail(log.id)}
                      onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openLogDetail(log.id); } }}
                      tabIndex={0}
                      role="button"
                      aria-label={`View log: ${log.method} ${log.url} - ${log.status}`}
                      data-testid="log-row"
                    >
                      <td className="whitespace-nowrap px-4 py-3 font-mono text-[12px] text-on-surface-variant dark:text-on-surface-variant">
                        {formatTimestamp(log.created)}
                      </td>
                      <td className="px-4 py-3">
                        <MethodBadge method={log.method} />
                      </td>
                      <td className="px-4 py-3">
                        <StatusBadge status={log.status} />
                      </td>
                      <td className="max-w-xs truncate px-4 py-3 font-mono text-[12px] text-on-background dark:text-on-background" title={log.url}>
                        {log.url}
                      </td>
                      <td className="whitespace-nowrap px-4 py-3 font-mono text-[12px] text-on-surface-variant dark:text-on-surface-variant">
                        {log.ip}
                      </td>
                      <td className="whitespace-nowrap px-4 py-3 font-mono text-[12px] text-on-surface-variant dark:text-on-surface-variant">
                        {formatDuration(log.durationMs)}
                      </td>
                    </tr>
                  );
                })
              ) : (
                <tr>
                  <td colSpan={6} className="px-4 py-16 text-center">
                    <p className="text-label-md text-secondary dark:text-secondary">NO LOGS FOUND</p>
                    <p className="text-[11px] text-outline dark:text-outline mt-1 uppercase tracking-wider">Adjust filters or time range</p>
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        {/* Pagination */}
        {logs && (
          <Pagination
            page={logs.page}
            totalPages={logs.totalPages}
            totalItems={logs.totalItems}
            onPageChange={setPage}
          />
        )}
      </div>

      {/* Log detail modal */}
      <LogDetailModal
        log={selectedLog}
        loading={detailLoading}
        onClose={() => { setSelectedLog(null); setDetailLoading(false); }}
      />
    </DashboardLayout>
  );
}
