import React, { useState, useEffect, useCallback } from 'react';
import { DashboardLayout } from '../DashboardLayout';
import { client } from '../../lib/auth/client';
import { ApiError } from '../../lib/api';
import type { BackupEntry } from '../../lib/api/types';

// ── Helpers ──────────────────────────────────────────────────────────────────

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const value = bytes / Math.pow(1024, i);
  return `${value.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

function formatDate(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

// ── Icons ────────────────────────────────────────────────────────────────────

function DownloadIcon() {
  return (
    <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" aria-hidden="true">
      <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3" />
    </svg>
  );
}

function TrashIcon() {
  return (
    <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" aria-hidden="true">
      <path strokeLinecap="round" strokeLinejoin="round" d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0" />
    </svg>
  );
}

function RestoreIcon() {
  return (
    <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" aria-hidden="true">
      <path strokeLinecap="round" strokeLinejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
    </svg>
  );
}

function PlusIcon() {
  return (
    <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" aria-hidden="true">
      <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
    </svg>
  );
}

function DatabaseIcon() {
  return (
    <svg className="h-12 w-12 text-gray-300 dark:text-gray-600" fill="none" viewBox="0 0 24 24" strokeWidth={1} stroke="currentColor" aria-hidden="true">
      <path strokeLinecap="round" strokeLinejoin="round" d="M20.25 6.375c0 2.278-3.694 4.125-8.25 4.125S3.75 8.653 3.75 6.375m16.5 0c0-2.278-3.694-4.125-8.25-4.125S3.75 4.097 3.75 6.375m16.5 0v11.25c0 2.278-3.694 4.125-8.25 4.125s-8.25-1.847-8.25-4.125V6.375m16.5 0v3.75m-16.5-3.75v3.75m16.5 0v3.75C20.25 16.153 16.556 18 12 18s-8.25-1.847-8.25-4.125v-3.75" />
    </svg>
  );
}

// ── Confirmation Modal ───────────────────────────────────────────────────────

interface ConfirmModalProps {
  title: string;
  message: string;
  confirmLabel: string;
  confirmVariant?: 'danger' | 'primary';
  onConfirm: () => void;
  onCancel: () => void;
  loading?: boolean;
}

function ConfirmModal({ title, message, confirmLabel, confirmVariant = 'danger', onConfirm, onCancel, loading }: ConfirmModalProps) {
  const dialogRef = React.useRef<HTMLDivElement>(null);
  const confirmClasses = confirmVariant === 'danger'
    ? 'bg-red-600 text-white hover:bg-red-700 dark:hover:bg-red-600 focus-visible:ring-red-500'
    : 'bg-blue-600 text-white hover:bg-blue-700 dark:hover:bg-blue-600 focus-visible:ring-blue-500';

  React.useEffect(() => {
    const cancelBtn = dialogRef.current?.querySelector<HTMLElement>('[data-testid="confirm-cancel"]');
    cancelBtn?.focus();
  }, []);

  React.useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape' && !loading) {
        onCancel();
        return;
      }
      if (e.key === 'Tab' && dialogRef.current) {
        const focusable = dialogRef.current.querySelectorAll<HTMLElement>('button:not(:disabled), [href], input, select, textarea, [tabindex]:not([tabindex="-1"])');
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
  }, [loading, onCancel]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      data-testid="confirm-modal"
      role="dialog"
      aria-modal="true"
      aria-labelledby="confirm-modal-title"
      onClick={(e) => { if (e.target === e.currentTarget && !loading) onCancel(); }}
      ref={dialogRef}
    >
      <div className="mx-4 w-full max-w-md rounded-lg bg-white dark:bg-gray-800 p-6 shadow-xl">
        <h3 id="confirm-modal-title" className="text-lg font-semibold text-gray-900 dark:text-gray-100">{title}</h3>
        <p className="mt-2 text-sm text-gray-600 dark:text-gray-400">{message}</p>

        <div className="mt-6 flex justify-end gap-3">
          <button
            type="button"
            onClick={onCancel}
            disabled={loading}
            className="rounded-md border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700 disabled:opacity-50"
            data-testid="confirm-cancel"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={loading}
            className={`rounded-md px-4 py-2 text-sm font-medium focus-visible:outline-none focus-visible:ring-2 focus:ring-offset-2 disabled:opacity-50 ${confirmClasses}`}
            data-testid="confirm-action"
          >
            {loading ? 'Processing\u2026' : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Main component ───────────────────────────────────────────────────────────

export function BackupsPage() {
  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Action states
  const [creating, setCreating] = useState(false);
  const [downloadingName, setDownloadingName] = useState<string | null>(null);
  const [deletingName, setDeletingName] = useState<string | null>(null);
  const [restoringName, setRestoringName] = useState<string | null>(null);

  // Modal states
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [confirmRestore, setConfirmRestore] = useState<string | null>(null);

  // Success/info messages
  const [successMessage, setSuccessMessage] = useState<string | null>(null);

  // ── Data fetching ────────────────────────────────────────────────────────

  const fetchBackups = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const data = await client.listBackups();
      // Sort by created date descending (newest first)
      data.sort((a, b) => new Date(b.created).getTime() - new Date(a.created).getTime());
      setBackups(data);
    } catch (err) {
      if (err instanceof ApiError) {
        setError(`Failed to load backups: ${err.message}`);
      } else {
        setError('Failed to load backups. Please try again.');
      }
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchBackups();
  }, [fetchBackups]);

  // Clear success messages after a timeout
  useEffect(() => {
    if (!successMessage) return;
    const timer = setTimeout(() => setSuccessMessage(null), 5000);
    return () => clearTimeout(timer);
  }, [successMessage]);

  // ── Actions ──────────────────────────────────────────────────────────────

  const handleCreateBackup = useCallback(async () => {
    try {
      setCreating(true);
      setError(null);
      setSuccessMessage(null);
      await client.createBackup();
      setSuccessMessage('Backup created successfully.');
      await fetchBackups();
    } catch (err) {
      if (err instanceof ApiError) {
        setError(`Failed to create backup: ${err.message}`);
      } else {
        setError('Failed to create backup. Please try again.');
      }
    } finally {
      setCreating(false);
    }
  }, [fetchBackups]);

  const handleDownload = useCallback(async (name: string) => {
    try {
      setDownloadingName(name);
      setError(null);
      const response = await client.downloadBackup(name);
      const blob = await response.blob();
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = name;
      document.body.appendChild(link);
      link.click();
      document.body.removeChild(link);
      URL.revokeObjectURL(url);
    } catch (err) {
      if (err instanceof ApiError) {
        setError(`Failed to download backup: ${err.message}`);
      } else {
        setError('Failed to download backup. Please try again.');
      }
    } finally {
      setDownloadingName(null);
    }
  }, []);

  const handleDelete = useCallback(async (name: string) => {
    try {
      setDeletingName(name);
      setConfirmDelete(null);
      setError(null);
      setSuccessMessage(null);
      await client.deleteBackup(name);
      setSuccessMessage(`Backup "${name}" deleted successfully.`);
      await fetchBackups();
    } catch (err) {
      if (err instanceof ApiError) {
        setError(`Failed to delete backup: ${err.message}`);
      } else {
        setError('Failed to delete backup. Please try again.');
      }
    } finally {
      setDeletingName(null);
    }
  }, [fetchBackups]);

  const handleRestore = useCallback(async (name: string) => {
    try {
      setRestoringName(name);
      setConfirmRestore(null);
      setError(null);
      setSuccessMessage(null);
      await client.restoreBackup(name);
      setSuccessMessage(`Database restored from "${name}" successfully. The page will reload shortly.`);
      // Reload page after restore to reflect any changes
      setTimeout(() => { window.location.reload(); }, 3000);
    } catch (err) {
      if (err instanceof ApiError) {
        setError(`Failed to restore backup: ${err.message}`);
      } else {
        setError('Failed to restore backup. Please try again.');
      }
    } finally {
      setRestoringName(null);
    }
  }, []);

  // ── Render ───────────────────────────────────────────────────────────────

  return (
    <DashboardLayout currentPath="/_/backups" pageTitle="Backups">
      {/* Success message */}
      {successMessage && (
        <div
          className="mb-4 rounded-lg border border-green-200 dark:border-green-800 bg-green-50 dark:bg-green-900/30 px-4 py-3 text-sm text-green-700 dark:text-green-400"
          role="status"
          aria-live="polite"
          data-testid="success-message"
        >
          {successMessage}
        </div>
      )}

      {/* Error banner */}
      {error && (
        <div
          className="mb-4 rounded-lg border border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30 px-4 py-3 text-sm text-red-700 dark:text-red-400"
          data-testid="error-banner"
        >
          <div className="flex items-center justify-between">
            <span>{error}</span>
            <button
              type="button"
              onClick={() => { setError(null); fetchBackups(); }}
              className="ml-4 text-sm font-medium text-red-700 dark:text-red-400 underline hover:text-red-900 dark:hover:text-red-300"
            >
              Retry
            </button>
          </div>
        </div>
      )}

      {/* Header with create button */}
      <div className="mb-6 flex items-center justify-between" data-testid="backups-header">
        <p className="text-sm text-gray-500 dark:text-gray-400">
          Create, download, and restore database backups.
        </p>
        <button
          type="button"
          onClick={handleCreateBackup}
          disabled={creating}
          className="inline-flex items-center gap-2 rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-blue-700 dark:hover:bg-blue-600 focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500 focus:ring-offset-2 disabled:opacity-50"
          data-testid="create-backup-btn"
        >
          {creating ? (
            <>
              <svg className="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
              </svg>
              Creating\u2026
            </>
          ) : (
            <>
              <PlusIcon />
              Create Backup
            </>
          )}
        </button>
      </div>

      {/* Loading skeletons */}
      {loading && (
        <div className="space-y-3" data-testid="loading-skeleton">
          {[1, 2, 3].map((i) => (
            <div key={i} className="animate-pulse rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4">
              <div className="flex items-center justify-between">
                <div className="space-y-2">
                  <div className="h-4 w-48 rounded bg-gray-200 dark:bg-gray-600" />
                  <div className="h-3 w-32 rounded bg-gray-100 dark:bg-gray-700" />
                </div>
                <div className="flex gap-2">
                  <div className="h-8 w-8 rounded bg-gray-200 dark:bg-gray-600" />
                  <div className="h-8 w-8 rounded bg-gray-200 dark:bg-gray-600" />
                  <div className="h-8 w-8 rounded bg-gray-200 dark:bg-gray-600" />
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Empty state */}
      {!loading && backups.length === 0 && (
        <div className="rounded-lg border-2 border-dashed border-gray-200 dark:border-gray-700 p-12 text-center" data-testid="empty-state">
          <DatabaseIcon />
          <h3 className="mt-4 text-base font-semibold text-gray-900 dark:text-gray-100">No backups yet</h3>
          <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
            Create your first backup to protect your data.
          </p>
          <button
            type="button"
            onClick={handleCreateBackup}
            disabled={creating}
            className="mt-4 inline-flex items-center gap-2 rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 dark:hover:bg-blue-600 disabled:opacity-50"
            data-testid="empty-create-btn"
          >
            <PlusIcon />
            Create Backup
          </button>
        </div>
      )}

      {/* Backup list */}
      {!loading && backups.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 shadow-sm" data-testid="backups-list">
          <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-700">
            <thead className="bg-gray-50 dark:bg-gray-900">
              <tr>
                <th scope="col" className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">
                  Name
                </th>
                <th scope="col" className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">
                  Size
                </th>
                <th scope="col" className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">
                  Created
                </th>
                <th scope="col" className="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">
                  Actions
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 dark:divide-gray-700">
              {backups.map((backup) => {
                const isDeleting = deletingName === backup.name;
                const isDownloading = downloadingName === backup.name;
                const isRestoring = restoringName === backup.name;
                const isBusy = isDeleting || isDownloading || isRestoring;

                return (
                  <tr key={backup.name} className="hover:bg-gray-50 dark:hover:bg-gray-700" data-testid={`backup-row-${backup.name}`}>
                    <td className="whitespace-nowrap px-4 py-3">
                      <div className="flex items-center gap-2">
                        <svg className="h-5 w-5 text-gray-400 dark:text-gray-500" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" aria-hidden="true">
                          <path strokeLinecap="round" strokeLinejoin="round" d="M20.25 7.5l-.625 10.632a2.25 2.25 0 01-2.247 2.118H6.622a2.25 2.25 0 01-2.247-2.118L3.75 7.5m8.25 3v6.75m0 0l-3-3m3 3l3-3M3.375 7.5h17.25c.621 0 1.125-.504 1.125-1.125v-1.5c0-.621-.504-1.125-1.125-1.125H3.375c-.621 0-1.125.504-1.125 1.125v1.5c0 .621.504 1.125 1.125 1.125z" />
                        </svg>
                        <span className="text-sm font-medium text-gray-900 dark:text-gray-100">{backup.name}</span>
                      </div>
                    </td>
                    <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-500 dark:text-gray-400">
                      {formatBytes(backup.size)}
                    </td>
                    <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-500 dark:text-gray-400">
                      {formatDate(backup.created)}
                    </td>
                    <td className="whitespace-nowrap px-4 py-3 text-right">
                      <div className="flex items-center justify-end gap-1">
                        {/* Download */}
                        <button
                          type="button"
                          onClick={() => handleDownload(backup.name)}
                          disabled={isBusy}
                          title="Download backup"
                          aria-label={`Download ${backup.name}`}
                          className="rounded p-1.5 text-gray-400 dark:text-gray-500 hover:bg-blue-50 dark:hover:bg-blue-900/30 hover:text-blue-600 dark:hover:text-blue-400 disabled:opacity-50"
                          data-testid={`download-${backup.name}`}
                        >
                          {isDownloading ? (
                            <svg className="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                              <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                              <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                            </svg>
                          ) : (
                            <DownloadIcon />
                          )}
                        </button>

                        {/* Restore */}
                        <button
                          type="button"
                          onClick={() => setConfirmRestore(backup.name)}
                          disabled={isBusy}
                          title="Restore from backup"
                          aria-label={`Restore from ${backup.name}`}
                          className="rounded p-1.5 text-gray-400 dark:text-gray-500 hover:bg-yellow-50 dark:hover:bg-yellow-900/30 hover:text-yellow-600 dark:hover:text-yellow-400 disabled:opacity-50"
                          data-testid={`restore-${backup.name}`}
                        >
                          {isRestoring ? (
                            <svg className="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                              <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                              <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                            </svg>
                          ) : (
                            <RestoreIcon />
                          )}
                        </button>

                        {/* Delete */}
                        <button
                          type="button"
                          onClick={() => setConfirmDelete(backup.name)}
                          disabled={isBusy}
                          title="Delete backup"
                          aria-label={`Delete ${backup.name}`}
                          className="rounded p-1.5 text-gray-400 dark:text-gray-500 hover:bg-red-50 dark:hover:bg-red-900/30 hover:text-red-600 dark:hover:text-red-400 disabled:opacity-50"
                          data-testid={`delete-${backup.name}`}
                        >
                          {isDeleting ? (
                            <svg className="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                              <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                              <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                            </svg>
                          ) : (
                            <TrashIcon />
                          )}
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {/* Summary */}
      {!loading && backups.length > 0 && (
        <p className="mt-3 text-xs text-gray-400 dark:text-gray-500" data-testid="backup-count">
          {backups.length} backup{backups.length !== 1 ? 's' : ''} &middot;{' '}
          {formatBytes(backups.reduce((sum, b) => sum + b.size, 0))} total
        </p>
      )}

      {/* Delete confirmation modal */}
      {confirmDelete && (
        <ConfirmModal
          title="Delete Backup"
          message={`Are you sure you want to delete "${confirmDelete}"? This action cannot be undone.`}
          confirmLabel="Delete"
          confirmVariant="danger"
          onConfirm={() => handleDelete(confirmDelete)}
          onCancel={() => setConfirmDelete(null)}
          loading={deletingName !== null}
        />
      )}

      {/* Restore confirmation modal */}
      {confirmRestore && (
        <ConfirmModal
          title="Restore from Backup"
          message={`Are you sure you want to restore the database from "${confirmRestore}"? This will replace the current database with the backup data. This action cannot be undone.`}
          confirmLabel="Restore"
          confirmVariant="danger"
          onConfirm={() => handleRestore(confirmRestore)}
          onCancel={() => setConfirmRestore(null)}
          loading={restoringName !== null}
        />
      )}
    </DashboardLayout>
  );
}
