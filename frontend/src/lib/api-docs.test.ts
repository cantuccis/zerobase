import { describe, it, expect } from 'vitest';
import {
  generateEndpointDocs,
  generateCurlExample,
  formatAccessRule,
  exampleValueForType,
  FILTER_OPERATORS,
} from './api-docs';
import type { Collection, Field, FieldType } from './api/types';

// ── Helpers ──────────────────────────────────────────────────────────────────

function makeField(name: string, fieldType: FieldType, overrides?: Partial<Field>): Field {
  return {
    id: `f_${name}`,
    name,
    type: fieldType,
    required: false,
    unique: false,
    sortOrder: 0,
    ...overrides,
  };
}

function makeCollection(overrides?: Partial<Collection>): Collection {
  return {
    id: 'col_test',
    name: 'posts',
    type: 'base',
    fields: [
      makeField('title', { type: 'text', options: { minLength: 0, maxLength: 500, pattern: null, searchable: true } }),
      makeField('views', { type: 'number', options: { min: null, max: null, noDecimal: true } }),
    ],
    rules: {
      listRule: null,
      viewRule: null,
      createRule: null,
      updateRule: null,
      deleteRule: null,
    },
    ...overrides,
  };
}

// ── exampleValueForType ─────────────────────────────────────────────────────

describe('exampleValueForType', () => {
  it('returns string for text type', () => {
    expect(exampleValueForType({ type: 'text', options: { minLength: 0, maxLength: 100, pattern: null, searchable: false } })).toBe('example text');
  });

  it('returns integer for noDecimal number', () => {
    expect(exampleValueForType({ type: 'number', options: { min: null, max: null, noDecimal: true } })).toBe(42);
  });

  it('returns decimal for regular number', () => {
    expect(exampleValueForType({ type: 'number', options: { min: null, max: null, noDecimal: false } })).toBe(3.14);
  });

  it('returns boolean for bool type', () => {
    expect(exampleValueForType({ type: 'bool', options: {} })).toBe(true);
  });

  it('returns email string for email type', () => {
    expect(exampleValueForType({ type: 'email', options: { exceptDomains: [], onlyDomains: [] } })).toBe('user@example.com');
  });

  it('returns URL for url type', () => {
    expect(exampleValueForType({ type: 'url', options: { exceptDomains: [], onlyDomains: [] } })).toBe('https://example.com');
  });

  it('returns ISO date string for dateTime type', () => {
    expect(exampleValueForType({ type: 'dateTime', options: { min: '', max: '' } })).toContain('2024');
  });

  it('returns first value for select type with values', () => {
    expect(exampleValueForType({ type: 'select', options: { values: ['draft', 'published'] } })).toBe('draft');
  });

  it('returns fallback for select type without values', () => {
    expect(exampleValueForType({ type: 'select', options: { values: [] } })).toBe('option1');
  });

  it('returns array for multiSelect type', () => {
    expect(exampleValueForType({ type: 'multiSelect', options: { values: ['a', 'b'], maxSelect: 3 } })).toEqual(['a']);
  });

  it('returns filename for file type', () => {
    expect(exampleValueForType({ type: 'file', options: { maxSize: 1000, maxSelect: 1, mimeTypes: [], thumbs: [] } })).toBe('filename.png');
  });

  it('returns RECORD_ID for relation type', () => {
    expect(exampleValueForType({ type: 'relation', options: { collectionId: 'c1', cascadeDelete: false, maxSelect: null } })).toBe('RECORD_ID');
  });

  it('returns object for json type', () => {
    expect(exampleValueForType({ type: 'json', options: { maxSize: 1000 } })).toEqual({ key: 'value' });
  });

  it('returns HTML string for editor type', () => {
    expect(exampleValueForType({ type: 'editor', options: { maxLength: 5000, searchable: true } })).toContain('<p>');
  });

  it('returns date for autoDate type', () => {
    expect(exampleValueForType({ type: 'autoDate', options: { onCreate: true, onUpdate: false } })).toContain('2024');
  });
});

// ── generateEndpointDocs ────────────────────────────────────────────────────

describe('generateEndpointDocs', () => {
  it('generates 5 endpoints for base collection', () => {
    const col = makeCollection();
    const docs = generateEndpointDocs(col);
    expect(docs).toHaveLength(5);
  });

  it('generates CRUD endpoints with correct methods', () => {
    const col = makeCollection();
    const docs = generateEndpointDocs(col);
    const methods = docs.map((d) => `${d.method} ${d.description}`);

    expect(methods).toContain('GET List records');
    expect(methods).toContain('GET View record');
    expect(methods).toContain('POST Create record');
    expect(methods).toContain('PATCH Update record');
    expect(methods).toContain('DELETE Delete record');
  });

  it('uses collection name in paths', () => {
    const col = makeCollection({ name: 'tasks' });
    const docs = generateEndpointDocs(col);

    expect(docs[0].path).toBe('/api/collections/tasks/records');
  });

  it('uses :collection placeholder when name is empty', () => {
    const col = makeCollection({ name: '' });
    const docs = generateEndpointDocs(col);

    expect(docs[0].path).toBe('/api/collections/:collection/records');
  });

  it('includes query params for list endpoint', () => {
    const col = makeCollection();
    const docs = generateEndpointDocs(col);
    const listEndpoint = docs.find((d) => d.description === 'List records')!;

    expect(listEndpoint.queryParams.length).toBeGreaterThan(0);
    const paramNames = listEndpoint.queryParams.map((p) => p.name);
    expect(paramNames).toContain('page');
    expect(paramNames).toContain('perPage');
    expect(paramNames).toContain('sort');
    expect(paramNames).toContain('filter');
    expect(paramNames).toContain('fields');
    expect(paramNames).toContain('expand');
  });

  it('includes fields and expand params for view endpoint', () => {
    const col = makeCollection();
    const docs = generateEndpointDocs(col);
    const viewEndpoint = docs.find((d) => d.description === 'View record')!;

    const paramNames = viewEndpoint.queryParams.map((p) => p.name);
    expect(paramNames).toContain('fields');
    expect(paramNames).toContain('expand');
    expect(paramNames).not.toContain('page');
  });

  it('has no query params for create/update/delete', () => {
    const col = makeCollection();
    const docs = generateEndpointDocs(col);

    const createEndpoint = docs.find((d) => d.description === 'Create record')!;
    const updateEndpoint = docs.find((d) => d.description === 'Update record')!;
    const deleteEndpoint = docs.find((d) => d.description === 'Delete record')!;

    expect(createEndpoint.queryParams).toHaveLength(0);
    expect(updateEndpoint.queryParams).toHaveLength(0);
    expect(deleteEndpoint.queryParams).toHaveLength(0);
  });

  it('includes request examples for create and update', () => {
    const col = makeCollection();
    const docs = generateEndpointDocs(col);

    const createEndpoint = docs.find((d) => d.description === 'Create record')!;
    const updateEndpoint = docs.find((d) => d.description === 'Update record')!;

    expect(createEndpoint.requestExample).not.toBeNull();
    expect(updateEndpoint.requestExample).not.toBeNull();

    const createBody = JSON.parse(createEndpoint.requestExample!);
    expect(createBody).toHaveProperty('title');
    expect(createBody).toHaveProperty('views');
  });

  it('does not include autoDate fields in request examples', () => {
    const col = makeCollection({
      fields: [
        makeField('title', { type: 'text', options: { minLength: 0, maxLength: 100, pattern: null, searchable: false } }),
        makeField('createdAt', { type: 'autoDate', options: { onCreate: true, onUpdate: false } }),
      ],
    });
    const docs = generateEndpointDocs(col);
    const createEndpoint = docs.find((d) => d.description === 'Create record')!;
    const body = JSON.parse(createEndpoint.requestExample!);

    expect(body).toHaveProperty('title');
    expect(body).not.toHaveProperty('createdAt');
  });

  it('has no request examples for list, view, and delete', () => {
    const col = makeCollection();
    const docs = generateEndpointDocs(col);

    const listEndpoint = docs.find((d) => d.description === 'List records')!;
    const viewEndpoint = docs.find((d) => d.description === 'View record')!;
    const deleteEndpoint = docs.find((d) => d.description === 'Delete record')!;

    expect(listEndpoint.requestExample).toBeNull();
    expect(viewEndpoint.requestExample).toBeNull();
    expect(deleteEndpoint.requestExample).toBeNull();
  });

  it('includes response examples as valid JSON', () => {
    const col = makeCollection();
    const docs = generateEndpointDocs(col);

    for (const endpoint of docs) {
      expect(() => JSON.parse(endpoint.responseExample)).not.toThrow();
    }
  });

  it('response example for list includes pagination wrapper', () => {
    const col = makeCollection();
    const docs = generateEndpointDocs(col);
    const listEndpoint = docs.find((d) => d.description === 'List records')!;
    const response = JSON.parse(listEndpoint.responseExample);

    expect(response).toHaveProperty('page');
    expect(response).toHaveProperty('perPage');
    expect(response).toHaveProperty('totalPages');
    expect(response).toHaveProperty('totalItems');
    expect(response).toHaveProperty('items');
  });

  it('response example for view includes record fields', () => {
    const col = makeCollection();
    const docs = generateEndpointDocs(col);
    const viewEndpoint = docs.find((d) => d.description === 'View record')!;
    const response = JSON.parse(viewEndpoint.responseExample);

    expect(response).toHaveProperty('id');
    expect(response).toHaveProperty('collectionId');
    expect(response).toHaveProperty('collectionName', 'posts');
    expect(response).toHaveProperty('title');
    expect(response).toHaveProperty('views');
  });

  it('maps access rules from collection rules', () => {
    const col = makeCollection({
      rules: {
        listRule: '',
        viewRule: '',
        createRule: '@request.auth.id != ""',
        updateRule: '@request.auth.id != ""',
        deleteRule: null,
      },
    });
    const docs = generateEndpointDocs(col);

    const listEndpoint = docs.find((d) => d.description === 'List records')!;
    expect(listEndpoint.accessRule).toBe('');

    const createEndpoint = docs.find((d) => d.description === 'Create record')!;
    expect(createEndpoint.accessRule).toBe('@request.auth.id != ""');

    const deleteEndpoint = docs.find((d) => d.description === 'Delete record')!;
    expect(deleteEndpoint.accessRule).toBeNull();
  });

  // Auth collection tests
  it('generates additional auth endpoints for auth collections', () => {
    const col = makeCollection({ type: 'auth' });
    const docs = generateEndpointDocs(col);

    // 5 CRUD + 6 auth = 11
    expect(docs).toHaveLength(11);
  });

  it('includes auth-specific endpoint descriptions', () => {
    const col = makeCollection({ type: 'auth', name: 'users' });
    const docs = generateEndpointDocs(col);
    const descriptions = docs.map((d) => d.description);

    expect(descriptions).toContain('Auth with password');
    expect(descriptions).toContain('Refresh auth token');
    expect(descriptions).toContain('Request OTP');
    expect(descriptions).toContain('Auth with OTP');
    expect(descriptions).toContain('Request verification');
    expect(descriptions).toContain('Request password reset');
  });

  it('auth endpoints use correct paths', () => {
    const col = makeCollection({ type: 'auth', name: 'users' });
    const docs = generateEndpointDocs(col);

    const authWithPassword = docs.find((d) => d.description === 'Auth with password')!;
    expect(authWithPassword.path).toBe('/api/collections/users/auth-with-password');
    expect(authWithPassword.method).toBe('POST');
  });

  it('auth endpoints include request examples', () => {
    const col = makeCollection({ type: 'auth', name: 'users' });
    const docs = generateEndpointDocs(col);

    const authWithPassword = docs.find((d) => d.description === 'Auth with password')!;
    expect(authWithPassword.requestExample).not.toBeNull();
    const body = JSON.parse(authWithPassword.requestExample!);
    expect(body).toHaveProperty('identity');
    expect(body).toHaveProperty('password');
  });

  it('auth response includes email and verified fields', () => {
    const col = makeCollection({ type: 'auth', name: 'users' });
    const docs = generateEndpointDocs(col);
    const viewEndpoint = docs.find((d) => d.description === 'View record')!;
    const response = JSON.parse(viewEndpoint.responseExample);

    expect(response).toHaveProperty('email');
    expect(response).toHaveProperty('emailVisibility');
    expect(response).toHaveProperty('verified');
  });

  // View collections
  it('generates only 5 endpoints for view collections', () => {
    const col = makeCollection({ type: 'view' });
    const docs = generateEndpointDocs(col);
    expect(docs).toHaveLength(5);
  });
});

// ── generateCurlExample ──────────────────────────────────────────────────────

describe('generateCurlExample', () => {
  it('generates simple GET curl', () => {
    const col = makeCollection();
    const docs = generateEndpointDocs(col);
    const listEndpoint = docs.find((d) => d.description === 'List records')!;

    const curl = generateCurlExample(listEndpoint, 'http://localhost:8090');
    expect(curl).toContain('curl');
    expect(curl).toContain('"http://localhost:8090/api/collections/posts/records"');
    expect(curl).not.toContain('-X');
  });

  it('includes -X for non-GET methods', () => {
    const col = makeCollection();
    const docs = generateEndpointDocs(col);
    const createEndpoint = docs.find((d) => d.description === 'Create record')!;

    const curl = generateCurlExample(createEndpoint, 'http://localhost:8090');
    expect(curl).toContain('-X POST');
  });

  it('includes Content-Type and body for POST', () => {
    const col = makeCollection();
    const docs = generateEndpointDocs(col);
    const createEndpoint = docs.find((d) => d.description === 'Create record')!;

    const curl = generateCurlExample(createEndpoint, 'http://localhost:8090');
    expect(curl).toContain('Content-Type: application/json');
    expect(curl).toContain("-d '");
  });

  it('does not include body for DELETE', () => {
    const col = makeCollection();
    const docs = generateEndpointDocs(col);
    const deleteEndpoint = docs.find((d) => d.description === 'Delete record')!;

    const curl = generateCurlExample(deleteEndpoint, 'http://localhost:8090');
    expect(curl).toContain('-X DELETE');
    expect(curl).not.toContain("-d '");
  });
});

// ── formatAccessRule ─────────────────────────────────────────────────────────

describe('formatAccessRule', () => {
  it('returns "Superusers only" for null', () => {
    const result = formatAccessRule(null);
    expect(result.label).toBe('Superusers only');
  });

  it('returns "Public" for empty string', () => {
    const result = formatAccessRule('');
    expect(result.label).toBe('Public');
  });

  it('returns "Auth required" for undefined', () => {
    const result = formatAccessRule(undefined);
    expect(result.label).toBe('Auth required');
  });

  it('returns the rule expression for custom rules', () => {
    const result = formatAccessRule('@request.auth.id != ""');
    expect(result.label).toBe('@request.auth.id != ""');
  });

  it('uses red color for superusers only', () => {
    const result = formatAccessRule(null);
    expect(result.color).toContain('red');
  });

  it('uses green color for public', () => {
    const result = formatAccessRule('');
    expect(result.color).toContain('green');
  });

  it('uses yellow color for custom rules', () => {
    const result = formatAccessRule('@request.auth.id != ""');
    expect(result.color).toContain('yellow');
  });

  it('uses blue color for auth required', () => {
    const result = formatAccessRule(undefined);
    expect(result.color).toContain('blue');
  });
});

// ── FILTER_OPERATORS ────────────────────────────────────────────────────────

describe('FILTER_OPERATORS', () => {
  it('includes common operators', () => {
    const ops = FILTER_OPERATORS.map((o) => o.operator);
    expect(ops).toContain('=');
    expect(ops).toContain('!=');
    expect(ops).toContain('>');
    expect(ops).toContain('<');
    expect(ops).toContain('~');
    expect(ops).toContain('!~');
  });

  it('each operator has description and example', () => {
    for (const op of FILTER_OPERATORS) {
      expect(op.description).toBeTruthy();
      expect(op.example).toBeTruthy();
    }
  });
});
