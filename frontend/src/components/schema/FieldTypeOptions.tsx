import type { FieldType, Collection } from '../../lib/api/types';

// ── Shared input styles ─────────────────────────────────────────────────────

const INPUT_CLASS =
  'w-full border border-primary px-3 py-1.5 text-sm text-on-surface bg-background placeholder-outline focus:outline-none focus:border-primary';

const CHECKBOX_CLASS =
  'h-4 w-4 border border-primary text-primary bg-background accent-[var(--color-primary)] focus:ring-0 focus:ring-offset-0';

// ── Props ───────────────────────────────────────────────────────────────────

interface FieldTypeOptionsProps {
  fieldType: FieldType;
  onChange: (updated: FieldType) => void;
  collections: Collection[];
}

// ── Component ───────────────────────────────────────────────────────────────

export function FieldTypeOptions({ fieldType, onChange, collections }: FieldTypeOptionsProps) {
  switch (fieldType.type) {
    case 'text':
      return (
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label htmlFor="fto-text-min-length" className="text-label-sm text-on-surface-variant block mb-1">Min Length</label>
            <input
              id="fto-text-min-length"
              type="number"
              min={0}
              value={fieldType.options.minLength}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, minLength: Number(e.target.value) } })}
              className={INPUT_CLASS}
              data-testid="text-min-length"
            />
          </div>
          <div>
            <label htmlFor="fto-text-max-length" className="text-label-sm text-on-surface-variant block mb-1">Max Length</label>
            <input
              id="fto-text-max-length"
              type="number"
              min={0}
              value={fieldType.options.maxLength}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, maxLength: Number(e.target.value) } })}
              className={INPUT_CLASS}
              data-testid="text-max-length"
            />
          </div>
          <div className="col-span-2">
            <label htmlFor="fto-text-pattern" className="text-label-sm text-on-surface-variant block mb-1">Pattern (regex)</label>
            <input
              id="fto-text-pattern"
              type="text"
              value={fieldType.options.pattern ?? ''}
              onChange={(e) =>
                onChange({ ...fieldType, options: { ...fieldType.options, pattern: e.target.value || null } })
              }
              placeholder="e.g. ^[a-z]+$"
              className={INPUT_CLASS}
              data-testid="text-pattern"
            />
          </div>
          <div className="col-span-2 flex items-center gap-2">
            <input
              type="checkbox"
              id="fto-text-searchable"
              checked={fieldType.options.searchable}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, searchable: e.target.checked } })}
              className={CHECKBOX_CLASS}
            />
            <label htmlFor="fto-text-searchable" className="text-xs text-on-surface-variant">
              Searchable
            </label>
          </div>
        </div>
      );

    case 'number':
      return (
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label htmlFor="fto-number-min" className="text-label-sm text-on-surface-variant block mb-1">Min</label>
            <input
              id="fto-number-min"
              type="number"
              value={fieldType.options.min ?? ''}
              onChange={(e) =>
                onChange({ ...fieldType, options: { ...fieldType.options, min: e.target.value === '' ? null : Number(e.target.value) } })
              }
              placeholder="No minimum"
              className={INPUT_CLASS}
              data-testid="number-min"
            />
          </div>
          <div>
            <label htmlFor="fto-number-max" className="text-label-sm text-on-surface-variant block mb-1">Max</label>
            <input
              id="fto-number-max"
              type="number"
              value={fieldType.options.max ?? ''}
              onChange={(e) =>
                onChange({ ...fieldType, options: { ...fieldType.options, max: e.target.value === '' ? null : Number(e.target.value) } })
              }
              placeholder="No maximum"
              className={INPUT_CLASS}
              data-testid="number-max"
            />
          </div>
          <div className="col-span-2 flex items-center gap-2">
            <input
              type="checkbox"
              id="fto-number-no-decimal"
              checked={fieldType.options.noDecimal}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, noDecimal: e.target.checked } })}
              className={CHECKBOX_CLASS}
            />
            <label htmlFor="fto-number-no-decimal" className="text-xs text-on-surface-variant">
              Integer only (no decimals)
            </label>
          </div>
        </div>
      );

    case 'bool':
      return (
        <p className="text-xs text-secondary italic">No additional options for Boolean fields.</p>
      );

    case 'email':
      return (
        <div className="space-y-3">
          <div>
            <label htmlFor="fto-email-only-domains" className="text-label-sm text-on-surface-variant block mb-1">
              Only Domains (comma-separated)
            </label>
            <input
              id="fto-email-only-domains"
              type="text"
              value={fieldType.options.onlyDomains.join(', ')}
              onChange={(e) =>
                onChange({
                  ...fieldType,
                  options: {
                    ...fieldType.options,
                    onlyDomains: e.target.value
                      .split(',')
                      .map((s) => s.trim())
                      .filter(Boolean),
                  },
                })
              }
              placeholder="e.g. example.com, company.org"
              className={INPUT_CLASS}
              data-testid="email-only-domains"
            />
          </div>
          <div>
            <label htmlFor="fto-email-except-domains" className="text-label-sm text-on-surface-variant block mb-1">
              Except Domains (comma-separated)
            </label>
            <input
              id="fto-email-except-domains"
              type="text"
              value={fieldType.options.exceptDomains.join(', ')}
              onChange={(e) =>
                onChange({
                  ...fieldType,
                  options: {
                    ...fieldType.options,
                    exceptDomains: e.target.value
                      .split(',')
                      .map((s) => s.trim())
                      .filter(Boolean),
                  },
                })
              }
              placeholder="e.g. spam.com"
              className={INPUT_CLASS}
              data-testid="email-except-domains"
            />
          </div>
        </div>
      );

    case 'url':
      return (
        <div className="space-y-3">
          <div>
            <label htmlFor="fto-url-only-domains" className="text-label-sm text-on-surface-variant block mb-1">
              Only Domains (comma-separated)
            </label>
            <input
              id="fto-url-only-domains"
              type="text"
              value={fieldType.options.onlyDomains.join(', ')}
              onChange={(e) =>
                onChange({
                  ...fieldType,
                  options: {
                    ...fieldType.options,
                    onlyDomains: e.target.value
                      .split(',')
                      .map((s) => s.trim())
                      .filter(Boolean),
                  },
                })
              }
              placeholder="e.g. github.com"
              className={INPUT_CLASS}
              data-testid="url-only-domains"
            />
          </div>
          <div>
            <label htmlFor="fto-url-except-domains" className="text-label-sm text-on-surface-variant block mb-1">
              Except Domains (comma-separated)
            </label>
            <input
              id="fto-url-except-domains"
              type="text"
              value={fieldType.options.exceptDomains.join(', ')}
              onChange={(e) =>
                onChange({
                  ...fieldType,
                  options: {
                    ...fieldType.options,
                    exceptDomains: e.target.value
                      .split(',')
                      .map((s) => s.trim())
                      .filter(Boolean),
                  },
                })
              }
              placeholder="e.g. evil.com"
              className={INPUT_CLASS}
              data-testid="url-except-domains"
            />
          </div>
        </div>
      );

    case 'dateTime':
      return (
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label htmlFor="fto-datetime-min" className="text-label-sm text-on-surface-variant block mb-1">Min Date</label>
            <input
              id="fto-datetime-min"
              type="datetime-local"
              value={fieldType.options.min}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, min: e.target.value } })}
              className={INPUT_CLASS}
              data-testid="datetime-min"
            />
          </div>
          <div>
            <label htmlFor="fto-datetime-max" className="text-label-sm text-on-surface-variant block mb-1">Max Date</label>
            <input
              id="fto-datetime-max"
              type="datetime-local"
              value={fieldType.options.max}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, max: e.target.value } })}
              className={INPUT_CLASS}
              data-testid="datetime-max"
            />
          </div>
        </div>
      );

    case 'select':
      return (
        <div>
          <label htmlFor="fto-select-values" className="text-label-sm text-on-surface-variant block mb-1">
            Values (comma-separated)
          </label>
          <input
            id="fto-select-values"
            type="text"
            value={fieldType.options.values.join(', ')}
            onChange={(e) =>
              onChange({
                ...fieldType,
                options: {
                  ...fieldType.options,
                  values: e.target.value
                    .split(',')
                    .map((s) => s.trim())
                    .filter(Boolean),
                },
              })
            }
            placeholder="e.g. draft, published, archived"
            className={INPUT_CLASS}
            data-testid="select-values"
          />
        </div>
      );

    case 'multiSelect':
      return (
        <div className="space-y-3">
          <div>
            <label htmlFor="fto-multiselect-values" className="text-label-sm text-on-surface-variant block mb-1">
              Values (comma-separated)
            </label>
            <input
              id="fto-multiselect-values"
              type="text"
              value={fieldType.options.values.join(', ')}
              onChange={(e) =>
                onChange({
                  ...fieldType,
                  options: {
                    ...fieldType.options,
                    values: e.target.value
                      .split(',')
                      .map((s) => s.trim())
                      .filter(Boolean),
                  },
                })
              }
              placeholder="e.g. tag1, tag2, tag3"
              className={INPUT_CLASS}
              data-testid="multiselect-values"
            />
          </div>
          <div>
            <label htmlFor="fto-multiselect-max-select" className="text-label-sm text-on-surface-variant block mb-1">
              Max Selections (0 = unlimited)
            </label>
            <input
              id="fto-multiselect-max-select"
              type="number"
              min={0}
              value={fieldType.options.maxSelect}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, maxSelect: Number(e.target.value) } })}
              className={INPUT_CLASS}
              data-testid="multiselect-max"
            />
          </div>
        </div>
      );

    case 'autoDate':
      return (
        <div className="space-y-2">
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              id="fto-autodate-oncreate"
              checked={fieldType.options.onCreate}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, onCreate: e.target.checked } })}
              className={CHECKBOX_CLASS}
            />
            <label htmlFor="fto-autodate-oncreate" className="text-xs text-on-surface-variant">
              Set on create
            </label>
          </div>
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              id="fto-autodate-onupdate"
              checked={fieldType.options.onUpdate}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, onUpdate: e.target.checked } })}
              className={CHECKBOX_CLASS}
            />
            <label htmlFor="fto-autodate-onupdate" className="text-xs text-on-surface-variant">
              Set on update
            </label>
          </div>
        </div>
      );

    case 'file':
      return (
        <div className="space-y-3">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label htmlFor="fto-file-max-size" className="text-label-sm text-on-surface-variant block mb-1">Max Size (bytes)</label>
              <input
                id="fto-file-max-size"
                type="number"
                min={0}
                value={fieldType.options.maxSize}
                onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, maxSize: Number(e.target.value) } })}
                className={INPUT_CLASS}
                data-testid="file-max-size"
              />
            </div>
            <div>
              <label htmlFor="fto-file-max-select" className="text-label-sm text-on-surface-variant block mb-1">Max Files</label>
              <input
                id="fto-file-max-select"
                type="number"
                min={1}
                value={fieldType.options.maxSelect}
                onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, maxSelect: Number(e.target.value) } })}
                className={INPUT_CLASS}
                data-testid="file-max-select"
              />
            </div>
          </div>
          <div>
            <label htmlFor="fto-file-mime-types" className="text-label-sm text-on-surface-variant block mb-1">
              MIME Types (comma-separated, empty = all)
            </label>
            <input
              id="fto-file-mime-types"
              type="text"
              value={fieldType.options.mimeTypes.join(', ')}
              onChange={(e) =>
                onChange({
                  ...fieldType,
                  options: {
                    ...fieldType.options,
                    mimeTypes: e.target.value
                      .split(',')
                      .map((s) => s.trim())
                      .filter(Boolean),
                  },
                })
              }
              placeholder="e.g. image/png, image/jpeg, application/pdf"
              className={INPUT_CLASS}
              data-testid="file-mime-types"
            />
          </div>
          <div>
            <label htmlFor="fto-file-thumbs" className="text-label-sm text-on-surface-variant block mb-1">
              Thumbnail Sizes (comma-separated)
            </label>
            <input
              id="fto-file-thumbs"
              type="text"
              value={fieldType.options.thumbs.join(', ')}
              onChange={(e) =>
                onChange({
                  ...fieldType,
                  options: {
                    ...fieldType.options,
                    thumbs: e.target.value
                      .split(',')
                      .map((s) => s.trim())
                      .filter(Boolean),
                  },
                })
              }
              placeholder="e.g. 100x100, 200x200"
              className={INPUT_CLASS}
              data-testid="file-thumbs"
            />
          </div>
        </div>
      );

    case 'relation':
      return (
        <div className="space-y-3">
          <div>
            <label htmlFor="fto-relation-collection" className="text-label-sm text-on-surface-variant block mb-1">Related Collection</label>
            <select
              id="fto-relation-collection"
              value={fieldType.options.collectionId}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, collectionId: e.target.value } })}
              className={INPUT_CLASS}
              data-testid="relation-collection"
            >
              <option value="">Select a collection</option>
              {collections.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </div>
          <div>
            <label htmlFor="fto-relation-max-select" className="text-label-sm text-on-surface-variant block mb-1">
              Max Relations (empty = unlimited)
            </label>
            <input
              id="fto-relation-max-select"
              type="number"
              min={1}
              value={fieldType.options.maxSelect ?? ''}
              onChange={(e) =>
                onChange({
                  ...fieldType,
                  options: {
                    ...fieldType.options,
                    maxSelect: e.target.value === '' ? null : Number(e.target.value),
                  },
                })
              }
              placeholder="Unlimited"
              className={INPUT_CLASS}
              data-testid="relation-max-select"
            />
          </div>
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              id="fto-relation-cascade-delete"
              checked={fieldType.options.cascadeDelete}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, cascadeDelete: e.target.checked } })}
              className={CHECKBOX_CLASS}
            />
            <label htmlFor="fto-relation-cascade-delete" className="text-xs text-on-surface-variant">
              Cascade delete
            </label>
          </div>
        </div>
      );

    case 'json':
      return (
        <div>
          <label htmlFor="fto-json-max-size" className="text-label-sm text-on-surface-variant block mb-1">Max Size (bytes)</label>
          <input
            id="fto-json-max-size"
            type="number"
            min={0}
            value={fieldType.options.maxSize}
            onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, maxSize: Number(e.target.value) } })}
            className={INPUT_CLASS}
            data-testid="json-max-size"
          />
        </div>
      );

    case 'editor':
      return (
        <div className="space-y-3">
          <div>
            <label htmlFor="fto-editor-max-length" className="text-label-sm text-on-surface-variant block mb-1">Max Length</label>
            <input
              id="fto-editor-max-length"
              type="number"
              min={0}
              value={fieldType.options.maxLength}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, maxLength: Number(e.target.value) } })}
              className={INPUT_CLASS}
              data-testid="editor-max-length"
            />
          </div>
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              id="fto-editor-searchable"
              checked={fieldType.options.searchable}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, searchable: e.target.checked } })}
              className={CHECKBOX_CLASS}
            />
            <label htmlFor="fto-editor-searchable" className="text-xs text-on-surface-variant">
              Searchable
            </label>
          </div>
        </div>
      );
  }
}
