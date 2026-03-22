import { useState, useCallback, useRef, useEffect } from 'react';
import type { ApiRules } from '../../lib/api/types';

// ── Types ───────────────────────────────────────────────────────────────────

export interface RulesEditorProps {
  rules: ApiRules;
  onChange: (rules: ApiRules) => void;
  collectionType?: 'base' | 'auth' | 'view';
}

interface RuleFieldDef {
  key: keyof ApiRules;
  label: string;
  description: string;
}

interface ValidationError {
  message: string;
  position?: number;
}

// ── Constants ───────────────────────────────────────────────────────────────

const RULE_FIELDS: RuleFieldDef[] = [
  {
    key: 'listRule',
    label: 'List Rule',
    description: 'Filter applied when listing records. Acts as both access gate and result filter.',
  },
  {
    key: 'viewRule',
    label: 'View Rule',
    description: 'Filter applied when viewing a single record.',
  },
  {
    key: 'createRule',
    label: 'Create Rule',
    description: 'Condition for creating new records.',
  },
  {
    key: 'updateRule',
    label: 'Update Rule',
    description: 'Condition for updating existing records.',
  },
  {
    key: 'deleteRule',
    label: 'Delete Rule',
    description: 'Condition for deleting records.',
  },
  {
    key: 'manageRule',
    label: 'Manage Rule',
    description: 'Grants full CRUD access when matched, bypassing all individual operation rules. Enables delegated administration.',
  },
];

const RULE_PRESETS = [
  { label: 'Locked', value: null as string | null, description: 'Superusers only' },
  { label: 'Public', value: '', description: 'Open to everyone' },
  { label: 'Authenticated', value: '@request.auth.id != ""', description: 'Any logged-in user' },
  { label: 'Owner only', value: '@request.auth.id = id', description: 'Record owner' },
];

// ── Syntax highlighting tokens ──────────────────────────────────────────────

interface HighlightToken {
  text: string;
  className: string;
}

const OPERATORS = new Set(['=', '!=', '>', '>=', '<', '<=', '~', '!~']);
const LOGICAL_KEYWORDS = new Set(['&&', '||', 'AND', 'OR', 'and', 'or']);
const LITERAL_KEYWORDS = new Set(['true', 'false', 'null']);

function tokenize(expr: string): HighlightToken[] {
  const tokens: HighlightToken[] = [];
  const chars = expr;
  let i = 0;

  while (i < chars.length) {
    // Whitespace
    if (/\s/.test(chars[i])) {
      let start = i;
      while (i < chars.length && /\s/.test(chars[i])) i++;
      tokens.push({ text: chars.slice(start, i), className: '' });
      continue;
    }

    // Strings (single or double quoted)
    if (chars[i] === '"' || chars[i] === "'") {
      const quote = chars[i];
      let start = i;
      i++;
      while (i < chars.length && chars[i] !== quote) {
        if (chars[i] === '\\') i++;
        i++;
      }
      if (i < chars.length) i++; // closing quote
      tokens.push({ text: chars.slice(start, i), className: 'rule-string' });
      continue;
    }

    // @-prefixed macros (@request.auth.id, @now, etc.)
    if (chars[i] === '@') {
      let start = i;
      i++;
      while (i < chars.length && /[a-zA-Z0-9_.]/.test(chars[i])) i++;
      tokens.push({ text: chars.slice(start, i), className: 'rule-macro' });
      continue;
    }

    // Multi-character operators: &&, ||, !=, >=, <=, !~
    if (i + 1 < chars.length) {
      const two = chars.slice(i, i + 2);
      if (LOGICAL_KEYWORDS.has(two)) {
        tokens.push({ text: two, className: 'rule-logical' });
        i += 2;
        continue;
      }
      if (OPERATORS.has(two)) {
        tokens.push({ text: two, className: 'rule-operator' });
        i += 2;
        continue;
      }
    }

    // Single-character operators: =, >, <, ~
    if (OPERATORS.has(chars[i])) {
      tokens.push({ text: chars[i], className: 'rule-operator' });
      i++;
      continue;
    }

    // Parentheses
    if (chars[i] === '(' || chars[i] === ')') {
      tokens.push({ text: chars[i], className: 'rule-paren' });
      i++;
      continue;
    }

    // Numbers
    if (/[0-9]/.test(chars[i])) {
      let start = i;
      while (i < chars.length && /[0-9.]/.test(chars[i])) i++;
      tokens.push({ text: chars.slice(start, i), className: 'rule-number' });
      continue;
    }

    // Identifiers / keywords
    if (/[a-zA-Z_]/.test(chars[i])) {
      let start = i;
      while (i < chars.length && /[a-zA-Z0-9_.]/.test(chars[i])) i++;
      const word = chars.slice(start, i);

      if (LOGICAL_KEYWORDS.has(word)) {
        tokens.push({ text: word, className: 'rule-logical' });
      } else if (LITERAL_KEYWORDS.has(word)) {
        tokens.push({ text: word, className: 'rule-literal' });
      } else {
        tokens.push({ text: word, className: 'rule-field' });
      }
      continue;
    }

    // Anything else
    tokens.push({ text: chars[i], className: '' });
    i++;
  }

  return tokens;
}

// ── Validation ──────────────────────────────────────────────────────────────

export function validateRuleExpression(expr: string): ValidationError | null {
  if (expr.trim() === '') return null; // empty = public access, valid

  const trimmed = expr.trim();

  // Check for balanced quotes
  let inString = false;
  let stringChar = '';
  for (let i = 0; i < trimmed.length; i++) {
    const ch = trimmed[i];
    if (inString) {
      if (ch === '\\') { i++; continue; }
      if (ch === stringChar) inString = false;
    } else {
      if (ch === '"' || ch === "'") {
        inString = true;
        stringChar = ch;
      }
    }
  }
  if (inString) {
    return { message: 'Unterminated string literal.' };
  }

  // Check for balanced parentheses
  let parenDepth = 0;
  for (let i = 0; i < trimmed.length; i++) {
    if (trimmed[i] === '"' || trimmed[i] === "'") {
      const q = trimmed[i];
      i++;
      while (i < trimmed.length && trimmed[i] !== q) {
        if (trimmed[i] === '\\') i++;
        i++;
      }
      continue;
    }
    if (trimmed[i] === '(') parenDepth++;
    if (trimmed[i] === ')') parenDepth--;
    if (parenDepth < 0) {
      return { message: 'Unexpected closing parenthesis.', position: i };
    }
  }
  if (parenDepth > 0) {
    return { message: `Unclosed parenthesis (${parenDepth} open).` };
  }

  // Check for empty comparisons (e.g., "= value" or "field =")
  if (/^[=!<>~]/.test(trimmed)) {
    return { message: 'Expression cannot start with an operator.' };
  }
  if (/[=!<>~]$/.test(trimmed) && !trimmed.endsWith('"') && !trimmed.endsWith("'")) {
    return { message: 'Expression cannot end with an operator.' };
  }

  // Check for consecutive operators
  if (/[=!<>~]{3,}/.test(trimmed)) {
    return { message: 'Invalid operator sequence.' };
  }

  // Check that @-prefixed macros reference known roots
  const macroPattern = /@([a-zA-Z_][a-zA-Z0-9_.]*)/g;
  let match;
  const knownRoots = new Set(['request', 'collection', 'now']);
  while ((match = macroPattern.exec(trimmed)) !== null) {
    const root = match[1].split('.')[0];
    if (!knownRoots.has(root)) {
      return {
        message: `Unknown variable "@${root}". Available: @request, @collection, @now.`,
        position: match.index,
      };
    }
  }

  return null;
}

// ── Highlighted Input ───────────────────────────────────────────────────────

interface HighlightedRuleInputProps {
  value: string;
  onChange: (value: string) => void;
  error?: ValidationError | null;
  testId: string;
}

function HighlightedRuleInput({ value, onChange, error, testId }: HighlightedRuleInputProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const highlightRef = useRef<HTMLDivElement>(null);

  // Sync scroll between textarea and highlight overlay
  const handleScroll = useCallback(() => {
    if (textareaRef.current && highlightRef.current) {
      highlightRef.current.scrollLeft = textareaRef.current.scrollLeft;
    }
  }, []);

  const tokens = tokenize(value);

  return (
    <div className="relative">
      <div className="relative overflow-hidden rounded-md border border-gray-300 dark:border-gray-600 focus-within:border-blue-500 focus-within:ring-1 focus-within:ring-blue-500">
        {/* Syntax highlight overlay */}
        <div
          ref={highlightRef}
          className="pointer-events-none absolute inset-0 overflow-hidden whitespace-pre px-3 py-2 font-mono text-sm"
          aria-hidden="true"
          data-testid={`${testId}-highlight`}
        >
          {tokens.map((token, i) => (
            <span key={i} className={token.className}>
              {token.text}
            </span>
          ))}
        </div>
        {/* Actual textarea (transparent text, visible caret) */}
        <textarea
          ref={textareaRef}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onScroll={handleScroll}
          className="relative w-full resize-none bg-transparent px-3 py-2 font-mono text-sm text-transparent caret-gray-900 dark:caret-gray-100 outline-none"
          rows={1}
          spellCheck={false}
          autoComplete="off"
          data-testid={testId}
          aria-invalid={error ? 'true' : undefined}
          aria-describedby={error ? `${testId}-error` : undefined}
        />
      </div>
      {error && (
        <p
          id={`${testId}-error`}
          className="mt-1 text-xs text-red-600 dark:text-red-400"
          role="alert"
          data-testid={`${testId}-error`}
        >
          {error.message}
        </p>
      )}
    </div>
  );
}

// ── Helper Documentation ────────────────────────────────────────────────────

const HELPER_SECTIONS = [
  {
    title: 'Request Variables',
    items: [
      { variable: '@request.auth.id', description: 'ID of the authenticated user (empty if unauthenticated)' },
      { variable: '@request.auth.email', description: 'Email of the authenticated user' },
      { variable: '@request.auth.verified', description: 'Whether the user\'s email is verified' },
      { variable: '@request.auth.collectionId', description: 'Collection ID of the auth record' },
      { variable: '@request.auth.collectionName', description: 'Collection name of the auth record' },
      { variable: '@request.data.*', description: 'Fields from the incoming request body' },
      { variable: '@request.query.*', description: 'URL query parameters' },
      { variable: '@request.headers.*', description: 'Request headers' },
      { variable: '@request.method', description: 'HTTP method (GET, POST, PATCH, DELETE)' },
    ],
  },
  {
    title: 'Record Fields',
    items: [
      { variable: 'id', description: 'Record ID' },
      { variable: 'created', description: 'Record creation timestamp' },
      { variable: 'updated', description: 'Record last update timestamp' },
      { variable: '<fieldName>', description: 'Any field defined in the collection schema' },
      { variable: '<relation>.<field>', description: 'Dot-notation to traverse relations' },
    ],
  },
  {
    title: 'Other Variables',
    items: [
      { variable: '@collection.<name>.*', description: 'Reference records from another collection' },
      { variable: '@now', description: 'Current date/time' },
    ],
  },
  {
    title: 'Operators',
    items: [
      { variable: '=, !=', description: 'Equality / inequality' },
      { variable: '>, >=, <, <=', description: 'Numeric / date comparison' },
      { variable: '~, !~', description: 'LIKE / NOT LIKE (use % as wildcard)' },
      { variable: '&&, ||', description: 'Logical AND / OR' },
      { variable: '()', description: 'Grouping' },
    ],
  },
  {
    title: 'Examples',
    items: [
      { variable: '@request.auth.id != ""', description: 'Any authenticated user' },
      { variable: '@request.auth.id = id', description: 'Only the record owner' },
      { variable: 'status = "published"', description: 'Only published records' },
      { variable: '@request.auth.role = "admin"', description: 'Only admin users' },
      { variable: '@request.auth.id = author.id', description: 'Author of the record (via relation)' },
    ],
  },
];

function RulesHelperDocs({ isOpen, onClose }: { isOpen: boolean; onClose: () => void }) {
  if (!isOpen) return null;

  return (
    <div
      className="rounded-lg border border-blue-200 dark:border-blue-800 bg-blue-50 dark:bg-blue-900/30 p-4"
      data-testid="rules-helper-docs"
    >
      <div className="mb-3 flex items-center justify-between">
        <h4 className="text-sm font-semibold text-blue-900 dark:text-blue-200">Rule Expression Reference</h4>
        <button
          type="button"
          onClick={onClose}
          className="rounded p-1 text-blue-600 dark:text-blue-400 hover:bg-blue-100 dark:hover:bg-blue-800"
          aria-label="Close reference"
          data-testid="rules-helper-close"
        >
          <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      </div>

      <div className="space-y-4">
        {HELPER_SECTIONS.map((section) => (
          <div key={section.title}>
            <h5 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-blue-800 dark:text-blue-300">
              {section.title}
            </h5>
            <div className="space-y-1">
              {section.items.map((item) => (
                <div key={item.variable} className="flex gap-3 text-xs">
                  <code className="shrink-0 rounded bg-blue-100 dark:bg-blue-900/20 px-1.5 py-0.5 font-mono text-blue-800 dark:text-blue-300">
                    {item.variable}
                  </code>
                  <span className="text-blue-700 dark:text-blue-400">{item.description}</span>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>

      <div className="mt-4 rounded-md bg-blue-100 dark:bg-blue-900/20 p-3">
        <h5 className="mb-1 text-xs font-semibold text-blue-900 dark:text-blue-200">Rule Values</h5>
        <ul className="space-y-1 text-xs text-blue-700 dark:text-blue-400">
          <li><strong>Locked (null)</strong> — Only superusers can perform this operation.</li>
          <li><strong>Empty string</strong> — Open to everyone, no restrictions.</li>
          <li><strong>Expression</strong> — Conditional access based on the request context.</li>
        </ul>
      </div>
    </div>
  );
}

// ── Main Component ──────────────────────────────────────────────────────────

export function RulesEditor({ rules, onChange, collectionType }: RulesEditorProps) {
  const [validationErrors, setValidationErrors] = useState<Record<string, ValidationError | null>>({});
  const [showHelperDocs, setShowHelperDocs] = useState(false);

  // Validate a single rule on change
  const validateRule = useCallback((key: string, value: string | null) => {
    if (value === null) {
      setValidationErrors((prev) => ({ ...prev, [key]: null }));
      return;
    }
    const error = validateRuleExpression(value);
    setValidationErrors((prev) => ({ ...prev, [key]: error }));
  }, []);

  // Toggle between locked (null) and unlocked (empty string)
  const handleToggleLock = useCallback(
    (key: keyof ApiRules) => {
      const currentValue = rules[key];
      const newValue = currentValue === null ? '' : null;
      onChange({ ...rules, [key]: newValue });
      validateRule(key, newValue);
    },
    [rules, onChange, validateRule],
  );

  // Update rule expression value
  const handleRuleChange = useCallback(
    (key: keyof ApiRules, value: string) => {
      onChange({ ...rules, [key]: value });
      validateRule(key, value);
    },
    [rules, onChange, validateRule],
  );

  // Apply a preset to a specific rule
  const handleApplyPreset = useCallback(
    (key: keyof ApiRules, presetValue: string | null) => {
      onChange({ ...rules, [key]: presetValue });
      validateRule(key, presetValue);
    },
    [rules, onChange, validateRule],
  );

  // Filter rule fields — hide manageRule for view collections
  const visibleFields = RULE_FIELDS.filter((f) => {
    if (f.key === 'manageRule' && collectionType === 'view') return false;
    return true;
  });

  const hasAnyError = Object.values(validationErrors).some((e) => e !== null);

  return (
    <section className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-6" data-testid="rules-editor">
      <div className="mb-4 flex items-center justify-between">
        <div>
          <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100">API Rules</h3>
          <p className="mt-0.5 text-xs text-gray-500 dark:text-gray-400">
            Control who can access this collection&apos;s API endpoints.
          </p>
        </div>
        <button
          type="button"
          onClick={() => setShowHelperDocs((prev) => !prev)}
          className="inline-flex items-center gap-1.5 rounded-md border border-gray-300 dark:border-gray-600 px-3 py-1.5 text-xs font-medium text-gray-700 dark:text-gray-300 transition-colors hover:bg-gray-50 dark:hover:bg-gray-700"
          data-testid="rules-helper-toggle"
        >
          <svg className="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <circle cx="12" cy="12" r="10" />
            <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
            <line x1="12" y1="17" x2="12.01" y2="17" />
          </svg>
          {showHelperDocs ? 'Hide Reference' : 'Show Reference'}
        </button>
      </div>

      {/* Helper documentation */}
      <RulesHelperDocs isOpen={showHelperDocs} onClose={() => setShowHelperDocs(false)} />

      {/* Rule fields */}
      <div className={`space-y-4 ${showHelperDocs ? 'mt-4' : ''}`}>
        {visibleFields.map((fieldDef) => {
          const ruleValue = rules[fieldDef.key];
          const isLocked = ruleValue === null || ruleValue === undefined;
          const error = validationErrors[fieldDef.key];

          return (
            <div
              key={fieldDef.key}
              className="rounded-md border border-gray-100 dark:border-gray-700 bg-gray-50 dark:bg-gray-900 p-3"
              data-testid={`rule-field-${fieldDef.key}`}
            >
              <div className="mb-2 flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <label className="text-sm font-medium text-gray-700 dark:text-gray-300">
                    {fieldDef.label}
                  </label>
                  {isLocked ? (
                    <span
                      className="inline-flex items-center gap-1 rounded-full bg-gray-200 dark:bg-gray-600 px-2 py-0.5 text-xs font-medium text-gray-600 dark:text-gray-400"
                      data-testid={`rule-badge-${fieldDef.key}`}
                    >
                      <svg className="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                        <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
                        <path d="M7 11V7a5 5 0 0110 0v4" />
                      </svg>
                      Locked
                    </span>
                  ) : ruleValue === '' ? (
                    <span
                      className="inline-flex items-center gap-1 rounded-full bg-green-100 dark:bg-green-900/20 px-2 py-0.5 text-xs font-medium text-green-700 dark:text-green-400"
                      data-testid={`rule-badge-${fieldDef.key}`}
                    >
                      <svg className="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                        <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
                        <path d="M7 11V7a5 5 0 019.9-1" />
                      </svg>
                      Public
                    </span>
                  ) : (
                    <span
                      className="inline-flex items-center gap-1 rounded-full bg-blue-100 dark:bg-blue-900/20 px-2 py-0.5 text-xs font-medium text-blue-700 dark:text-blue-400"
                      data-testid={`rule-badge-${fieldDef.key}`}
                    >
                      <svg className="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                        <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
                      </svg>
                      Conditional
                    </span>
                  )}
                </div>

                <div className="flex items-center gap-1">
                  {/* Preset dropdown */}
                  {!isLocked && (
                    <select
                      className="rounded border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 px-2 py-1 text-xs text-gray-600 dark:text-gray-400 focus:border-blue-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-blue-500"
                      value=""
                      onChange={(e) => {
                        const preset = RULE_PRESETS.find((p) =>
                          p.value === null ? e.target.value === '__locked__' : p.value === e.target.value,
                        );
                        if (preset) {
                          handleApplyPreset(fieldDef.key, preset.value);
                        }
                      }}
                      data-testid={`rule-preset-${fieldDef.key}`}
                    >
                      <option value="" disabled>
                        Presets…
                      </option>
                      {RULE_PRESETS.map((preset) => (
                        <option
                          key={preset.label}
                          value={preset.value === null ? '__locked__' : preset.value}
                        >
                          {preset.label} — {preset.description}
                        </option>
                      ))}
                    </select>
                  )}

                  {/* Lock/unlock toggle */}
                  <button
                    type="button"
                    onClick={() => handleToggleLock(fieldDef.key)}
                    className={`rounded p-1.5 text-xs font-medium transition-colors ${
                      isLocked
                        ? 'text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-600 hover:text-gray-700 dark:hover:text-gray-300'
                        : 'text-red-500 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30 hover:text-red-700 dark:hover:text-red-400'
                    }`}
                    title={isLocked ? 'Unlock (allow access)' : 'Lock (superusers only)'}
                    data-testid={`rule-toggle-${fieldDef.key}`}
                    aria-label={isLocked ? `Unlock ${fieldDef.label}` : `Lock ${fieldDef.label}`}
                  >
                    {isLocked ? (
                      <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                        <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
                        <path d="M7 11V7a5 5 0 019.9-1" />
                      </svg>
                    ) : (
                      <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                        <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
                        <path d="M7 11V7a5 5 0 0110 0v4" />
                      </svg>
                    )}
                  </button>
                </div>
              </div>

              <p className="mb-2 text-xs text-gray-500 dark:text-gray-400">{fieldDef.description}</p>

              {isLocked ? (
                <div
                  className="rounded-md border border-dashed border-gray-300 dark:border-gray-600 bg-gray-100 dark:bg-gray-700 px-3 py-2 text-center text-xs text-gray-400 dark:text-gray-500"
                  data-testid={`rule-locked-${fieldDef.key}`}
                >
                  Only superusers can perform this operation. Click the unlock icon to set a rule.
                </div>
              ) : (
                <HighlightedRuleInput
                  value={ruleValue ?? ''}
                  onChange={(v) => handleRuleChange(fieldDef.key, v)}
                  error={error}
                  testId={`rule-input-${fieldDef.key}`}
                />
              )}
            </div>
          );
        })}
      </div>

      {/* Validation summary */}
      {hasAnyError && (
        <div
          className="mt-4 rounded-md bg-yellow-50 dark:bg-yellow-900/30 p-3"
          role="alert"
          data-testid="rules-validation-summary"
        >
          <p className="text-xs font-medium text-yellow-800 dark:text-yellow-300">
            Some rule expressions have syntax issues. Fix them before saving.
          </p>
        </div>
      )}

      {/* Syntax highlight styles */}
      <style>{`
        .rule-macro { color: #7c3aed; font-weight: 500; }
        .rule-operator { color: #dc2626; }
        .rule-logical { color: #2563eb; font-weight: 600; }
        .rule-string { color: #059669; }
        .rule-number { color: #d97706; }
        .rule-literal { color: #d97706; font-style: italic; }
        .rule-field { color: #1e293b; }
        .rule-paren { color: #6b7280; font-weight: 600; }
      `}</style>
    </section>
  );
}

// Re-export for testing
export { RULE_FIELDS, RULE_PRESETS, HELPER_SECTIONS };
