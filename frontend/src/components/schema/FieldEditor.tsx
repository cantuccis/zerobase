import type { Field, FieldType, FieldTypeName, Collection } from '../../lib/api/types';
import { FIELD_TYPE_LABELS, FIELD_TYPE_NAMES, defaultFieldType } from './field-defaults';
import { FieldTypeOptions } from './FieldTypeOptions';

// ── Styles ──────────────────────────────────────────────────────────────────

const INPUT_CLASS =
  'w-full rounded-md border border-gray-300 dark:border-gray-600 px-3 py-1.5 text-sm placeholder-gray-400 dark:placeholder-gray-500 focus:border-blue-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-blue-500';

const CHECKBOX_CLASS =
  'h-4 w-4 rounded border-gray-300 dark:border-gray-600 text-blue-600 dark:text-blue-400 focus:ring-2 focus:ring-blue-500';

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
      className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-4"
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
            className={`${INPUT_CLASS} ${nameError ? 'border-red-500 dark:border-red-700 focus:border-red-500 focus:ring-red-500' : ''}`}
            data-testid={`field-name-${index}`}
            aria-invalid={nameError ? 'true' : undefined}
            aria-describedby={nameError ? `field-name-error-${field.id}` : undefined}
          />
          {nameError && (
            <p
              id={`field-name-error-${field.id}`}
              className="mt-1 text-xs text-red-600 dark:text-red-400"
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
        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={onMoveUp}
            disabled={index === 0}
            className="rounded p-1 text-gray-400 dark:text-gray-500 transition-colors hover:bg-gray-100 dark:hover:bg-gray-700 hover:text-gray-600 dark:hover:text-gray-400 disabled:opacity-30 disabled:hover:bg-transparent"
            aria-label={`Move ${field.name || 'field'} up`}
            data-testid={`field-move-up-${index}`}
          >
            <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <polyline points="18 15 12 9 6 15" />
            </svg>
          </button>
          <button
            type="button"
            onClick={onMoveDown}
            disabled={index === totalFields - 1}
            className="rounded p-1 text-gray-400 dark:text-gray-500 transition-colors hover:bg-gray-100 dark:hover:bg-gray-700 hover:text-gray-600 dark:hover:text-gray-400 disabled:opacity-30 disabled:hover:bg-transparent"
            aria-label={`Move ${field.name || 'field'} down`}
            data-testid={`field-move-down-${index}`}
          >
            <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <polyline points="6 9 12 15 18 9" />
            </svg>
          </button>
          <button
            type="button"
            onClick={onRemove}
            className="rounded p-1 text-red-400 dark:text-red-500 transition-colors hover:bg-red-50 dark:hover:bg-red-900/30 hover:text-red-600 dark:hover:text-red-400"
            aria-label={`Remove ${field.name || 'field'}`}
            data-testid={`field-remove-${index}`}
          >
            <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
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
          <label htmlFor={`field-required-${field.id}`} className="text-xs text-gray-600 dark:text-gray-400">
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
          <label htmlFor={`field-unique-${field.id}`} className="text-xs text-gray-600 dark:text-gray-400">
            Unique
          </label>
        </div>
      </div>

      {/* Type-specific options */}
      <div className="mt-3 border-t border-gray-100 dark:border-gray-700 pt-3">
        <FieldTypeOptions
          fieldType={field.type}
          onChange={handleTypeOptionsChange}
          collections={collections}
        />
      </div>
    </div>
  );
}
