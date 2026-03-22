/**
 * Reusable relation picker component for selecting records from a target collection.
 *
 * Supports single and multi-select modes with:
 * - Debounced async search against target collection
 * - Display of meaningful record labels (not just IDs)
 * - Keyboard navigation (ArrowUp/Down, Enter, Escape)
 * - Accessible ARIA attributes (combobox pattern)
 * - Remove individual selections
 * - Empty-query initial load on focus
 */

import { useState, useCallback, useRef, useEffect } from 'react';

// ── Types ────────────────────────────────────────────────────────────────────

export interface RelationOption {
  id: string;
  label: string;
}

export interface RelationPickerProps {
  /** Unique name for the picker (used in IDs and test IDs). */
  name: string;
  /** Target collection ID to search in. */
  collectionId: string;
  /** Human-readable name of the target collection. */
  collectionName: string;
  /** Whether multiple records can be selected. */
  multiple: boolean;
  /** Currently selected record IDs. */
  value: string[];
  /** Labels for selected IDs. Map of id → label string. */
  selectedLabels?: Record<string, string>;
  /** Callback when selection changes. */
  onChange: (ids: string[]) => void;
  /** Async search function. Called with (collectionId, query). */
  onSearch: (collectionId: string, query: string) => Promise<RelationOption[]>;
  /** Placeholder text for the search input. */
  placeholder?: string;
  /** Whether the picker is in an error state. */
  hasError?: boolean;
  /** Debounce delay in ms for search input. Default: 250. */
  debounceMs?: number;
}

// ── Constants ────────────────────────────────────────────────────────────────

const DEFAULT_DEBOUNCE_MS = 250;

const inputClasses =
  'w-full rounded-md border border-gray-300 dark:border-gray-600 px-3 py-2 text-sm placeholder-gray-400 dark:placeholder-gray-500 focus:border-blue-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-blue-500';

const errorInputClasses =
  'w-full rounded-md border border-red-300 dark:border-red-700 px-3 py-2 text-sm placeholder-gray-400 dark:placeholder-gray-500 focus:border-red-500 focus-visible:outline-none focus-visible:ring-1 focus:ring-red-500';

// ── Component ────────────────────────────────────────────────────────────────

export function RelationPicker({
  name,
  collectionId,
  collectionName,
  multiple,
  value,
  selectedLabels = {},
  onChange,
  onSearch,
  placeholder,
  hasError = false,
  debounceMs = DEFAULT_DEBOUNCE_MS,
}: RelationPickerProps) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<RelationOption[]>([]);
  const [loading, setLoading] = useState(false);
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);

  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const debounceTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const searchId = useRef(0);

  const listboxId = `relation-listbox-${name}`;
  const inputId = `relation-search-${name}`;

  // ── Search logic ─────────────────────────────────────────────────────────

  const executeSearch = useCallback(
    async (searchQuery: string) => {
      const currentId = ++searchId.current;
      setLoading(true);
      try {
        const items = await onSearch(collectionId, searchQuery);
        // Only apply results if this is still the latest search
        if (currentId === searchId.current) {
          setResults(items);
          setActiveIndex(-1);
        }
      } catch {
        if (currentId === searchId.current) {
          setResults([]);
        }
      } finally {
        if (currentId === searchId.current) {
          setLoading(false);
        }
      }
    },
    [onSearch, collectionId],
  );

  const debouncedSearch = useCallback(
    (searchQuery: string) => {
      if (debounceTimer.current) {
        clearTimeout(debounceTimer.current);
      }
      debounceTimer.current = setTimeout(() => {
        executeSearch(searchQuery);
      }, debounceMs);
    },
    [executeSearch, debounceMs],
  );

  // Cleanup debounce on unmount
  useEffect(() => {
    return () => {
      if (debounceTimer.current) {
        clearTimeout(debounceTimer.current);
      }
    };
  }, []);

  // ── Event handlers ───────────────────────────────────────────────────────

  const handleInputChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const newQuery = e.target.value;
      setQuery(newQuery);
      setOpen(true);
      debouncedSearch(newQuery);
    },
    [debouncedSearch],
  );

  const handleFocus = useCallback(() => {
    setOpen(true);
    // Load initial results on focus if no results yet
    if (results.length === 0 && !loading) {
      executeSearch(query);
    }
  }, [executeSearch, query, results.length, loading]);

  const selectOption = useCallback(
    (id: string) => {
      if (multiple) {
        if (!value.includes(id)) {
          onChange([...value, id]);
        }
      } else {
        onChange([id]);
      }
      setQuery('');
      setResults([]);
      setOpen(false);
      setActiveIndex(-1);
    },
    [multiple, value, onChange],
  );

  const removeOption = useCallback(
    (id: string) => {
      onChange(value.filter((v) => v !== id));
    },
    [value, onChange],
  );

  const clearAll = useCallback(() => {
    onChange([]);
    setQuery('');
    inputRef.current?.focus();
  }, [onChange]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (!open) {
        if (e.key === 'ArrowDown' || e.key === 'Enter') {
          e.preventDefault();
          setOpen(true);
          if (results.length === 0) {
            executeSearch(query);
          }
        }
        return;
      }

      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault();
          setActiveIndex((prev) => {
            const next = prev < results.length - 1 ? prev + 1 : 0;
            scrollToIndex(next);
            return next;
          });
          break;
        case 'ArrowUp':
          e.preventDefault();
          setActiveIndex((prev) => {
            const next = prev > 0 ? prev - 1 : results.length - 1;
            scrollToIndex(next);
            return next;
          });
          break;
        case 'Enter':
          e.preventDefault();
          if (activeIndex >= 0 && activeIndex < results.length) {
            selectOption(results[activeIndex].id);
          }
          break;
        case 'Escape':
          e.preventDefault();
          setOpen(false);
          setActiveIndex(-1);
          break;
      }
    },
    [open, results, activeIndex, selectOption, executeSearch, query],
  );

  const scrollToIndex = (index: number) => {
    const list = listRef.current;
    if (!list) return;
    const item = list.children[index] as HTMLElement | undefined;
    if (item && typeof item.scrollIntoView === 'function') {
      item.scrollIntoView({ block: 'nearest' });
    }
  };

  // ── Click outside to close ───────────────────────────────────────────────

  useEffect(() => {
    if (!open) return;

    const handleClickOutside = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
        setActiveIndex(-1);
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [open]);

  // ── Filter out already-selected from results ────────────────────────────

  const filteredResults = multiple
    ? results.filter((r) => !value.includes(r.id))
    : results;

  // ── Render helpers ───────────────────────────────────────────────────────

  const getLabelForId = (id: string): string => {
    // Check provided labels first
    if (selectedLabels[id] && selectedLabels[id] !== id) {
      return selectedLabels[id];
    }
    // Check current search results
    const fromResults = results.find((r) => r.id === id);
    if (fromResults && fromResults.label !== fromResults.id) {
      return fromResults.label;
    }
    return id;
  };

  const activeOptionId =
    activeIndex >= 0 && activeIndex < filteredResults.length
      ? `relation-option-${name}-${filteredResults[activeIndex].id}`
      : undefined;

  // ── Render ───────────────────────────────────────────────────────────────

  return (
    <div ref={containerRef} className="space-y-1.5" data-testid={`relation-picker-${name}`}>
      {/* Collection label */}
      <p className="text-xs text-gray-400 dark:text-gray-500">
        Related to: <span className="font-medium" data-testid={`relation-collection-${name}`}>{collectionName}</span>
      </p>

      {/* Selected items (multi-select mode) */}
      {multiple && value.length > 0 && (
        <div className="flex flex-wrap gap-1.5" data-testid={`relation-selected-${name}`} role="list" aria-label="Selected records">
          {value.map((id) => {
            const label = getLabelForId(id);
            return (
              <span
                key={id}
                role="listitem"
                className="inline-flex items-center gap-1 rounded-md bg-blue-50 dark:bg-blue-900/30 px-2 py-1 text-xs text-blue-700 dark:text-blue-400 border border-blue-200 dark:border-blue-800"
                data-testid={`relation-chip-${id}`}
              >
                <span className="max-w-[180px] truncate" title={label !== id ? `${label} (${id})` : id}>
                  {label !== id ? label : <span className="font-mono">{id}</span>}
                </span>
                <button
                  type="button"
                  onClick={() => removeOption(id)}
                  className="ml-0.5 rounded-sm text-blue-400 dark:text-blue-500 hover:bg-blue-100 dark:hover:bg-blue-800 hover:text-blue-600 dark:hover:text-blue-400 focus-visible:outline-none focus-visible:ring-1 focus:ring-blue-400"
                  aria-label={`Remove ${label}`}
                  data-testid={`relation-remove-${id}`}
                >
                  <svg className="h-3 w-3" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
                    <path d="M3 3l6 6M9 3l-6 6" />
                  </svg>
                </button>
              </span>
            );
          })}
        </div>
      )}

      {/* Single-select display */}
      {!multiple && value.length === 1 && (
        <div className="flex items-center gap-2" data-testid={`relation-single-display-${name}`}>
          <span className="inline-flex items-center gap-1 rounded-md bg-blue-50 dark:bg-blue-900/30 px-2.5 py-1 text-sm text-blue-700 dark:text-blue-400 border border-blue-200 dark:border-blue-800">
            <span className="max-w-[240px] truncate" title={getLabelForId(value[0])}>
              {getLabelForId(value[0]) !== value[0] ? getLabelForId(value[0]) : <span className="font-mono text-xs">{value[0]}</span>}
            </span>
          </span>
          <button
            type="button"
            onClick={clearAll}
            className="text-xs text-gray-500 dark:text-gray-400 hover:text-red-600 dark:hover:text-red-400 transition-colors"
            data-testid={`relation-clear-${name}`}
          >
            Clear
          </button>
        </div>
      )}

      {/* Search input (always shown in multi, hidden when value set in single) */}
      {(multiple || value.length === 0) && (
        <div className="relative">
          <input
            ref={inputRef}
            id={inputId}
            type="text"
            value={query}
            onChange={handleInputChange}
            onFocus={handleFocus}
            onKeyDown={handleKeyDown}
            className={hasError ? errorInputClasses : inputClasses}
            placeholder={placeholder ?? `Search ${collectionName} records\u2026`}
            autoComplete="off"
            role="combobox"
            aria-expanded={open}
            aria-controls={listboxId}
            aria-activedescendant={activeOptionId}
            aria-haspopup="listbox"
            data-testid={`relation-search-input-${name}`}
          />

          {/* Dropdown */}
          {open && (
            <ul
              ref={listRef}
              id={listboxId}
              role="listbox"
              aria-label={`${collectionName} records`}
              className="absolute z-20 mt-1 max-h-56 w-full overflow-y-auto rounded-md border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 py-1 shadow-lg"
              data-testid={`relation-dropdown-${name}`}
            >
              {loading && filteredResults.length === 0 && (
                <li className="px-3 py-2 text-sm text-gray-500 dark:text-gray-400" role="presentation" data-testid={`relation-loading-${name}`}>
                  Searching\u2026
                </li>
              )}

              {!loading && filteredResults.length === 0 && (
                <li className="px-3 py-2 text-sm text-gray-500 dark:text-gray-400" role="presentation" data-testid={`relation-empty-${name}`}>
                  {query.trim() ? 'No matching records found.' : 'Type to search records\u2026'}
                </li>
              )}

              {filteredResults.map((option, index) => {
                const isActive = index === activeIndex;
                const isAlreadySelected = value.includes(option.id);
                return (
                  <li
                    key={option.id}
                    id={`relation-option-${name}-${option.id}`}
                    role="option"
                    aria-selected={isAlreadySelected}
                    className={`cursor-pointer px-3 py-2 text-sm transition-colors ${
                      isActive ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-400' : 'text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700'
                    }`}
                    onClick={() => selectOption(option.id)}
                    onMouseEnter={() => setActiveIndex(index)}
                    data-testid={`relation-option-${option.id}`}
                  >
                    <span className="block truncate">
                      {option.label !== option.id ? (
                        <>
                          <span>{option.label}</span>
                          <span className="ml-2 font-mono text-xs text-gray-400 dark:text-gray-500">{option.id}</span>
                        </>
                      ) : (
                        <span className="font-mono text-xs">{option.id}</span>
                      )}
                    </span>
                  </li>
                );
              })}

              {loading && filteredResults.length > 0 && (
                <li className="px-3 py-1.5 text-xs text-gray-400 dark:text-gray-500" role="presentation">
                  Loading more\u2026
                </li>
              )}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
