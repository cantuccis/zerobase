import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { FieldEditor } from './FieldEditor';
import type { Field, Collection } from '../../lib/api/types';

// ── Helpers ──────────────────────────────────────────────────────────────────

function makeField(overrides: Partial<Field> = {}): Field {
  return {
    id: 'test_field_1',
    name: 'test_field',
    type: { type: 'text', options: { minLength: 0, maxLength: 500, pattern: null, searchable: true } },
    required: false,
    unique: false,
    sortOrder: 0,
    ...overrides,
  };
}

const TEST_COLLECTIONS: Collection[] = [
  {
    id: 'col_posts',
    name: 'posts',
    type: 'base',
    fields: [],
    rules: { listRule: null, viewRule: null, createRule: null, updateRule: null, deleteRule: null },
  },
  {
    id: 'col_users',
    name: 'users',
    type: 'auth',
    fields: [],
    rules: { listRule: null, viewRule: null, createRule: null, updateRule: null, deleteRule: null },
  },
];

interface RenderOptions {
  field?: Partial<Field>;
  index?: number;
  totalFields?: number;
  nameError?: string;
}

function renderFieldEditor(options: RenderOptions = {}) {
  const field = makeField(options.field);
  const onChange = vi.fn();
  const onRemove = vi.fn();
  const onMoveUp = vi.fn();
  const onMoveDown = vi.fn();
  const index = options.index ?? 0;
  const totalFields = options.totalFields ?? 2;

  render(
    <FieldEditor
      field={field}
      index={index}
      totalFields={totalFields}
      onChange={onChange}
      onRemove={onRemove}
      onMoveUp={onMoveUp}
      onMoveDown={onMoveDown}
      collections={TEST_COLLECTIONS}
      nameError={options.nameError}
    />,
  );

  return { field, onChange, onRemove, onMoveUp, onMoveDown };
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('FieldEditor', () => {
  // ── Basic rendering ───────────────────────────────────────────────────

  it('renders field name input with current value', () => {
    renderFieldEditor({ field: { name: 'my_field' } });
    expect(screen.getByTestId('field-name-0')).toHaveValue('my_field');
  });

  it('renders field type dropdown with current type selected', () => {
    renderFieldEditor({ field: { type: { type: 'number', options: { min: null, max: null, noDecimal: false } } } });
    expect(screen.getByTestId('field-type-0')).toHaveValue('number');
  });

  it('renders required checkbox', () => {
    renderFieldEditor({ field: { required: true } });
    expect(screen.getByTestId('field-required-0')).toBeChecked();
  });

  it('renders unique checkbox', () => {
    renderFieldEditor({ field: { unique: true } });
    expect(screen.getByTestId('field-unique-0')).toBeChecked();
  });

  // ── Name editing ──────────────────────────────────────────────────────

  it('calls onChange when field name is edited', async () => {
    const { onChange } = renderFieldEditor();
    const user = userEvent.setup();

    await user.type(screen.getByTestId('field-name-0'), 'x');

    expect(onChange).toHaveBeenCalled();
    // The first call appends 'x' to the existing 'test_field' name
    const firstCall = onChange.mock.calls[0][0];
    expect(firstCall.name).toBe('test_fieldx');
  });

  // ── Name validation error ─────────────────────────────────────────────

  it('displays name validation error when provided', () => {
    renderFieldEditor({ nameError: 'Field name is required.' });
    expect(screen.getByText('Field name is required.')).toBeInTheDocument();
  });

  it('sets aria-invalid on name input when error is present', () => {
    renderFieldEditor({ nameError: 'Invalid name' });
    expect(screen.getByTestId('field-name-0')).toHaveAttribute('aria-invalid', 'true');
  });

  // ── Type change ───────────────────────────────────────────────────────

  it('calls onChange with default type options when type is changed', async () => {
    const { onChange } = renderFieldEditor();
    const user = userEvent.setup();

    await user.selectOptions(screen.getByTestId('field-type-0'), 'number');

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        type: expect.objectContaining({ type: 'number', options: { min: null, max: null, noDecimal: false } }),
      }),
    );
  });

  // ── Required toggle ───────────────────────────────────────────────────

  it('calls onChange with toggled required', async () => {
    const { onChange } = renderFieldEditor({ field: { required: false } });
    const user = userEvent.setup();

    await user.click(screen.getByTestId('field-required-0'));

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ required: true }),
    );
  });

  // ── Unique toggle ─────────────────────────────────────────────────────

  it('calls onChange with toggled unique', async () => {
    const { onChange } = renderFieldEditor({ field: { unique: false } });
    const user = userEvent.setup();

    await user.click(screen.getByTestId('field-unique-0'));

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ unique: true }),
    );
  });

  // ── Remove button ─────────────────────────────────────────────────────

  it('calls onRemove when remove button is clicked', async () => {
    const { onRemove } = renderFieldEditor();
    const user = userEvent.setup();

    await user.click(screen.getByTestId('field-remove-0'));
    expect(onRemove).toHaveBeenCalledTimes(1);
  });

  // ── Move buttons ──────────────────────────────────────────────────────

  it('calls onMoveUp when move up is clicked', async () => {
    const { onMoveUp } = renderFieldEditor({ index: 1 });
    const user = userEvent.setup();

    await user.click(screen.getByTestId('field-move-up-1'));
    expect(onMoveUp).toHaveBeenCalledTimes(1);
  });

  it('calls onMoveDown when move down is clicked', async () => {
    const { onMoveDown } = renderFieldEditor({ index: 0 });
    const user = userEvent.setup();

    await user.click(screen.getByTestId('field-move-down-0'));
    expect(onMoveDown).toHaveBeenCalledTimes(1);
  });

  it('disables move up for first field', () => {
    renderFieldEditor({ index: 0, totalFields: 3 });
    expect(screen.getByTestId('field-move-up-0')).toBeDisabled();
  });

  it('disables move down for last field', () => {
    renderFieldEditor({ index: 2, totalFields: 3 });
    expect(screen.getByTestId('field-move-down-2')).toBeDisabled();
  });

  // ── Type-specific options ─────────────────────────────────────────────

  describe('text field options', () => {
    it('shows min/max length and pattern inputs', () => {
      renderFieldEditor();
      expect(screen.getByTestId('text-min-length')).toBeInTheDocument();
      expect(screen.getByTestId('text-max-length')).toBeInTheDocument();
      expect(screen.getByTestId('text-pattern')).toBeInTheDocument();
    });

    it('updates type options when min length is changed', async () => {
      const { onChange } = renderFieldEditor();
      const user = userEvent.setup();

      await user.clear(screen.getByTestId('text-min-length'));
      await user.type(screen.getByTestId('text-min-length'), '5');

      const lastCall = onChange.mock.calls[onChange.mock.calls.length - 1][0];
      expect(lastCall.type).toEqual(expect.objectContaining({ type: 'text', options: expect.objectContaining({ minLength: 5 }) }));
    });
  });

  describe('number field options', () => {
    it('shows min, max, and no-decimal options', () => {
      renderFieldEditor({
        field: { type: { type: 'number', options: { min: null, max: null, noDecimal: false } } },
      });
      expect(screen.getByTestId('number-min')).toBeInTheDocument();
      expect(screen.getByTestId('number-max')).toBeInTheDocument();
      expect(screen.getByText('Integer only (no decimals)')).toBeInTheDocument();
    });
  });

  describe('bool field options', () => {
    it('shows no additional options message', () => {
      renderFieldEditor({ field: { type: { type: 'bool', options: {} } } });
      expect(screen.getByText(/No additional options/)).toBeInTheDocument();
    });
  });

  describe('select field options', () => {
    it('shows values input', () => {
      renderFieldEditor({
        field: { type: { type: 'select', options: { values: ['draft', 'published'] } } },
      });
      expect(screen.getByTestId('select-values')).toHaveValue('draft, published');
    });
  });

  describe('relation field options', () => {
    it('shows collection dropdown with available collections', () => {
      renderFieldEditor({
        field: { type: { type: 'relation', options: { collectionId: '', cascadeDelete: false, maxSelect: null } } },
      });
      const select = screen.getByTestId('relation-collection');
      expect(select).toBeInTheDocument();
      expect(screen.getByText('posts')).toBeInTheDocument();
      expect(screen.getByText('users')).toBeInTheDocument();
    });

    it('shows cascade delete checkbox', () => {
      renderFieldEditor({
        field: { type: { type: 'relation', options: { collectionId: 'col_posts', cascadeDelete: true, maxSelect: null } } },
      });
      expect(screen.getByLabelText('Cascade delete')).toBeChecked();
    });
  });

  describe('file field options', () => {
    it('shows max size, max select, mime types, and thumbs inputs', () => {
      renderFieldEditor({
        field: { type: { type: 'file', options: { maxSize: 5242880, maxSelect: 1, mimeTypes: [], thumbs: [] } } },
      });
      expect(screen.getByTestId('file-max-size')).toBeInTheDocument();
      expect(screen.getByTestId('file-max-select')).toBeInTheDocument();
      expect(screen.getByTestId('file-mime-types')).toBeInTheDocument();
      expect(screen.getByTestId('file-thumbs')).toBeInTheDocument();
    });
  });

  describe('email field options', () => {
    it('shows domain filter inputs', () => {
      renderFieldEditor({
        field: { type: { type: 'email', options: { exceptDomains: [], onlyDomains: ['example.com'] } } },
      });
      expect(screen.getByTestId('email-only-domains')).toHaveValue('example.com');
      expect(screen.getByTestId('email-except-domains')).toBeInTheDocument();
    });
  });

  describe('url field options', () => {
    it('shows domain filter inputs', () => {
      renderFieldEditor({
        field: { type: { type: 'url', options: { exceptDomains: ['evil.com'], onlyDomains: [] } } },
      });
      expect(screen.getByTestId('url-except-domains')).toHaveValue('evil.com');
      expect(screen.getByTestId('url-only-domains')).toBeInTheDocument();
    });
  });

  describe('dateTime field options', () => {
    it('shows min and max date inputs', () => {
      renderFieldEditor({
        field: { type: { type: 'dateTime', options: { min: '', max: '' } } },
      });
      expect(screen.getByTestId('datetime-min')).toBeInTheDocument();
      expect(screen.getByTestId('datetime-max')).toBeInTheDocument();
    });
  });

  describe('multiSelect field options', () => {
    it('shows values and max select inputs', () => {
      renderFieldEditor({
        field: { type: { type: 'multiSelect', options: { values: ['a', 'b'], maxSelect: 3 } } },
      });
      expect(screen.getByTestId('multiselect-values')).toHaveValue('a, b');
      expect(screen.getByTestId('multiselect-max')).toHaveValue(3);
    });
  });

  describe('json field options', () => {
    it('shows max size input', () => {
      renderFieldEditor({
        field: { type: { type: 'json', options: { maxSize: 2097152 } } },
      });
      expect(screen.getByTestId('json-max-size')).toBeInTheDocument();
    });
  });

  describe('editor field options', () => {
    it('shows max length and searchable options', () => {
      renderFieldEditor({
        field: { type: { type: 'editor', options: { maxLength: 50000, searchable: true } } },
      });
      expect(screen.getByTestId('editor-max-length')).toBeInTheDocument();
    });
  });

  // ── Accessibility ─────────────────────────────────────────────────────

  it('has accessible labels for move buttons', () => {
    renderFieldEditor({ field: { name: 'title' }, index: 1 });
    expect(screen.getByLabelText('Move title up')).toBeInTheDocument();
    expect(screen.getByLabelText('Move title down')).toBeInTheDocument();
  });

  it('has accessible label for remove button', () => {
    renderFieldEditor({ field: { name: 'title' } });
    expect(screen.getByLabelText('Remove title')).toBeInTheDocument();
  });
});
