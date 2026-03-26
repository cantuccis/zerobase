/**
 * Modal dialog for creating and editing records.
 *
 * Dynamically renders form fields based on the collection schema.
 * Supports create (empty form) and edit (pre-populated) modes.
 */

import { useState, useCallback, useEffect, useRef } from 'react';
import type { Collection, BaseRecord, Field } from '../../lib/api/types';
import { FieldInput } from './field-inputs';
import type { RelationOption } from './field-inputs';
import { validateRecord, type ValidationErrors } from './validate-record';

// ── Types ────────────────────────────────────────────────────────────────────

export interface RecordFormModalProps {
  /** The collection this record belongs to. */
  collection: Collection;
  /** Existing record for edit mode. Null for create mode. */
  record: BaseRecord | null;
  /** Close the modal. */
  onClose: () => void;
  /** Called after successful save with the saved record. */
  onSave: (record: BaseRecord) => void;
  /** Perform the create/update API call. */
  onSubmit: (data: Record<string, unknown> | FormData) => Promise<BaseRecord>;
  /** Available collections for relation pickers. */
  collections?: Collection[];
  /** Search callback for relation pickers. */
  onSearchRelation?: (collectionId: string, query: string) => Promise<RelationOption[]>;
}

// ── Constants ────────────────────────────────────────────────────────────────

const SYSTEM_FIELD_NAMES = new Set(['id', 'created', 'updated', 'collectionId', 'collectionName']);
const AUTH_SYSTEM_FIELD_NAMES = new Set(['emailVisibility', 'verified', 'tokenKey']);

// ── Helpers ──────────────────────────────────────────────────────────────────

/** Get fields that are editable by the user. */
function getEditableFields(collection: Collection): Field[] {
  return collection.fields.filter((f) => {
    if (f.type.type === 'autoDate') return false;
    return true;
  });
}

/** Build initial form values from a record or empty defaults. */
function buildInitialValues(
  fields: Field[],
  record: BaseRecord | null,
): Record<string, unknown> {
  const values: Record<string, unknown> = {};

  for (const field of fields) {
    if (field.type.type === 'autoDate') continue;

    if (record && field.name in record) {
      values[field.name] = record[field.name];
    } else {
      // Default values by type
      switch (field.type.type) {
        case 'bool':
          values[field.name] = false;
          break;
        case 'number':
          values[field.name] = null;
          break;
        case 'multiSelect':
          values[field.name] = [];
          break;
        case 'json':
          values[field.name] = null;
          break;
        default:
          values[field.name] = '';
      }
    }
  }

  return values;
}

/** Check if any field uses file uploads. */
function hasFileFields(fields: Field[]): boolean {
  return fields.some((f) => f.type.type === 'file');
}

/** Build FormData or JSON payload from form values. */
function buildPayload(
  fields: Field[],
  values: Record<string, unknown>,
): Record<string, unknown> | FormData {
  const useFormData = hasFileFields(fields) && hasNewFileValues(values, fields);

  if (useFormData) {
    const fd = new FormData();
    for (const field of fields) {
      if (field.type.type === 'autoDate') continue;
      const val = values[field.name];

      if (field.type.type === 'file') {
        if (val instanceof File) {
          fd.append(field.name, val);
        } else if (Array.isArray(val)) {
          for (const item of val) {
            if (item instanceof File) {
              fd.append(field.name, item);
            }
          }
        }
        // If val is a string (existing filename), don't re-upload
      } else if (val !== undefined) {
        fd.append(
          field.name,
          typeof val === 'object' && val !== null ? JSON.stringify(val) : String(val ?? ''),
        );
      }
    }
    return fd;
  }

  // JSON payload
  const data: Record<string, unknown> = {};
  for (const field of fields) {
    if (field.type.type === 'autoDate') continue;
    const val = values[field.name];
    if (val !== undefined) {
      data[field.name] = val;
    }
  }
  return data;
}

function hasNewFileValues(values: Record<string, unknown>, fields: Field[]): boolean {
  for (const field of fields) {
    if (field.type.type !== 'file') continue;
    const val = values[field.name];
    if (val instanceof File) return true;
    if (Array.isArray(val) && val.some((v) => v instanceof File)) return true;
  }
  return false;
}

// ── Component ────────────────────────────────────────────────────────────────

export function RecordFormModal({
  collection,
  record,
  onClose,
  onSave,
  onSubmit,
  collections,
  onSearchRelation,
}: RecordFormModalProps) {
  const isEdit = record !== null;
  const editableFields = getEditableFields(collection);

  const [values, setValues] = useState<Record<string, unknown>>(() =>
    buildInitialValues(editableFields, record),
  );
  const [errors, setErrors] = useState<ValidationErrors>({});
  const [serverError, setServerError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const modalRef = useRef<HTMLDivElement>(null);

  // Close on Escape
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [onClose]);

  // Focus trap: focus the modal on mount
  useEffect(() => {
    modalRef.current?.focus();
  }, []);

  const handleFieldChange = useCallback((name: string, value: unknown) => {
    setValues((prev) => ({ ...prev, [name]: value }));
    // Clear field error on change
    setErrors((prev) => {
      if (prev[name]) {
        const next = { ...prev };
        delete next[name];
        return next;
      }
      return prev;
    });
  }, []);

  const handleSubmit = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();

      // Validate
      const validationErrors = validateRecord(editableFields, values);
      if (Object.keys(validationErrors).length > 0) {
        setErrors(validationErrors);
        return;
      }

      setSubmitting(true);
      setServerError(null);

      try {
        const payload = buildPayload(editableFields, values);
        const savedRecord = await onSubmit(payload);
        onSave(savedRecord);
      } catch (err: unknown) {
        if (err && typeof err === 'object' && 'response' in err) {
          const apiErr = err as { response: { message: string; data?: Record<string, { message: string }> } };
          setServerError(apiErr.response.message);
          // Map server field errors
          if (apiErr.response.data) {
            const fieldErrors: ValidationErrors = {};
            for (const [key, val] of Object.entries(apiErr.response.data)) {
              fieldErrors[key] = val.message;
            }
            setErrors(fieldErrors);
          }
        } else {
          setServerError(err instanceof Error ? err.message : 'An unexpected error occurred.');
        }
      } finally {
        setSubmitting(false);
      }
    },
    [editableFields, values, onSubmit, onSave],
  );

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-primary/40 dark:bg-on-primary/40 p-4 sm:items-center animate-fade-in"
      role="dialog"
      aria-modal="true"
      aria-labelledby="record-form-title"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={modalRef}
        tabIndex={-1}
        className="relative w-full max-w-2xl max-h-[90vh] overflow-y-auto border border-primary dark:border-on-primary bg-surface dark:bg-surface animate-slide-up"
        data-testid="record-form-modal"
      >
        {/* Header */}
        <div className="sticky top-0 z-10 flex items-center justify-between border-b border-primary dark:border-on-primary bg-primary dark:bg-on-primary px-6 py-4">
          <h3 id="record-form-title" className="text-lg font-semibold text-on-primary dark:text-primary">
            {isEdit ? 'Edit Record' : 'New Record'}
            <span className="ml-2 text-sm font-normal text-on-primary/70 dark:text-primary/70">
              {collection.name}
            </span>
          </h3>
          <button
            type="button"
            onClick={onClose}
            className="p-1.5 text-on-primary dark:text-primary hover:text-on-primary/70 dark:hover:text-primary/70"
            aria-label="Close form"
          >
            <svg className="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        {/* Form */}
        <form onSubmit={handleSubmit} data-testid="record-form">
          <div className="space-y-5 px-6 py-5">
            {/* Server error */}
            {serverError && (
              <div
                role="alert"
                className="border border-error dark:border-error px-4 py-3 text-sm text-error dark:text-error"
                data-testid="server-error"
              >
                {serverError}
              </div>
            )}

            {/* Record ID (read-only in edit mode) */}
            {isEdit && record && (
              <div className="space-y-1.5">
                <label className="text-label-sm font-bold uppercase tracking-[0.05em] text-on-surface-variant dark:text-on-surface-variant">Record ID</label>
                <p className="font-mono text-sm text-secondary dark:text-secondary" data-testid="record-id-display">
                  {record.id}
                </p>
              </div>
            )}

            {/* Dynamic fields */}
            {editableFields.map((field) => (
              <FieldInput
                key={field.id}
                field={field}
                value={values[field.name]}
                onChange={handleFieldChange}
                error={errors[field.name]}
                collections={collections}
                onSearchRelation={onSearchRelation}
              />
            ))}

            {editableFields.length === 0 && (
              <p className="py-4 text-center text-sm text-secondary dark:text-secondary">
                No editable fields in this collection.
              </p>
            )}
          </div>

          {/* Footer */}
          <div className="sticky bottom-0 flex items-center justify-end gap-3 border-t border-primary dark:border-on-primary bg-surface-container-low dark:bg-surface-container-low px-6 py-4">
            <button
              type="button"
              onClick={onClose}
              className="border border-primary dark:border-on-primary bg-surface dark:bg-surface px-4 py-2 text-sm font-medium text-on-surface dark:text-on-surface hover:bg-surface-container-low dark:hover:bg-surface-container-low"
              disabled={submitting}
            >
              Cancel
            </button>
            <button
              type="submit"
              className="bg-primary dark:bg-on-primary px-4 py-2 text-sm font-medium text-on-primary dark:text-primary disabled:cursor-not-allowed disabled:opacity-60"
              disabled={submitting}
              data-testid="record-form-submit"
            >
              {submitting ? (isEdit ? 'Saving…' : 'Creating…') : isEdit ? 'Save Changes' : 'Create Record'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
