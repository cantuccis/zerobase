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
  ariaLabel?: string;
}

function HighlightedRuleInput({ value, onChange, error, testId, ariaLabel }: HighlightedRuleInputProps) {
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
      <div className="relative overflow-hidden border border-primary focus-within:border-primary" style={{ borderWidth: '1px' }}>
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
          id={testId}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onScroll={handleScroll}
          className="relative w-full resize-none bg-transparent px-3 py-2 font-mono text-sm text-transparent caret-on-surface outline-none"
          rows={1}
          spellCheck={false}
          autoComplete="off"
          data-testid={testId}
          aria-label={ariaLabel}
          aria-invalid={error ? 'true' : undefined}
          aria-describedby={error ? `${testId}-error` : undefined}
        />
      </div>
      {error && (
        <p
          id={`${testId}-error`}
          className="mt-1 text-xs text-error"
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
      className="border border-primary bg-surface-container-low p-4"
      data-testid="rules-helper-docs"
    >
      <div className="mb-3 flex items-center justify-between">
        <h4 className="text-label-md text-on-surface">Rule Expression Reference</h4>
        <button
          type="button"
          onClick={onClose}
          className="p-1 text-on-surface hover:bg-surface-container-high"
          aria-label="Close reference"
          data-testid="rules-helper-close"
        >
          <span className="material-symbols-outlined text-base" aria-hidden="true">close</span>
        </button>
      </div>

      <div className="space-y-4">
        {HELPER_SECTIONS.map((section) => (
          <div key={section.title}>
            <h5 className="text-label-sm text-secondary mb-1.5">
              {section.title}
            </h5>
            <div className="space-y-1">
              {section.items.map((item) => (
                <div key={item.variable} className="flex gap-3 text-xs">
                  <code className="shrink-0 border border-outline-variant bg-surface-container px-1.5 py-0.5 font-mono text-on-surface">
                    {item.variable}
                  </code>
                  <span className="text-secondary">{item.description}</span>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>

      <div className="mt-4 border border-outline-variant bg-surface-container p-3">
        <h5 className="text-label-sm text-on-surface mb-1">Rule Values</h5>
        <ul className="space-y-1 text-xs text-secondary">
          <li><strong className="text-on-surface">Locked (null)</strong> — Only superusers can perform this operation.</li>
          <li><strong className="text-on-surface">Empty string</strong> — Open to everyone, no restrictions.</li>
          <li><strong className="text-on-surface">Expression</strong> — Conditional access based on the request context.</li>
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
    <div data-testid="rules-editor">
      <div className="mb-4 flex items-center justify-between">
        <div>
          <p className="text-xs text-secondary">
            Control who can access this collection&apos;s API endpoints.
          </p>
        </div>
        <button
          type="button"
          onClick={() => setShowHelperDocs((prev) => !prev)}
          className="inline-flex items-center gap-1.5 border border-primary px-3 py-1.5 text-xs font-semibold text-on-surface hover:bg-surface-container-low"
          data-testid="rules-helper-toggle"
        >
          <span className="material-symbols-outlined text-sm" aria-hidden="true">help_outline</span>
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
              className="border border-outline-variant bg-surface-container-low p-3"
              data-testid={`rule-field-${fieldDef.key}`}
            >
              <div className="mb-2 flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <label htmlFor={`rule-input-${fieldDef.key}`} className="text-sm font-semibold text-on-surface">
                    {fieldDef.label}
                  </label>
                  {isLocked ? (
                    <span
                      className="inline-flex items-center gap-1 border border-primary bg-primary text-on-primary px-2 py-0.5 text-label-sm"
                      data-testid={`rule-badge-${fieldDef.key}`}
                    >
                      <span className="material-symbols-outlined text-xs" aria-hidden="true">lock</span>
                      LOCKED
                    </span>
                  ) : ruleValue === '' ? (
                    <span
                      className="inline-flex items-center gap-1 border border-primary px-2 py-0.5 text-label-sm text-on-surface"
                      data-testid={`rule-badge-${fieldDef.key}`}
                    >
                      <span className="material-symbols-outlined text-xs" aria-hidden="true">lock_open</span>
                      PUBLIC
                    </span>
                  ) : (
                    <span
                      className="inline-flex items-center gap-1 border border-outline px-2 py-0.5 text-label-sm text-secondary"
                      data-testid={`rule-badge-${fieldDef.key}`}
                    >
                      <span className="material-symbols-outlined text-xs" aria-hidden="true">shield</span>
                      CONDITIONAL
                    </span>
                  )}
                </div>

                <div className="flex items-center gap-1">
                  {/* Preset dropdown */}
                  {!isLocked && (
                    <select
                      className="border border-primary bg-background px-2 py-1 text-xs text-on-surface focus:outline-none"
                      aria-label={`Select preset rule for ${fieldDef.label}`}
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
                        Presets\u2026
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
                    className={`p-1.5 text-xs font-medium ${
                      isLocked
                        ? 'text-on-surface hover:bg-surface-container-high'
                        : 'text-error hover:bg-error-container'
                    }`}
                    title={isLocked ? 'Unlock (allow access)' : 'Lock (superusers only)'}
                    data-testid={`rule-toggle-${fieldDef.key}`}
                    aria-label={isLocked ? `Unlock ${fieldDef.label}` : `Lock ${fieldDef.label}`}
                  >
                    <span className="material-symbols-outlined text-base" aria-hidden="true">
                      {isLocked ? 'lock_open' : 'lock'}
                    </span>
                  </button>
                </div>
              </div>

              <p className="mb-2 text-xs text-secondary">{fieldDef.description}</p>

              {isLocked ? (
                <div
                  className="border border-dashed border-outline bg-surface-container px-3 py-2 text-center text-xs text-secondary"
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
                  ariaLabel={`${fieldDef.label} expression`}
                />
              )}
            </div>
          );
        })}
      </div>

      {/* Validation summary */}
      {hasAnyError && (
        <div
          className="mt-4 border border-error bg-error-container p-3"
          role="alert"
          data-testid="rules-validation-summary"
        >
          <p className="text-xs font-semibold text-on-error-container">
            Some rule expressions have syntax issues. Fix them before saving.
          </p>
        </div>
      )}

      {/* Syntax highlight styles — monolith palette */}
      <style>{`
        .rule-macro { color: var(--color-primary); font-weight: 600; }
        .rule-operator { color: var(--color-error); }
        .rule-logical { color: var(--color-secondary); font-weight: 700; }
        .rule-string { color: var(--color-secondary); }
        .dark .rule-string { color: var(--color-secondary); }
        .rule-number { color: var(--color-secondary); }
        .rule-literal { color: var(--color-secondary); font-style: italic; }
        .rule-field { color: var(--color-on-surface); }
        .rule-paren { color: var(--color-outline); font-weight: 700; }
      `}</style>
    </div>
  );
}

// Re-export for testing
export { RULE_FIELDS, RULE_PRESETS, HELPER_SECTIONS };
