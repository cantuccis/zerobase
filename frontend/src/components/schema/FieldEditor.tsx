import type { Field, FieldType, FieldTypeName, Collection } from '../../lib/api/types';
import { FIELD_TYPE_LABELS, FIELD_TYPE_NAMES, defaultFieldType } from './field-defaults';
import { FieldTypeOptions } from './FieldTypeOptions';

// ── Styles ──────────────────────────────────────────────────────────────────

const INPUT_CLASS =
  'w-full border border-primary px-3 py-1.5 text-sm text-on-surface bg-background placeholder-outline focus:outline-none focus:border-primary';

const CHECKBOX_CLASS =
  'h-4 w-4 border border-primary text-primary bg-background accent-[var(--color-primary)] focus:ring-0 focus:ring-offset-0';

// ── Props ───────────────────────────────────────────────────────────────────

interface FieldEditorProps {
  field: Field;
  index: number;
  totalFields: number;
  onChange: (updated: Field) => void;
  onRemove: () => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
  collections: Collection[];
  nameError?: string;
}

// ── Component ───────────────────────────────────────────────────────────────

export function FieldEditor({
  field,
  index,
  totalFields,
  onChange,
  onRemove,
  onMoveUp,
  onMoveDown,
  collections,
  nameError,
}: FieldEditorProps) {
  function handleTypeChange(newTypeName: FieldTypeName) {
    if (newTypeName === field.type.type) return;
    onChange({ ...field, type: defaultFieldType(newTypeName) });
  }

  function handleTypeOptionsChange(updatedType: FieldType) {
    onChange({ ...field, type: updatedType });
  }

  return (
    <div
      className="border border-primary bg-background p-4"
      data-testid={`field-editor-${index}`}
    >
      {/* Header row: field name, type dropdown, actions */}
      <div className="flex flex-wrap items-start gap-3">
        {/* Field name */}
        <div className="min-w-0 flex-1">
          <label htmlFor={`field-name-${field.id}`} className="sr-only">
            Field name
          </label>
          <input
            id={`field-name-${field.id}`}
            type="text"
            value={field.name}
            onChange={(e) => onChange({ ...field, name: e.target.value })}
            placeholder="Field name"
            className={`${INPUT_CLASS} ${nameError ? 'border-error' : ''}`}
            style={nameError ? { borderWidth: '2px' } : undefined}
            data-testid={`field-name-${index}`}
            aria-invalid={nameError ? 'true' : undefined}
            aria-describedby={nameError ? `field-name-error-${field.id}` : undefined}
          />
          {nameError && (
            <p
              id={`field-name-error-${field.id}`}
              className="mt-1 text-xs text-error"
              role="alert"
            >
              {nameError}
            </p>
          )}
        </div>

        {/* Type dropdown */}
        <div className="w-36">
          <label htmlFor={`field-type-${field.id}`} className="sr-only">
            Field type
          </label>
          <select
            id={`field-type-${field.id}`}
            value={field.type.type}
            onChange={(e) => handleTypeChange(e.target.value as FieldTypeName)}
            className={INPUT_CLASS}
            data-testid={`field-type-${index}`}
          >
            {FIELD_TYPE_NAMES.map((tn) => (
              <option key={tn} value={tn}>
                {FIELD_TYPE_LABELS[tn]}
              </option>
            ))}
          </select>
        </div>

        {/* Reorder + Remove */}
        <div className="flex items-center gap-0.5">
          <button
            type="button"
            onClick={onMoveUp}
            disabled={index === 0}
            className="p-1.5 text-on-surface hover:bg-surface-container-high disabled:opacity-30 disabled:hover:bg-transparent"
            aria-label={`Move ${field.name || 'field'} up`}
            data-testid={`field-move-up-${index}`}
          >
            <span className="material-symbols-outlined text-base" aria-hidden="true">keyboard_arrow_up</span>
          </button>
          <button
            type="button"
            onClick={onMoveDown}
            disabled={index === totalFields - 1}
            className="p-1.5 text-on-surface hover:bg-surface-container-high disabled:opacity-30 disabled:hover:bg-transparent"
            aria-label={`Move ${field.name || 'field'} down`}
            data-testid={`field-move-down-${index}`}
          >
            <span className="material-symbols-outlined text-base" aria-hidden="true">keyboard_arrow_down</span>
          </button>
          <button
            type="button"
            onClick={onRemove}
            className="p-1.5 text-error hover:bg-error-container"
            aria-label={`Remove ${field.name || 'field'}`}
            data-testid={`field-remove-${index}`}
          >
            <span className="material-symbols-outlined text-base" aria-hidden="true">close</span>
          </button>
        </div>
      </div>

      {/* Required + Unique toggles */}
      <div className="mt-3 flex items-center gap-6">
        <div className="flex items-center gap-2">
          <input
            type="checkbox"
            id={`field-required-${field.id}`}
            checked={field.required}
            onChange={(e) => onChange({ ...field, required: e.target.checked })}
            className={CHECKBOX_CLASS}
            data-testid={`field-required-${index}`}
          />
          <label htmlFor={`field-required-${field.id}`} className="text-xs text-on-surface-variant">
            Required
          </label>
        </div>
        <div className="flex items-center gap-2">
          <input
            type="checkbox"
            id={`field-unique-${field.id}`}
            checked={field.unique}
            onChange={(e) => onChange({ ...field, unique: e.target.checked })}
            className={CHECKBOX_CLASS}
            data-testid={`field-unique-${index}`}
          />
          <label htmlFor={`field-unique-${field.id}`} className="text-xs text-on-surface-variant">
            Unique
          </label>
        </div>
      </div>

      {/* Type-specific options */}
      <div className="mt-3 border-t border-outline-variant pt-3">
        <FieldTypeOptions
          fieldType={field.type}
          onChange={handleTypeOptionsChange}
          collections={collections}
        />
      </div>
    </div>
  );
}
