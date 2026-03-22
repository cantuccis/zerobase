import { useState, useEffect, useCallback } from 'react';
import { DashboardLayout } from '../DashboardLayout';
import { FieldEditor } from '../schema/FieldEditor';
import { ApiPreview } from '../schema/ApiPreview';
import { AuthFieldsDisplay } from '../schema/AuthFieldsDisplay';
import { AuthSettingsEditor, DEFAULT_AUTH_OPTIONS } from '../schema/AuthSettingsEditor';
import { defaultFieldType, generateFieldId } from '../schema/field-defaults';
import { client } from '../../lib/auth/client';
import { ApiError } from '../../lib/api';
import { RulesEditor } from '../schema/RulesEditor';
import type { ApiRules, AuthOptions, Collection, CollectionType, Field, FieldTypeName } from '../../lib/api/types';

// ── Types ───────────────────────────────────────────────────────────────────

export interface CollectionEditorPageProps {
  mode: 'create' | 'edit';
  collectionId?: string;
}

const DEFAULT_RULES: ApiRules = {
  listRule: null,
  viewRule: null,
  createRule: null,
  updateRule: null,
  deleteRule: null,
};

interface FormState {
  name: string;
  type: CollectionType;
  fields: Field[];
  rules: ApiRules;
  authOptions: AuthOptions;
}

interface ValidationErrors {
  name?: string;
  fields?: Record<string, string>;
  general?: string;
}

// ── Helpers ─────────────────────────────────────────────────────────────────

const COLLECTION_TYPE_OPTIONS: { value: CollectionType; label: string; description: string }[] = [
  { value: 'base', label: 'Base', description: 'Standard data collection with auto-generated CRUD API.' },
  { value: 'auth', label: 'Auth', description: 'Collection with built-in auth fields (email, password, verified).' },
  { value: 'view', label: 'View', description: 'Read-only collection backed by a SQL view query.' },
];

function createEmptyField(): Field {
  return {
    id: generateFieldId(),
    name: '',
    type: defaultFieldType('text'),
    required: false,
    unique: false,
    sortOrder: 0,
  };
}

function validateForm(form: FormState): ValidationErrors {
  const errors: ValidationErrors = {};

  // Collection name validation
  if (!form.name.trim()) {
    errors.name = 'Collection name is required.';
  } else if (!/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(form.name)) {
    errors.name = 'Name must start with a letter or underscore, and contain only letters, digits, and underscores.';
  }

  // Field validation
  const fieldErrors: Record<string, string> = {};
  const seenNames = new Set<string>();

  for (const field of form.fields) {
    if (!field.name.trim()) {
      fieldErrors[field.id] = 'Field name is required.';
    } else if (!/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(field.name)) {
      fieldErrors[field.id] = 'Invalid field name. Use letters, digits, and underscores.';
    } else if (seenNames.has(field.name.toLowerCase())) {
      fieldErrors[field.id] = 'Duplicate field name.';
    }
    seenNames.add(field.name.toLowerCase());
  }

  if (Object.keys(fieldErrors).length > 0) {
    errors.fields = fieldErrors;
  }

  return errors;
}

// ── Main component ──────────────────────────────────────────────────────────

export function CollectionEditorPage({ mode, collectionId }: CollectionEditorPageProps) {
  const [form, setForm] = useState<FormState>({
    name: '',
    type: 'base',
    fields: [],
    rules: { ...DEFAULT_RULES },
    authOptions: { ...DEFAULT_AUTH_OPTIONS },
  });
  const [loading, setLoading] = useState(mode === 'edit');
  const [saving, setSaving] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveSuccess, setSaveSuccess] = useState(false);
  const [validationErrors, setValidationErrors] = useState<ValidationErrors>({});
  const [allCollections, setAllCollections] = useState<Collection[]>([]);

  // Fetch existing collection for edit mode
  useEffect(() => {
    if (mode === 'edit' && collectionId) {
      (async () => {
        try {
          const collection = await client.getCollection(collectionId);
          setForm({
            name: collection.name,
            type: collection.type,
            fields: collection.fields,
            rules: collection.rules ?? { ...DEFAULT_RULES },
            authOptions: collection.authOptions ?? { ...DEFAULT_AUTH_OPTIONS },
          });
          setLoading(false);
        } catch (err) {
          const message = err instanceof ApiError ? err.message : 'Failed to load collection.';
          setLoadError(message);
          setLoading(false);
        }
      })();
    }
  }, [mode, collectionId]);

  // Fetch all collections (for relation field dropdown)
  useEffect(() => {
    (async () => {
      try {
        const resp = await client.listCollections();
        setAllCollections(resp.items);
      } catch {
        // Non-critical: relation dropdown will be empty
      }
    })();
  }, []);

  // ── Field operations ────────────────────────────────────────────────────

  const addField = useCallback(() => {
    setForm((prev) => ({
      ...prev,
      fields: [...prev.fields, createEmptyField()],
    }));
  }, []);

  const updateField = useCallback((fieldId: string, updated: Field) => {
    setForm((prev) => ({
      ...prev,
      fields: prev.fields.map((f) => (f.id === fieldId ? updated : f)),
    }));
    // Clear field-specific validation error on edit
    setValidationErrors((prev) => {
      if (!prev.fields?.[fieldId]) return prev;
      const { [fieldId]: _, ...rest } = prev.fields;
      return { ...prev, fields: Object.keys(rest).length > 0 ? rest : undefined };
    });
  }, []);

  const removeField = useCallback((fieldId: string) => {
    setForm((prev) => ({
      ...prev,
      fields: prev.fields.filter((f) => f.id !== fieldId),
    }));
  }, []);

  const moveField = useCallback((index: number, direction: -1 | 1) => {
    setForm((prev) => {
      const fields = [...prev.fields];
      const target = index + direction;
      if (target < 0 || target >= fields.length) return prev;
      [fields[index], fields[target]] = [fields[target], fields[index]];
      return { ...prev, fields };
    });
  }, []);

  // ── Save ────────────────────────────────────────────────────────────────

  const handleSave = useCallback(async () => {
    setSaveError(null);
    setSaveSuccess(false);

    const errors = validateForm(form);
    setValidationErrors(errors);

    if (errors.name || errors.fields) {
      return;
    }

    setSaving(true);

    // Assign sort order based on position
    const fieldsWithOrder = form.fields.map((f, i) => ({ ...f, sortOrder: i }));

    try {
      const payload: Partial<Collection> & { name: string; type: string } = {
        name: form.name,
        type: form.type,
        fields: fieldsWithOrder,
        rules: form.rules,
      };

      if (form.type === 'auth') {
        payload.authOptions = form.authOptions;
      }

      if (mode === 'create') {
        const created = await client.createCollection(payload);
        // Redirect to edit page for the newly created collection
        window.location.href = `/_/collections/${encodeURIComponent(created.id)}/edit`;
      } else if (collectionId) {
        await client.updateCollection(collectionId, payload);
        setSaveSuccess(true);
        setTimeout(() => setSaveSuccess(false), 3000);
      }
    } catch (err) {
      if (err instanceof ApiError) {
        setSaveError(err.message);
        // Map server-side field errors
        if (err.isValidation && err.response.data) {
          const serverFieldErrors: Record<string, string> = {};
          for (const [key, value] of Object.entries(err.response.data)) {
            if (key === 'name') {
              setValidationErrors((prev) => ({ ...prev, name: value.message }));
            } else {
              serverFieldErrors[key] = value.message;
            }
          }
          if (Object.keys(serverFieldErrors).length > 0) {
            setValidationErrors((prev) => ({
              ...prev,
              fields: { ...prev.fields, ...serverFieldErrors },
            }));
          }
        }
      } else {
        setSaveError('An unexpected error occurred. Please try again.');
      }
    } finally {
      setSaving(false);
    }
  }, [form, mode, collectionId]);

  // ── Render ──────────────────────────────────────────────────────────────

  const pageTitle = mode === 'create' ? 'New Collection' : `Edit Collection`;

  if (loading) {
    return (
      <DashboardLayout currentPath="/_/collections" pageTitle={pageTitle}>
        <div className="space-y-4" data-testid="editor-loading">
          <div className="animate-pulse rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-6">
            <div className="h-6 w-48 rounded bg-gray-200 dark:bg-gray-600" />
            <div className="mt-4 h-10 w-full rounded bg-gray-200 dark:bg-gray-600" />
            <div className="mt-4 h-10 w-full rounded bg-gray-200 dark:bg-gray-600" />
          </div>
        </div>
      </DashboardLayout>
    );
  }

  if (loadError) {
    return (
      <DashboardLayout currentPath="/_/collections" pageTitle={pageTitle}>
        <div role="alert" className="rounded-md bg-red-50 dark:bg-red-900/30 p-4">
          <p className="text-sm text-red-700 dark:text-red-400">{loadError}</p>
          <a
            href="/_/collections"
            className="mt-2 inline-block text-sm font-medium text-red-700 dark:text-red-400 underline hover:text-red-800 dark:hover:text-red-300"
          >
            Back to collections
          </a>
        </div>
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout currentPath="/_/collections" pageTitle={pageTitle}>
      <div className="mx-auto max-w-4xl space-y-6">
        {/* Save error */}
        {saveError && (
          <div role="alert" className="rounded-md bg-red-50 dark:bg-red-900/30 p-4">
            <p className="text-sm text-red-700 dark:text-red-400">{saveError}</p>
          </div>
        )}

        {/* Save success */}
        {saveSuccess && (
          <div role="status" className="rounded-md bg-green-50 dark:bg-green-900/30 p-4">
            <p className="text-sm text-green-700 dark:text-green-400">Collection saved successfully.</p>
          </div>
        )}

        {/* ── Collection basics ────────────────────────────────────────── */}
        <section className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-6">
          <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100">Collection Details</h3>

          {/* Name */}
          <div className="mt-4">
            <label htmlFor="collection-name" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
              Name
            </label>
            <input
              id="collection-name"
              type="text"
              value={form.name}
              onChange={(e) => {
                setForm((prev) => ({ ...prev, name: e.target.value }));
                if (validationErrors.name) {
                  setValidationErrors((prev) => ({ ...prev, name: undefined }));
                }
              }}
              placeholder="e.g. posts, users, comments"
              className={`mt-1 w-full rounded-md border px-3 py-2 text-sm text-gray-900 dark:text-gray-100 bg-white dark:bg-gray-800 placeholder-gray-400 dark:placeholder-gray-500 focus-visible:outline-none focus-visible:ring-1 ${
                validationErrors.name
                  ? 'border-red-500 focus:border-red-500 focus:ring-red-500'
                  : 'border-gray-300 dark:border-gray-600 focus:border-blue-500 focus:ring-blue-500'
              }`}
              aria-invalid={validationErrors.name ? 'true' : undefined}
              aria-describedby={validationErrors.name ? 'collection-name-error' : undefined}
              data-testid="collection-name"
            />
            {validationErrors.name && (
              <p id="collection-name-error" className="mt-1 text-sm text-red-600 dark:text-red-400" role="alert">
                {validationErrors.name}
              </p>
            )}
          </div>

          {/* Type */}
          <fieldset className="mt-4">
            <legend className="block text-sm font-medium text-gray-700 dark:text-gray-300">Type</legend>
            <div className="mt-2 grid grid-cols-1 gap-3 sm:grid-cols-3">
              {COLLECTION_TYPE_OPTIONS.map((opt) => (
                <label
                  key={opt.value}
                  className={`cursor-pointer rounded-lg border-2 p-3 transition-colors ${
                    form.type === opt.value
                      ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/30'
                      : 'border-gray-200 dark:border-gray-700 hover:border-gray-300 dark:hover:border-gray-600'
                  }`}
                  data-testid={`type-${opt.value}`}
                >
                  <input
                    type="radio"
                    name="collection-type"
                    value={opt.value}
                    checked={form.type === opt.value}
                    onChange={() => setForm((prev) => ({ ...prev, type: opt.value }))}
                    className="sr-only"
                  />
                  <span className="block text-sm font-semibold text-gray-900 dark:text-gray-100">{opt.label}</span>
                  <span className="mt-0.5 block text-xs text-gray-500 dark:text-gray-400">{opt.description}</span>
                </label>
              ))}
            </div>
          </fieldset>
        </section>

        {/* ── Auth System Fields (read-only) ────────────────────────── */}
        <AuthFieldsDisplay collectionType={form.type} />

        {/* ── Fields ──────────────────────────────────────────────────── */}
        <section>
          <div className="mb-3 flex items-center justify-between">
            <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100">{form.type === 'auth' ? 'Additional Fields' : 'Fields'}</h3>
            <button
              type="button"
              onClick={addField}
              className="inline-flex items-center gap-1.5 rounded-md bg-blue-600 px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-blue-700 dark:hover:bg-blue-600"
              data-testid="add-field"
            >
              <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <line x1="12" y1="5" x2="12" y2="19" />
                <line x1="5" y1="12" x2="19" y2="12" />
              </svg>
              Add Field
            </button>
          </div>

          {form.fields.length === 0 ? (
            <div className="rounded-lg border-2 border-dashed border-gray-300 dark:border-gray-600 py-8 text-center">
              <p className="text-sm text-gray-500 dark:text-gray-400">No fields yet. Click "Add Field" to start.</p>
            </div>
          ) : (
            <div className="space-y-3">
              {form.fields.map((field, index) => (
                <FieldEditor
                  key={field.id}
                  field={field}
                  index={index}
                  totalFields={form.fields.length}
                  onChange={(updated) => updateField(field.id, updated)}
                  onRemove={() => removeField(field.id)}
                  onMoveUp={() => moveField(index, -1)}
                  onMoveDown={() => moveField(index, 1)}
                  collections={allCollections}
                  nameError={validationErrors.fields?.[field.id]}
                />
              ))}
            </div>
          )}
        </section>

        {/* ── Auth Settings ─────────────────────────────────────────── */}
        {form.type === 'auth' && (
          <AuthSettingsEditor
            authOptions={form.authOptions}
            onChange={(authOptions) => setForm((prev) => ({ ...prev, authOptions }))}
          />
        )}

        {/* ── API Rules ──────────────────────────────────────────────── */}
        <RulesEditor
          rules={form.rules}
          onChange={(rules) => setForm((prev) => ({ ...prev, rules }))}
          collectionType={form.type}
        />

        {/* ── API Preview ─────────────────────────────────────────────── */}
        <section>
          <ApiPreview collectionName={form.name} collectionType={form.type} />
        </section>

        {/* ── Actions ─────────────────────────────────────────────────── */}
        <div className="flex items-center justify-between border-t border-gray-200 dark:border-gray-700 pt-6">
          <a
            href="/_/collections"
            className="rounded-md border border-gray-300 dark:border-gray-600 px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 transition-colors hover:bg-gray-50 dark:hover:bg-gray-700"
          >
            Cancel
          </a>
          <button
            type="button"
            onClick={handleSave}
            disabled={saving}
            className="rounded-md bg-blue-600 px-6 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-700 dark:hover:bg-blue-600 disabled:opacity-50"
            data-testid="save-collection"
          >
            {saving ? 'Saving...' : mode === 'create' ? 'Create Collection' : 'Save Changes'}
          </button>
        </div>
      </div>
    </DashboardLayout>
  );
}
