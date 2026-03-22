/**
 * Auto-generated API documentation for collections.
 *
 * Pure functions that produce structured endpoint docs from a Collection definition.
 * No side effects — easy to test and reuse.
 */

import type { Collection, CollectionType, Field, FieldType } from './api/types';

// ── Types ────────────────────────────────────────────────────────────────────

export interface EndpointDoc {
  /** HTTP method. */
  method: 'GET' | 'POST' | 'PATCH' | 'DELETE';
  /** URL pattern (e.g. `/api/collections/posts/records`). */
  path: string;
  /** Short human-readable description. */
  description: string;
  /** Longer explanation of what the endpoint does. */
  details: string;
  /** Example request body (JSON string) or null. */
  requestExample: string | null;
  /** Example response body (JSON string). */
  responseExample: string;
  /** Query parameters available for this endpoint. */
  queryParams: QueryParamDoc[];
  /** Access rule expression (null = superusers only). */
  accessRule: string | null | undefined;
}

export interface QueryParamDoc {
  name: string;
  type: string;
  description: string;
  example: string;
}

export interface FilterDoc {
  operator: string;
  description: string;
  example: string;
}

// ── Constants ────────────────────────────────────────────────────────────────

const COMMON_QUERY_PARAMS: QueryParamDoc[] = [
  { name: 'page', type: 'number', description: 'Page number (1-based).', example: '?page=2' },
  { name: 'perPage', type: 'number', description: 'Items per page (default 30, max 500).', example: '?perPage=50' },
  { name: 'sort', type: 'string', description: 'Sort field with optional - prefix for DESC.', example: '?sort=-created' },
  { name: 'filter', type: 'string', description: 'Filter expression using PocketBase syntax.', example: '?filter=(status="active")' },
  { name: 'fields', type: 'string', description: 'Comma-separated list of fields to return.', example: '?fields=id,title,created' },
  { name: 'expand', type: 'string', description: 'Comma-separated relation fields to expand.', example: '?expand=author,tags' },
];

const SINGLE_QUERY_PARAMS: QueryParamDoc[] = [
  { name: 'fields', type: 'string', description: 'Comma-separated list of fields to return.', example: '?fields=id,title,created' },
  { name: 'expand', type: 'string', description: 'Comma-separated relation fields to expand.', example: '?expand=author,tags' },
];

export const FILTER_OPERATORS: FilterDoc[] = [
  { operator: '=', description: 'Equal', example: 'status = "active"' },
  { operator: '!=', description: 'Not equal', example: 'status != "draft"' },
  { operator: '>', description: 'Greater than', example: 'count > 10' },
  { operator: '>=', description: 'Greater than or equal', example: 'count >= 10' },
  { operator: '<', description: 'Less than', example: 'count < 100' },
  { operator: '<=', description: 'Less than or equal', example: 'count <= 100' },
  { operator: '~', description: 'Contains (like "%value%")', example: 'title ~ "hello"' },
  { operator: '!~', description: 'Does not contain', example: 'title !~ "draft"' },
  { operator: '?=', description: 'Any/has equal (for multi-value fields)', example: 'tags ?= "news"' },
  { operator: '?!=', description: 'Any/has not equal', example: 'tags ?!= "spam"' },
  { operator: '?>', description: 'Any/has greater than', example: 'scores ?> 90' },
  { operator: '?<', description: 'Any/has less than', example: 'scores ?< 10' },
  { operator: '?~', description: 'Any/has contains', example: 'tags ?~ "new"' },
  { operator: '?!~', description: 'Any/has does not contain', example: 'tags ?!~ "old"' },
];

// ── Field helpers ────────────────────────────────────────────────────────────

/** Return a sensible example value for a field type. */
export function exampleValueForType(ft: FieldType): unknown {
  switch (ft.type) {
    case 'text':
      return 'example text';
    case 'number':
      return ft.options.noDecimal ? 42 : 3.14;
    case 'bool':
      return true;
    case 'email':
      return 'user@example.com';
    case 'url':
      return 'https://example.com';
    case 'dateTime':
      return '2024-01-15 12:00:00.000Z';
    case 'select':
      return ft.options.values.length > 0 ? ft.options.values[0] : 'option1';
    case 'multiSelect':
      return ft.options.values.length > 0 ? [ft.options.values[0]] : ['option1'];
    case 'autoDate':
      return '2024-01-15 12:00:00.000Z';
    case 'file':
      return 'filename.png';
    case 'relation':
      return 'RECORD_ID';
    case 'json':
      return { key: 'value' };
    case 'editor':
      return '<p>Rich text content</p>';
  }
}

/** Build example record body from collection fields. */
function buildExampleRecord(collection: Collection): Record<string, unknown> {
  const rec: Record<string, unknown> = {
    id: 'RECORD_ID',
    collectionId: collection.id,
    collectionName: collection.name,
    created: '2024-01-15 12:00:00.000Z',
    updated: '2024-01-15 12:00:00.000Z',
  };

  for (const field of collection.fields) {
    if (field.type.type === 'autoDate') continue;
    rec[field.name] = exampleValueForType(field.type);
  }

  if (collection.type === 'auth') {
    rec.email = 'user@example.com';
    rec.emailVisibility = true;
    rec.verified = false;
  }

  return rec;
}

/** Build example create/update body (writable fields only). */
function buildExampleWriteBody(fields: Field[], mode: 'create' | 'update'): Record<string, unknown> {
  const body: Record<string, unknown> = {};
  for (const field of fields) {
    if (field.type.type === 'autoDate') continue;
    body[field.name] = exampleValueForType(field.type);
  }
  return body;
}

// ── Endpoint generation ──────────────────────────────────────────────────────

/** Generate all endpoint docs for a collection. */
export function generateEndpointDocs(collection: Collection): EndpointDoc[] {
  const name = collection.name || ':collection';
  const basePath = `/api/collections/${name}/records`;
  const exampleRecord = buildExampleRecord(collection);
  const exampleWriteBody = buildExampleWriteBody(collection.fields, 'create');
  const rules = collection.rules;

  const endpoints: EndpointDoc[] = [
    // List records
    {
      method: 'GET',
      path: basePath,
      description: 'List records',
      details: `Returns a paginated list of ${name} records. Supports filtering, sorting, field selection, and relation expansion.`,
      requestExample: null,
      responseExample: JSON.stringify(
        {
          page: 1,
          perPage: 30,
          totalPages: 1,
          totalItems: 2,
          items: [exampleRecord],
        },
        null,
        2,
      ),
      queryParams: COMMON_QUERY_PARAMS,
      accessRule: rules.listRule,
    },
    // View record
    {
      method: 'GET',
      path: `${basePath}/:id`,
      description: 'View record',
      details: `Returns a single ${name} record by its ID.`,
      requestExample: null,
      responseExample: JSON.stringify(exampleRecord, null, 2),
      queryParams: SINGLE_QUERY_PARAMS,
      accessRule: rules.viewRule,
    },
    // Create record
    {
      method: 'POST',
      path: basePath,
      description: 'Create record',
      details: `Creates a new ${name} record. Send fields as JSON body or multipart form-data (for file uploads).`,
      requestExample: JSON.stringify(exampleWriteBody, null, 2),
      responseExample: JSON.stringify(exampleRecord, null, 2),
      queryParams: [],
      accessRule: rules.createRule,
    },
    // Update record
    {
      method: 'PATCH',
      path: `${basePath}/:id`,
      description: 'Update record',
      details: `Updates an existing ${name} record. Only include fields you want to change.`,
      requestExample: JSON.stringify(exampleWriteBody, null, 2),
      responseExample: JSON.stringify(exampleRecord, null, 2),
      queryParams: [],
      accessRule: rules.updateRule,
    },
    // Delete record
    {
      method: 'DELETE',
      path: `${basePath}/:id`,
      description: 'Delete record',
      details: `Permanently deletes a single ${name} record by its ID.`,
      requestExample: null,
      responseExample: JSON.stringify(null),
      queryParams: [],
      accessRule: rules.deleteRule,
    },
  ];

  // Auth-specific endpoints
  if (collection.type === 'auth') {
    const authBase = `/api/collections/${name}`;
    endpoints.push(
      {
        method: 'POST',
        path: `${authBase}/auth-with-password`,
        description: 'Auth with password',
        details: `Authenticates a ${name} record with email/username and password. Returns a JWT token and the authenticated record.`,
        requestExample: JSON.stringify({ identity: 'user@example.com', password: 'securepassword' }, null, 2),
        responseExample: JSON.stringify({ token: 'JWT_TOKEN', record: exampleRecord }, null, 2),
        queryParams: [],
        accessRule: undefined,
      },
      {
        method: 'POST',
        path: `${authBase}/auth-refresh`,
        description: 'Refresh auth token',
        details: 'Refreshes the current JWT token. Requires a valid auth token in the Authorization header.',
        requestExample: null,
        responseExample: JSON.stringify({ token: 'NEW_JWT_TOKEN', record: exampleRecord }, null, 2),
        queryParams: [],
        accessRule: undefined,
      },
      {
        method: 'POST',
        path: `${authBase}/request-otp`,
        description: 'Request OTP',
        details: `Sends a one-time password to the specified ${name} email address.`,
        requestExample: JSON.stringify({ email: 'user@example.com' }, null, 2),
        responseExample: JSON.stringify({ otpId: 'OTP_ID' }, null, 2),
        queryParams: [],
        accessRule: undefined,
      },
      {
        method: 'POST',
        path: `${authBase}/auth-with-otp`,
        description: 'Auth with OTP',
        details: 'Authenticates using a previously requested one-time password.',
        requestExample: JSON.stringify({ otpId: 'OTP_ID', password: '123456' }, null, 2),
        responseExample: JSON.stringify({ token: 'JWT_TOKEN', record: exampleRecord }, null, 2),
        queryParams: [],
        accessRule: undefined,
      },
      {
        method: 'POST',
        path: `${authBase}/request-verification`,
        description: 'Request verification',
        details: `Sends a verification email to the specified ${name} email address.`,
        requestExample: JSON.stringify({ email: 'user@example.com' }, null, 2),
        responseExample: JSON.stringify(null),
        queryParams: [],
        accessRule: undefined,
      },
      {
        method: 'POST',
        path: `${authBase}/request-password-reset`,
        description: 'Request password reset',
        details: `Sends a password reset email to the specified ${name} email address.`,
        requestExample: JSON.stringify({ email: 'user@example.com' }, null, 2),
        responseExample: JSON.stringify(null),
        queryParams: [],
        accessRule: undefined,
      },
    );
  }

  return endpoints;
}

/** Generate a curl example for an endpoint. */
export function generateCurlExample(endpoint: EndpointDoc, baseUrl: string): string {
  const parts: string[] = ['curl'];

  if (endpoint.method !== 'GET') {
    parts.push(`-X ${endpoint.method}`);
  }

  parts.push(`"${baseUrl}${endpoint.path}"`);

  if (endpoint.requestExample) {
    parts.push('-H "Content-Type: application/json"');
    // Compact the JSON for curl
    const compact = JSON.stringify(JSON.parse(endpoint.requestExample));
    parts.push(`-d '${compact}'`);
  }

  return parts.join(' \\\n  ');
}

/** Format access rule for display. */
export function formatAccessRule(rule: string | null | undefined): { label: string; color: string } {
  if (rule === undefined) {
    return { label: 'Auth required', color: 'bg-blue-100 text-blue-700' };
  }
  if (rule === null) {
    return { label: 'Superusers only', color: 'bg-red-100 text-red-700' };
  }
  if (rule === '') {
    return { label: 'Public', color: 'bg-green-100 text-green-700' };
  }
  return { label: rule, color: 'bg-yellow-100 text-yellow-700' };
}
