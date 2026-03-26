/**
 * Dynamic field input components for record create/edit forms.
 *
 * Each field type from the collection schema maps to a specialized input control:
 * text, number, bool, email, url, dateTime, select, multiSelect, file, relation,
 * json, editor. AutoDate fields are excluded (auto-managed by the server).
 */

import { useState, useCallback, useRef } from 'react';
import type { Field, FieldType, Collection } from '../../lib/api/types';
import { RelationPicker } from './RelationPicker';
import type { RelationOption } from './RelationPicker';
import { FileUpload } from './FileUpload';

// ── Types ────────────────────────────────────────────────────────────────────

export interface FieldInputProps {
  field: Field;
  value: unknown;
  onChange: (name: string, value: unknown) => void;
  error?: string;
  /** Available collections for relation pickers. */
  collections?: Collection[];
  /** Callback to search records for relation fields. */
  onSearchRelation?: (collectionId: string, query: string) => Promise<RelationOption[]>;
  /** Labels for currently selected relation IDs. Map of id → label. */
  selectedRelationLabels?: Record<string, string>;
}

export type { RelationOption } from './RelationPicker';

// ── Main dispatcher ──────────────────────────────────────────────────────────

/** Renders the appropriate input control for a given field. */
export function FieldInput(props: FieldInputProps) {
  const { field } = props;
  const fieldType = field.type;

  switch (fieldType.type) {
    case 'text':
      return <TextInput {...props} options={fieldType.options} />;
    case 'number':
      return <NumberInput {...props} options={fieldType.options} />;
    case 'bool':
      return <BoolInput {...props} />;
    case 'email':
      return <EmailInput {...props} />;
    case 'url':
      return <UrlInput {...props} />;
    case 'dateTime':
      return <DateTimeInput {...props} options={fieldType.options} />;
    case 'select':
      return <SelectInput {...props} options={fieldType.options} />;
    case 'multiSelect':
      return <MultiSelectInput {...props} options={fieldType.options} />;
    case 'file':
      return <FileInput {...props} options={fieldType.options} />;
    case 'relation':
      return <RelationInput {...props} options={fieldType.options} />;
    case 'json':
      return <JsonInput {...props} />;
    case 'editor':
      return <EditorInput {...props} options={fieldType.options} />;
    case 'autoDate':
      return <AutoDateDisplay />;
    default:
      return <FallbackInput {...props} />;
  }
}

// ── Shared wrapper ───────────────────────────────────────────────────────────

function FieldWrapper({
  field,
  error,
  children,
}: {
  field: Field;
  error?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-1.5" data-testid={`field-input-${field.name}`}>
      <label htmlFor={`field-${field.name}`} className="flex items-center gap-1.5 text-label-sm font-bold uppercase tracking-[0.05em] text-on-surface-variant dark:text-on-surface-variant">
        {field.name}
        {field.required && <span className="text-error dark:text-error" aria-label="required">*</span>}
        {field.unique && (
          <span className="border border-outline-variant dark:border-outline-variant bg-surface-container-low dark:bg-surface-container-low px-1 py-0.5 text-[10px] font-normal uppercase text-secondary dark:text-secondary">unique</span>
        )}
      </label>
      {children}
      {error && (
        <p className="text-xs text-error dark:text-error" role="alert" data-testid={`field-error-${field.name}`}>
          {error}
        </p>
      )}
    </div>
  );
}

const inputClasses =
  'w-full border border-primary dark:border-on-primary bg-surface dark:bg-surface px-3 py-2 text-sm text-on-surface dark:text-on-surface placeholder-secondary dark:placeholder-secondary focus:border-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary disabled:bg-surface-dim dark:disabled:bg-surface-dim disabled:text-secondary dark:disabled:text-secondary';

const errorInputClasses =
  'w-full border border-error dark:border-error bg-surface dark:bg-surface px-3 py-2 text-sm text-on-surface dark:text-on-surface placeholder-secondary dark:placeholder-secondary focus:border-error focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-error';

// ── Text ─────────────────────────────────────────────────────────────────────

function TextInput({
  field,
  value,
  onChange,
  error,
  options,
}: FieldInputProps & { options: Extract<FieldType, { type: 'text' }>['options'] }) {
  const val = (value as string) ?? '';
  const isLong = options.maxLength > 500;

  return (
    <FieldWrapper field={field} error={error}>
      {isLong ? (
        <textarea
          id={`field-${field.name}`}
          value={val}
          onChange={(e) => onChange(field.name, e.target.value)}
          className={error ? errorInputClasses : inputClasses}
          rows={4}
          maxLength={options.maxLength > 0 ? options.maxLength : undefined}
          placeholder={`Enter ${field.name}…`}
        />
      ) : (
        <input
          id={`field-${field.name}`}
          type="text"
          value={val}
          onChange={(e) => onChange(field.name, e.target.value)}
          className={error ? errorInputClasses : inputClasses}
          maxLength={options.maxLength > 0 ? options.maxLength : undefined}
          placeholder={`Enter ${field.name}…`}
        />
      )}
      {options.maxLength > 0 && (
        <p className="text-xs text-secondary dark:text-secondary">{val.length}/{options.maxLength}</p>
      )}
    </FieldWrapper>
  );
}

// ── Number ───────────────────────────────────────────────────────────────────

function NumberInput({
  field,
  value,
  onChange,
  error,
  options,
}: FieldInputProps & { options: Extract<FieldType, { type: 'number' }>['options'] }) {
  const val = value ?? '';

  return (
    <FieldWrapper field={field} error={error}>
      <input
        id={`field-${field.name}`}
        type="number"
        value={String(val)}
        onChange={(e) => {
          const raw = e.target.value;
          if (raw === '') {
            onChange(field.name, null);
          } else {
            onChange(field.name, options.noDecimal ? parseInt(raw, 10) : parseFloat(raw));
          }
        }}
        className={error ? errorInputClasses : inputClasses}
        min={options.min ?? undefined}
        max={options.max ?? undefined}
        step={options.noDecimal ? 1 : 'any'}
        placeholder={`Enter ${field.name}…`}
      />
    </FieldWrapper>
  );
}

// ── Bool ─────────────────────────────────────────────────────────────────────

function BoolInput({ field, value, onChange, error }: FieldInputProps) {
  const checked = value === true;

  return (
    <FieldWrapper field={field} error={error}>
      <div className="flex items-center gap-3">
        <button
          id={`field-${field.name}`}
          type="button"
          role="switch"
          aria-checked={checked}
          onClick={() => onChange(field.name, !checked)}
          className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer border border-primary dark:border-on-primary ${
            checked ? 'bg-primary dark:bg-on-primary' : 'bg-surface-container dark:bg-surface-container'
          }`}
          data-testid={`bool-toggle-${field.name}`}
        >
          <span
            className={`pointer-events-none inline-block h-5 w-5 ${
              checked ? 'translate-x-5 bg-on-primary dark:bg-primary' : 'translate-x-0 bg-primary dark:bg-on-primary'
            }`}
          />
        </button>
        <span className="text-sm text-on-surface-variant dark:text-on-surface-variant">{checked ? 'True' : 'False'}</span>
      </div>
    </FieldWrapper>
  );
}

// ── Email ────────────────────────────────────────────────────────────────────

function EmailInput({ field, value, onChange, error }: FieldInputProps) {
  const val = (value as string) ?? '';

  return (
    <FieldWrapper field={field} error={error}>
      <input
        id={`field-${field.name}`}
        type="email"
        value={val}
        onChange={(e) => onChange(field.name, e.target.value)}
        className={error ? errorInputClasses : inputClasses}
        placeholder="user@example.com"
        autoComplete="off"
        spellCheck={false}
      />
    </FieldWrapper>
  );
}

// ── URL ──────────────────────────────────────────────────────────────────────

function UrlInput({ field, value, onChange, error }: FieldInputProps) {
  const val = (value as string) ?? '';

  return (
    <FieldWrapper field={field} error={error}>
      <input
        id={`field-${field.name}`}
        type="url"
        value={val}
        onChange={(e) => onChange(field.name, e.target.value)}
        className={error ? errorInputClasses : inputClasses}
        placeholder="https://…"
        autoComplete="off"
      />
    </FieldWrapper>
  );
}

// ── DateTime ─────────────────────────────────────────────────────────────────

function DateTimeInput({
  field,
  value,
  onChange,
  error,
  options,
}: FieldInputProps & { options: Extract<FieldType, { type: 'dateTime' }>['options'] }) {
  const val = (value as string) ?? '';

  return (
    <FieldWrapper field={field} error={error}>
      <input
        id={`field-${field.name}`}
        type="datetime-local"
        value={val ? toLocalDateTimeValue(val) : ''}
        onChange={(e) => {
          const local = e.target.value;
          onChange(field.name, local ? new Date(local).toISOString() : '');
        }}
        className={error ? errorInputClasses : inputClasses}
        min={options.min ? toLocalDateTimeValue(options.min) : undefined}
        max={options.max ? toLocalDateTimeValue(options.max) : undefined}
      />
    </FieldWrapper>
  );
}

function toLocalDateTimeValue(isoString: string): string {
  try {
    const d = new Date(isoString);
    if (isNaN(d.getTime())) return '';
    return d.toISOString().slice(0, 16);
  } catch {
    return '';
  }
}

// ── Select ───────────────────────────────────────────────────────────────────

function SelectInput({
  field,
  value,
  onChange,
  error,
  options,
}: FieldInputProps & { options: Extract<FieldType, { type: 'select' }>['options'] }) {
  const val = (value as string) ?? '';

  return (
    <FieldWrapper field={field} error={error}>
      <select
        id={`field-${field.name}`}
        value={val}
        onChange={(e) => onChange(field.name, e.target.value || null)}
        className={error ? errorInputClasses : inputClasses}
      >
        <option value="">— Select —</option>
        {options.values.map((v) => (
          <option key={v} value={v}>{v}</option>
        ))}
      </select>
    </FieldWrapper>
  );
}

// ── MultiSelect ──────────────────────────────────────────────────────────────

function MultiSelectInput({
  field,
  value,
  onChange,
  error,
  options,
}: FieldInputProps & { options: Extract<FieldType, { type: 'multiSelect' }>['options'] }) {
  const selected = Array.isArray(value) ? (value as string[]) : [];

  const toggle = (v: string) => {
    const next = selected.includes(v)
      ? selected.filter((s) => s !== v)
      : [...selected, v];
    onChange(field.name, next);
  };

  return (
    <FieldWrapper field={field} error={error}>
      <div className="flex flex-wrap gap-2" role="group" aria-label={`${field.name} options`}>
        {options.values.map((v) => {
          const isSelected = selected.includes(v);
          return (
            <button
              key={v}
              type="button"
              onClick={() => toggle(v)}
              className={`border px-3 py-1 text-sm ${
                isSelected
                  ? 'border-primary dark:border-on-primary bg-primary dark:bg-on-primary text-on-primary dark:text-primary'
                  : 'border-primary dark:border-on-primary bg-surface dark:bg-surface text-on-surface dark:text-on-surface hover:bg-surface-container-low dark:hover:bg-surface-container-low'
              }`}
              aria-pressed={isSelected}
              data-testid={`multiselect-option-${v}`}
            >
              {v}
            </button>
          );
        })}
      </div>
      {options.maxSelect > 0 && (
        <p className="text-xs text-secondary dark:text-secondary">
          {selected.length}/{options.maxSelect} selected
        </p>
      )}
    </FieldWrapper>
  );
}

// ── File ─────────────────────────────────────────────────────────────────────

function FileInput({
  field,
  value,
  onChange,
  error,
  options,
}: FieldInputProps & { options: Extract<FieldType, { type: 'file' }>['options'] }) {
  const isMulti = options.maxSelect > 1;

  // Separate existing filenames from new File objects
  const existingFiles: string[] = [];
  const newFiles: File[] = [];

  if (Array.isArray(value)) {
    for (const item of value) {
      if (typeof item === 'string') existingFiles.push(item);
      else if (item instanceof File) newFiles.push(item);
    }
  } else if (typeof value === 'string' && value) {
    existingFiles.push(value);
  } else if (value instanceof File) {
    newFiles.push(value);
  }

  const handleFilesChange = useCallback(
    (files: File[]) => {
      if (isMulti) {
        // Combine existing filenames + new files
        onChange(field.name, [...existingFiles, ...files]);
      } else {
        onChange(field.name, files.length > 0 ? files[0] : null);
      }
    },
    [field.name, onChange, isMulti, existingFiles],
  );

  const handleRemoveExisting = useCallback(
    (filename: string) => {
      const remaining = existingFiles.filter((f) => f !== filename);
      if (isMulti) {
        onChange(field.name, [...remaining, ...newFiles]);
      } else {
        onChange(field.name, remaining.length > 0 ? remaining[0] : null);
      }
    },
    [field.name, onChange, isMulti, existingFiles, newFiles],
  );

  return (
    <FieldWrapper field={field} error={error}>
      <FileUpload
        name={field.name}
        multiple={isMulti}
        options={{
          maxSize: options.maxSize,
          maxSelect: options.maxSelect,
          mimeTypes: options.mimeTypes,
        }}
        value={newFiles}
        existingFiles={existingFiles}
        onChange={handleFilesChange}
        onRemoveExisting={handleRemoveExisting}
        hasError={!!error}
      />
    </FieldWrapper>
  );
}

// ── Relation ─────────────────────────────────────────────────────────────────

function RelationInput({
  field,
  value,
  onChange,
  error,
  options,
  collections,
  onSearchRelation,
  selectedRelationLabels,
}: FieldInputProps & { options: Extract<FieldType, { type: 'relation' }>['options'] }) {
  const isMulti = options.maxSelect === null || options.maxSelect > 1;

  // Normalize value to string[]
  const selectedIds: string[] = isMulti
    ? (Array.isArray(value) ? (value as string[]) : value ? [value as string] : [])
    : (value ? [value as string] : []);

  const relatedCollectionName =
    collections?.find((c) => c.id === options.collectionId)?.name ?? options.collectionId;

  const handleChange = useCallback(
    (ids: string[]) => {
      if (isMulti) {
        onChange(field.name, ids);
      } else {
        onChange(field.name, ids.length > 0 ? ids[0] : '');
      }
    },
    [field.name, isMulti, onChange],
  );

  // Provide a no-op search if callback not provided
  const handleSearch = useCallback(
    async (collectionId: string, query: string): Promise<RelationOption[]> => {
      if (!onSearchRelation) return [];
      return onSearchRelation(collectionId, query);
    },
    [onSearchRelation],
  );

  return (
    <FieldWrapper field={field} error={error}>
      <RelationPicker
        name={field.name}
        collectionId={options.collectionId}
        collectionName={relatedCollectionName}
        multiple={isMulti}
        value={selectedIds}
        selectedLabels={selectedRelationLabels}
        onChange={handleChange}
        onSearch={handleSearch}
        hasError={!!error}
      />
    </FieldWrapper>
  );
}

// ── JSON ─────────────────────────────────────────────────────────────────────

function JsonInput({ field, value, onChange, error }: FieldInputProps) {
  const [rawText, setRawText] = useState<string>(() => {
    if (value === null || value === undefined) return '';
    return typeof value === 'string' ? value : JSON.stringify(value, null, 2);
  });
  const [parseError, setParseError] = useState<string | null>(null);

  const handleChange = (text: string) => {
    setRawText(text);
    if (!text.trim()) {
      setParseError(null);
      onChange(field.name, null);
      return;
    }
    try {
      const parsed = JSON.parse(text);
      setParseError(null);
      onChange(field.name, parsed);
    } catch {
      setParseError('Invalid JSON');
    }
  };

  return (
    <FieldWrapper field={field} error={error || parseError || undefined}>
      <textarea
        id={`field-${field.name}`}
        value={rawText}
        onChange={(e) => handleChange(e.target.value)}
        className={`${error || parseError ? errorInputClasses : inputClasses} font-mono text-xs`}
        rows={6}
        placeholder='{ "key": "value" }'
        spellCheck={false}
      />
    </FieldWrapper>
  );
}

// ── Editor (rich text) ───────────────────────────────────────────────────────

function EditorInput({
  field,
  value,
  onChange,
  error,
  options,
}: FieldInputProps & { options: Extract<FieldType, { type: 'editor' }>['options'] }) {
  const val = (value as string) ?? '';

  return (
    <FieldWrapper field={field} error={error}>
      <textarea
        id={`field-${field.name}`}
        value={val}
        onChange={(e) => onChange(field.name, e.target.value)}
        className={error ? errorInputClasses : inputClasses}
        rows={8}
        maxLength={options.maxLength > 0 ? options.maxLength : undefined}
        placeholder="Enter HTML content…"
      />
      <p className="text-xs text-secondary dark:text-secondary">
        Supports HTML content
        {options.maxLength > 0 && ` · ${val.length}/${options.maxLength}`}
      </p>
    </FieldWrapper>
  );
}

// ── AutoDate display ─────────────────────────────────────────────────────────

function AutoDateDisplay() {
  return (
    <p className="text-xs italic text-on-surface-variant dark:text-on-surface-variant" data-testid="autodate-notice">
      Automatically managed by the server.
    </p>
  );
}

// ── Fallback ─────────────────────────────────────────────────────────────────

function FallbackInput({ field, value, onChange, error }: FieldInputProps) {
  const val = value !== null && value !== undefined ? String(value) : '';

  return (
    <FieldWrapper field={field} error={error}>
      <input
        id={`field-${field.name}`}
        type="text"
        value={val}
        onChange={(e) => onChange(field.name, e.target.value)}
        className={error ? errorInputClasses : inputClasses}
        placeholder={`Enter ${field.name}…`}
      />
    </FieldWrapper>
  );
}
