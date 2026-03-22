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
  { label: '2xx Success', min: 200, max: 299 },
  { label: '3xx Redirect', min: 300, max: 399 },
  { label: '4xx Client Error', min: 400, max: 499 },
  { label: '5xx Server Error', min: 500, max: 599 },
] as const;

// ── Helpers ──────────────────────────────────────────────────────────────────

function formatTimestamp(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
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

function statusColorClass(status: number): string {
  if (status >= 500) return 'bg-red-100 dark:bg-red-900/20 text-red-800 dark:text-red-300';
  if (status >= 400) return 'bg-yellow-100 dark:bg-yellow-900/30 text-yellow-800 dark:text-yellow-300';
  if (status >= 300) return 'bg-blue-100 dark:bg-blue-900/20 text-blue-800 dark:text-blue-300';
  if (status >= 200) return 'bg-green-100 dark:bg-green-900/20 text-green-800 dark:text-green-300';
  return 'bg-gray-100 dark:bg-gray-700 text-gray-800 dark:text-gray-200';
}

function methodColorClass(method: string): string {
  switch (method.toUpperCase()) {
    case 'GET': return 'text-blue-700 dark:text-blue-400 bg-blue-50 dark:bg-blue-900/30';
    case 'POST': return 'text-green-700 dark:text-green-400 bg-green-50 dark:bg-green-900/30';
    case 'PATCH':
    case 'PUT': return 'text-amber-700 dark:text-amber-400 bg-amber-50 dark:bg-amber-900/30';
    case 'DELETE': return 'text-red-700 dark:text-red-400 bg-red-50 dark:bg-red-900/30';
    default: return 'text-gray-700 dark:text-gray-300 bg-gray-50 dark:bg-gray-900';
  }
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

// ── Stats Overview Component ─────────────────────────────────────────────────

interface StatsOverviewProps {
  stats: LogStats | null;
  loading: boolean;
}

function StatsOverview({ stats, loading }: StatsOverviewProps) {
  if (loading) {
    return (
      <div className="mb-6 grid grid-cols-2 gap-4 sm:grid-cols-4">
        {Array.from({ length: 4 }).map((_, i) => (
          <div key={i} className="animate-pulse rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4">
            <div className="mb-2 h-4 w-24 rounded bg-gray-200 dark:bg-gray-600" />
            <div className="h-8 w-16 rounded bg-gray-200 dark:bg-gray-600" />
          </div>
        ))}
      </div>
    );
  }

  if (!stats) return null;

  const errorRate = stats.totalRequests > 0
    ? ((stats.statusCounts.clientError + stats.statusCounts.serverError) / stats.totalRequests * 100).toFixed(1)
    : '0.0';

  const cards = [
    {
      label: 'Total Requests',
      value: stats.totalRequests.toLocaleString(),
      color: 'text-gray-900 dark:text-gray-100',
    },
    {
      label: 'Error Rate',
      value: `${errorRate}%`,
      color: Number(errorRate) > 10 ? 'text-red-600 dark:text-red-400' : 'text-green-600 dark:text-green-400',
    },
    {
      label: 'Avg Duration',
      value: formatDuration(stats.avgDurationMs),
      color: 'text-gray-900 dark:text-gray-100',
    },
    {
      label: 'Max Duration',
      value: formatDuration(stats.maxDurationMs),
      color: stats.maxDurationMs > 5000 ? 'text-amber-600 dark:text-amber-400' : 'text-gray-900 dark:text-gray-100',
    },
  ];

  return (
    <div className="mb-6 grid grid-cols-2 gap-4 sm:grid-cols-4" data-testid="stats-overview">
      {cards.map((card) => (
        <div key={card.label} className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4">
          <p className="text-sm font-medium text-gray-500 dark:text-gray-400">{card.label}</p>
          <p className={`mt-1 text-2xl font-semibold ${card.color}`}>{card.value}</p>
        </div>
      ))}
    </div>
  );
}

// ── Timeline Chart Component ─────────────────────────────────────────────────

interface TimelineChartProps {
  stats: LogStats | null;
  loading: boolean;
}

function TimelineChart({ stats, loading }: TimelineChartProps) {
  if (loading) {
    return (
      <div className="mb-6 animate-pulse rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4">
        <div className="mb-3 h-4 w-32 rounded bg-gray-200 dark:bg-gray-600" />
        <div className="h-32 rounded bg-gray-100 dark:bg-gray-700" />
      </div>
    );
  }

  if (!stats || stats.timeline.length === 0) return null;

  const maxTotal = Math.max(...stats.timeline.map((t) => t.total), 1);

  return (
    <div className="mb-6 rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4" data-testid="timeline-chart">
      <h3 className="mb-3 text-sm font-medium text-gray-700 dark:text-gray-300">Requests Over Time</h3>
      <div className="flex h-32 items-end gap-px">
        {stats.timeline.map((entry, i) => {
          const height = (entry.total / maxTotal) * 100;
          const label = formatTimelineLabel(entry.date);
          return (
            <div
              key={i}
              className="group relative flex flex-1 flex-col items-center"
            >
              <div
                className="w-full rounded-t bg-blue-500 transition-colors hover:bg-blue-600"
                style={{ height: `${Math.max(height, 2)}%` }}
                title={`${label}: ${entry.total} requests`}
                data-testid="timeline-bar"
              />
            </div>
          );
        })}
      </div>
      <div className="mt-1 flex justify-between text-xs text-gray-400 dark:text-gray-500">
        <span>{formatTimelineLabel(stats.timeline[0].date)}</span>
        <span>{formatTimelineLabel(stats.timeline[stats.timeline.length - 1].date)}</span>
      </div>
    </div>
  );
}

function formatTimelineLabel(date: string): string {
  try {
    const d = new Date(date);
    return d.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  } catch {
    return date;
  }
}

// ── Status Breakdown Component ───────────────────────────────────────────────

interface StatusBreakdownProps {
  stats: LogStats | null;
}

function StatusBreakdown({ stats }: StatusBreakdownProps) {
  if (!stats || stats.totalRequests === 0) return null;

  const segments = [
    { label: '2xx', count: stats.statusCounts.success, color: 'bg-green-500' },
    { label: '3xx', count: stats.statusCounts.redirect, color: 'bg-blue-500' },
    { label: '4xx', count: stats.statusCounts.clientError, color: 'bg-yellow-500' },
    { label: '5xx', count: stats.statusCounts.serverError, color: 'bg-red-500' },
  ].filter((s) => s.count > 0);

  return (
    <div className="mb-6 rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4" data-testid="status-breakdown">
      <h3 className="mb-3 text-sm font-medium text-gray-700 dark:text-gray-300">Status Breakdown</h3>
      <div className="mb-2 flex h-3 overflow-hidden rounded-full bg-gray-100 dark:bg-gray-700">
        {segments.map((seg) => (
          <div
            key={seg.label}
            className={`${seg.color} transition-all`}
            style={{ width: `${(seg.count / stats.totalRequests) * 100}%` }}
            title={`${seg.label}: ${seg.count} (${((seg.count / stats.totalRequests) * 100).toFixed(1)}%)`}
          />
        ))}
      </div>
      <div className="flex gap-4 text-xs text-gray-500 dark:text-gray-400">
        {segments.map((seg) => (
          <span key={seg.label} className="flex items-center gap-1">
            <span className={`inline-block h-2 w-2 rounded-full ${seg.color}`} />
            {seg.label}: {seg.count.toLocaleString()}
          </span>
        ))}
      </div>
    </div>
  );
}

// ── Filters Component ────────────────────────────────────────────────────────

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
    <div className="mb-4 flex flex-wrap items-end gap-3 rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4" data-testid="logs-filters">
      {/* Method filter */}
      <div>
        <label htmlFor="method-filter" className="mb-1 block text-xs font-medium text-gray-600 dark:text-gray-400">
          Method
        </label>
        <select
          id="method-filter"
          value={method}
          onChange={(e) => { onMethodChange(e.target.value); onApply(); }}
          className="rounded-md border border-gray-300 dark:border-gray-600 px-3 py-1.5 text-sm focus:border-blue-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-blue-500"
        >
          <option value="">All</option>
          {HTTP_METHODS.map((m) => (
            <option key={m} value={m}>{m}</option>
          ))}
        </select>
      </div>

      {/* Status filter */}
      <div>
        <label htmlFor="status-filter" className="mb-1 block text-xs font-medium text-gray-600 dark:text-gray-400">
          Status
        </label>
        <select
          id="status-filter"
          value={statusRange}
          onChange={(e) => { onStatusRangeChange(Number(e.target.value)); onApply(); }}
          className="rounded-md border border-gray-300 dark:border-gray-600 px-3 py-1.5 text-sm focus:border-blue-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-blue-500"
        >
          {STATUS_RANGES.map((r, i) => (
            <option key={r.label} value={i}>{r.label}</option>
          ))}
        </select>
      </div>

      {/* Date range */}
      <div>
        <label htmlFor="date-filter" className="mb-1 block text-xs font-medium text-gray-600 dark:text-gray-400">
          Time Range
        </label>
        <select
          id="date-filter"
          value={datePreset}
          onChange={(e) => { onDatePresetChange(e.target.value); onApply(); }}
          className="rounded-md border border-gray-300 dark:border-gray-600 px-3 py-1.5 text-sm focus:border-blue-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-blue-500"
        >
          <option value="1h">Last Hour</option>
          <option value="24h">Last 24 Hours</option>
          <option value="7d">Last 7 Days</option>
          <option value="30d">Last 30 Days</option>
        </select>
      </div>

      {/* URL search */}
      <div className="flex-1">
        <label htmlFor="url-filter" className="mb-1 block text-xs font-medium text-gray-600 dark:text-gray-400">
          URL
        </label>
        <input
          id="url-filter"
          type="text"
          value={urlFilter}
          onChange={(e) => onUrlFilterChange(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') onApply(); }}
          placeholder="Filter by URL path..."
          className="w-full min-w-[200px] rounded-md border border-gray-300 dark:border-gray-600 px-3 py-1.5 text-sm placeholder:text-gray-400 dark:placeholder:text-gray-500 focus:border-blue-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-blue-500"
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

  return (
    <div className="flex items-center justify-between border-t border-gray-200 dark:border-gray-700 px-4 py-3" data-testid="pagination">
      <p className="text-sm text-gray-500 dark:text-gray-400">
        {totalItems.toLocaleString()} total {totalItems === 1 ? 'entry' : 'entries'}
      </p>
      <div className="flex gap-1">
        <button
          type="button"
          disabled={page <= 1}
          onClick={() => onPageChange(page - 1)}
          className="rounded-md border border-gray-300 dark:border-gray-600 px-3 py-1 text-sm disabled:cursor-not-allowed disabled:opacity-50 hover:bg-gray-50 dark:hover:bg-gray-700"
          aria-label="Previous page"
        >
          Previous
        </button>
        <span className="flex items-center px-3 text-sm text-gray-600 dark:text-gray-400">
          {page} / {totalPages}
        </span>
        <button
          type="button"
          disabled={page >= totalPages}
          onClick={() => onPageChange(page + 1)}
          className="rounded-md border border-gray-300 dark:border-gray-600 px-3 py-1 text-sm disabled:cursor-not-allowed disabled:opacity-50 hover:bg-gray-50 dark:hover:bg-gray-700"
          aria-label="Next page"
        >
          Next
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
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/30"
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
      role="dialog"
      aria-modal="true"
      aria-label="Log detail"
      data-testid="log-detail-modal"
      ref={dialogRef}
    >
      <div className="mx-4 w-full max-w-2xl rounded-lg bg-white dark:bg-gray-800 shadow-xl">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-gray-200 dark:border-gray-700 px-6 py-4">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">Request Log Detail</h3>
          <button
            type="button"
            onClick={onClose}
            className="rounded-md p-1 text-gray-400 dark:text-gray-500 transition-colors hover:bg-gray-100 dark:hover:bg-gray-700 hover:text-gray-600 dark:hover:text-gray-400"
            aria-label="Close detail"
          >
            <svg className="h-5 w-5" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" aria-hidden="true">
              <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        {/* Body */}
        <div className="max-h-[70vh] overflow-y-auto px-6 py-4">
          {loading ? (
            <div className="space-y-3">
              {Array.from({ length: 6 }).map((_, i) => (
                <div key={i} className="animate-pulse">
                  <div className="mb-1 h-3 w-20 rounded bg-gray-200 dark:bg-gray-600" />
                  <div className="h-5 w-48 rounded bg-gray-100 dark:bg-gray-700" />
                </div>
              ))}
            </div>
          ) : log ? (
            <dl className="space-y-4">
              <DetailRow label="ID" value={log.id} />
              <DetailRow label="Timestamp" value={formatTimestamp(log.created)} />
              <DetailRow label="Method">
                <span className={`inline-flex rounded px-2 py-0.5 text-xs font-semibold ${methodColorClass(log.method)}`}>
                  {log.method}
                </span>
              </DetailRow>
              <DetailRow label="URL" value={log.url} mono />
              <DetailRow label="Status">
                <span className={`inline-flex rounded-full px-2.5 py-0.5 text-xs font-medium ${statusColorClass(log.status)}`}>
                  {log.status}
                </span>
              </DetailRow>
              <DetailRow label="Duration" value={formatDuration(log.durationMs)} />
              <DetailRow label="IP Address" value={log.ip} mono />
              <DetailRow label="Auth ID" value={log.authId || '(anonymous)'} mono />
              <DetailRow label="User Agent" value={log.userAgent || '(none)'} />
              <DetailRow label="Request ID" value={log.requestId || '(none)'} mono />
            </dl>
          ) : null}
        </div>

        {/* Footer */}
        <div className="flex justify-end border-t border-gray-200 dark:border-gray-700 px-6 py-3">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md bg-gray-100 dark:bg-gray-700 px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 transition-colors hover:bg-gray-200 dark:hover:bg-gray-600"
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
    <div>
      <dt className="text-xs font-medium text-gray-500 dark:text-gray-400">{label}</dt>
      <dd className={`mt-0.5 text-sm text-gray-900 dark:text-gray-100 ${mono ? 'font-mono' : ''}`}>
        {children ?? value ?? ''}
      </dd>
    </div>
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
    // fetchLogs will be triggered by the useEffect dependency change
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

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <DashboardLayout currentPath="/_/logs" pageTitle="Logs">
      {/* Error banner */}
      {error && (
        <div className="mb-4 rounded-lg border border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30 px-4 py-3 text-sm text-red-700 dark:text-red-400" role="alert">
          {error}
          <button
            type="button"
            onClick={() => { setError(null); fetchLogs(); fetchStats(); }}
            className="ml-2 font-medium underline"
          >
            Retry
          </button>
        </div>
      )}

      {/* Stats overview */}
      <StatsOverview stats={stats} loading={statsLoading} />

      {/* Charts row */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <div className="lg:col-span-2">
          <TimelineChart stats={stats} loading={statsLoading} />
        </div>
        <div>
          <StatusBreakdown stats={stats} />
        </div>
      </div>

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
      <div className="overflow-hidden rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800">
        <div className="overflow-x-auto">
          <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-700" data-testid="logs-table">
            <thead className="bg-gray-50 dark:bg-gray-900">
              <tr>
                <th
                  scope="col"
                  tabIndex={0}
                  className="cursor-pointer px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500"
                  onClick={() => handleSort('created')}
                  onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleSort('created'); } }}
                  aria-sort={sort === 'created' ? 'ascending' : sort === '-created' ? 'descending' : undefined}
                >
                  Timestamp{getSortIndicator('created')}
                </th>
                <th
                  scope="col"
                  tabIndex={0}
                  className="cursor-pointer px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500"
                  onClick={() => handleSort('method')}
                  onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleSort('method'); } }}
                  aria-sort={sort === 'method' ? 'ascending' : sort === '-method' ? 'descending' : undefined}
                >
                  Method{getSortIndicator('method')}
                </th>
                <th scope="col" className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">
                  URL
                </th>
                <th
                  scope="col"
                  tabIndex={0}
                  className="cursor-pointer px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500"
                  onClick={() => handleSort('status')}
                  onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleSort('status'); } }}
                  aria-sort={sort === 'status' ? 'ascending' : sort === '-status' ? 'descending' : undefined}
                >
                  Status{getSortIndicator('status')}
                </th>
                <th scope="col" className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">
                  IP
                </th>
                <th scope="col" className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">
                  User
                </th>
                <th
                  scope="col"
                  tabIndex={0}
                  className="cursor-pointer px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500"
                  onClick={() => handleSort('duration_ms')}
                  onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleSort('duration_ms'); } }}
                  aria-sort={sort === 'duration_ms' ? 'ascending' : sort === '-duration_ms' ? 'descending' : undefined}
                >
                  Duration{getSortIndicator('duration_ms')}
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100 dark:divide-gray-700">
              {logsLoading ? (
                Array.from({ length: 8 }).map((_, i) => (
                  <tr key={i} className="animate-pulse">
                    {Array.from({ length: 7 }).map((_, j) => (
                      <td key={j} className="px-4 py-3">
                        <div className="h-4 w-20 rounded bg-gray-200 dark:bg-gray-600" />
                      </td>
                    ))}
                  </tr>
                ))
              ) : logs && logs.items.length > 0 ? (
                logs.items.map((log) => (
                  <tr
                    key={log.id}
                    className="cursor-pointer transition-colors hover:bg-gray-50 dark:hover:bg-gray-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-blue-500"
                    onClick={() => openLogDetail(log.id)}
                    onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openLogDetail(log.id); } }}
                    tabIndex={0}
                    role="button"
                    aria-label={`View log: ${log.method} ${log.url} - ${log.status}`}
                    data-testid="log-row"
                  >
                    <td className="whitespace-nowrap px-4 py-3 text-xs text-gray-500 dark:text-gray-400">
                      {formatTimestamp(log.created)}
                    </td>
                    <td className="px-4 py-3">
                      <span className={`inline-flex rounded px-2 py-0.5 text-xs font-semibold ${methodColorClass(log.method)}`}>
                        {log.method}
                      </span>
                    </td>
                    <td className="max-w-xs truncate px-4 py-3 font-mono text-xs text-gray-700 dark:text-gray-300" title={log.url}>
                      {log.url}
                    </td>
                    <td className="px-4 py-3">
                      <span className={`inline-flex rounded-full px-2.5 py-0.5 text-xs font-medium ${statusColorClass(log.status)}`}>
                        {log.status}
                      </span>
                    </td>
                    <td className="whitespace-nowrap px-4 py-3 font-mono text-xs text-gray-500 dark:text-gray-400">
                      {log.ip}
                    </td>
                    <td className="whitespace-nowrap px-4 py-3 text-xs text-gray-500 dark:text-gray-400">
                      {log.authId || '\u2014'}
                    </td>
                    <td className="whitespace-nowrap px-4 py-3 text-xs text-gray-500 dark:text-gray-400">
                      {formatDuration(log.durationMs)}
                    </td>
                  </tr>
                ))
              ) : (
                <tr>
                  <td colSpan={7} className="px-4 py-12 text-center text-sm text-gray-400 dark:text-gray-500">
                    No logs found matching the current filters.
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
