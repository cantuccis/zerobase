/**
 * Client-side validation for record forms based on collection field schemas.
 *
 * Returns a map of field name → error message. An empty map means no errors.
 */

import type { Field, FieldType } from '../../lib/api/types';
import { isAllowedMimeType, formatFileSize } from './file-validation';

export type ValidationErrors = Record<string, string>;

/** Validate all editable fields and return errors. */
export function validateRecord(
  fields: Field[],
  values: Record<string, unknown>,
): ValidationErrors {
  const errors: ValidationErrors = {};

  for (const field of fields) {
    // Skip auto-managed fields
    if (field.type.type === 'autoDate') continue;

    const value = values[field.name];
    const error = validateField(field, value);
    if (error) {
      errors[field.name] = error;
    }
  }

  return errors;
}

/** Validate a single field value against its schema. */
export function validateField(field: Field, value: unknown): string | null {
  // Required check
  if (field.required && isEmpty(value, field.type)) {
    return `${field.name} is required.`;
  }

  // Skip further validation if empty and not required
  if (isEmpty(value, field.type)) return null;

  return validateFieldType(field.type, value);
}

function isEmpty(value: unknown, fieldType: FieldType): boolean {
  if (value === null || value === undefined) return true;

  switch (fieldType.type) {
    case 'text':
    case 'email':
    case 'url':
    case 'editor':
      return typeof value === 'string' && value.trim() === '';
    case 'number':
      return value === '' || value === null;
    case 'bool':
      return false; // Booleans are never "empty"
    case 'select':
      return !value;
    case 'multiSelect':
      return !Array.isArray(value) || value.length === 0;
    case 'dateTime':
      return typeof value === 'string' && value.trim() === '';
    case 'file':
      if (Array.isArray(value)) return value.length === 0;
      return !value;
    case 'relation':
      if (Array.isArray(value)) return value.length === 0;
      return typeof value === 'string' && value.trim() === '';
    case 'json':
      return value === null || value === undefined;
    default:
      return !value;
  }
}

function validateFieldType(fieldType: FieldType, value: unknown): string | null {
  switch (fieldType.type) {
    case 'text':
      return validateText(fieldType.options, value);
    case 'number':
      return validateNumber(fieldType.options, value);
    case 'email':
      return validateEmail(value);
    case 'url':
      return validateUrl(value);
    case 'dateTime':
      return validateDateTime(fieldType.options, value);
    case 'multiSelect':
      return validateMultiSelect(fieldType.options, value);
    case 'file':
      return validateFileField(fieldType.options, value);
    case 'json':
      return validateJson(value);
    case 'editor':
      return validateEditor(fieldType.options, value);
    default:
      return null;
  }
}

function validateText(options: Extract<FieldType, { type: 'text' }>['options'], value: unknown): string | null {
  const str = String(value);
  if (options.minLength > 0 && str.length < options.minLength) {
    return `Must be at least ${options.minLength} characters.`;
  }
  if (options.maxLength > 0 && str.length > options.maxLength) {
    return `Must be at most ${options.maxLength} characters.`;
  }
  if (options.pattern) {
    try {
      const re = new RegExp(options.pattern);
      if (!re.test(str)) {
        return `Must match pattern: ${options.pattern}`;
      }
    } catch {
      // Ignore invalid regex from schema
    }
  }
  return null;
}

function validateNumber(options: Extract<FieldType, { type: 'number' }>['options'], value: unknown): string | null {
  const num = typeof value === 'number' ? value : parseFloat(String(value));
  if (isNaN(num)) return 'Must be a valid number.';
  if (options.noDecimal && !Number.isInteger(num)) return 'Must be a whole number.';
  if (options.min !== null && num < options.min) return `Must be at least ${options.min}.`;
  if (options.max !== null && num > options.max) return `Must be at most ${options.max}.`;
  return null;
}

function validateEmail(value: unknown): string | null {
  const str = String(value);
  // Basic email check
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(str)) {
    return 'Must be a valid email address.';
  }
  return null;
}

function validateUrl(value: unknown): string | null {
  const str = String(value);
  try {
    new URL(str);
    return null;
  } catch {
    return 'Must be a valid URL.';
  }
}

function validateDateTime(options: Extract<FieldType, { type: 'dateTime' }>['options'], value: unknown): string | null {
  const str = String(value);
  const date = new Date(str);
  if (isNaN(date.getTime())) return 'Must be a valid date/time.';
  if (options.min) {
    const minDate = new Date(options.min);
    if (!isNaN(minDate.getTime()) && date < minDate) {
      return `Must be after ${options.min}.`;
    }
  }
  if (options.max) {
    const maxDate = new Date(options.max);
    if (!isNaN(maxDate.getTime()) && date > maxDate) {
      return `Must be before ${options.max}.`;
    }
  }
  return null;
}

function validateMultiSelect(
  options: Extract<FieldType, { type: 'multiSelect' }>['options'],
  value: unknown,
): string | null {
  if (!Array.isArray(value)) return null;
  if (options.maxSelect > 0 && value.length > options.maxSelect) {
    return `At most ${options.maxSelect} items allowed.`;
  }
  return null;
}

function validateJson(value: unknown): string | null {
  if (typeof value === 'string') {
    try {
      JSON.parse(value);
    } catch {
      return 'Must be valid JSON.';
    }
  }
  return null;
}

function validateEditor(options: Extract<FieldType, { type: 'editor' }>['options'], value: unknown): string | null {
  const str = String(value);
  if (options.maxLength > 0 && str.length > options.maxLength) {
    return `Must be at most ${options.maxLength} characters.`;
  }
  return null;
}

function validateFileField(
  options: Extract<FieldType, { type: 'file' }>['options'],
  value: unknown,
): string | null {
  // Collect File objects from the value
  const files: File[] = [];
  if (value instanceof File) {
    files.push(value);
  } else if (Array.isArray(value)) {
    for (const item of value) {
      if (item instanceof File) files.push(item);
    }
  }

  // No new files to validate
  if (files.length === 0) return null;

  // Count check
  if (options.maxSelect > 0 && files.length > options.maxSelect) {
    return `At most ${options.maxSelect} file${options.maxSelect > 1 ? 's' : ''} allowed.`;
  }

  // Per-file checks
  for (const file of files) {
    if (options.maxSize > 0 && file.size > options.maxSize) {
      return `"${file.name}" exceeds the maximum size of ${formatFileSize(options.maxSize)}.`;
    }
    if (!isAllowedMimeType(file, options.mimeTypes)) {
      return `"${file.name}" is not an allowed file type.`;
    }
  }

  return null;
}
