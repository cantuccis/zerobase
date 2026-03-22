import type { FieldType, FieldTypeName } from '../../lib/api/types';

/** Human-readable labels for field types. */
export const FIELD_TYPE_LABELS: Record<FieldTypeName, string> = {
  text: 'Text',
  number: 'Number',
  bool: 'Boolean',
  email: 'Email',
  url: 'URL',
  dateTime: 'Date/Time',
  select: 'Select',
  multiSelect: 'Multi-Select',
  autoDate: 'Auto Date',
  file: 'File',
  relation: 'Relation',
  json: 'JSON',
  editor: 'Rich Editor',
};

/** All available field type names in display order. */
export const FIELD_TYPE_NAMES: FieldTypeName[] = [
  'text',
  'number',
  'bool',
  'email',
  'url',
  'dateTime',
  'select',
  'multiSelect',
  'autoDate',
  'file',
  'relation',
  'json',
  'editor',
];

/** Returns default options for a given field type. */
export function defaultFieldType(typeName: FieldTypeName): FieldType {
  switch (typeName) {
    case 'text':
      return { type: 'text', options: { minLength: 0, maxLength: 500, pattern: null, searchable: true } };
    case 'number':
      return { type: 'number', options: { min: null, max: null, noDecimal: false } };
    case 'bool':
      return { type: 'bool', options: {} as Record<string, never> };
    case 'email':
      return { type: 'email', options: { exceptDomains: [], onlyDomains: [] } };
    case 'url':
      return { type: 'url', options: { exceptDomains: [], onlyDomains: [] } };
    case 'dateTime':
      return { type: 'dateTime', options: { min: '', max: '' } };
    case 'select':
      return { type: 'select', options: { values: [] } };
    case 'multiSelect':
      return { type: 'multiSelect', options: { values: [], maxSelect: 0 } };
    case 'autoDate':
      return { type: 'autoDate', options: { onCreate: true, onUpdate: true } };
    case 'file':
      return { type: 'file', options: { maxSize: 5242880, maxSelect: 1, mimeTypes: [], thumbs: [] } };
    case 'relation':
      return { type: 'relation', options: { collectionId: '', cascadeDelete: false, maxSelect: null } };
    case 'json':
      return { type: 'json', options: { maxSize: 2097152 } };
    case 'editor':
      return { type: 'editor', options: { maxLength: 50000, searchable: true } };
  }
}

let fieldIdCounter = 0;

/** Generate a temporary client-side field ID. */
export function generateFieldId(): string {
  fieldIdCounter += 1;
  return `new_field_${Date.now()}_${fieldIdCounter}`;
}
