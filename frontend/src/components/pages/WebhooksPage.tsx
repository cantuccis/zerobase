import { useState, useEffect, useCallback } from 'react';
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
  { value: 'create', label: 'Create' },
  { value: 'update', label: 'Update' },
  { value: 'delete', label: 'Delete' },
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

function deliveryStatusBadgeClass(status: string): string {
  switch (status) {
    case 'success':
      return 'bg-green-100 dark:bg-green-900/20 text-green-800 dark:text-green-300';
    case 'failed':
      return 'bg-red-100 dark:bg-red-900/20 text-red-800 dark:text-red-300';
    case 'pending':
      return 'bg-yellow-100 dark:bg-yellow-900/30 text-yellow-800 dark:text-yellow-300';
    default:
      return 'bg-gray-100 dark:bg-gray-700 text-gray-800 dark:text-gray-200';
  }
}

function eventBadgeClass(event: string): string {
  switch (event) {
    case 'create':
      return 'bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400';
    case 'update':
      return 'bg-amber-50 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400';
    case 'delete':
      return 'bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400';
    default:
      return 'bg-gray-50 dark:bg-gray-900 text-gray-700 dark:text-gray-300';
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
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" role="dialog" aria-modal="true" aria-label={isEdit ? 'Edit webhook' : 'Add webhook'}>
      <div className="w-full max-w-lg rounded-lg bg-white p-6 shadow-xl dark:bg-gray-800">
        <h2 className="mb-4 text-lg font-semibold text-gray-900 dark:text-gray-100">
          {isEdit ? 'Edit Webhook' : 'Add Webhook'}
        </h2>

        {error && (
          <div className="mb-4 rounded-md bg-red-50 px-4 py-3 text-sm text-red-700 dark:bg-red-900/30 dark:text-red-300" role="alert">
            {error}
          </div>
        )}

        <form onSubmit={handleSubmit} className="space-y-4">
          {/* Collection (only on create) */}
          {!isEdit && (
            <div>
              <label htmlFor="webhook-collection" className="mb-1 block text-sm font-medium text-gray-700 dark:text-gray-300">
                Collection
              </label>
              <select
                id="webhook-collection"
                value={collection}
                onChange={(e) => setCollection(e.target.value)}
                className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-100"
              >
                {collections.map((c) => (
                  <option key={c.id} value={c.name}>{c.name}</option>
                ))}
              </select>
              {fieldErrors.collection && <p className="mt-1 text-sm text-red-600 dark:text-red-400">{fieldErrors.collection}</p>}
            </div>
          )}

          {/* URL */}
          <div>
            <label htmlFor="webhook-url" className="mb-1 block text-sm font-medium text-gray-700 dark:text-gray-300">
              URL
            </label>
            <input
              id="webhook-url"
              type="url"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              required
              placeholder="https://example.com/webhook"
              className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-100"
            />
            {fieldErrors.url && <p className="mt-1 text-sm text-red-600 dark:text-red-400">{fieldErrors.url}</p>}
          </div>

          {/* Events */}
          <fieldset>
            <legend className="mb-2 block text-sm font-medium text-gray-700 dark:text-gray-300">Events</legend>
            <div className="flex gap-4">
              {WEBHOOK_EVENTS.map(({ value, label }) => (
                <label key={value} className="flex items-center gap-2 text-sm text-gray-700 dark:text-gray-300 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={events.includes(value)}
                    onChange={() => toggleEvent(value)}
                    className="h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-2 focus:ring-blue-500"
                  />
                  {label}
                </label>
              ))}
            </div>
            {fieldErrors.events && <p className="mt-1 text-sm text-red-600 dark:text-red-400">{fieldErrors.events}</p>}
          </fieldset>

          {/* Secret */}
          <div>
            <label htmlFor="webhook-secret" className="mb-1 block text-sm font-medium text-gray-700 dark:text-gray-300">
              Secret <span className="text-gray-400">(optional)</span>
            </label>
            <input
              id="webhook-secret"
              type="password"
              value={secret}
              onChange={(e) => setSecret(e.target.value)}
              placeholder={isEdit ? 'Leave empty to keep current' : 'HMAC-SHA256 signing secret'}
              autoComplete="off"
              className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-100"
            />
          </div>

          {/* Enabled */}
          <label className="flex items-center gap-2 text-sm text-gray-700 dark:text-gray-300 cursor-pointer">
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) => setEnabled(e.target.checked)}
              className="h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-2 focus:ring-blue-500"
            />
            Enabled
          </label>

          {/* Actions */}
          <div className="flex justify-end gap-3 pt-2">
            <button
              type="button"
              onClick={onClose}
              className="rounded-md border border-gray-300 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={saving || events.length === 0}
              className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
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
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" role="dialog" aria-modal="true" aria-label="Delivery history">
      <div className="w-full max-w-3xl rounded-lg bg-white p-6 shadow-xl dark:bg-gray-800" style={{ maxHeight: '80vh', overflow: 'auto' }}>
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">Delivery History</h2>
          <button
            type="button"
            onClick={onClose}
            className="rounded-md p-1.5 text-gray-500 hover:bg-gray-100 dark:text-gray-400 dark:hover:bg-gray-700"
            aria-label="Close delivery history"
          >
            <svg className="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        {error && (
          <div className="mb-4 rounded-md bg-red-50 px-4 py-3 text-sm text-red-700 dark:bg-red-900/30 dark:text-red-300" role="alert">
            {error}
          </div>
        )}

        {loading ? (
          <div className="flex justify-center py-12">
            <div className="h-8 w-8 animate-spin rounded-full border-4 border-blue-600 border-t-transparent" role="status" aria-label="Loading deliveries" />
          </div>
        ) : deliveries.length === 0 ? (
          <p className="py-8 text-center text-sm text-gray-500 dark:text-gray-400">No delivery history yet.</p>
        ) : (
          <>
            <table className="w-full text-left text-sm" aria-label="Webhook delivery log">
              <thead>
                <tr className="border-b border-gray-200 dark:border-gray-700">
                  <th className="px-3 py-2 font-medium text-gray-600 dark:text-gray-400">Event</th>
                  <th className="px-3 py-2 font-medium text-gray-600 dark:text-gray-400">Record</th>
                  <th className="px-3 py-2 font-medium text-gray-600 dark:text-gray-400">Status</th>
                  <th className="px-3 py-2 font-medium text-gray-600 dark:text-gray-400">HTTP</th>
                  <th className="px-3 py-2 font-medium text-gray-600 dark:text-gray-400">Attempt</th>
                  <th className="px-3 py-2 font-medium text-gray-600 dark:text-gray-400">Date</th>
                </tr>
              </thead>
              <tbody>
                {deliveries.map((d) => (
                  <tr key={d.id} className="border-b border-gray-100 hover:bg-gray-50 dark:border-gray-800 dark:hover:bg-gray-700/50">
                    <td className="px-3 py-2">
                      <span className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${eventBadgeClass(d.event)}`}>{d.event}</span>
                    </td>
                    <td className="px-3 py-2 font-mono text-xs text-gray-600 dark:text-gray-400">{d.recordId}</td>
                    <td className="px-3 py-2">
                      <span className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${deliveryStatusBadgeClass(d.status)}`}>{d.status}</span>
                    </td>
                    <td className="px-3 py-2 text-gray-600 dark:text-gray-400">{d.responseStatus || '\u2014'}</td>
                    <td className="px-3 py-2 text-gray-600 dark:text-gray-400">{d.attempt}</td>
                    <td className="px-3 py-2 text-gray-500 dark:text-gray-400 whitespace-nowrap">{formatTimestamp(d.created)}</td>
                  </tr>
                ))}
              </tbody>
            </table>

            {/* Pagination */}
            {totalPages > 1 && (
              <div className="mt-4 flex items-center justify-between">
                <button
                  type="button"
                  onClick={() => setPage((p) => Math.max(1, p - 1))}
                  disabled={page <= 1}
                  className="rounded-md border border-gray-300 px-3 py-1.5 text-sm disabled:opacity-50 dark:border-gray-600 dark:text-gray-300"
                >
                  Previous
                </button>
                <span className="text-sm text-gray-500 dark:text-gray-400">
                  Page {page} of {totalPages}
                </span>
                <button
                  type="button"
                  onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
                  disabled={page >= totalPages}
                  className="rounded-md border border-gray-300 px-3 py-1.5 text-sm disabled:opacity-50 dark:border-gray-600 dark:text-gray-300"
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
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" role="dialog" aria-modal="true" aria-label="Confirm delete webhook">
      <div className="w-full max-w-sm rounded-lg bg-white p-6 shadow-xl dark:bg-gray-800">
        <h2 className="mb-2 text-lg font-semibold text-gray-900 dark:text-gray-100">Delete Webhook</h2>
        <p className="mb-4 text-sm text-gray-600 dark:text-gray-400">
          Are you sure you want to delete the webhook for <span className="font-medium text-gray-900 dark:text-gray-100">{webhookUrl}</span>? This action cannot be undone.
        </p>
        <div className="flex justify-end gap-3">
          <button
            type="button"
            onClick={onCancel}
            className="rounded-md border border-gray-300 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={deleting}
            className="rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700 disabled:opacity-50"
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
      <div className="space-y-6">
        {/* Header */}
        <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Webhooks</h1>
            <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
              Configure HTTP callbacks triggered by record events.
            </p>
          </div>
          <button
            type="button"
            onClick={openCreateForm}
            className="inline-flex items-center gap-2 rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700"
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
          <label htmlFor="filter-collection" className="text-sm font-medium text-gray-700 dark:text-gray-300">
            Collection:
          </label>
          <select
            id="filter-collection"
            value={filterCollection}
            onChange={(e) => setFilterCollection(e.target.value)}
            className="rounded-md border border-gray-300 bg-white px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-100"
          >
            <option value="">All collections</option>
            {collections.map((c) => (
              <option key={c.id} value={c.name}>{c.name}</option>
            ))}
          </select>
        </div>

        {/* Error */}
        {error && (
          <div className="rounded-md bg-red-50 px-4 py-3 text-sm text-red-700 dark:bg-red-900/30 dark:text-red-300" role="alert">
            {error}
            <button type="button" onClick={() => setError(null)} className="ml-2 font-medium underline" aria-label="Dismiss error">
              Dismiss
            </button>
          </div>
        )}

        {/* Loading */}
        {loading ? (
          <div className="flex justify-center py-16">
            <div className="h-8 w-8 animate-spin rounded-full border-4 border-blue-600 border-t-transparent" role="status" aria-label="Loading webhooks" />
          </div>
        ) : webhooks.length === 0 ? (
          /* Empty state */
          <div className="rounded-lg border-2 border-dashed border-gray-300 p-12 text-center dark:border-gray-600">
            <svg className="mx-auto h-12 w-12 text-gray-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M18 16.98h-5.99c-1.1 0-1.95.94-2.48 1.9A4 4 0 0 1 2 17c.01-.7.2-1.4.57-2" />
              <path d="m6 17 3.13-5.78c.53-.97.1-2.18-.5-3.1a4 4 0 1 1 6.89-4.06" />
              <path d="m12 6 3.13 5.73C15.66 12.7 16.9 13 18 13a4 4 0 0 1 0 8H12" />
            </svg>
            <h3 className="mt-4 text-sm font-medium text-gray-900 dark:text-gray-100">No webhooks configured</h3>
            <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
              Get started by adding a webhook to receive HTTP callbacks on record events.
            </p>
            <button
              type="button"
              onClick={openCreateForm}
              className="mt-4 inline-flex items-center gap-2 rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700"
            >
              Add Webhook
            </button>
          </div>
        ) : (
          /* Webhooks table */
          <div className="overflow-x-auto rounded-lg border border-gray-200 dark:border-gray-700">
            <table className="w-full text-left text-sm" aria-label="Webhooks">
              <thead className="bg-gray-50 dark:bg-gray-800">
                <tr>
                  <th className="px-4 py-3 font-medium text-gray-600 dark:text-gray-400">URL</th>
                  <th className="px-4 py-3 font-medium text-gray-600 dark:text-gray-400">Collection</th>
                  <th className="px-4 py-3 font-medium text-gray-600 dark:text-gray-400">Events</th>
                  <th className="px-4 py-3 font-medium text-gray-600 dark:text-gray-400">Status</th>
                  <th className="px-4 py-3 font-medium text-gray-600 dark:text-gray-400">Actions</th>
                </tr>
              </thead>
              <tbody>
                {webhooks.map((wh) => (
                  <tr key={wh.id} className="border-t border-gray-100 hover:bg-gray-50 dark:border-gray-800 dark:hover:bg-gray-700/50">
                    <td className="px-4 py-3">
                      <span className="font-mono text-xs text-gray-900 dark:text-gray-100 break-all">{wh.url}</span>
                    </td>
                    <td className="px-4 py-3 text-gray-600 dark:text-gray-400">{wh.collection}</td>
                    <td className="px-4 py-3">
                      <div className="flex flex-wrap gap-1">
                        {wh.events.map((evt) => (
                          <span key={evt} className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${eventBadgeClass(evt)}`}>
                            {evt}
                          </span>
                        ))}
                      </div>
                    </td>
                    <td className="px-4 py-3">
                      <button
                        type="button"
                        onClick={() => handleToggleEnabled(wh)}
                        className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium cursor-pointer ${
                          wh.enabled
                            ? 'bg-green-100 text-green-800 dark:bg-green-900/20 dark:text-green-300'
                            : 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-400'
                        }`}
                        aria-label={wh.enabled ? 'Disable webhook' : 'Enable webhook'}
                      >
                        <span className={`inline-block h-1.5 w-1.5 rounded-full ${wh.enabled ? 'bg-green-500' : 'bg-gray-400'}`} />
                        {wh.enabled ? 'Active' : 'Disabled'}
                      </button>
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex items-center gap-2">
                        {/* Test */}
                        <button
                          type="button"
                          onClick={() => handleTest(wh)}
                          disabled={testingId === wh.id}
                          className="rounded-md border border-gray-300 px-2.5 py-1 text-xs font-medium text-gray-700 hover:bg-gray-50 disabled:opacity-50 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
                          aria-label="Test webhook"
                        >
                          {testingId === wh.id ? 'Testing\u2026' : 'Test'}
                        </button>

                        {/* Delivery history */}
                        <button
                          type="button"
                          onClick={() => setDeliveryWebhookId(wh.id)}
                          className="rounded-md border border-gray-300 px-2.5 py-1 text-xs font-medium text-gray-700 hover:bg-gray-50 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
                          aria-label="View delivery history"
                        >
                          History
                        </button>

                        {/* Edit */}
                        <button
                          type="button"
                          onClick={() => openEditForm(wh)}
                          className="rounded-md border border-gray-300 px-2.5 py-1 text-xs font-medium text-gray-700 hover:bg-gray-50 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
                          aria-label="Edit webhook"
                        >
                          Edit
                        </button>

                        {/* Delete */}
                        <button
                          type="button"
                          onClick={() => setDeletingWebhook(wh)}
                          className="rounded-md border border-red-300 px-2.5 py-1 text-xs font-medium text-red-700 hover:bg-red-50 dark:border-red-700 dark:text-red-400 dark:hover:bg-red-900/30"
                          aria-label="Delete webhook"
                        >
                          Delete
                        </button>
                      </div>

                      {/* Test result inline */}
                      {testResult?.id === wh.id && (
                        <div className={`mt-1 text-xs ${testResult.success ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'}`} role="status">
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
