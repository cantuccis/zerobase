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

// ── Section Header ──────────────────────────────────────────────────────────

function SectionHeader({ number, title, description }: { number: string; title: string; description?: string }) {
  return (
    <div className="col-span-12 border-b border-primary pb-4 mb-6">
      <div className="grid grid-cols-12 gap-6">
        <div className="col-span-12 md:col-span-4">
          <span className="text-label-md text-secondary">{number}</span>
          <h3 className="text-title-md text-on-surface mt-1">{title}</h3>
          {description && (
            <p className="text-sm text-secondary mt-1">{description}</p>
          )}
        </div>
      </div>
    </div>
  );
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
        <div className="space-y-6" data-testid="editor-loading" role="status" aria-label="Loading collection">
          <div className="border border-primary bg-surface p-6">
            <div className="h-6 w-48 bg-surface-container-high" />
            <div className="mt-4 h-10 w-full bg-surface-container-high" />
            <div className="mt-4 h-10 w-full bg-surface-container-high" />
          </div>
        </div>
      </DashboardLayout>
    );
  }

  if (loadError) {
    return (
      <DashboardLayout currentPath="/_/collections" pageTitle={pageTitle}>
        <div role="alert" className="border border-error bg-error-container p-4">
          <p className="text-sm text-on-error-container">{loadError}</p>
          <a
            href="/_/collections"
            className="mt-2 inline-block text-sm font-semibold text-on-error-container underline"
          >
            Back to collections
          </a>
        </div>
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout currentPath="/_/collections" pageTitle={pageTitle}>
      <div className="mx-auto max-w-5xl space-y-0">
        {/* Save error */}
        {saveError && (
          <div role="alert" className="border border-error bg-error-container p-4 mb-6">
            <p className="text-sm text-on-error-container">{saveError}</p>
          </div>
        )}

        {/* Save success */}
        {saveSuccess && (
          <div role="status" className="border border-primary bg-surface-container-low p-4 mb-6">
            <p className="text-sm text-on-surface">Collection saved successfully.</p>
          </div>
        )}

        {/* ── 01. General ────────────────────────────────────────── */}
        <section className="py-8">
          <SectionHeader number="01" title="General" description="Basic collection configuration." />
          <div className="grid grid-cols-12 gap-6">
            <div className="col-span-12 md:col-span-4">
              <p className="text-sm text-secondary">Define the collection name and type. The name is used for API endpoints.</p>
            </div>
            <div className="col-span-12 md:col-span-8 space-y-5">
              {/* Name */}
              <div>
                <label htmlFor="collection-name" className="text-label-md text-on-surface-variant block mb-1.5">
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
                  className={`w-full border px-3 py-2 text-sm text-on-surface bg-background placeholder-outline focus:outline-none ${
                    validationErrors.name
                      ? 'border-error focus:border-error'
                      : 'border-primary focus:border-primary'
                  }`}
                  style={{ borderWidth: validationErrors.name ? '2px' : '1px' }}
                  aria-invalid={validationErrors.name ? 'true' : undefined}
                  aria-describedby={validationErrors.name ? 'collection-name-error' : undefined}
                  data-testid="collection-name"
                />
                {validationErrors.name && (
                  <p id="collection-name-error" className="mt-1 text-sm text-error" role="alert">
                    {validationErrors.name}
                  </p>
                )}
              </div>

              {/* Type */}
              <fieldset>
                <legend className="text-label-md text-on-surface-variant block mb-2">Type</legend>
                <div className="grid grid-cols-1 gap-0 sm:grid-cols-3">
                  {COLLECTION_TYPE_OPTIONS.map((opt) => (
                    <label
                      key={opt.value}
                      className={`cursor-pointer border p-4 ${
                        form.type === opt.value
                          ? 'border-primary bg-primary text-on-primary border-2'
                          : 'border-outline-variant bg-background text-on-surface hover:bg-surface-container-low transition-colors-fast'
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
                      <span className="block text-sm font-semibold">{opt.label}</span>
                      <span className={`mt-0.5 block text-xs ${form.type === opt.value ? 'opacity-80' : 'text-secondary'}`}>{opt.description}</span>
                    </label>
                  ))}
                </div>
              </fieldset>
            </div>
          </div>
        </section>

        {/* ── 02. Auth System Fields (read-only) ────────────────────────── */}
        {form.type === 'auth' && (
          <section className="py-8">
            <SectionHeader number="02" title="Auth System Fields" description="Auto-managed authentication fields." />
            <div className="grid grid-cols-12 gap-6">
              <div className="col-span-12 md:col-span-4">
                <p className="text-sm text-secondary">These fields are automatically managed and cannot be removed.</p>
              </div>
              <div className="col-span-12 md:col-span-8">
                <AuthFieldsDisplay collectionType={form.type} />
              </div>
            </div>
          </section>
        )}

        {/* ── 03. Fields ──────────────────────────────────────────────────── */}
        <section className="py-8">
          <SectionHeader
            number={form.type === 'auth' ? '03' : '02'}
            title={form.type === 'auth' ? 'Additional Fields' : 'Fields'}
            description="Define the schema fields for this collection."
          />
          <div className="grid grid-cols-12 gap-6">
            <div className="col-span-12 md:col-span-4">
              <p className="text-sm text-secondary">Add custom fields to define your data structure. Each field has a type and optional constraints.</p>
              <button
                type="button"
                onClick={addField}
                className="mt-4 inline-flex items-center gap-2 border border-primary bg-primary text-on-primary px-4 py-2 text-sm font-semibold hover:opacity-90"
                data-testid="add-field"
              >
                <span className="material-symbols-outlined text-base" aria-hidden="true">add</span>
                Add Field
              </button>
            </div>
            <div className="col-span-12 md:col-span-8">
              {form.fields.length === 0 ? (
                <div className="border border-dashed border-outline py-8 text-center">
                  <p className="text-sm text-secondary">No fields yet. Click &ldquo;Add Field&rdquo; to start.</p>
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
            </div>
          </div>
        </section>

        {/* ── 04. Auth Settings ─────────────────────────────────────────── */}
        {form.type === 'auth' && (
          <section className="py-8">
            <SectionHeader number="04" title="Auth Settings" description="Authentication methods and security policies." />
            <div className="grid grid-cols-12 gap-6">
              <div className="col-span-12 md:col-span-4">
                <p className="text-sm text-secondary">Configure how users authenticate with this collection.</p>
              </div>
              <div className="col-span-12 md:col-span-8">
                <AuthSettingsEditor
                  authOptions={form.authOptions}
                  onChange={(authOptions) => setForm((prev) => ({ ...prev, authOptions }))}
                />
              </div>
            </div>
          </section>
        )}

        {/* ── 05. API Rules ──────────────────────────────────────────────── */}
        <section className="py-8">
          <SectionHeader
            number={form.type === 'auth' ? '05' : '03'}
            title="API Rules"
            description="Control access to API endpoints."
          />
          <div className="grid grid-cols-12 gap-6">
            <div className="col-span-12 md:col-span-4">
              <p className="text-sm text-secondary">Set rules to control who can list, view, create, update, or delete records.</p>
            </div>
            <div className="col-span-12 md:col-span-8">
              <RulesEditor
                rules={form.rules}
                onChange={(rules) => setForm((prev) => ({ ...prev, rules }))}
                collectionType={form.type}
              />
            </div>
          </div>
        </section>

        {/* ── 06. API Preview ─────────────────────────────────────────────── */}
        <section className="py-8">
          <SectionHeader
            number={form.type === 'auth' ? '06' : '04'}
            title="API Preview"
            description="Auto-generated endpoints for this collection."
          />
          <div className="grid grid-cols-12 gap-6">
            <div className="col-span-12 md:col-span-4">
              <p className="text-sm text-secondary">These endpoints are automatically generated based on your collection configuration.</p>
            </div>
            <div className="col-span-12 md:col-span-8">
              <ApiPreview collectionName={form.name} collectionType={form.type} />
            </div>
          </div>
        </section>

        {/* ── Actions ─────────────────────────────────────────────────── */}
        <div className="flex items-center justify-between border-t border-primary pt-6 pb-8">
          <a
            href="/_/collections"
            className="border border-primary px-5 py-2.5 text-sm font-semibold text-on-surface hover:bg-surface-container-low transition-colors-fast"
          >
            Cancel
          </a>
          <button
            type="button"
            onClick={handleSave}
            disabled={saving}
            className="bg-primary text-on-primary px-6 py-2.5 text-sm font-semibold hover:opacity-90 disabled:opacity-50"
            data-testid="save-collection"
          >
            {saving ? 'Saving\u2026' : mode === 'create' ? 'Create Collection' : 'Save Changes'}
          </button>
        </div>
      </div>
    </DashboardLayout>
  );
}
