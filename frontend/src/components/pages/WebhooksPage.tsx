import { useState, useEffect, useCallback, useRef } from 'react';
import { DashboardLayout } from '../DashboardLayout';
import { client } from '../../lib/auth/client';
import { ApiError } from '../../lib/api';
import type {
  Webhook,
  WebhookEvent,
  WebhookDeliveryLog,
  CreateWebhookInput,
  UpdateWebhookInput,
  ListResponse,
  Collection,
} from '../../lib/api/types';

// ── Constants ────────────────────────────────────────────────────────────────

const WEBHOOK_EVENTS: { value: WebhookEvent; label: string }[] = [
  { value: 'create', label: 'CREATE' },
  { value: 'update', label: 'UPDATE' },
  { value: 'delete', label: 'DELETE' },
];

const DELIVERIES_PER_PAGE = 20;

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

// ── Webhook Form Modal ───────────────────────────────────────────────────────

interface WebhookFormProps {
  webhook: Webhook | null;
  collections: Collection[];
  onSave: (data: CreateWebhookInput | UpdateWebhookInput, id?: string) => Promise<void>;
  onClose: () => void;
  saving: boolean;
  error: string | null;
  fieldErrors: Record<string, string>;
}

function WebhookFormModal({ webhook, collections, onSave, onClose, saving, error, fieldErrors }: WebhookFormProps) {
  const isEdit = webhook !== null;
  const [url, setUrl] = useState(webhook?.url ?? '');
  const [collection, setCollection] = useState(webhook?.collection ?? (collections[0]?.name ?? ''));
  const [events, setEvents] = useState<WebhookEvent[]>(webhook?.events ?? ['create']);
  const [secret, setSecret] = useState('');
  const [enabled, setEnabled] = useState(webhook?.enabled ?? true);
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const firstFocusable = dialogRef.current?.querySelector<HTMLElement>('button, a, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])');
    firstFocusable?.focus();

    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        onClose();
        return;
      }
      if (e.key === 'Tab' && dialogRef.current) {
        const focusable = dialogRef.current.querySelectorAll<HTMLElement>('button, a, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])');
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
  }, [onClose]);

  function toggleEvent(event: WebhookEvent) {
    setEvents((prev) =>
      prev.includes(event) ? prev.filter((e) => e !== event) : [...prev, event],
    );
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (isEdit) {
      const data: UpdateWebhookInput = { url, events, enabled };
      if (secret) data.secret = secret;
      await onSave(data, webhook.id);
    } else {
      const data: CreateWebhookInput = { collection, url, events, enabled };
      if (secret) data.secret = secret;
      await onSave(data);
    }
  }

  return (
    <div ref={dialogRef} className="fixed inset-0 z-50 flex items-center justify-center bg-primary/30 dark:bg-on-primary/30 animate-fade-in" role="dialog" aria-modal="true" aria-label={isEdit ? 'Edit webhook' : 'Add webhook'}>
      <div className="w-full max-w-lg border border-primary dark:border-on-primary bg-surface-lowest dark:bg-surface-container p-6 animate-scale-in">
        <h2 className="mb-6 text-title-md text-on-surface dark:text-on-surface">
          {isEdit ? 'Edit Webhook' : 'Add Webhook'}
        </h2>

        {error && (
          <div className="mb-4 border border-error bg-error-container dark:bg-error/10 px-4 py-3 text-sm text-on-error-container dark:text-error" role="alert">
            {error}
          </div>
        )}

        <form onSubmit={handleSubmit} className="space-y-5">
          {/* Collection (only on create) */}
          {!isEdit && (
            <div>
              <label htmlFor="webhook-collection" className="text-label-md block mb-2 text-on-surface dark:text-on-surface">
                Collection
              </label>
              <select
                id="webhook-collection"
                value={collection}
                onChange={(e) => setCollection(e.target.value)}
                className="w-full border border-primary dark:border-on-primary bg-surface-lowest dark:bg-surface-lowest px-4 py-3 text-sm text-on-surface dark:text-on-surface focus:border-2 focus:px-[15px] focus:py-[11px] outline-none"
              >
                {collections.map((c) => (
                  <option key={c.id} value={c.name}>{c.name}</option>
                ))}
              </select>
              {fieldErrors.collection && <p className="mt-1 text-sm text-error">{fieldErrors.collection}</p>}
            </div>
          )}

          {/* URL */}
          <div>
            <label htmlFor="webhook-url" className="text-label-md block mb-2 text-on-surface dark:text-on-surface">
              URL
            </label>
            <input
              id="webhook-url"
              type="url"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              required
              placeholder="https://example.com/webhook"
              className="w-full border border-primary dark:border-on-primary bg-surface-lowest dark:bg-surface-lowest px-4 py-3 text-sm font-data text-on-surface dark:text-on-surface placeholder:text-outline focus:border-2 focus:px-[15px] focus:py-[11px] outline-none"
            />
            {fieldErrors.url && <p className="mt-1 text-sm text-error">{fieldErrors.url}</p>}
          </div>

          {/* Events */}
          <fieldset>
            <legend className="text-label-md block mb-3 text-on-surface dark:text-on-surface">Events</legend>
            <div className="flex gap-0">
              {WEBHOOK_EVENTS.map(({ value, label }) => {
                const checked = events.includes(value);
                return (
                  <button
                    key={value}
                    type="button"
                    onClick={() => toggleEvent(value)}
                    className={`border border-primary dark:border-on-primary -ml-px first:ml-0 px-4 py-2 text-label-sm cursor-pointer ${
                      checked
                        ? 'bg-primary dark:bg-on-primary text-on-primary dark:text-primary'
                        : 'bg-surface-lowest dark:bg-surface-lowest text-on-surface dark:text-on-surface hover:bg-surface-container dark:hover:bg-surface-container'
                    }`}
                    aria-pressed={checked}
                  >
                    {label}
                  </button>
                );
              })}
            </div>
            {fieldErrors.events && <p className="mt-1 text-sm text-error">{fieldErrors.events}</p>}
          </fieldset>

          {/* Secret */}
          <div>
            <label htmlFor="webhook-secret" className="text-label-md block mb-2 text-on-surface dark:text-on-surface">
              Secret <span className="text-outline font-normal normal-case tracking-normal text-xs">(optional)</span>
            </label>
            <input
              id="webhook-secret"
              type="password"
              value={secret}
              onChange={(e) => setSecret(e.target.value)}
              placeholder={isEdit ? 'Leave empty to keep current' : 'HMAC-SHA256 signing secret'}
              autoComplete="off"
              className="w-full border border-primary dark:border-on-primary bg-surface-lowest dark:bg-surface-lowest px-4 py-3 text-sm text-on-surface dark:text-on-surface placeholder:text-outline focus:border-2 focus:px-[15px] focus:py-[11px] outline-none"
            />
          </div>

          {/* Enabled toggle */}
          <div className="flex items-center gap-3">
            <button
              type="button"
              role="switch"
              aria-checked={enabled}
              aria-label="Enable webhook"
              onClick={() => setEnabled(!enabled)}
              className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer border focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-on-surface ${
                enabled
                  ? 'bg-on-surface dark:bg-on-surface border-on-surface dark:border-on-surface'
                  : 'bg-surface-container dark:bg-surface-container border-outline dark:border-outline'
              }`}
            >
              <span
                className={`pointer-events-none inline-block h-4 w-4 transform bg-surface-lowest dark:bg-surface-lowest ${
                  enabled ? 'translate-x-[22px]' : 'translate-x-[3px]'
                } translate-y-[3px]`}
              />
            </button>
            <span className="text-sm text-on-surface dark:text-on-surface">{enabled ? 'Enabled' : 'Disabled'}</span>
          </div>

          {/* Actions */}
          <div className="flex justify-end gap-3 pt-2">
            <button
              type="button"
              onClick={onClose}
              className="border border-primary dark:border-on-primary bg-transparent px-4 py-2 text-label-md text-on-surface dark:text-on-surface hover:bg-primary hover:text-on-primary dark:hover:bg-on-primary dark:hover:text-primary cursor-pointer"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={saving || events.length === 0}
              className="border border-primary dark:border-on-primary bg-primary dark:bg-on-primary px-4 py-2 text-label-md text-on-primary dark:text-primary hover:bg-transparent hover:text-on-surface dark:hover:bg-transparent dark:hover:text-on-surface disabled:opacity-50 cursor-pointer"
            >
              {saving ? 'Saving\u2026' : isEdit ? 'Update' : 'Create'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

// ── Delivery History Panel ───────────────────────────────────────────────────

interface DeliveryHistoryProps {
  webhookId: string;
  onClose: () => void;
}

function DeliveryHistory({ webhookId, onClose }: DeliveryHistoryProps) {
  const [deliveries, setDeliveries] = useState<WebhookDeliveryLog[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [page, setPage] = useState(1);
  const [totalPages, setTotalPages] = useState(1);
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const closeBtn = dialogRef.current?.querySelector<HTMLElement>('[aria-label="Close delivery history"]');
    closeBtn?.focus();

    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        onClose();
        return;
      }
      if (e.key === 'Tab' && dialogRef.current) {
        const focusable = dialogRef.current.querySelectorAll<HTMLElement>('button, a, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])');
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
  }, [onClose]);

  const loadDeliveries = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result: ListResponse<WebhookDeliveryLog> = await client.listWebhookDeliveries(webhookId, {
        page,
        perPage: DELIVERIES_PER_PAGE,
      });
      setDeliveries(result.items);
      setTotalPages(result.totalPages);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Failed to load delivery history');
    } finally {
      setLoading(false);
    }
  }, [webhookId, page]);

  useEffect(() => {
    loadDeliveries();
  }, [loadDeliveries]);

  return (
    <div ref={dialogRef} className="fixed inset-0 z-50 flex items-center justify-center bg-primary/30 dark:bg-on-primary/30 animate-fade-in" role="dialog" aria-modal="true" aria-label="Delivery history">
      <div className="w-full max-w-3xl border border-primary dark:border-on-primary bg-surface-lowest dark:bg-surface-container p-6 animate-slide-up" style={{ maxHeight: '80vh', overflow: 'auto' }}>
        <div className="mb-6 flex items-center justify-between">
          <h2 className="text-title-md text-on-surface dark:text-on-surface">Delivery History</h2>
          <button
            type="button"
            onClick={onClose}
            className="border border-primary dark:border-on-primary p-1.5 text-on-surface dark:text-on-surface hover:bg-primary hover:text-on-primary dark:hover:bg-on-primary dark:hover:text-primary cursor-pointer"
            aria-label="Close delivery history"
          >
            <svg className="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        {error && (
          <div className="mb-4 border border-error bg-error-container dark:bg-error/10 px-4 py-3 text-sm text-on-error-container dark:text-error" role="alert">
            {error}
          </div>
        )}

        {loading ? (
          <div className="flex justify-center py-12">
            <div className="h-6 w-6 animate-spin border-2 border-primary dark:border-on-primary border-t-transparent" role="status" aria-label="Loading deliveries" />
          </div>
        ) : deliveries.length === 0 ? (
          <p className="py-8 text-center text-label-md text-secondary">NO DELIVERY HISTORY</p>
        ) : (
          <>
            <div className="border border-primary dark:border-on-primary">
              <table className="w-full text-left text-sm" aria-label="Webhook delivery log">
                <thead>
                  <tr className="bg-primary dark:bg-on-primary">
                    <th scope="col" className="px-4 py-2.5 text-label-sm text-on-primary dark:text-primary">Event</th>
                    <th scope="col" className="px-4 py-2.5 text-label-sm text-on-primary dark:text-primary">Record</th>
                    <th scope="col" className="px-4 py-2.5 text-label-sm text-on-primary dark:text-primary">Status</th>
                    <th scope="col" className="px-4 py-2.5 text-label-sm text-on-primary dark:text-primary">HTTP</th>
                    <th scope="col" className="px-4 py-2.5 text-label-sm text-on-primary dark:text-primary">Attempt</th>
                    <th scope="col" className="px-4 py-2.5 text-label-sm text-on-primary dark:text-primary">Date</th>
                  </tr>
                </thead>
                <tbody>
                  {deliveries.map((d) => (
                    <tr key={d.id} className="border-t border-outline-variant dark:border-outline hover:bg-surface-container-low dark:hover:bg-surface-container-low transition-colors-fast">
                      <td className="px-4 py-2.5">
                        <span className="inline-flex items-center border border-primary dark:border-on-primary px-2 py-0.5 text-label-sm text-on-surface dark:text-on-surface">
                          {d.event.toUpperCase()}
                        </span>
                      </td>
                      <td className="px-4 py-2.5 font-data text-xs text-on-surface-variant dark:text-on-surface-variant">{d.recordId}</td>
                      <td className="px-4 py-2.5">
                        <span className={`inline-flex items-center px-2 py-0.5 text-label-sm ${
                          d.status === 'success'
                            ? 'border border-primary dark:border-on-primary bg-primary dark:bg-on-primary text-on-primary dark:text-primary'
                            : d.status === 'failed'
                              ? 'border border-error bg-error text-on-error'
                              : 'border border-outline dark:border-outline text-on-surface-variant dark:text-on-surface-variant'
                        }`}>
                          {d.status.toUpperCase()}
                        </span>
                      </td>
                      <td className="px-4 py-2.5 font-data text-xs text-on-surface-variant dark:text-on-surface-variant">{d.responseStatus || '\u2014'}</td>
                      <td className="px-4 py-2.5 text-on-surface-variant dark:text-on-surface-variant">{d.attempt}</td>
                      <td className="px-4 py-2.5 font-data text-xs text-on-surface-variant dark:text-on-surface-variant whitespace-nowrap">{formatTimestamp(d.created)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            {/* Pagination */}
            {totalPages > 1 && (
              <div className="mt-4 flex items-center justify-between">
                <button
                  type="button"
                  onClick={() => setPage((p) => Math.max(1, p - 1))}
                  disabled={page <= 1}
                  className="border border-primary dark:border-on-primary px-3 py-1.5 text-label-sm text-on-surface dark:text-on-surface hover:bg-primary hover:text-on-primary dark:hover:bg-on-primary dark:hover:text-primary disabled:opacity-30 cursor-pointer"
                >
                  Previous
                </button>
                <span className="text-sm text-on-surface-variant dark:text-on-surface-variant font-data">
                  {page} / {totalPages}
                </span>
                <button
                  type="button"
                  onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
                  disabled={page >= totalPages}
                  className="border border-primary dark:border-on-primary px-3 py-1.5 text-label-sm text-on-surface dark:text-on-surface hover:bg-primary hover:text-on-primary dark:hover:bg-on-primary dark:hover:text-primary disabled:opacity-30 cursor-pointer"
                >
                  Next
                </button>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

// ── Delete Confirmation Modal ────────────────────────────────────────────────

interface DeleteConfirmProps {
  webhookUrl: string;
  onConfirm: () => void;
  onCancel: () => void;
  deleting: boolean;
}

function DeleteConfirmModal({ webhookUrl, onConfirm, onCancel, deleting }: DeleteConfirmProps) {
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const firstFocusable = dialogRef.current?.querySelector<HTMLElement>('button, a, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])');
    firstFocusable?.focus();

    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        onCancel();
        return;
      }
      if (e.key === 'Tab' && dialogRef.current) {
        const focusable = dialogRef.current.querySelectorAll<HTMLElement>('button, a, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])');
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
  }, [onCancel]);

  return (
    <div ref={dialogRef} className="fixed inset-0 z-50 flex items-center justify-center bg-primary/30 dark:bg-on-primary/30 animate-fade-in" role="dialog" aria-modal="true" aria-label="Confirm delete webhook">
      <div className="w-full max-w-sm border border-primary dark:border-on-primary bg-surface-lowest dark:bg-surface-container p-6 animate-scale-in">
        <h2 className="mb-2 text-title-md text-on-surface dark:text-on-surface">Delete Webhook</h2>
        <p className="mb-4 text-sm text-on-surface-variant dark:text-on-surface-variant">
          Are you sure you want to delete the webhook for{' '}
          <span className="font-data text-on-surface dark:text-on-surface">{webhookUrl}</span>?
          This action cannot be undone.
        </p>
        <div className="flex justify-end gap-3">
          <button
            type="button"
            onClick={onCancel}
            className="border border-primary dark:border-on-primary bg-transparent px-4 py-2 text-label-md text-on-surface dark:text-on-surface hover:bg-primary hover:text-on-primary dark:hover:bg-on-primary dark:hover:text-primary cursor-pointer"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={deleting}
            className="border border-error bg-error px-4 py-2 text-label-md text-on-error hover:bg-transparent hover:text-error disabled:opacity-50 cursor-pointer"
          >
            {deleting ? 'Deleting\u2026' : 'Delete'}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Main WebhooksPage Component ──────────────────────────────────────────────

export function WebhooksPage() {
  // Data state
  const [webhooks, setWebhooks] = useState<Webhook[]>([]);
  const [collections, setCollections] = useState<Collection[]>([]);
  const [filterCollection, setFilterCollection] = useState<string>('');

  // UI state
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Form modal
  const [formOpen, setFormOpen] = useState(false);
  const [editingWebhook, setEditingWebhook] = useState<Webhook | null>(null);
  const [formSaving, setFormSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [formFieldErrors, setFormFieldErrors] = useState<Record<string, string>>({});

  // Delete modal
  const [deletingWebhook, setDeletingWebhook] = useState<Webhook | null>(null);
  const [deleteInProgress, setDeleteInProgress] = useState(false);

  // Delivery history
  const [deliveryWebhookId, setDeliveryWebhookId] = useState<string | null>(null);

  // Test webhook
  const [testingId, setTestingId] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<{ id: string; success: boolean; message: string } | null>(null);

  // ── Load data ──────────────────────────────────────────────────────────

  const loadWebhooks = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [webhookList, collectionList] = await Promise.all([
        client.listWebhooks(filterCollection || undefined),
        client.listCollections(),
      ]);
      setWebhooks(webhookList);
      setCollections(collectionList.items);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Failed to load webhooks');
    } finally {
      setLoading(false);
    }
  }, [filterCollection]);

  useEffect(() => {
    loadWebhooks();
  }, [loadWebhooks]);

  // ── Handlers ───────────────────────────────────────────────────────────

  function openCreateForm() {
    setEditingWebhook(null);
    setFormError(null);
    setFormFieldErrors({});
    setFormOpen(true);
  }

  function openEditForm(webhook: Webhook) {
    setEditingWebhook(webhook);
    setFormError(null);
    setFormFieldErrors({});
    setFormOpen(true);
  }

  function closeForm() {
    setFormOpen(false);
    setEditingWebhook(null);
  }

  async function handleSave(data: CreateWebhookInput | UpdateWebhookInput, id?: string) {
    setFormSaving(true);
    setFormError(null);
    setFormFieldErrors({});
    try {
      if (id) {
        await client.updateWebhook(id, data as UpdateWebhookInput);
      } else {
        await client.createWebhook(data as CreateWebhookInput);
      }
      closeForm();
      await loadWebhooks();
    } catch (err) {
      if (err instanceof ApiError && err.isValidation) {
        const fields: Record<string, string> = {};
        for (const [key, val] of Object.entries(err.response.data)) {
          fields[key] = val.message;
        }
        setFormFieldErrors(fields);
        setFormError(err.message);
      } else {
        setFormError(err instanceof ApiError ? err.message : 'Failed to save webhook');
      }
    } finally {
      setFormSaving(false);
    }
  }

  async function handleDelete() {
    if (!deletingWebhook) return;
    setDeleteInProgress(true);
    try {
      await client.deleteWebhook(deletingWebhook.id);
      setDeletingWebhook(null);
      await loadWebhooks();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Failed to delete webhook');
      setDeletingWebhook(null);
    } finally {
      setDeleteInProgress(false);
    }
  }

  async function handleToggleEnabled(webhook: Webhook) {
    try {
      await client.updateWebhook(webhook.id, { enabled: !webhook.enabled });
      await loadWebhooks();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Failed to update webhook');
    }
  }

  async function handleTest(webhook: Webhook) {
    setTestingId(webhook.id);
    setTestResult(null);
    try {
      const result = await client.testWebhook(webhook.id);
      setTestResult({
        id: webhook.id,
        success: result.success,
        message: result.success
          ? `Test delivery succeeded (HTTP ${result.statusCode})`
          : `Test delivery failed: ${result.error ?? `HTTP ${result.statusCode}`}`,
      });
    } catch (err) {
      setTestResult({
        id: webhook.id,
        success: false,
        message: err instanceof ApiError ? err.message : 'Test request failed',
      });
    } finally {
      setTestingId(null);
    }
  }

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <DashboardLayout currentPath="/_/webhooks">
      <div className="space-y-8">
        {/* Header */}
        <div className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <h1 className="text-display-lg text-on-surface dark:text-on-surface">Webhooks</h1>
            <p className="mt-2 text-body-lg text-on-surface-variant dark:text-on-surface-variant">
              Configure HTTP callbacks triggered by record events.
            </p>
          </div>
          <button
            type="button"
            onClick={openCreateForm}
            className="inline-flex items-center gap-2 border border-primary dark:border-on-primary bg-primary dark:bg-on-primary px-5 py-2.5 text-label-md text-on-primary dark:text-primary hover:bg-transparent hover:text-on-surface dark:hover:bg-transparent dark:hover:text-on-surface cursor-pointer"
          >
            <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
            Add Webhook
          </button>
        </div>

        {/* Filter */}
        <div className="flex items-center gap-3">
          <label htmlFor="filter-collection" className="text-label-md text-on-surface dark:text-on-surface">
            Collection
          </label>
          <select
            id="filter-collection"
            value={filterCollection}
            onChange={(e) => setFilterCollection(e.target.value)}
            className="border border-primary dark:border-on-primary bg-surface-lowest dark:bg-surface-lowest px-4 py-2 text-sm text-on-surface dark:text-on-surface outline-none focus:border-2 focus:px-[15px] focus:py-[7px]"
          >
            <option value="">All collections</option>
            {collections.map((c) => (
              <option key={c.id} value={c.name}>{c.name}</option>
            ))}
          </select>
        </div>

        {/* Error */}
        {error && (
          <div className="border border-error bg-error-container dark:bg-error/10 px-4 py-3 text-sm text-on-error-container dark:text-error" role="alert">
            {error}
            <button type="button" onClick={() => setError(null)} className="ml-2 text-label-sm underline cursor-pointer" aria-label="Dismiss error">
              Dismiss
            </button>
          </div>
        )}

        {/* Loading */}
        {loading ? (
          <div className="flex justify-center py-16">
            <div className="h-6 w-6 animate-spin border-2 border-primary dark:border-on-primary border-t-transparent" role="status" aria-label="Loading webhooks" />
          </div>
        ) : webhooks.length === 0 ? (
          /* Empty state */
          <div className="border border-primary dark:border-on-primary p-12 text-center">
            <svg className="mx-auto h-12 w-12 text-outline dark:text-outline" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M18 16.98h-5.99c-1.1 0-1.95.94-2.48 1.9A4 4 0 0 1 2 17c.01-.7.2-1.4.57-2" />
              <path d="m6 17 3.13-5.78c.53-.97.1-2.18-.5-3.1a4 4 0 1 1 6.89-4.06" />
              <path d="m12 6 3.13 5.73C15.66 12.7 16.9 13 18 13a4 4 0 0 1 0 8H12" />
            </svg>
            <p className="mt-6 text-label-md text-secondary">NO WEBHOOKS CONFIGURED</p>
            <p className="mt-2 text-sm text-on-surface-variant dark:text-on-surface-variant">
              Add a webhook to receive HTTP callbacks on record events.
            </p>
            <button
              type="button"
              onClick={openCreateForm}
              className="mt-6 inline-flex items-center gap-2 border border-primary dark:border-on-primary bg-primary dark:bg-on-primary px-5 py-2.5 text-label-md text-on-primary dark:text-primary hover:bg-transparent hover:text-on-surface dark:hover:bg-transparent dark:hover:text-on-surface cursor-pointer"
            >
              Add Webhook
            </button>
          </div>
        ) : (
          /* Webhooks table */
          <div className="border border-primary dark:border-on-primary">
            <table className="w-full text-left text-sm" aria-label="Webhooks">
              <thead>
                <tr className="bg-primary dark:bg-on-primary">
                  <th scope="col" className="px-4 py-2.5 text-label-sm text-on-primary dark:text-primary">URL</th>
                  <th scope="col" className="px-4 py-2.5 text-label-sm text-on-primary dark:text-primary">Collection</th>
                  <th scope="col" className="px-4 py-2.5 text-label-sm text-on-primary dark:text-primary">Events</th>
                  <th scope="col" className="px-4 py-2.5 text-label-sm text-on-primary dark:text-primary">Status</th>
                  <th scope="col" className="px-4 py-2.5 text-label-sm text-on-primary dark:text-primary">Actions</th>
                </tr>
              </thead>
              <tbody>
                {webhooks.map((wh) => (
                  <tr key={wh.id} className="border-t border-outline-variant dark:border-outline hover:bg-surface-container-low dark:hover:bg-surface-container-low transition-colors-fast">
                    <td className="px-4 py-3">
                      <span className="font-data text-xs text-on-surface dark:text-on-surface break-all">{wh.url}</span>
                    </td>
                    <td className="px-4 py-3 text-on-surface-variant dark:text-on-surface-variant">{wh.collection}</td>
                    <td className="px-4 py-3">
                      <div className="flex flex-wrap gap-1">
                        {wh.events.map((evt) => (
                          <span
                            key={evt}
                            className="inline-flex items-center border border-primary dark:border-on-primary px-2 py-0.5 text-label-sm text-on-surface dark:text-on-surface"
                          >
                            {evt.toUpperCase()}
                          </span>
                        ))}
                      </div>
                    </td>
                    <td className="px-4 py-3">
                      <button
                        type="button"
                        onClick={() => handleToggleEnabled(wh)}
                        className={`inline-flex items-center gap-1.5 border px-2.5 py-0.5 text-label-sm cursor-pointer ${
                          wh.enabled
                            ? 'border-primary dark:border-on-primary bg-primary dark:bg-on-primary text-on-primary dark:text-primary'
                            : 'border-outline dark:border-outline bg-transparent text-on-surface-variant dark:text-on-surface-variant'
                        }`}
                        aria-label={wh.enabled ? 'Disable webhook' : 'Enable webhook'}
                      >
                        <span className={`inline-block h-1.5 w-1.5 ${
                          wh.enabled
                            ? 'bg-on-primary dark:bg-primary'
                            : 'border border-outline dark:border-outline'
                        }`} />
                        {wh.enabled ? 'ACTIVE' : 'INACTIVE'}
                      </button>
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex items-center gap-1">
                        {/* Test */}
                        <button
                          type="button"
                          onClick={() => handleTest(wh)}
                          disabled={testingId === wh.id}
                          className="border border-primary dark:border-on-primary px-2.5 py-1 text-label-sm text-on-surface dark:text-on-surface hover:bg-primary hover:text-on-primary dark:hover:bg-on-primary dark:hover:text-primary disabled:opacity-30 cursor-pointer"
                          aria-label="Test webhook"
                        >
                          {testingId === wh.id ? 'TESTING\u2026' : 'TEST'}
                        </button>

                        {/* Delivery history */}
                        <button
                          type="button"
                          onClick={() => setDeliveryWebhookId(wh.id)}
                          className="-ml-px border border-primary dark:border-on-primary px-2.5 py-1 text-label-sm text-on-surface dark:text-on-surface hover:bg-primary hover:text-on-primary dark:hover:bg-on-primary dark:hover:text-primary cursor-pointer"
                          aria-label="View delivery history"
                        >
                          History
                        </button>

                        {/* Edit */}
                        <button
                          type="button"
                          onClick={() => openEditForm(wh)}
                          className="-ml-px border border-primary dark:border-on-primary px-2.5 py-1 text-label-sm text-on-surface dark:text-on-surface hover:bg-primary hover:text-on-primary dark:hover:bg-on-primary dark:hover:text-primary cursor-pointer"
                          aria-label="Edit webhook"
                        >
                          Edit
                        </button>

                        {/* Delete */}
                        <button
                          type="button"
                          onClick={() => setDeletingWebhook(wh)}
                          className="-ml-px border border-error px-2.5 py-1 text-label-sm text-error hover:bg-error hover:text-on-error cursor-pointer"
                          aria-label="Delete webhook"
                        >
                          Delete
                        </button>
                      </div>

                      {/* Test result inline */}
                      {testResult?.id === wh.id && (
                        <div className={`mt-1 font-data text-xs ${testResult.success ? 'text-on-surface dark:text-on-surface' : 'text-error'}`} role="status">
                          {testResult.message}
                        </div>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* Modals */}
      {formOpen && (
        <WebhookFormModal
          webhook={editingWebhook}
          collections={collections}
          onSave={handleSave}
          onClose={closeForm}
          saving={formSaving}
          error={formError}
          fieldErrors={formFieldErrors}
        />
      )}

      {deletingWebhook && (
        <DeleteConfirmModal
          webhookUrl={deletingWebhook.url}
          onConfirm={handleDelete}
          onCancel={() => setDeletingWebhook(null)}
          deleting={deleteInProgress}
        />
      )}

      {deliveryWebhookId && (
        <DeliveryHistory
          webhookId={deliveryWebhookId}
          onClose={() => setDeliveryWebhookId(null)}
        />
      )}
    </DashboardLayout>
  );
}
