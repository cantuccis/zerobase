import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { useState } from 'react';
import { RulesEditor, validateRuleExpression, RULE_FIELDS, RULE_PRESETS, HELPER_SECTIONS } from './RulesEditor';
import type { ApiRules } from '../../lib/api/types';

// Wrapper that manages state for controlled component testing
function StatefulRulesEditor({
  initialRules,
  onChangeSpy,
  collectionType,
}: {
  initialRules: ApiRules;
  onChangeSpy?: (rules: ApiRules) => void;
  collectionType?: 'base' | 'auth' | 'view';
}) {
  const [rules, setRules] = useState(initialRules);
  return (
    <RulesEditor
      rules={rules}
      onChange={(updated) => {
        setRules(updated);
        onChangeSpy?.(updated);
      }}
      collectionType={collectionType}
    />
  );
}

// ── Test data ────────────────────────────────────────────────────────────────

function lockedRules(): ApiRules {
  return {
    listRule: null,
    viewRule: null,
    createRule: null,
    updateRule: null,
    deleteRule: null,
  };
}

function openRules(): ApiRules {
  return {
    listRule: '',
    viewRule: '',
    createRule: '',
    updateRule: '',
    deleteRule: '',
  };
}

function mixedRules(): ApiRules {
  return {
    listRule: '',
    viewRule: '@request.auth.id != ""',
    createRule: '@request.auth.id != ""',
    updateRule: '@request.auth.id = id',
    deleteRule: null,
    manageRule: '@request.auth.role = "admin"',
  };
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('RulesEditor', () => {
  let onChange: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    onChange = vi.fn();
  });

  // ── Rendering ──────────────────────────────────────────────────────────

  describe('rendering', () => {
    it('renders the rules editor section', () => {
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);
      expect(screen.getByTestId('rules-editor')).toBeInTheDocument();
      expect(screen.getByText('API Rules')).toBeInTheDocument();
    });

    it('renders all 5 base rule fields', () => {
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);
      expect(screen.getByTestId('rule-field-listRule')).toBeInTheDocument();
      expect(screen.getByTestId('rule-field-viewRule')).toBeInTheDocument();
      expect(screen.getByTestId('rule-field-createRule')).toBeInTheDocument();
      expect(screen.getByTestId('rule-field-updateRule')).toBeInTheDocument();
      expect(screen.getByTestId('rule-field-deleteRule')).toBeInTheDocument();
    });

    it('renders the manageRule field for base collections', () => {
      render(<RulesEditor rules={lockedRules()} onChange={onChange} collectionType="base" />);
      expect(screen.getByTestId('rule-field-manageRule')).toBeInTheDocument();
    });

    it('renders the manageRule field for auth collections', () => {
      render(<RulesEditor rules={lockedRules()} onChange={onChange} collectionType="auth" />);
      expect(screen.getByTestId('rule-field-manageRule')).toBeInTheDocument();
    });

    it('hides the manageRule field for view collections', () => {
      render(<RulesEditor rules={lockedRules()} onChange={onChange} collectionType="view" />);
      expect(screen.queryByTestId('rule-field-manageRule')).not.toBeInTheDocument();
    });

    it('shows locked state for null rules', () => {
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);
      expect(screen.getByTestId('rule-locked-listRule')).toBeInTheDocument();
      expect(screen.getByTestId('rule-locked-viewRule')).toBeInTheDocument();
    });

    it('shows input fields for unlocked rules', () => {
      render(<RulesEditor rules={openRules()} onChange={onChange} />);
      expect(screen.getByTestId('rule-input-listRule')).toBeInTheDocument();
      expect(screen.getByTestId('rule-input-viewRule')).toBeInTheDocument();
    });

    it('shows "Locked" badge for null rules', () => {
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);
      const listField = screen.getByTestId('rule-field-listRule');
      const badge = within(listField).getByTestId('rule-badge-listRule');
      expect(badge).toHaveTextContent('Locked');
    });

    it('shows "Public" badge for empty string rules', () => {
      render(<RulesEditor rules={openRules()} onChange={onChange} />);
      const listField = screen.getByTestId('rule-field-listRule');
      const badge = within(listField).getByTestId('rule-badge-listRule');
      expect(badge).toHaveTextContent('Public');
    });

    it('shows "Conditional" badge for expression rules', () => {
      render(<RulesEditor rules={mixedRules()} onChange={onChange} />);
      const viewField = screen.getByTestId('rule-field-viewRule');
      const badge = within(viewField).getByTestId('rule-badge-viewRule');
      expect(badge).toHaveTextContent('Conditional');
    });

    it('displays rule descriptions', () => {
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);
      expect(screen.getByText(/Filter applied when listing records/)).toBeInTheDocument();
      expect(screen.getByText(/Filter applied when viewing a single record/)).toBeInTheDocument();
      expect(screen.getByText(/Condition for creating new records/)).toBeInTheDocument();
    });

    it('displays existing rule expressions in inputs', () => {
      render(<RulesEditor rules={mixedRules()} onChange={onChange} />);
      const viewInput = screen.getByTestId('rule-input-viewRule') as HTMLTextAreaElement;
      expect(viewInput.value).toBe('@request.auth.id != ""');
    });
  });

  // ── Lock/unlock toggle ─────────────────────────────────────────────────

  describe('lock/unlock toggle', () => {
    it('unlocks a rule when clicking the toggle on a locked rule', async () => {
      const user = userEvent.setup();
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);

      await user.click(screen.getByTestId('rule-toggle-listRule'));

      expect(onChange).toHaveBeenCalledWith(
        expect.objectContaining({ listRule: '' }),
      );
    });

    it('locks a rule when clicking the toggle on an unlocked rule', async () => {
      const user = userEvent.setup();
      render(<RulesEditor rules={openRules()} onChange={onChange} />);

      await user.click(screen.getByTestId('rule-toggle-listRule'));

      expect(onChange).toHaveBeenCalledWith(
        expect.objectContaining({ listRule: null }),
      );
    });

    it('preserves other rules when toggling one', async () => {
      const user = userEvent.setup();
      render(<RulesEditor rules={mixedRules()} onChange={onChange} />);

      await user.click(screen.getByTestId('rule-toggle-deleteRule'));

      const call = onChange.mock.calls[0][0] as ApiRules;
      expect(call.deleteRule).toBe('');
      expect(call.listRule).toBe('');
      expect(call.viewRule).toBe('@request.auth.id != ""');
    });

    it('has accessible label on toggle buttons', () => {
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);
      const toggle = screen.getByTestId('rule-toggle-listRule');
      expect(toggle).toHaveAttribute('aria-label', 'Unlock List Rule');
    });

    it('has accessible label on unlock toggle buttons', () => {
      render(<RulesEditor rules={openRules()} onChange={onChange} />);
      const toggle = screen.getByTestId('rule-toggle-listRule');
      expect(toggle).toHaveAttribute('aria-label', 'Lock List Rule');
    });
  });

  // ── Rule expression editing ────────────────────────────────────────────

  describe('expression editing', () => {
    it('calls onChange when typing a rule expression', async () => {
      const spy = vi.fn();
      const user = userEvent.setup();
      render(<StatefulRulesEditor initialRules={openRules()} onChangeSpy={spy} />);

      const input = screen.getByTestId('rule-input-listRule');
      await user.type(input, '@request.auth.id != ""');

      // Each character triggers onChange, check the final call
      const lastCall = spy.mock.calls[spy.mock.calls.length - 1][0] as ApiRules;
      expect(lastCall.listRule).toBe('@request.auth.id != ""');
    });

    it('preserves other rules when editing one', async () => {
      const spy = vi.fn();
      const user = userEvent.setup();
      render(<StatefulRulesEditor initialRules={mixedRules()} onChangeSpy={spy} />);

      const input = screen.getByTestId('rule-input-viewRule');
      await user.clear(input);
      await user.type(input, 'status = "active"');

      const lastCall = spy.mock.calls[spy.mock.calls.length - 1][0] as ApiRules;
      expect(lastCall.viewRule).toBe('status = "active"');
      expect(lastCall.listRule).toBe('');
      expect(lastCall.updateRule).toBe('@request.auth.id = id');
    });
  });

  // ── Presets ────────────────────────────────────────────────────────────

  describe('presets', () => {
    it('shows preset dropdown for unlocked rules', () => {
      render(<RulesEditor rules={openRules()} onChange={onChange} />);
      expect(screen.getByTestId('rule-preset-listRule')).toBeInTheDocument();
    });

    it('does not show preset dropdown for locked rules', () => {
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);
      expect(screen.queryByTestId('rule-preset-listRule')).not.toBeInTheDocument();
    });

    it('applies "Authenticated" preset when selected', async () => {
      const user = userEvent.setup();
      render(<RulesEditor rules={openRules()} onChange={onChange} />);

      const preset = screen.getByTestId('rule-preset-listRule');
      await user.selectOptions(preset, '@request.auth.id != ""');

      expect(onChange).toHaveBeenCalledWith(
        expect.objectContaining({ listRule: '@request.auth.id != ""' }),
      );
    });

    it('applies "Public" preset (empty string)', async () => {
      const user = userEvent.setup();
      const rules = { ...mixedRules(), viewRule: '@request.auth.id != ""' as string | null };
      render(<RulesEditor rules={rules} onChange={onChange} />);

      const preset = screen.getByTestId('rule-preset-viewRule');
      // Select the "Public" option by its label text
      await user.selectOptions(preset, 'Public — Open to everyone');

      expect(onChange).toHaveBeenCalledWith(
        expect.objectContaining({ viewRule: '' }),
      );
    });

    it('applies "Locked" preset (null)', async () => {
      const user = userEvent.setup();
      render(<RulesEditor rules={openRules()} onChange={onChange} />);

      const preset = screen.getByTestId('rule-preset-listRule');
      await user.selectOptions(preset, '__locked__');

      expect(onChange).toHaveBeenCalledWith(
        expect.objectContaining({ listRule: null }),
      );
    });
  });

  // ── Helper documentation ───────────────────────────────────────────────

  describe('helper documentation', () => {
    it('hides helper docs by default', () => {
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);
      expect(screen.queryByTestId('rules-helper-docs')).not.toBeInTheDocument();
    });

    it('shows helper docs when clicking the toggle button', async () => {
      const user = userEvent.setup();
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);

      await user.click(screen.getByTestId('rules-helper-toggle'));
      expect(screen.getByTestId('rules-helper-docs')).toBeInTheDocument();
    });

    it('shows rule expression reference title', async () => {
      const user = userEvent.setup();
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);

      await user.click(screen.getByTestId('rules-helper-toggle'));
      expect(screen.getByText('Rule Expression Reference')).toBeInTheDocument();
    });

    it('displays all helper sections', async () => {
      const user = userEvent.setup();
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);

      await user.click(screen.getByTestId('rules-helper-toggle'));
      for (const section of HELPER_SECTIONS) {
        expect(screen.getByText(section.title)).toBeInTheDocument();
      }
    });

    it('shows @request.auth.id variable documentation', async () => {
      const user = userEvent.setup();
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);

      await user.click(screen.getByTestId('rules-helper-toggle'));
      expect(screen.getByText('@request.auth.id')).toBeInTheDocument();
      expect(screen.getByText(/ID of the authenticated user/)).toBeInTheDocument();
    });

    it('shows operator documentation', async () => {
      const user = userEvent.setup();
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);

      await user.click(screen.getByTestId('rules-helper-toggle'));
      expect(screen.getByText('=, !=')).toBeInTheDocument();
      expect(screen.getByText('&&, ||')).toBeInTheDocument();
    });

    it('shows example expressions', async () => {
      const user = userEvent.setup();
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);

      await user.click(screen.getByTestId('rules-helper-toggle'));
      expect(screen.getByText('@request.auth.id != ""')).toBeInTheDocument();
      expect(screen.getByText('@request.auth.id = id')).toBeInTheDocument();
    });

    it('shows rule values explanation in helper docs', async () => {
      const user = userEvent.setup();
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);

      await user.click(screen.getByTestId('rules-helper-toggle'));
      const helperDocs = screen.getByTestId('rules-helper-docs');
      expect(within(helperDocs).getByText(/Open to everyone, no restrictions/)).toBeInTheDocument();
      expect(within(helperDocs).getByText(/Conditional access based on the request context/)).toBeInTheDocument();
    });

    it('hides helper docs when clicking the close button', async () => {
      const user = userEvent.setup();
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);

      await user.click(screen.getByTestId('rules-helper-toggle'));
      expect(screen.getByTestId('rules-helper-docs')).toBeInTheDocument();

      await user.click(screen.getByTestId('rules-helper-close'));
      expect(screen.queryByTestId('rules-helper-docs')).not.toBeInTheDocument();
    });

    it('toggles helper docs with the same button', async () => {
      const user = userEvent.setup();
      render(<RulesEditor rules={lockedRules()} onChange={onChange} />);

      const toggle = screen.getByTestId('rules-helper-toggle');

      await user.click(toggle);
      expect(screen.getByTestId('rules-helper-docs')).toBeInTheDocument();
      expect(toggle).toHaveTextContent('Hide Reference');

      await user.click(toggle);
      expect(screen.queryByTestId('rules-helper-docs')).not.toBeInTheDocument();
      expect(toggle).toHaveTextContent('Show Reference');
    });
  });

  // ── Syntax highlighting ────────────────────────────────────────────────

  describe('syntax highlighting', () => {
    it('renders highlight overlay for unlocked rules', () => {
      render(<RulesEditor rules={mixedRules()} onChange={onChange} />);
      expect(screen.getByTestId('rule-input-viewRule-highlight')).toBeInTheDocument();
    });

    it('highlights @request macros with the rule-macro class', () => {
      render(<RulesEditor rules={mixedRules()} onChange={onChange} />);
      const highlight = screen.getByTestId('rule-input-viewRule-highlight');
      const macroSpans = highlight.querySelectorAll('.rule-macro');
      expect(macroSpans.length).toBeGreaterThan(0);
      expect(macroSpans[0].textContent).toBe('@request.auth.id');
    });

    it('highlights operators with the rule-operator class', () => {
      render(<RulesEditor rules={mixedRules()} onChange={onChange} />);
      const highlight = screen.getByTestId('rule-input-viewRule-highlight');
      const opSpans = highlight.querySelectorAll('.rule-operator');
      expect(opSpans.length).toBeGreaterThan(0);
      expect(opSpans[0].textContent).toBe('!=');
    });

    it('highlights strings with the rule-string class', () => {
      render(<RulesEditor rules={mixedRules()} onChange={onChange} />);
      const highlight = screen.getByTestId('rule-input-viewRule-highlight');
      const stringSpans = highlight.querySelectorAll('.rule-string');
      expect(stringSpans.length).toBeGreaterThan(0);
      expect(stringSpans[0].textContent).toBe('""');
    });

    it('highlights field identifiers with the rule-field class', () => {
      const rules: ApiRules = {
        ...openRules(),
        listRule: 'status = "published"',
      };
      render(<RulesEditor rules={rules} onChange={onChange} />);
      const highlight = screen.getByTestId('rule-input-listRule-highlight');
      const fieldSpans = highlight.querySelectorAll('.rule-field');
      expect(fieldSpans.length).toBeGreaterThan(0);
      expect(fieldSpans[0].textContent).toBe('status');
    });
  });

  // ── Validation ─────────────────────────────────────────────────────────

  describe('validation', () => {
    it('shows validation error for unterminated string', async () => {
      const user = userEvent.setup();
      render(<StatefulRulesEditor initialRules={openRules()} />);

      const input = screen.getByTestId('rule-input-listRule');
      await user.type(input, 'status = "published');

      expect(screen.getByTestId('rule-input-listRule-error')).toBeInTheDocument();
      expect(screen.getByText('Unterminated string literal.')).toBeInTheDocument();
    });

    it('shows validation error for unbalanced parentheses', async () => {
      const user = userEvent.setup();
      render(<StatefulRulesEditor initialRules={openRules()} />);

      const input = screen.getByTestId('rule-input-listRule');
      await user.type(input, '(status = "a"');

      expect(screen.getByTestId('rule-input-listRule-error')).toBeInTheDocument();
      expect(screen.getByText(/Unclosed parenthesis/)).toBeInTheDocument();
    });

    it('shows validation error for unknown @ variable', async () => {
      const user = userEvent.setup();
      render(<StatefulRulesEditor initialRules={openRules()} />);

      const input = screen.getByTestId('rule-input-listRule');
      await user.type(input, '@unknown.field = "test"');

      expect(screen.getByTestId('rule-input-listRule-error')).toBeInTheDocument();
      expect(screen.getByText(/Unknown variable "@unknown"/)).toBeInTheDocument();
    });

    it('does not show error for valid expressions', async () => {
      const user = userEvent.setup();
      render(<StatefulRulesEditor initialRules={openRules()} />);

      const input = screen.getByTestId('rule-input-listRule');
      await user.type(input, '@request.auth.id != ""');

      expect(screen.queryByTestId('rule-input-listRule-error')).not.toBeInTheDocument();
    });

    it('does not show error for empty expression (public access)', () => {
      render(<RulesEditor rules={openRules()} onChange={onChange} />);
      expect(screen.queryByTestId('rule-input-listRule-error')).not.toBeInTheDocument();
    });

    it('shows validation summary when there are errors', async () => {
      const user = userEvent.setup();
      render(<StatefulRulesEditor initialRules={openRules()} />);

      const input = screen.getByTestId('rule-input-listRule');
      await user.type(input, 'status = "unclosed');

      expect(screen.getByTestId('rules-validation-summary')).toBeInTheDocument();
      expect(screen.getByText(/Some rule expressions have syntax issues/)).toBeInTheDocument();
    });

    it('clears validation error when rule is locked', async () => {
      const spy = vi.fn();
      const user = userEvent.setup();
      const rules: ApiRules = {
        ...openRules(),
        listRule: 'status = "unclosed',
      };
      render(<StatefulRulesEditor initialRules={rules} onChangeSpy={spy} />);

      // Lock the rule (which clears the error)
      await user.click(screen.getByTestId('rule-toggle-listRule'));

      // onChange was called with null
      expect(spy).toHaveBeenCalledWith(
        expect.objectContaining({ listRule: null }),
      );
    });

    it('sets aria-invalid on input when there is a validation error', async () => {
      const user = userEvent.setup();
      render(<StatefulRulesEditor initialRules={openRules()} />);

      const input = screen.getByTestId('rule-input-listRule');
      await user.type(input, 'status = "unclosed');

      expect(input).toHaveAttribute('aria-invalid', 'true');
    });
  });

  // ── Mixed state rendering ──────────────────────────────────────────────

  describe('mixed rule states', () => {
    it('renders different states correctly for each rule', () => {
      render(<RulesEditor rules={mixedRules()} onChange={onChange} />);

      // listRule = '' (public)
      expect(screen.getByTestId('rule-input-listRule')).toBeInTheDocument();
      const listBadge = screen.getByTestId('rule-badge-listRule');
      expect(listBadge).toHaveTextContent('Public');

      // viewRule = expression (conditional)
      expect(screen.getByTestId('rule-input-viewRule')).toBeInTheDocument();
      const viewBadge = screen.getByTestId('rule-badge-viewRule');
      expect(viewBadge).toHaveTextContent('Conditional');

      // deleteRule = null (locked)
      expect(screen.getByTestId('rule-locked-deleteRule')).toBeInTheDocument();
      const deleteBadge = screen.getByTestId('rule-badge-deleteRule');
      expect(deleteBadge).toHaveTextContent('Locked');
    });
  });
});

// ── validateRuleExpression unit tests ────────────────────────────────────────

describe('validateRuleExpression', () => {
  it('returns null for empty string (public access)', () => {
    expect(validateRuleExpression('')).toBeNull();
  });

  it('returns null for whitespace-only string', () => {
    expect(validateRuleExpression('   ')).toBeNull();
  });

  it('returns null for valid simple expression', () => {
    expect(validateRuleExpression('@request.auth.id != ""')).toBeNull();
  });

  it('returns null for valid compound expression', () => {
    expect(validateRuleExpression('status = "published" && @request.auth.id != ""')).toBeNull();
  });

  it('returns null for valid expression with parentheses', () => {
    expect(validateRuleExpression('(status = "active") && (@request.auth.id != "")')).toBeNull();
  });

  it('returns null for nested parentheses', () => {
    expect(validateRuleExpression('((a = "b") && (c = "d")) || (e = "f")')).toBeNull();
  });

  it('returns null for expression with @collection reference', () => {
    expect(validateRuleExpression('@collection.users.id = @request.auth.id')).toBeNull();
  });

  it('returns null for expression with @now', () => {
    expect(validateRuleExpression('expiry > @now')).toBeNull();
  });

  it('returns null for expression with numeric comparison', () => {
    expect(validateRuleExpression('views > 100')).toBeNull();
  });

  it('returns null for expression with LIKE operator', () => {
    expect(validateRuleExpression('name ~ "%test%"')).toBeNull();
  });

  it('returns error for unterminated double-quoted string', () => {
    const result = validateRuleExpression('status = "published');
    expect(result).not.toBeNull();
    expect(result!.message).toBe('Unterminated string literal.');
  });

  it('returns error for unterminated single-quoted string', () => {
    const result = validateRuleExpression("status = 'published");
    expect(result).not.toBeNull();
    expect(result!.message).toBe('Unterminated string literal.');
  });

  it('returns error for unclosed parenthesis', () => {
    const result = validateRuleExpression('(status = "a"');
    expect(result).not.toBeNull();
    expect(result!.message).toContain('Unclosed parenthesis');
  });

  it('returns error for unexpected closing parenthesis', () => {
    const result = validateRuleExpression('status = "a")');
    expect(result).not.toBeNull();
    expect(result!.message).toBe('Unexpected closing parenthesis.');
  });

  it('returns error for multiple unclosed parentheses', () => {
    const result = validateRuleExpression('((status = "a"');
    expect(result).not.toBeNull();
    expect(result!.message).toContain('2 open');
  });

  it('returns error when expression starts with operator', () => {
    const result = validateRuleExpression('= "value"');
    expect(result).not.toBeNull();
    expect(result!.message).toBe('Expression cannot start with an operator.');
  });

  it('returns error when expression ends with operator', () => {
    const result = validateRuleExpression('field =');
    expect(result).not.toBeNull();
    expect(result!.message).toBe('Expression cannot end with an operator.');
  });

  it('does not flag expression ending with quoted string followed by operator-like char', () => {
    // Expression like 'field = "value"' should be valid
    expect(validateRuleExpression('field = "value"')).toBeNull();
  });

  it('returns error for unknown @ variable root', () => {
    const result = validateRuleExpression('@unknown.field = "test"');
    expect(result).not.toBeNull();
    expect(result!.message).toContain('Unknown variable "@unknown"');
    expect(result!.message).toContain('@request');
    expect(result!.message).toContain('@collection');
    expect(result!.message).toContain('@now');
  });

  it('accepts known @ variable roots', () => {
    expect(validateRuleExpression('@request.auth.id = "test"')).toBeNull();
    expect(validateRuleExpression('@collection.users.id = "test"')).toBeNull();
    expect(validateRuleExpression('expiry > @now')).toBeNull();
  });

  it('returns error for consecutive operators', () => {
    const result = validateRuleExpression('field === "value"');
    expect(result).not.toBeNull();
    expect(result!.message).toBe('Invalid operator sequence.');
  });

  it('handles escaped quotes in strings correctly', () => {
    expect(validateRuleExpression('field = "val\\"ue"')).toBeNull();
  });
});
