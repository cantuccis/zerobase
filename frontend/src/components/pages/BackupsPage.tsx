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

// ── Spinner ──────────────────────────────────────────────────────────────────

function Spinner({ className = 'h-4 w-4' }: { className?: string }) {
  return (
    <svg className={`${className} animate-spin`} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
      <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
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
      className="fixed inset-0 z-50 flex items-center justify-center bg-primary/50 animate-fade-in"
      data-testid="confirm-modal"
      role="dialog"
      aria-modal="true"
      aria-labelledby="confirm-modal-title"
      onClick={(e) => { if (e.target === e.currentTarget && !loading) onCancel(); }}
      ref={dialogRef}
    >
      <div className="mx-4 w-full max-w-md border border-primary dark:border-primary bg-background dark:bg-background p-6 animate-scale-in">
        <h3 id="confirm-modal-title" className="text-[12px] font-bold uppercase tracking-widest text-primary dark:text-primary">{title}</h3>
        <p className="mt-3 text-sm text-on-surface dark:text-on-surface leading-relaxed">{message}</p>

        <div className="mt-6 flex justify-end gap-3">
          <button
            type="button"
            onClick={onCancel}
            disabled={loading}
            className="border border-primary dark:border-primary bg-background dark:bg-background px-5 py-2.5 text-[11px] font-bold uppercase tracking-wider text-primary dark:text-primary hover:bg-surface-container dark:hover:bg-surface-container disabled:opacity-50 transition-colors-fast"
            data-testid="confirm-cancel"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={loading}
            className={`px-5 py-2.5 text-[11px] font-bold uppercase tracking-wider disabled:opacity-50 ${
              confirmVariant === 'danger'
                ? 'bg-error text-on-error hover:opacity-90'
                : 'bg-primary text-on-primary hover:opacity-90'
            }`}
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

  // ── Computed metrics ───────────────────────────────────────────────────────

  const totalSize = backups.reduce((sum, b) => sum + b.size, 0);
  const newestBackup = backups.length > 0 ? backups[0] : null;

  // ── Render ───────────────────────────────────────────────────────────────

  return (
    <DashboardLayout currentPath="/_/backups" pageTitle="Backups">
      {/* Success message */}
      {successMessage && (
        <div
          className="mb-6 border border-primary dark:border-primary bg-surface-container-low dark:bg-surface-container-low px-4 py-3 text-[12px] font-bold uppercase tracking-wider text-primary dark:text-primary flex items-center gap-3"
          role="status"
          aria-live="polite"
          data-testid="success-message"
        >
          <span className="material-symbols-outlined text-[16px]" aria-hidden="true">check_circle</span>
          {successMessage}
        </div>
      )}

      {/* Error banner */}
      {error && (
        <div
          className="mb-6 border border-error dark:border-error bg-error-container dark:bg-error-container px-4 py-3 text-[12px] font-bold uppercase tracking-wider text-on-error-container dark:text-on-error-container"
          role="alert"
          data-testid="error-banner"
        >
          <div className="flex items-center justify-between">
            <span className="flex items-center gap-2">
              <span className="material-symbols-outlined text-[16px]" aria-hidden="true">error</span>
              {error}
            </span>
            <button
              type="button"
              onClick={() => { setError(null); fetchBackups(); }}
              className="ml-4 text-[11px] font-bold uppercase tracking-wider underline"
            >
              Retry
            </button>
          </div>
        </div>
      )}

      {/* Hero Header Section */}
      <section className="mb-12" data-testid="backups-header">
        <div className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between mb-6">
          <div>
            <p className="text-[12px] font-bold uppercase tracking-[0.2em] text-outline dark:text-outline mb-3">
              Storage / System
            </p>
            <h2 className="text-[2rem] sm:text-[3.5rem] font-extrabold leading-none tracking-tighter text-primary dark:text-primary">
              DATABASE BACKUPS
            </h2>
          </div>
          <div className="pb-2">
            <button
              type="button"
              onClick={handleCreateBackup}
              disabled={creating}
              className="bg-primary text-on-primary px-8 py-4 text-[12px] font-bold uppercase tracking-widest flex items-center gap-3 hover:opacity-90 disabled:opacity-50"
              data-testid="create-backup-btn"
            >
              {creating ? (
                <>
                  <Spinner />
                  Creating\u2026
                </>
              ) : (
                <>
                  <span className="material-symbols-outlined text-[20px]" aria-hidden="true">add_circle</span>
                  Create Backup
                </>
              )}
            </button>
          </div>
        </div>
        <div className="h-1 bg-primary dark:bg-primary w-24"></div>
      </section>

      {/* Loading skeletons */}
      {loading && (
        <div className="border border-primary dark:border-primary animate-pulse-subtle" data-testid="loading-skeleton">
          {[1, 2, 3].map((i) => (
            <div key={i} className={`p-6 ${i < 3 ? 'border-b border-primary dark:border-primary' : ''}`}>
              <div className="animate-pulse flex items-center gap-4">
                <div className="h-6 w-6 bg-surface-container-high dark:bg-surface-container-high shrink-0" />
                <div className="space-y-2 flex-1 min-w-0">
                  <div className="h-4 w-48 max-w-full bg-surface-container-high dark:bg-surface-container-high" />
                  <div className="h-3 w-32 max-w-full bg-surface-container dark:bg-surface-container" />
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Empty state */}
      {!loading && backups.length === 0 && (
        <div className="border border-primary dark:border-primary p-16 text-center" data-testid="empty-state">
          <span className="material-symbols-outlined text-[48px] text-outline dark:text-outline" aria-hidden="true">database</span>
          <h3 className="mt-4 text-[12px] font-bold uppercase tracking-widest text-primary dark:text-primary">No Backups Yet</h3>
          <p className="mt-2 text-sm text-secondary dark:text-secondary">
            Create your first backup to protect your data.
          </p>
          <button
            type="button"
            onClick={handleCreateBackup}
            disabled={creating}
            className="mt-6 bg-primary text-on-primary px-8 py-3 text-[12px] font-bold uppercase tracking-widest inline-flex items-center gap-3 hover:opacity-90 disabled:opacity-50"
            data-testid="empty-create-btn"
          >
            <span className="material-symbols-outlined text-[18px]" aria-hidden="true">add_circle</span>
            Create Backup
          </button>
        </div>
      )}

      {/* Backup Grid Table */}
      {!loading && backups.length > 0 && (
        <div className="border border-primary dark:border-primary" role="table" aria-label="Backups" data-testid="backups-list">
          {/* Table Header — hidden on mobile */}
          <div className="hidden md:grid md:grid-cols-12 gap-0" role="row">
            <div className="col-span-6 border-r border-b border-primary dark:border-primary p-4 bg-primary dark:bg-primary" role="columnheader">
              <span className="text-[11px] font-bold uppercase tracking-widest text-on-primary dark:text-on-primary">Name &amp; Path</span>
            </div>
            <div className="col-span-2 border-r border-b border-primary dark:border-primary p-4 bg-primary dark:bg-primary text-center" role="columnheader">
              <span className="text-[11px] font-bold uppercase tracking-widest text-on-primary dark:text-on-primary">Snapshot Date</span>
            </div>
            <div className="col-span-2 border-r border-b border-primary dark:border-primary p-4 bg-primary dark:bg-primary text-center" role="columnheader">
              <span className="text-[11px] font-bold uppercase tracking-widest text-on-primary dark:text-on-primary">Size</span>
            </div>
            <div className="col-span-2 border-b border-primary dark:border-primary p-4 bg-primary dark:bg-primary text-center" role="columnheader">
              <span className="text-[11px] font-bold uppercase tracking-widest text-on-primary dark:text-on-primary">Actions</span>
            </div>
          </div>

          {/* Table Rows */}
          {backups.map((backup) => {
            const isDeleting = deletingName === backup.name;
            const isDownloading = downloadingName === backup.name;
            const isRestoring = restoringName === backup.name;
            const isBusy = isDeleting || isDownloading || isRestoring;

            return (
              <div
                key={backup.name}
                className="border-b border-outline-variant dark:border-outline-variant last:border-b-0"
                role="row"
                data-testid={`backup-row-${backup.name}`}
              >
                {/* Mobile card layout */}
                <div className="md:hidden p-5 space-y-3 hover:bg-surface-container-low dark:hover:bg-surface-container-low">
                  <div className="flex items-center gap-3">
                    <span className="material-symbols-outlined text-primary dark:text-primary text-[24px] shrink-0" aria-hidden="true">folder_zip</span>
                    <div className="min-w-0">
                      <p className="font-semibold text-base text-on-background dark:text-on-background truncate font-mono">
                        {backup.name}
                      </p>
                      <code className="text-[10px] text-outline dark:text-outline uppercase tracking-wider">
                        pb_data/backups
                      </code>
                    </div>
                  </div>
                  <div className="flex items-center justify-between text-sm">
                    <span className="font-mono text-[13px] text-on-background dark:text-on-background">{formatDate(backup.created)}</span>
                    <span className="font-bold font-mono text-on-background dark:text-on-background">{formatBytes(backup.size)}</span>
                  </div>
                  <div className="flex items-center gap-2">
                    <button type="button" onClick={() => setConfirmRestore(backup.name)} disabled={isBusy} aria-label={`Restore from ${backup.name}`} className="min-h-[44px] min-w-[44px] flex items-center justify-center border border-primary hover:bg-primary hover:text-on-primary dark:hover:bg-primary dark:hover:text-on-primary text-on-background dark:text-on-background disabled:opacity-50 transition-colors-fast" data-testid={`restore-${backup.name}`}>
                      {isRestoring ? <Spinner /> : <span className="material-symbols-outlined text-[20px]" aria-hidden="true">settings_backup_restore</span>}
                    </button>
                    <button type="button" onClick={() => handleDownload(backup.name)} disabled={isBusy} aria-label={`Download ${backup.name}`} className="min-h-[44px] min-w-[44px] flex items-center justify-center border border-primary hover:bg-primary hover:text-on-primary dark:hover:bg-primary dark:hover:text-on-primary text-on-background dark:text-on-background disabled:opacity-50 transition-colors-fast" data-testid={`download-${backup.name}`}>
                      {isDownloading ? <Spinner /> : <span className="material-symbols-outlined text-[20px]" aria-hidden="true">download</span>}
                    </button>
                    <button type="button" onClick={() => setConfirmDelete(backup.name)} disabled={isBusy} aria-label={`Delete ${backup.name}`} className="min-h-[44px] min-w-[44px] flex items-center justify-center border border-error hover:bg-error hover:text-on-error text-on-background dark:text-on-background disabled:opacity-50 transition-colors-fast" data-testid={`delete-${backup.name}`}>
                      {isDeleting ? <Spinner /> : <span className="material-symbols-outlined text-[20px]" aria-hidden="true">delete</span>}
                    </button>
                  </div>
                </div>

                {/* Desktop grid layout */}
                <div className="hidden md:grid md:grid-cols-12 gap-0">
                  <div className="col-span-6 border-r border-outline-variant dark:border-outline-variant p-5 flex items-center gap-4 hover:bg-surface-container-low dark:hover:bg-surface-container-low" role="cell">
                    <span className="material-symbols-outlined text-primary dark:text-primary text-[24px]" aria-hidden="true">folder_zip</span>
                    <div className="min-w-0">
                      <p className="font-semibold text-base text-on-background dark:text-on-background truncate font-mono">
                        {backup.name}
                      </p>
                      <code className="text-[10px] text-outline dark:text-outline uppercase tracking-wider">
                        pb_data/backups
                      </code>
                    </div>
                  </div>
                  <div className="col-span-2 border-r border-outline-variant dark:border-outline-variant p-5 flex items-center justify-center hover:bg-surface-container-low dark:hover:bg-surface-container-low" role="cell">
                    <p className="text-[13px] text-on-background dark:text-on-background font-mono">
                      {formatDate(backup.created)}
                    </p>
                  </div>
                  <div className="col-span-2 border-r border-outline-variant dark:border-outline-variant p-5 flex items-center justify-center hover:bg-surface-container-low dark:hover:bg-surface-container-low" role="cell">
                    <p className="font-bold text-lg text-on-background dark:text-on-background font-mono">
                      {formatBytes(backup.size)}
                    </p>
                  </div>
                  <div className="col-span-2 p-5 flex items-center justify-center gap-3 hover:bg-surface-container-low dark:hover:bg-surface-container-low" role="cell">
                    <button type="button" onClick={() => setConfirmRestore(backup.name)} disabled={isBusy} title="Restore from backup" aria-label={`Restore from ${backup.name}`} className="p-2 border border-transparent hover:bg-primary hover:text-on-primary dark:hover:bg-primary dark:hover:text-on-primary text-on-background dark:text-on-background hover:border-primary dark:hover:border-primary disabled:opacity-50 transition-colors-fast" data-testid={`restore-desktop-${backup.name}`}>
                      {isRestoring ? <Spinner /> : <span className="material-symbols-outlined text-[20px]" aria-hidden="true">settings_backup_restore</span>}
                    </button>
                    <button type="button" onClick={() => handleDownload(backup.name)} disabled={isBusy} title="Download backup" aria-label={`Download ${backup.name}`} className="p-2 border border-transparent hover:bg-primary hover:text-on-primary dark:hover:bg-primary dark:hover:text-on-primary text-on-background dark:text-on-background hover:border-primary dark:hover:border-primary disabled:opacity-50 transition-colors-fast" data-testid={`download-desktop-${backup.name}`}>
                      {isDownloading ? <Spinner /> : <span className="material-symbols-outlined text-[20px]" aria-hidden="true">download</span>}
                    </button>
                    <button type="button" onClick={() => setConfirmDelete(backup.name)} disabled={isBusy} title="Delete backup" aria-label={`Delete ${backup.name}`} className="p-2 border border-transparent hover:bg-error hover:text-on-error text-on-background dark:text-on-background hover:border-error disabled:opacity-50 transition-colors-fast" data-testid={`delete-desktop-${backup.name}`}>
                      {isDeleting ? <Spinner /> : <span className="material-symbols-outlined text-[20px]" aria-hidden="true">delete</span>}
                    </button>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* Warning / Technical Note */}
      {!loading && backups.length > 0 && (
        <div className="mt-12 p-6 border-l-4 border-primary dark:border-primary bg-surface-container dark:bg-surface-container flex items-start gap-5">
          <div className="bg-primary dark:bg-primary text-on-primary dark:text-on-primary p-2 shrink-0">
            <span className="material-symbols-outlined text-[24px]" aria-hidden="true">warning</span>
          </div>
          <div>
            <h4 className="text-[12px] font-bold uppercase tracking-widest mb-2 text-primary dark:text-primary">
              Technical Performance Note
            </h4>
            <p className="text-sm leading-relaxed max-w-3xl text-on-surface dark:text-on-surface">
              Database operations will enter a <span className="font-bold underline">READ-ONLY</span> state during the backup compression process.
              New record creation, file uploads, and session updates will be queued until the backup snapshot is successfully finalized.
              For high-traffic environments, we recommend scheduling backups during maintenance windows.
            </p>
          </div>
        </div>
      )}

      {/* Metrics Grid Footer */}
      {!loading && backups.length > 0 && (
        <div className="mt-12 grid grid-cols-2 sm:grid-cols-4 gap-0 border border-outline-variant dark:border-outline-variant" data-testid="backup-metrics">
          <div className="border-r border-b sm:border-b-0 border-outline-variant dark:border-outline-variant p-6">
            <p className="text-[10px] font-bold uppercase tracking-widest text-outline dark:text-outline mb-2">Total Backups</p>
            <p className="text-4xl font-extrabold text-primary dark:text-primary">{backups.length}</p>
          </div>
          <div className="border-b sm:border-b-0 sm:border-r border-outline-variant dark:border-outline-variant p-6">
            <p className="text-[10px] font-bold uppercase tracking-widest text-outline dark:text-outline mb-2">Storage Used</p>
            <p className="text-4xl font-extrabold text-primary dark:text-primary font-mono">{formatBytes(totalSize)}</p>
          </div>
          <div className="border-r border-outline-variant dark:border-outline-variant p-6">
            <p className="text-[10px] font-bold uppercase tracking-widest text-outline dark:text-outline mb-2">Latest Backup</p>
            <p className="text-lg font-extrabold text-primary dark:text-primary font-mono">
              {newestBackup ? new Date(newestBackup.created).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) : '\u2014'}
            </p>
          </div>
          <div className="p-6">
            <p className="text-[10px] font-bold uppercase tracking-widest text-outline dark:text-outline mb-2">Status</p>
            <p className="text-4xl font-extrabold text-primary dark:text-primary">OK</p>
          </div>
        </div>
      )}

      {/* Summary line */}
      {!loading && backups.length > 0 && (
        <p className="mt-4 text-[10px] font-bold uppercase tracking-widest text-outline dark:text-outline" data-testid="backup-count">
          {backups.length} backup{backups.length !== 1 ? 's' : ''} &middot; {formatBytes(totalSize)} total
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
