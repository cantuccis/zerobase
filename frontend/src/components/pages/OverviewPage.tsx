import { useState, useEffect, useCallback } from 'react';
import { DashboardLayout } from '../DashboardLayout';
import { client } from '../../lib/auth/client';
import { ApiError } from '../../lib/api';
import type { Collection, LogEntry, LogStats, ListResponse } from '../../lib/api/types';

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

function statusColorClass(status: number): string {
  if (status >= 500) return 'bg-red-100 dark:bg-red-900/20 text-red-800 dark:text-red-300';
  if (status >= 400) return 'bg-yellow-100 dark:bg-yellow-900/30 text-yellow-800 dark:text-yellow-300';
  if (status >= 300) return 'bg-blue-100 dark:bg-blue-900/20 text-blue-800 dark:text-blue-300';
  return 'bg-green-100 dark:bg-green-900/30 text-green-800 dark:text-green-300';
}

function methodColorClass(method: string): string {
  switch (method) {
    case 'GET': return 'text-blue-600 dark:text-blue-400';
    case 'POST': return 'text-green-600 dark:text-green-400';
    case 'PATCH':
    case 'PUT': return 'text-yellow-600 dark:text-yellow-400';
    case 'DELETE': return 'text-red-600 dark:text-red-400';
    default: return 'text-gray-600 dark:text-gray-400';
  }
}

// ── Stat Card ────────────────────────────────────────────────────────────────

interface StatCardProps {
  label: string;
  value: string | number;
  icon: React.ReactNode;
  color: string;
}

function StatCard({ label, value, icon, color }: StatCardProps) {
  return (
    <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-5 shadow-sm dark:shadow-gray-900/20" data-testid={`stat-${label.toLowerCase().replace(/\s+/g, '-')}`}>
      <div className="flex items-center gap-4">
        <div className={`flex h-10 w-10 items-center justify-center rounded-lg ${color}`}>
          {icon}
        </div>
        <div>
          <p className="text-sm font-medium text-gray-500 dark:text-gray-400">{label}</p>
          <p className="text-2xl font-bold text-gray-900 dark:text-gray-100">{value}</p>
        </div>
      </div>
    </div>
  );
}

// ── Health Badge ─────────────────────────────────────────────────────────────

function HealthBadge({ status }: { status: 'healthy' | 'unhealthy' | 'loading' }) {
  if (status === 'loading') {
    return (
      <span className="inline-flex items-center gap-1.5 rounded-full bg-gray-100 dark:bg-gray-700 px-3 py-1 text-xs font-medium text-gray-600 dark:text-gray-400" data-testid="health-badge">
        <span className="h-2 w-2 rounded-full bg-gray-400 dark:bg-gray-500 animate-pulse" />
        Checking
      </span>
    );
  }

  const isHealthy = status === 'healthy';
  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-medium ${
        isHealthy ? 'bg-green-100 dark:bg-green-900/30 text-green-800 dark:text-green-300' : 'bg-red-100 dark:bg-red-900/20 text-red-800 dark:text-red-300'
      }`}
      data-testid="health-badge"
    >
      <span className={`h-2 w-2 rounded-full ${isHealthy ? 'bg-green-500' : 'bg-red-500'}`} />
      {isHealthy ? 'Healthy' : 'Unhealthy'}
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
      // Fetch all data in parallel
      const [collectionsRes, logsRes, logStatsRes, healthRes] = await Promise.allSettled([
        client.listCollections(),
        client.listLogs({ perPage: 10, sort: '-created' }),
        client.getLogStats(),
        client.health(),
      ]);

      // Process collections
      let totalCollections = 0;
      let totalRecords = 0;
      let collections: Collection[] = [];

      if (collectionsRes.status === 'fulfilled') {
        collections = collectionsRes.value.items;
        totalCollections = collectionsRes.value.totalItems;

        // Fetch record counts for each collection in parallel
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

      // Process logs
      let recentLogs: LogEntry[] = [];
      if (logsRes.status === 'fulfilled') {
        recentLogs = logsRes.value.items;
      }

      // Process log stats
      let logStats: LogStats | null = null;
      if (logStatsRes.status === 'fulfilled') {
        logStats = logStatsRes.value;
      }

      // Process health
      const healthStatus: 'healthy' | 'unhealthy' =
        healthRes.status === 'fulfilled' && healthRes.value.status === 'ok'
          ? 'healthy'
          : 'unhealthy';

      // If all critical calls failed, show an error
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
        <div className="flex items-center justify-center py-16" data-testid="loading-state">
          <div className="text-center">
            <div className="mx-auto h-8 w-8 animate-spin rounded-full border-4 border-blue-600 border-t-transparent" />
            <p className="mt-3 text-sm text-gray-500 dark:text-gray-400">Loading dashboard data…</p>
          </div>
        </div>
      ) : state.error ? (
        <div className="rounded-lg border border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30 p-6" data-testid="error-state">
          <p className="text-sm font-medium text-red-800 dark:text-red-300">Error loading dashboard</p>
          <p className="mt-1 text-sm text-red-600 dark:text-red-400">{state.error}</p>
          <button
            type="button"
            onClick={fetchData}
            className="mt-3 rounded-md bg-red-100 dark:bg-red-900/20 px-3 py-1.5 text-sm font-medium text-red-800 dark:text-red-300 hover:bg-red-200 dark:hover:bg-red-900/30 transition-colors"
          >
            Retry
          </button>
        </div>
      ) : (
        <div className="space-y-8">
          {/* System Health */}
          <div className="flex items-center justify-between">
            <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">System Status</h3>
            <HealthBadge status={state.healthStatus} />
          </div>

          {/* Stats Grid */}
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <StatCard
              label="Collections"
              value={state.stats?.totalCollections ?? 0}
              color="bg-blue-100 dark:bg-blue-900/20"
              icon={
                <svg className="h-5 w-5 text-blue-600 dark:text-blue-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <rect x="3" y="3" width="7" height="7" rx="1" />
                  <rect x="14" y="3" width="7" height="7" rx="1" />
                  <rect x="3" y="14" width="7" height="7" rx="1" />
                  <rect x="14" y="14" width="7" height="7" rx="1" />
                </svg>
              }
            />
            <StatCard
              label="Total Records"
              value={state.stats?.totalRecords ?? 0}
              color="bg-green-100 dark:bg-green-900/30"
              icon={
                <svg className="h-5 w-5 text-green-600 dark:text-green-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                  <polyline points="14 2 14 8 20 8" />
                  <line x1="16" y1="13" x2="8" y2="13" />
                  <line x1="16" y1="17" x2="8" y2="17" />
                </svg>
              }
            />
            <StatCard
              label="Total Requests"
              value={state.logStats?.totalRequests ?? 0}
              color="bg-purple-100 dark:bg-purple-900/30"
              icon={
                <svg className="h-5 w-5 text-purple-600 dark:text-purple-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <polyline points="22 12 18 12 15 21 9 3 6 12 2 12" />
                </svg>
              }
            />
            <StatCard
              label="Avg Response"
              value={state.logStats ? formatDuration(state.logStats.avgDurationMs) : '—'}
              color="bg-orange-100 dark:bg-orange-900/30"
              icon={
                <svg className="h-5 w-5 text-orange-600 dark:text-orange-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <circle cx="12" cy="12" r="10" />
                  <polyline points="12 6 12 12 16 14" />
                </svg>
              }
            />
          </div>

          {/* Request Status Breakdown */}
          {state.logStats && (
            <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-5">
              <h3 className="mb-4 text-sm font-semibold text-gray-900 dark:text-gray-100">Request Status Breakdown</h3>
              <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
                <div data-testid="status-success">
                  <p className="text-xs font-medium text-gray-500 dark:text-gray-400">2xx Success</p>
                  <p className="mt-1 text-xl font-bold text-green-600 dark:text-green-400">{state.logStats.statusCounts.success}</p>
                </div>
                <div data-testid="status-redirect">
                  <p className="text-xs font-medium text-gray-500 dark:text-gray-400">3xx Redirect</p>
                  <p className="mt-1 text-xl font-bold text-blue-600 dark:text-blue-400">{state.logStats.statusCounts.redirect}</p>
                </div>
                <div data-testid="status-client-error">
                  <p className="text-xs font-medium text-gray-500 dark:text-gray-400">4xx Client Error</p>
                  <p className="mt-1 text-xl font-bold text-yellow-600 dark:text-yellow-400">{state.logStats.statusCounts.clientError}</p>
                </div>
                <div data-testid="status-server-error">
                  <p className="text-xs font-medium text-gray-500 dark:text-gray-400">5xx Server Error</p>
                  <p className="mt-1 text-xl font-bold text-red-600 dark:text-red-400">{state.logStats.statusCounts.serverError}</p>
                </div>
              </div>
            </div>
          )}

          {/* Collections List */}
          {state.stats && state.stats.collections.length > 0 && (
            <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800">
              <div className="border-b border-gray-200 dark:border-gray-700 px-5 py-4">
                <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">Collections</h3>
              </div>
              <ul className="divide-y divide-gray-100 dark:divide-gray-700" role="list" data-testid="collections-list">
                {state.stats.collections.map((col) => (
                  <li key={col.id} className="flex items-center justify-between px-5 py-3">
                    <div className="flex items-center gap-3">
                      <a
                        href={`/_/collections/${col.id}`}
                        className="text-sm font-medium text-gray-900 dark:text-gray-100 hover:text-blue-600 dark:hover:text-blue-400 transition-colors"
                      >
                        {col.name}
                      </a>
                      <span className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
                        col.type === 'auth' ? 'bg-green-100 dark:bg-green-900/30 text-green-800 dark:text-green-300' :
                        col.type === 'view' ? 'bg-purple-100 dark:bg-purple-900/30 text-purple-800 dark:text-purple-300' :
                        'bg-blue-100 dark:bg-blue-900/20 text-blue-800 dark:text-blue-300'
                      }`}>
                        {col.type}
                      </span>
                    </div>
                    <span className="text-xs text-gray-500 dark:text-gray-400">
                      {col.fields.length} field{col.fields.length !== 1 ? 's' : ''}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {/* Recent Activity */}
          <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800">
            <div className="border-b border-gray-200 dark:border-gray-700 px-5 py-4 flex items-center justify-between">
              <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">Recent Activity</h3>
              <a href="/_/logs" className="text-xs font-medium text-blue-600 dark:text-blue-400 hover:text-blue-800 dark:hover:text-blue-300 transition-colors">
                View all logs
              </a>
            </div>
            {state.recentLogs.length === 0 ? (
              <div className="px-5 py-8 text-center" data-testid="no-logs">
                <p className="text-sm text-gray-500 dark:text-gray-400">No recent activity</p>
              </div>
            ) : (
              <div className="overflow-x-auto">
                <table className="w-full text-sm" data-testid="recent-logs-table">
                  <thead>
                    <tr className="border-b border-gray-100 dark:border-gray-700 text-left text-xs font-medium text-gray-500 dark:text-gray-400">
                      <th scope="col" className="px-5 py-2">Method</th>
                      <th scope="col" className="px-5 py-2">URL</th>
                      <th scope="col" className="px-5 py-2">Status</th>
                      <th scope="col" className="px-5 py-2">Duration</th>
                      <th scope="col" className="px-5 py-2">Time</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-gray-50 dark:divide-gray-700">
                    {state.recentLogs.map((log) => (
                      <tr key={log.id} className="hover:bg-gray-50 dark:hover:bg-gray-700">
                        <td className={`px-5 py-2.5 font-mono text-xs font-semibold ${methodColorClass(log.method)}`}>
                          {log.method}
                        </td>
                        <td className="px-5 py-2.5 font-mono text-xs text-gray-700 dark:text-gray-300 max-w-xs truncate">
                          {log.url}
                        </td>
                        <td className="px-5 py-2.5">
                          <span className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${statusColorClass(log.status)}`}>
                            {log.status}
                          </span>
                        </td>
                        <td className="px-5 py-2.5 text-xs text-gray-500 dark:text-gray-400">
                          {formatDuration(log.durationMs)}
                        </td>
                        <td className="px-5 py-2.5 text-xs text-gray-500 dark:text-gray-400 whitespace-nowrap">
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
      )}
    </DashboardLayout>
  );
}
