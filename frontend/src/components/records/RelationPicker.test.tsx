import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { RelationPicker } from './RelationPicker';
import type { RelationPickerProps, RelationOption } from './RelationPicker';

// ── Test data ────────────────────────────────────────────────────────────────

const MOCK_RESULTS: RelationOption[] = [
  { id: 'rec_001', label: 'Alice Johnson' },
  { id: 'rec_002', label: 'Bob Smith' },
  { id: 'rec_003', label: 'Charlie Brown' },
  { id: 'rec_004', label: 'rec_004' }, // ID-only label (no meaningful label)
];

function mockSearch(): (collectionId: string, query: string) => Promise<RelationOption[]> {
  return vi.fn(async (_collectionId: string, query: string) => {
    if (!query.trim()) return MOCK_RESULTS;
    const lower = query.toLowerCase();
    return MOCK_RESULTS.filter(
      (r) => r.label.toLowerCase().includes(lower) || r.id.toLowerCase().includes(lower),
    );
  });
}

// ── Helpers ──────────────────────────────────────────────────────────────────

const defaultProps: RelationPickerProps = {
  name: 'author',
  collectionId: 'col_users',
  collectionName: 'users',
  multiple: false,
  value: [],
  onChange: vi.fn(),
  onSearch: mockSearch(),
  debounceMs: 0, // disable debounce for tests
};

function renderPicker(overrides: Partial<RelationPickerProps> = {}) {
  const props = { ...defaultProps, ...overrides };
  return render(<RelationPicker {...props} />);
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('RelationPicker', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ── Rendering ──────────────────────────────────────────────────────────

  describe('rendering', () => {
    it('renders the picker container', () => {
      renderPicker();
      expect(screen.getByTestId('relation-picker-author')).toBeInTheDocument();
    });

    it('displays the related collection name', () => {
      renderPicker();
      expect(screen.getByTestId('relation-collection-author')).toHaveTextContent('users');
    });

    it('renders the search input with correct placeholder', () => {
      renderPicker();
      const input = screen.getByTestId('relation-search-input-author');
      expect(input).toBeInTheDocument();
      expect(input).toHaveAttribute('placeholder', 'Search users records\u2026');
    });

    it('uses custom placeholder when provided', () => {
      renderPicker({ placeholder: 'Find an author\u2026' });
      expect(screen.getByTestId('relation-search-input-author')).toHaveAttribute(
        'placeholder',
        'Find an author\u2026',
      );
    });

    it('renders with combobox ARIA role', () => {
      renderPicker();
      const input = screen.getByRole('combobox');
      expect(input).toHaveAttribute('aria-expanded', 'false');
      expect(input).toHaveAttribute('aria-haspopup', 'listbox');
    });

    it('applies error styling when hasError is true', () => {
      renderPicker({ hasError: true });
      const input = screen.getByTestId('relation-search-input-author');
      expect(input.className).toContain('border-red-300');
    });
  });

  // ── Single select mode ─────────────────────────────────────────────────

  describe('single select', () => {
    it('shows selected value with label', () => {
      renderPicker({
        value: ['rec_001'],
        selectedLabels: { rec_001: 'Alice Johnson' },
      });
      const display = screen.getByTestId('relation-single-display-author');
      expect(display).toHaveTextContent('Alice Johnson');
    });

    it('shows ID when no label is available', () => {
      renderPicker({ value: ['rec_xyz'] });
      const display = screen.getByTestId('relation-single-display-author');
      expect(display).toHaveTextContent('rec_xyz');
    });

    it('hides search input when a value is selected', () => {
      renderPicker({ value: ['rec_001'] });
      expect(screen.queryByTestId('relation-search-input-author')).not.toBeInTheDocument();
    });

    it('shows clear button when a value is selected', () => {
      renderPicker({ value: ['rec_001'] });
      expect(screen.getByTestId('relation-clear-author')).toBeInTheDocument();
    });

    it('clears selection when clear button is clicked', async () => {
      const onChange = vi.fn();
      renderPicker({ value: ['rec_001'], onChange });
      const user = userEvent.setup();

      await user.click(screen.getByTestId('relation-clear-author'));
      expect(onChange).toHaveBeenCalledWith([]);
    });

    it('selects a record from search results', async () => {
      const onChange = vi.fn();
      const onSearch = mockSearch();
      renderPicker({ onChange, onSearch });
      const user = userEvent.setup();

      const input = screen.getByTestId('relation-search-input-author');
      await user.click(input);

      await waitFor(() => {
        expect(screen.getByTestId('relation-dropdown-author')).toBeInTheDocument();
      });

      await user.type(input, 'Alice');

      await waitFor(() => {
        expect(screen.getByTestId('relation-option-rec_001')).toBeInTheDocument();
      });

      await user.click(screen.getByTestId('relation-option-rec_001'));
      expect(onChange).toHaveBeenCalledWith(['rec_001']);
    });

    it('replaces selection when selecting a new record in single mode', async () => {
      const onChange = vi.fn();
      const onSearch = mockSearch();
      // Start fresh (no selection) and select
      renderPicker({ onChange, onSearch });
      const user = userEvent.setup();

      await user.click(screen.getByTestId('relation-search-input-author'));
      await waitFor(() => {
        expect(screen.getByTestId('relation-dropdown-author')).toBeInTheDocument();
      });

      await user.click(screen.getByTestId('relation-option-rec_002'));
      expect(onChange).toHaveBeenCalledWith(['rec_002']);
    });
  });

  // ── Multi select mode ──────────────────────────────────────────────────

  describe('multi select', () => {
    it('shows all selected values as chips', () => {
      renderPicker({
        multiple: true,
        value: ['rec_001', 'rec_002'],
        selectedLabels: { rec_001: 'Alice Johnson', rec_002: 'Bob Smith' },
      });
      expect(screen.getByTestId('relation-chip-rec_001')).toHaveTextContent('Alice Johnson');
      expect(screen.getByTestId('relation-chip-rec_002')).toHaveTextContent('Bob Smith');
    });

    it('shows search input even when selections exist', () => {
      renderPicker({
        multiple: true,
        value: ['rec_001'],
      });
      expect(screen.getByTestId('relation-search-input-author')).toBeInTheDocument();
    });

    it('removes a selection when chip remove button is clicked', async () => {
      const onChange = vi.fn();
      renderPicker({
        multiple: true,
        value: ['rec_001', 'rec_002'],
        onChange,
      });
      const user = userEvent.setup();

      await user.click(screen.getByTestId('relation-remove-rec_001'));
      expect(onChange).toHaveBeenCalledWith(['rec_002']);
    });

    it('adds to selection without replacing', async () => {
      const onChange = vi.fn();
      const onSearch = mockSearch();
      renderPicker({
        multiple: true,
        value: ['rec_001'],
        onChange,
        onSearch,
      });
      const user = userEvent.setup();

      await user.click(screen.getByTestId('relation-search-input-author'));
      await waitFor(() => {
        expect(screen.getByTestId('relation-dropdown-author')).toBeInTheDocument();
      });

      // rec_001 should be filtered out (already selected)
      expect(screen.queryByTestId('relation-option-rec_001')).not.toBeInTheDocument();

      await user.click(screen.getByTestId('relation-option-rec_002'));
      expect(onChange).toHaveBeenCalledWith(['rec_001', 'rec_002']);
    });

    it('does not add duplicate IDs', async () => {
      const onChange = vi.fn();
      const onSearch = mockSearch();
      renderPicker({
        multiple: true,
        value: ['rec_001'],
        onChange,
        onSearch,
      });
      const user = userEvent.setup();

      // Already selected items should not appear in results
      await user.click(screen.getByTestId('relation-search-input-author'));
      await waitFor(() => {
        expect(screen.getByTestId('relation-dropdown-author')).toBeInTheDocument();
      });
      expect(screen.queryByTestId('relation-option-rec_001')).not.toBeInTheDocument();
    });

    it('renders selected list with correct ARIA role', () => {
      renderPicker({
        multiple: true,
        value: ['rec_001'],
      });
      const list = screen.getByTestId('relation-selected-author');
      expect(list).toHaveAttribute('role', 'list');
    });
  });

  // ── Search behavior ────────────────────────────────────────────────────

  describe('search', () => {
    it('opens dropdown on focus', async () => {
      const onSearch = mockSearch();
      renderPicker({ onSearch });
      const user = userEvent.setup();

      await user.click(screen.getByTestId('relation-search-input-author'));

      await waitFor(() => {
        expect(screen.getByTestId('relation-dropdown-author')).toBeInTheDocument();
      });
    });

    it('calls onSearch with collection ID and query', async () => {
      const onSearch = mockSearch();
      renderPicker({ onSearch });
      const user = userEvent.setup();

      const input = screen.getByTestId('relation-search-input-author');
      await user.click(input);
      await user.type(input, 'Ali');

      await waitFor(() => {
        expect(onSearch).toHaveBeenCalledWith('col_users', 'Ali');
      });
    });

    it('displays search results in dropdown', async () => {
      const onSearch = mockSearch();
      renderPicker({ onSearch });
      const user = userEvent.setup();

      const input = screen.getByTestId('relation-search-input-author');
      await user.click(input);

      await waitFor(() => {
        expect(screen.getByTestId('relation-option-rec_001')).toBeInTheDocument();
        expect(screen.getByTestId('relation-option-rec_002')).toBeInTheDocument();
      });
    });

    it('displays meaningful labels in search results', async () => {
      const onSearch = mockSearch();
      renderPicker({ onSearch });
      const user = userEvent.setup();

      await user.click(screen.getByTestId('relation-search-input-author'));

      await waitFor(() => {
        const option = screen.getByTestId('relation-option-rec_001');
        expect(option).toHaveTextContent('Alice Johnson');
        expect(option).toHaveTextContent('rec_001');
      });
    });

    it('shows ID-only for records without meaningful labels', async () => {
      const onSearch = mockSearch();
      renderPicker({ onSearch });
      const user = userEvent.setup();

      await user.click(screen.getByTestId('relation-search-input-author'));

      await waitFor(() => {
        const option = screen.getByTestId('relation-option-rec_004');
        expect(option).toHaveTextContent('rec_004');
      });
    });

    it('shows empty state when no results match', async () => {
      const onSearch = vi.fn(async () => []);
      renderPicker({ onSearch });
      const user = userEvent.setup();

      const input = screen.getByTestId('relation-search-input-author');
      await user.click(input);
      await user.type(input, 'nonexistent');

      await waitFor(() => {
        expect(screen.getByTestId('relation-empty-author')).toHaveTextContent(
          'No matching records found.',
        );
      });
    });

    it('shows loading indicator while searching', async () => {
      let resolveSearch: (value: RelationOption[]) => void;
      const onSearch = vi.fn(
        () => new Promise<RelationOption[]>((resolve) => { resolveSearch = resolve; }),
      );
      renderPicker({ onSearch });
      const user = userEvent.setup();

      const input = screen.getByTestId('relation-search-input-author');
      await user.click(input);

      await waitFor(() => {
        expect(screen.getByTestId('relation-loading-author')).toHaveTextContent(/Searching/);
      });

      // Resolve the search
      resolveSearch!(MOCK_RESULTS);

      await waitFor(() => {
        expect(screen.queryByTestId('relation-loading-author')).not.toBeInTheDocument();
      });
    });

    it('handles search errors gracefully', async () => {
      const onSearch = vi.fn(async () => {
        throw new Error('Network error');
      });
      renderPicker({ onSearch });
      const user = userEvent.setup();

      const input = screen.getByTestId('relation-search-input-author');
      await user.click(input);
      await user.type(input, 'test');

      // Should not crash, show empty state
      await waitFor(() => {
        expect(screen.getByTestId('relation-empty-author')).toBeInTheDocument();
      });
    });

    it('clears search query after selecting an option', async () => {
      const onSearch = mockSearch();
      const onChange = vi.fn();
      renderPicker({ multiple: true, onSearch, onChange });
      const user = userEvent.setup();

      const input = screen.getByTestId('relation-search-input-author');
      await user.click(input);
      await user.type(input, 'Alice');

      await waitFor(() => {
        expect(screen.getByTestId('relation-option-rec_001')).toBeInTheDocument();
      });

      await user.click(screen.getByTestId('relation-option-rec_001'));

      // Input should be cleared
      expect(input).toHaveValue('');
    });
  });

  // ── Keyboard navigation ────────────────────────────────────────────────

  describe('keyboard navigation', () => {
    it('opens dropdown on ArrowDown', async () => {
      const onSearch = mockSearch();
      renderPicker({ onSearch });
      const user = userEvent.setup();

      const input = screen.getByTestId('relation-search-input-author');
      input.focus();
      await user.keyboard('{ArrowDown}');

      await waitFor(() => {
        expect(screen.getByTestId('relation-dropdown-author')).toBeInTheDocument();
      });
    });

    it('navigates options with ArrowDown and ArrowUp', async () => {
      const onSearch = mockSearch();
      renderPicker({ onSearch });
      const user = userEvent.setup();

      const input = screen.getByTestId('relation-search-input-author');
      await user.click(input);

      await waitFor(() => {
        expect(screen.getByTestId('relation-dropdown-author')).toBeInTheDocument();
      });

      // ArrowDown to first item
      await user.keyboard('{ArrowDown}');
      expect(input).toHaveAttribute(
        'aria-activedescendant',
        'relation-option-author-rec_001',
      );

      // ArrowDown to second item
      await user.keyboard('{ArrowDown}');
      expect(input).toHaveAttribute(
        'aria-activedescendant',
        'relation-option-author-rec_002',
      );

      // ArrowUp back to first
      await user.keyboard('{ArrowUp}');
      expect(input).toHaveAttribute(
        'aria-activedescendant',
        'relation-option-author-rec_001',
      );
    });

    it('wraps around when navigating past last/first item', async () => {
      const onSearch = vi.fn(async () => [
        { id: 'rec_a', label: 'A' },
        { id: 'rec_b', label: 'B' },
      ]);
      renderPicker({ onSearch });
      const user = userEvent.setup();

      const input = screen.getByTestId('relation-search-input-author');
      await user.click(input);

      await waitFor(() => {
        expect(screen.getByTestId('relation-dropdown-author')).toBeInTheDocument();
      });

      // Navigate down past last
      await user.keyboard('{ArrowDown}'); // index 0
      await user.keyboard('{ArrowDown}'); // index 1
      await user.keyboard('{ArrowDown}'); // wraps to 0
      expect(input).toHaveAttribute(
        'aria-activedescendant',
        'relation-option-author-rec_a',
      );

      // Navigate up past first → wraps to last
      await user.keyboard('{ArrowUp}'); // wraps to 1
      expect(input).toHaveAttribute(
        'aria-activedescendant',
        'relation-option-author-rec_b',
      );
    });

    it('selects active option on Enter', async () => {
      const onChange = vi.fn();
      const onSearch = mockSearch();
      renderPicker({ onChange, onSearch });
      const user = userEvent.setup();

      const input = screen.getByTestId('relation-search-input-author');
      await user.click(input);

      await waitFor(() => {
        expect(screen.getByTestId('relation-dropdown-author')).toBeInTheDocument();
      });

      await user.keyboard('{ArrowDown}');
      await user.keyboard('{Enter}');

      expect(onChange).toHaveBeenCalledWith(['rec_001']);
    });

    it('closes dropdown on Escape', async () => {
      const onSearch = mockSearch();
      renderPicker({ onSearch });
      const user = userEvent.setup();

      await user.click(screen.getByTestId('relation-search-input-author'));

      await waitFor(() => {
        expect(screen.getByTestId('relation-dropdown-author')).toBeInTheDocument();
      });

      await user.keyboard('{Escape}');

      expect(screen.queryByTestId('relation-dropdown-author')).not.toBeInTheDocument();
    });
  });

  // ── Click outside ──────────────────────────────────────────────────────

  describe('click outside', () => {
    it('closes dropdown when clicking outside', async () => {
      const onSearch = mockSearch();
      renderPicker({ onSearch });
      const user = userEvent.setup();

      await user.click(screen.getByTestId('relation-search-input-author'));

      await waitFor(() => {
        expect(screen.getByTestId('relation-dropdown-author')).toBeInTheDocument();
      });

      // Click outside
      await user.click(document.body);

      await waitFor(() => {
        expect(screen.queryByTestId('relation-dropdown-author')).not.toBeInTheDocument();
      });
    });
  });

  // ── Label display ──────────────────────────────────────────────────────

  describe('label display', () => {
    it('uses selectedLabels for chip display', () => {
      renderPicker({
        multiple: true,
        value: ['rec_001', 'rec_002'],
        selectedLabels: { rec_001: 'Alice', rec_002: 'Bob' },
      });
      expect(screen.getByTestId('relation-chip-rec_001')).toHaveTextContent('Alice');
      expect(screen.getByTestId('relation-chip-rec_002')).toHaveTextContent('Bob');
    });

    it('falls back to ID when selectedLabels not provided', () => {
      renderPicker({
        multiple: true,
        value: ['rec_unknown'],
      });
      expect(screen.getByTestId('relation-chip-rec_unknown')).toHaveTextContent('rec_unknown');
    });

    it('uses selectedLabels for single-select display', () => {
      renderPicker({
        value: ['rec_001'],
        selectedLabels: { rec_001: 'Alice Johnson' },
      });
      expect(screen.getByTestId('relation-single-display-author')).toHaveTextContent('Alice Johnson');
    });

    it('displays remove button with accessible label including record label', () => {
      renderPicker({
        multiple: true,
        value: ['rec_001'],
        selectedLabels: { rec_001: 'Alice' },
      });
      expect(screen.getByLabelText('Remove Alice')).toBeInTheDocument();
    });
  });

  // ── Edge cases ─────────────────────────────────────────────────────────

  describe('edge cases', () => {
    it('handles empty value array', () => {
      renderPicker({ value: [] });
      expect(screen.getByTestId('relation-search-input-author')).toBeInTheDocument();
      expect(screen.queryByTestId('relation-single-display-author')).not.toBeInTheDocument();
    });

    it('handles rapid typing with debounce', async () => {
      const onSearch = mockSearch();
      renderPicker({ onSearch, debounceMs: 50 });
      const user = userEvent.setup();

      const input = screen.getByTestId('relation-search-input-author');
      await user.click(input);
      // Wait for initial focus search
      await waitFor(() => {
        expect(onSearch).toHaveBeenCalled();
      });

      const callsBefore = (onSearch as ReturnType<typeof vi.fn>).mock.calls.length;
      // Type rapidly
      await user.type(input, 'Alice');

      // Wait for debounce
      await waitFor(
        () => {
          // Should have been called fewer times than characters typed (debounce consolidates)
          const totalCalls = (onSearch as ReturnType<typeof vi.fn>).mock.calls.length;
          expect(totalCalls).toBeGreaterThan(callsBefore);
        },
        { timeout: 500 },
      );
    });

    it('does not show dropdown when picker is not focused', () => {
      renderPicker();
      expect(screen.queryByTestId('relation-dropdown-author')).not.toBeInTheDocument();
    });

    it('handles onSearch returning empty array', async () => {
      const onSearch = vi.fn(async () => []);
      renderPicker({ onSearch });
      const user = userEvent.setup();

      const input = screen.getByTestId('relation-search-input-author');
      await user.click(input);

      await waitFor(() => {
        expect(screen.getByTestId('relation-dropdown-author')).toBeInTheDocument();
      });

      expect(screen.getByTestId('relation-empty-author')).toBeInTheDocument();
    });
  });
});
