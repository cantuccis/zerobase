import { describe, it, expect } from 'vitest';
import { validateRecord, validateField } from './validate-record';
import type { Field } from '../../lib/api/types';

// ── Helpers ──────────────────────────────────────────────────────────────────

function textField(name: string, opts: Partial<{ minLength: number; maxLength: number; pattern: string | null; required: boolean }> = {}): Field {
  return {
    id: `f_${name}`,
    name,
    type: { type: 'text', options: { minLength: opts.minLength ?? 0, maxLength: opts.maxLength ?? 500, pattern: opts.pattern ?? null, searchable: true } },
    required: opts.required ?? false,
    unique: false,
    sortOrder: 0,
  };
}

function numberField(name: string, opts: Partial<{ min: number | null; max: number | null; noDecimal: boolean; required: boolean }> = {}): Field {
  return {
    id: `f_${name}`,
    name,
    type: { type: 'number', options: { min: opts.min ?? null, max: opts.max ?? null, noDecimal: opts.noDecimal ?? false } },
    required: opts.required ?? false,
    unique: false,
    sortOrder: 0,
  };
}

function boolField(name: string, required = false): Field {
  return {
    id: `f_${name}`,
    name,
    type: { type: 'bool', options: {} },
    required,
    unique: false,
    sortOrder: 0,
  };
}

function emailField(name: string, required = false): Field {
  return {
    id: `f_${name}`,
    name,
    type: { type: 'email', options: { exceptDomains: [], onlyDomains: [] } },
    required,
    unique: false,
    sortOrder: 0,
  };
}

function urlField(name: string, required = false): Field {
  return {
    id: `f_${name}`,
    name,
    type: { type: 'url', options: { exceptDomains: [], onlyDomains: [] } },
    required,
    unique: false,
    sortOrder: 0,
  };
}

function selectField(name: string, values: string[], required = false): Field {
  return {
    id: `f_${name}`,
    name,
    type: { type: 'select', options: { values } },
    required,
    unique: false,
    sortOrder: 0,
  };
}

function multiSelectField(name: string, values: string[], maxSelect: number, required = false): Field {
  return {
    id: `f_${name}`,
    name,
    type: { type: 'multiSelect', options: { values, maxSelect } },
    required,
    unique: false,
    sortOrder: 0,
  };
}

function dateTimeField(name: string, opts: Partial<{ min: string; max: string; required: boolean }> = {}): Field {
  return {
    id: `f_${name}`,
    name,
    type: { type: 'dateTime', options: { min: opts.min ?? '', max: opts.max ?? '' } },
    required: opts.required ?? false,
    unique: false,
    sortOrder: 0,
  };
}

function editorField(name: string, maxLength = 50000, required = false): Field {
  return {
    id: `f_${name}`,
    name,
    type: { type: 'editor', options: { maxLength, searchable: true } },
    required,
    unique: false,
    sortOrder: 0,
  };
}

function autoDateField(name: string): Field {
  return {
    id: `f_${name}`,
    name,
    type: { type: 'autoDate', options: { onCreate: true, onUpdate: true } },
    required: false,
    unique: false,
    sortOrder: 0,
  };
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('validateRecord', () => {
  it('returns empty object for valid values', () => {
    const fields = [textField('title'), numberField('views')];
    const values = { title: 'Hello', views: 42 };
    expect(validateRecord(fields, values)).toEqual({});
  });

  it('returns errors for required fields that are empty', () => {
    const fields = [textField('title', { required: true }), numberField('views', { required: true })];
    const values = { title: '', views: null };
    const errors = validateRecord(fields, values);
    expect(errors.title).toBeDefined();
    expect(errors.views).toBeDefined();
  });

  it('skips autoDate fields', () => {
    const fields = [autoDateField('created'), textField('title', { required: true })];
    const values = { title: 'Hello' };
    const errors = validateRecord(fields, values);
    expect(errors).toEqual({});
    expect(errors.created).toBeUndefined();
  });

  it('validates multiple fields and returns all errors', () => {
    const fields = [
      textField('title', { required: true }),
      emailField('email', true),
      numberField('age', { min: 0, max: 150 }),
    ];
    const values = { title: '', email: 'invalid', age: 200 };
    const errors = validateRecord(fields, values);
    expect(Object.keys(errors).length).toBe(3);
  });
});

describe('validateField', () => {
  // ── Required ──────────────────────────────────────────────────────────

  describe('required fields', () => {
    it('fails for empty string on required text field', () => {
      expect(validateField(textField('t', { required: true }), '')).toBeTruthy();
    });

    it('fails for whitespace-only on required text field', () => {
      expect(validateField(textField('t', { required: true }), '   ')).toBeTruthy();
    });

    it('fails for null on required number field', () => {
      expect(validateField(numberField('n', { required: true }), null)).toBeTruthy();
    });

    it('passes for false on required bool field', () => {
      expect(validateField(boolField('b', true), false)).toBeNull();
    });

    it('fails for empty array on required multiSelect field', () => {
      expect(validateField(multiSelectField('ms', ['a', 'b'], 0, true), [])).toBeTruthy();
    });

    it('fails for empty string on required select field', () => {
      expect(validateField(selectField('s', ['a', 'b'], true), '')).toBeTruthy();
    });

    it('fails for null on required select field', () => {
      expect(validateField(selectField('s', ['a', 'b'], true), null)).toBeTruthy();
    });

    it('passes for non-empty value on required text field', () => {
      expect(validateField(textField('t', { required: true }), 'hello')).toBeNull();
    });
  });

  // ── Text ──────────────────────────────────────────────────────────────

  describe('text validation', () => {
    it('fails if shorter than minLength', () => {
      const err = validateField(textField('t', { minLength: 5 }), 'abc');
      expect(err).toContain('at least 5');
    });

    it('fails if longer than maxLength', () => {
      const err = validateField(textField('t', { maxLength: 3 }), 'abcdef');
      expect(err).toContain('at most 3');
    });

    it('fails if pattern does not match', () => {
      const err = validateField(textField('t', { pattern: '^[a-z]+$' }), 'ABC123');
      expect(err).toContain('pattern');
    });

    it('passes if pattern matches', () => {
      expect(validateField(textField('t', { pattern: '^[a-z]+$' }), 'abc')).toBeNull();
    });

    it('skips validation for empty non-required text', () => {
      expect(validateField(textField('t'), '')).toBeNull();
    });
  });

  // ── Number ────────────────────────────────────────────────────────────

  describe('number validation', () => {
    it('fails for non-numeric value', () => {
      const err = validateField(numberField('n'), 'abc');
      expect(err).toContain('valid number');
    });

    it('fails for decimal when noDecimal is true', () => {
      const err = validateField(numberField('n', { noDecimal: true }), 3.14);
      expect(err).toContain('whole number');
    });

    it('fails when below min', () => {
      const err = validateField(numberField('n', { min: 10 }), 5);
      expect(err).toContain('at least 10');
    });

    it('fails when above max', () => {
      const err = validateField(numberField('n', { max: 100 }), 150);
      expect(err).toContain('at most 100');
    });

    it('passes for valid number within range', () => {
      expect(validateField(numberField('n', { min: 0, max: 100 }), 50)).toBeNull();
    });

    it('passes for integer when noDecimal is true', () => {
      expect(validateField(numberField('n', { noDecimal: true }), 42)).toBeNull();
    });
  });

  // ── Email ─────────────────────────────────────────────────────────────

  describe('email validation', () => {
    it('fails for invalid email', () => {
      const err = validateField(emailField('e'), 'not-an-email');
      expect(err).toContain('valid email');
    });

    it('passes for valid email', () => {
      expect(validateField(emailField('e'), 'user@example.com')).toBeNull();
    });

    it('skips validation for empty non-required email', () => {
      expect(validateField(emailField('e'), '')).toBeNull();
    });
  });

  // ── URL ───────────────────────────────────────────────────────────────

  describe('url validation', () => {
    it('fails for invalid URL', () => {
      const err = validateField(urlField('u'), 'not-a-url');
      expect(err).toContain('valid URL');
    });

    it('passes for valid URL', () => {
      expect(validateField(urlField('u'), 'https://example.com')).toBeNull();
    });
  });

  // ── DateTime ──────────────────────────────────────────────────────────

  describe('dateTime validation', () => {
    it('fails for invalid date', () => {
      const err = validateField(dateTimeField('d'), 'not-a-date');
      expect(err).toContain('valid date');
    });

    it('passes for valid ISO date', () => {
      expect(validateField(dateTimeField('d'), '2024-01-15T10:00:00Z')).toBeNull();
    });
  });

  // ── MultiSelect ───────────────────────────────────────────────────────

  describe('multiSelect validation', () => {
    it('fails when exceeding maxSelect', () => {
      const err = validateField(multiSelectField('ms', ['a', 'b', 'c'], 2), ['a', 'b', 'c']);
      expect(err).toContain('most 2');
    });

    it('passes when within maxSelect limit', () => {
      expect(validateField(multiSelectField('ms', ['a', 'b', 'c'], 3), ['a', 'b'])).toBeNull();
    });

    it('passes when maxSelect is 0 (unlimited)', () => {
      expect(validateField(multiSelectField('ms', ['a', 'b', 'c'], 0), ['a', 'b', 'c'])).toBeNull();
    });
  });

  // ── Editor ────────────────────────────────────────────────────────────

  describe('editor validation', () => {
    it('fails when exceeding maxLength', () => {
      const err = validateField(editorField('e', 10), 'a'.repeat(15));
      expect(err).toContain('at most 10');
    });

    it('passes when within maxLength', () => {
      expect(validateField(editorField('e', 10), 'short')).toBeNull();
    });
  });
});
