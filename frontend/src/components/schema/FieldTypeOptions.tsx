import type { FieldType, Collection } from '../../lib/api/types';

// ── Shared input styles ─────────────────────────────────────────────────────

const INPUT_CLASS =
  'w-full rounded-md border border-gray-300 px-3 py-1.5 text-sm placeholder-gray-400 focus:border-blue-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-blue-500';

const CHECKBOX_CLASS =
  'h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-2 focus:ring-blue-500';

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
            <label className="block text-xs font-medium text-gray-600">Min Length</label>
            <input
              type="number"
              min={0}
              value={fieldType.options.minLength}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, minLength: Number(e.target.value) } })}
              className={INPUT_CLASS}
              data-testid="text-min-length"
            />
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-600">Max Length</label>
            <input
              type="number"
              min={0}
              value={fieldType.options.maxLength}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, maxLength: Number(e.target.value) } })}
              className={INPUT_CLASS}
              data-testid="text-max-length"
            />
          </div>
          <div className="col-span-2">
            <label className="block text-xs font-medium text-gray-600">Pattern (regex)</label>
            <input
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
              id="text-searchable"
              checked={fieldType.options.searchable}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, searchable: e.target.checked } })}
              className={CHECKBOX_CLASS}
            />
            <label htmlFor="text-searchable" className="text-xs text-gray-600">
              Searchable
            </label>
          </div>
        </div>
      );

    case 'number':
      return (
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="block text-xs font-medium text-gray-600">Min</label>
            <input
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
            <label className="block text-xs font-medium text-gray-600">Max</label>
            <input
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
              id="number-no-decimal"
              checked={fieldType.options.noDecimal}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, noDecimal: e.target.checked } })}
              className={CHECKBOX_CLASS}
            />
            <label htmlFor="number-no-decimal" className="text-xs text-gray-600">
              Integer only (no decimals)
            </label>
          </div>
        </div>
      );

    case 'bool':
      return (
        <p className="text-xs text-gray-500 italic">No additional options for Boolean fields.</p>
      );

    case 'email':
      return (
        <div className="space-y-3">
          <div>
            <label className="block text-xs font-medium text-gray-600">
              Only Domains (comma-separated)
            </label>
            <input
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
            <label className="block text-xs font-medium text-gray-600">
              Except Domains (comma-separated)
            </label>
            <input
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
            <label className="block text-xs font-medium text-gray-600">
              Only Domains (comma-separated)
            </label>
            <input
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
            <label className="block text-xs font-medium text-gray-600">
              Except Domains (comma-separated)
            </label>
            <input
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
            <label className="block text-xs font-medium text-gray-600">Min Date</label>
            <input
              type="datetime-local"
              value={fieldType.options.min}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, min: e.target.value } })}
              className={INPUT_CLASS}
              data-testid="datetime-min"
            />
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-600">Max Date</label>
            <input
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
          <label className="block text-xs font-medium text-gray-600">
            Values (comma-separated)
          </label>
          <input
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
            <label className="block text-xs font-medium text-gray-600">
              Values (comma-separated)
            </label>
            <input
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
            <label className="block text-xs font-medium text-gray-600">
              Max Selections (0 = unlimited)
            </label>
            <input
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
              id="autodate-oncreate"
              checked={fieldType.options.onCreate}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, onCreate: e.target.checked } })}
              className={CHECKBOX_CLASS}
            />
            <label htmlFor="autodate-oncreate" className="text-xs text-gray-600">
              Set on create
            </label>
          </div>
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              id="autodate-onupdate"
              checked={fieldType.options.onUpdate}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, onUpdate: e.target.checked } })}
              className={CHECKBOX_CLASS}
            />
            <label htmlFor="autodate-onupdate" className="text-xs text-gray-600">
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
              <label className="block text-xs font-medium text-gray-600">Max Size (bytes)</label>
              <input
                type="number"
                min={0}
                value={fieldType.options.maxSize}
                onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, maxSize: Number(e.target.value) } })}
                className={INPUT_CLASS}
                data-testid="file-max-size"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-gray-600">Max Files</label>
              <input
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
            <label className="block text-xs font-medium text-gray-600">
              MIME Types (comma-separated, empty = all)
            </label>
            <input
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
            <label className="block text-xs font-medium text-gray-600">
              Thumbnail Sizes (comma-separated)
            </label>
            <input
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
            <label className="block text-xs font-medium text-gray-600">Related Collection</label>
            <select
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
            <label className="block text-xs font-medium text-gray-600">
              Max Relations (empty = unlimited)
            </label>
            <input
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
              id="relation-cascade"
              checked={fieldType.options.cascadeDelete}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, cascadeDelete: e.target.checked } })}
              className={CHECKBOX_CLASS}
            />
            <label htmlFor="relation-cascade" className="text-xs text-gray-600">
              Cascade delete
            </label>
          </div>
        </div>
      );

    case 'json':
      return (
        <div>
          <label className="block text-xs font-medium text-gray-600">Max Size (bytes)</label>
          <input
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
            <label className="block text-xs font-medium text-gray-600">Max Length</label>
            <input
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
              id="editor-searchable"
              checked={fieldType.options.searchable}
              onChange={(e) => onChange({ ...fieldType, options: { ...fieldType.options, searchable: e.target.checked } })}
              className={CHECKBOX_CLASS}
            />
            <label htmlFor="editor-searchable" className="text-xs text-gray-600">
              Searchable
            </label>
          </div>
        </div>
      );
  }
}
