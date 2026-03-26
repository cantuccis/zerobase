import { useState, useEffect, useCallback } from 'react';
import { DashboardLayout } from '../DashboardLayout';
import { client } from '../../lib/auth/client';
import { ApiError } from '../../lib/api';
import type { Collection, LogEntry, LogStats } from '../../lib/api/types';

// ── Types ────────────────────────────────────────────────────────────────────

interface OverviewStats {
  totalCollections: number;
  totalRecords: number;
  collections: Collection[];
}

interface OverviewState {
  stats: OverviewStats | null;
  logStats: LogStats | null;
  recentLogs: LogEntry[];
  healthStatus: 'healthy' | 'unhealthy' | 'loading';
  loading: boolean;
  error: string | null;
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function formatTimestamp(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString(undefined, {
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

// ── Stat Cell ────────────────────────────────────────────────────────────────

interface StatCellProps {
  label: string;
  value: string | number;
}

function StatCell({ label, value }: StatCellProps) {
  return (
    <div
      className="border border-primary p-6"
      data-testid={`stat-${label.toLowerCase().replace(/\s+/g, '-')}`}
    >
      <div className="text-[9px] font-bold uppercase tracking-[0.15em] text-on-surface-variant mb-2">
        {label}
      </div>
      <div className="text-4xl font-extrabold tracking-tight text-on-surface font-data">
        {value}
      </div>
    </div>
  );
}

// ── Health Badge ─────────────────────────────────────────────────────────────

function HealthBadge({ status }: { status: 'healthy' | 'unhealthy' | 'loading' }) {
  if (status === 'loading') {
    return (
      <span
        className="inline-flex items-center gap-2 text-[10px] font-bold uppercase tracking-[0.15em] text-on-surface-variant"
        data-testid="health-badge"
      >
        <span className="h-2 w-2 bg-outline animate-pulse" />
        Checking
      </span>
    );
  }

  const isHealthy = status === 'healthy';
  return (
    <span
      className="inline-flex items-center gap-2 text-[10px] font-bold uppercase tracking-[0.15em] text-on-surface"
      data-testid="health-badge"
    >
      <span className={`h-2 w-2 ${isHealthy ? 'bg-primary' : 'bg-error'}`} />
      {isHealthy ? 'Operational' : 'Unhealthy'}
    </span>
  );
}

// ── Overview Page ────────────────────────────────────────────────────────────

export function OverviewPage() {
  const [state, setState] = useState<OverviewState>({
    stats: null,
    logStats: null,
    recentLogs: [],
    healthStatus: 'loading',
    loading: true,
    error: null,
  });

  const fetchData = useCallback(async () => {
    setState((prev) => ({ ...prev, loading: true, error: null }));

    try {
      const [collectionsRes, logsRes, logStatsRes, healthRes] = await Promise.allSettled([
        client.listCollections(),
        client.listLogs({ perPage: 10, sort: '-created' }),
        client.getLogStats(),
        client.health(),
      ]);

      let totalCollections = 0;
      let totalRecords = 0;
      let collections: Collection[] = [];

      if (collectionsRes.status === 'fulfilled') {
        collections = collectionsRes.value.items;
        totalCollections = collectionsRes.value.totalItems;

        const countResults = await Promise.allSettled(
          collections.map((col) => client.countRecords(col.name)),
        );
        totalRecords = countResults.reduce((sum, result) => {
          if (result.status === 'fulfilled') {
            return sum + result.value.count;
          }
          return sum;
        }, 0);
      }

      let recentLogs: LogEntry[] = [];
      if (logsRes.status === 'fulfilled') {
        recentLogs = logsRes.value.items;
      }

      let logStats: LogStats | null = null;
      if (logStatsRes.status === 'fulfilled') {
        logStats = logStatsRes.value;
      }

      const healthStatus: 'healthy' | 'unhealthy' =
        healthRes.status === 'fulfilled' && healthRes.value.status === 'ok'
          ? 'healthy'
          : 'unhealthy';

      if (collectionsRes.status === 'rejected' && logsRes.status === 'rejected' && healthRes.status === 'rejected') {
        const reason = collectionsRes.reason;
        const message = reason instanceof ApiError ? reason.message : 'Failed to load dashboard data';
        setState((prev) => ({ ...prev, loading: false, error: message }));
        return;
      }

      setState({
        stats: { totalCollections, totalRecords, collections },
        logStats,
        recentLogs,
        healthStatus,
        loading: false,
        error: null,
      });
    } catch (err) {
      const message = err instanceof ApiError ? err.message : 'Failed to load dashboard data';
      setState((prev) => ({ ...prev, loading: false, error: message }));
    }
  }, []);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  return (
    <DashboardLayout currentPath="/_/" pageTitle="Dashboard">
      {state.loading ? (
        <div className="flex items-center justify-center py-24" data-testid="loading-state">
          <div className="text-center">
            <div className="mx-auto h-6 w-6 border-2 border-primary border-t-transparent animate-spin" role="status" aria-label="Loading" />
            <p className="mt-4 text-[10px] font-bold uppercase tracking-[0.15em] text-on-surface-variant">
              Loading dashboard data…
            </p>
          </div>
        </div>
      ) : state.error ? (
        <div className="border border-error p-8" data-testid="error-state">
          <p className="text-label-md text-error">Error loading dashboard</p>
          <p className="mt-2 text-sm text-error">{state.error}</p>
          <button
            type="button"
            onClick={fetchData}
            className="mt-4 border border-primary bg-primary text-on-primary px-6 py-2 text-[10px] font-bold uppercase tracking-[0.15em] hover:bg-on-surface-variant cursor-pointer transition-colors-fast"
          >
            Retry
          </button>
        </div>
      ) : (
        <div>
          {/* ── Page Header ──────────────────────────────────────── */}
          <div className="mb-12">
            <div className="flex items-center justify-between mb-2">
              <div className="text-[10px] font-bold uppercase tracking-[0.2em] text-on-surface-variant">
                System Overview
              </div>
              <HealthBadge status={state.healthStatus} />
            </div>
            <h1 className="text-display-lg text-on-surface">Dashboard</h1>
          </div>

          {/* ── 12-Column Bento Layout ────────────────────────────── */}
          <div className="grid grid-cols-1 lg:grid-cols-12 gap-8">

            {/* ── Main Content (8 cols) ──────────────────────────── */}
            <div className="lg:col-span-8 space-y-8">

              {/* Stat Grid */}
              <div className="grid grid-cols-2 sm:grid-cols-4">
                <StatCell
                  label="Collections"
                  value={state.stats?.totalCollections ?? 0}
                />
                <StatCell
                  label="Total Records"
                  value={state.stats?.totalRecords ?? 0}
                />
                <StatCell
                  label="Total Requests"
                  value={state.logStats?.totalRequests ?? 0}
                />
                <StatCell
                  label="Avg Response"
                  value={state.logStats ? formatDuration(state.logStats.avgDurationMs) : '—'}
                />
              </div>

              {/* Request Status Breakdown */}
              {state.logStats && (
                <div className="border border-primary p-6">
                  <div className="text-[10px] font-bold uppercase tracking-[0.15em] text-on-surface-variant mb-6">
                    Request Status Breakdown
                  </div>
                  <div className="grid grid-cols-2 sm:grid-cols-4 gap-6">
                    <div data-testid="status-success">
                      <div className="text-[9px] font-bold uppercase tracking-[0.15em] text-on-surface-variant mb-1">
                        2xx Success
                      </div>
                      <div className="text-2xl font-extrabold tracking-tight text-on-surface font-data">
                        {state.logStats.statusCounts.success}
                      </div>
                    </div>
                    <div data-testid="status-redirect">
                      <div className="text-[9px] font-bold uppercase tracking-[0.15em] text-on-surface-variant mb-1">
                        3xx Redirect
                      </div>
                      <div className="text-2xl font-extrabold tracking-tight text-on-surface font-data">
                        {state.logStats.statusCounts.redirect}
                      </div>
                    </div>
                    <div data-testid="status-client-error">
                      <div className="text-[9px] font-bold uppercase tracking-[0.15em] text-on-surface-variant mb-1">
                        4xx Client Error
                      </div>
                      <div className="text-2xl font-extrabold tracking-tight text-on-surface font-data">
                        {state.logStats.statusCounts.clientError}
                      </div>
                    </div>
                    <div data-testid="status-server-error">
                      <div className="text-[9px] font-bold uppercase tracking-[0.15em] text-on-surface-variant mb-1">
                        5xx Server Error
                      </div>
                      <div className="text-2xl font-extrabold tracking-tight text-on-surface font-data">
                        {state.logStats.statusCounts.serverError}
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* Recent Activity Table */}
              <div className="border border-primary">
                <div className="border-b border-primary px-6 py-4 flex items-center justify-between">
                  <div className="text-[10px] font-bold uppercase tracking-[0.15em] text-on-surface">
                    Recent Activity
                  </div>
                  <a
                    href="/_/logs"
                    className="text-[10px] font-bold uppercase tracking-[0.15em] text-on-surface-variant hover:text-on-surface"
                  >
                    View All Logs
                  </a>
                </div>
                {state.recentLogs.length === 0 ? (
                  <div className="px-6 py-12 text-center" data-testid="no-logs">
                    <p className="text-[10px] font-bold uppercase tracking-[0.15em] text-on-surface-variant">
                      No recent activity
                    </p>
                  </div>
                ) : (
                  <div className="overflow-x-auto">
                    <table className="w-full text-left border-collapse" data-testid="recent-logs-table">
                      <thead>
                        <tr className="bg-primary text-on-primary">
                          <th scope="col" className="px-4 py-3 text-[10px] font-bold uppercase tracking-[0.15em]">Method</th>
                          <th scope="col" className="px-4 py-3 text-[10px] font-bold uppercase tracking-[0.15em]">URL</th>
                          <th scope="col" className="px-4 py-3 text-[10px] font-bold uppercase tracking-[0.15em]">Status</th>
                          <th scope="col" className="px-4 py-3 text-[10px] font-bold uppercase tracking-[0.15em]">Duration</th>
                          <th scope="col" className="px-4 py-3 text-[10px] font-bold uppercase tracking-[0.15em]">Time</th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-outline-variant">
                        {state.recentLogs.map((log) => (
                          <tr key={log.id} className="hover:bg-surface-container-low transition-colors-fast">
                            <td className="px-4 py-3 font-data text-[11px] font-bold text-on-surface">
                              {log.method}
                            </td>
                            <td className="px-4 py-3 font-data text-[11px] text-on-surface-variant max-w-xs truncate">
                              {log.url}
                            </td>
                            <td className="px-4 py-3">
                              <span className="font-data text-[11px] font-bold text-on-surface">
                                {log.status}
                              </span>
                            </td>
                            <td className="px-4 py-3 font-data text-[11px] text-on-surface-variant">
                              {formatDuration(log.durationMs)}
                            </td>
                            <td className="px-4 py-3 font-data text-[11px] text-on-surface-variant whitespace-nowrap">
                              {formatTimestamp(log.created)}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
              </div>
            </div>

            {/* ── Sidebar (4 cols) ───────────────────────────────── */}
            <div className="lg:col-span-4 space-y-8">

              {/* Metrics Overview (inverted block) */}
              <div className="border border-primary bg-primary text-on-primary p-6">
                <div className="text-[10px] font-bold uppercase tracking-[0.15em] opacity-60 mb-6">
                  Metrics Overview
                </div>
                <div className="grid grid-cols-2 gap-4">
                  <div>
                    <div className="text-4xl font-extrabold tracking-tight font-data mb-1">
                      {state.stats?.totalCollections ?? 0}
                    </div>
                    <div className="text-[9px] font-bold uppercase tracking-[0.15em] opacity-60">
                      Collections
                    </div>
                  </div>
                  <div>
                    <div className="text-4xl font-extrabold tracking-tight font-data mb-1">
                      {state.stats?.totalRecords ?? 0}
                    </div>
                    <div className="text-[9px] font-bold uppercase tracking-[0.15em] opacity-60">
                      Total Records
                    </div>
                  </div>
                </div>
                {state.logStats && (
                  <div className="mt-8 pt-6 border-t border-on-primary/20">
                    <div className="text-[9px] font-bold uppercase tracking-[0.15em] mb-1 opacity-60">
                      Avg Response
                    </div>
                    <div className="text-sm font-bold font-data">
                      {formatDuration(state.logStats.avgDurationMs)}
                    </div>
                  </div>
                )}
              </div>

              {/* Collections List */}
              {state.stats && state.stats.collections.length > 0 && (
                <div className="border border-primary">
                  <div className="border-b border-primary px-6 py-4">
                    <div className="text-[10px] font-bold uppercase tracking-[0.15em] text-on-surface">
                      Collections
                    </div>
                  </div>
                  <ul className="divide-y divide-outline-variant" role="list" data-testid="collections-list">
                    {state.stats.collections.map((col) => (
                      <li key={col.id} className="flex items-center justify-between px-6 py-3">
                        <div className="flex items-center gap-3">
                          <a
                            href={`/_/collections/${col.id}`}
                            className="text-sm font-bold text-on-surface hover:underline"
                          >
                            {col.name}
                          </a>
                          <span className="text-[9px] font-bold uppercase tracking-[0.15em] text-on-surface-variant">
                            {col.type}
                          </span>
                        </div>
                        <span className="text-[10px] font-bold uppercase tracking-[0.15em] text-on-surface-variant">
                          {col.fields.length} field{col.fields.length !== 1 ? 's' : ''}
                        </span>
                      </li>
                    ))}
                  </ul>
                </div>
              )}

              {/* Quick Actions */}
              <div className="border border-primary p-6">
                <div className="text-[10px] font-bold uppercase tracking-[0.15em] text-on-surface-variant mb-4">
                  Quick Actions
                </div>
                <div className="space-y-3">
                  <a
                    href="/_/collections"
                    className="block w-full border border-primary py-3 text-center text-[10px] font-bold uppercase tracking-[0.15em] text-on-surface hover:bg-primary hover:text-on-primary cursor-pointer transition-colors-fast"
                  >
                    Manage Collections
                  </a>
                  <a
                    href="/_/logs"
                    className="block w-full border border-primary py-3 text-center text-[10px] font-bold uppercase tracking-[0.15em] text-on-surface hover:bg-primary hover:text-on-primary cursor-pointer transition-colors-fast"
                  >
                    View Logs
                  </a>
                  <a
                    href="/_/settings"
                    className="block w-full border border-primary py-3 text-center text-[10px] font-bold uppercase tracking-[0.15em] text-on-surface hover:bg-primary hover:text-on-primary cursor-pointer transition-colors-fast"
                  >
                    Settings
                  </a>
                </div>
              </div>

              {/* Latest Operations Log */}
              {state.recentLogs.length > 0 && (
                <div className="border border-primary p-6 bg-surface-container-low">
                  <div className="text-[10px] font-bold uppercase tracking-[0.15em] text-on-surface mb-4">
                    Latest Operations
                  </div>
                  <ul className="font-data text-[11px] space-y-2 text-on-surface-variant">
                    {state.recentLogs.slice(0, 5).map((log) => (
                      <li key={log.id} className="flex gap-2">
                        <span className="whitespace-nowrap">[{new Date(log.created).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })}]</span>
                        <span className="truncate">{log.method} {log.url}</span>
                      </li>
                    ))}
                  </ul>
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </DashboardLayout>
  );
}
