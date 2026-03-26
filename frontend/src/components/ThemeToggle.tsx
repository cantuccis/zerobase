import { useState, useRef, useEffect, useCallback } from 'react';
import { useTheme, type Theme } from '../lib/theme';

const THEME_OPTIONS: { value: Theme; label: string }[] = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
  { value: 'system', label: 'System' },
];

function getOptionId(value: Theme) {
  return `theme-option-${value}`;
}

function SunIcon({ className }: { className?: string }) {
  return (
    <svg className={className ?? 'h-4 w-4'} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="5" />
      <line x1="12" y1="1" x2="12" y2="3" />
      <line x1="12" y1="21" x2="12" y2="23" />
      <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
      <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
      <line x1="1" y1="12" x2="3" y2="12" />
      <line x1="21" y1="12" x2="23" y2="12" />
      <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" />
      <line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
    </svg>
  );
}

function MoonIcon({ className }: { className?: string }) {
  return (
    <svg className={className ?? 'h-4 w-4'} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
    </svg>
  );
}

export function ThemeToggle() {
  const { theme, resolvedTheme, setTheme } = useTheme();
  const [open, setOpen] = useState(false);
  const [focusedIndex, setFocusedIndex] = useState(-1);
  const containerRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const optionRefs = useRef<(HTMLLIElement | null)[]>([]);

  const currentIndex = THEME_OPTIONS.findIndex((opt) => opt.value === theme);

  // When dropdown opens, focus the listbox and set focusedIndex to current selection
  useEffect(() => {
    if (open) {
      setFocusedIndex(currentIndex >= 0 ? currentIndex : 0);
      // Focus the listbox so it receives keyboard events
      requestAnimationFrame(() => {
        listRef.current?.focus();
      });
    } else {
      setFocusedIndex(-1);
    }
  }, [open, currentIndex]);

  // Scroll focused option into view
  useEffect(() => {
    if (open && focusedIndex >= 0) {
      optionRefs.current[focusedIndex]?.scrollIntoView?.({ block: 'nearest' });
    }
  }, [open, focusedIndex]);

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const handleListKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      switch (e.key) {
        case 'ArrowDown': {
          e.preventDefault();
          setFocusedIndex((prev) =>
            prev < THEME_OPTIONS.length - 1 ? prev + 1 : prev,
          );
          break;
        }
        case 'ArrowUp': {
          e.preventDefault();
          setFocusedIndex((prev) => (prev > 0 ? prev - 1 : prev));
          break;
        }
        case 'Enter': {
          e.preventDefault();
          if (focusedIndex >= 0 && focusedIndex < THEME_OPTIONS.length) {
            setTheme(THEME_OPTIONS[focusedIndex].value);
            setOpen(false);
          }
          break;
        }
        case 'Escape': {
          e.preventDefault();
          setOpen(false);
          break;
        }
        default:
          break;
      }
    },
    [focusedIndex, setTheme],
  );

  const focusedValue =
    focusedIndex >= 0 && focusedIndex < THEME_OPTIONS.length
      ? THEME_OPTIONS[focusedIndex].value
      : undefined;

  return (
    <div ref={containerRef} className="relative">
      <button
        type="button"
        onClick={() => setOpen((prev) => !prev)}
        className="flex items-center gap-2 border border-primary bg-background px-2 py-1.5 text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary"
        aria-label={`Theme: ${theme}. Click to change.`}
        aria-expanded={open}
        aria-haspopup="listbox"
      >
        {resolvedTheme === 'dark' ? <MoonIcon /> : <SunIcon />}
        <span className="text-xs font-bold uppercase tracking-wider">
          {theme}
        </span>
      </button>

      {open && (
        <ul
          ref={listRef}
          role="listbox"
          aria-label="Select theme"
          aria-activedescendant={focusedValue ? getOptionId(focusedValue) : undefined}
          tabIndex={-1}
          onKeyDown={handleListKeyDown}
          className="absolute right-0 top-full z-50 mt-0 min-w-full border border-primary bg-background animate-slide-down-in focus:outline-none"
        >
          {THEME_OPTIONS.map((opt, index) => {
            const isFocused = index === focusedIndex;
            const isSelected = theme === opt.value;
            return (
              <li
                key={opt.value}
                id={getOptionId(opt.value)}
                ref={(el) => {
                  optionRefs.current[index] = el;
                }}
                role="option"
                aria-selected={isSelected}
              >
                <button
                  type="button"
                  tabIndex={-1}
                  onClick={() => {
                    setTheme(opt.value);
                    setOpen(false);
                  }}
                  onMouseEnter={() => setFocusedIndex(index)}
                  className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs font-bold uppercase tracking-wider transition-colors-fast ${
                    isFocused
                      ? 'bg-primary text-on-primary'
                      : 'text-primary hover:bg-primary hover:text-on-primary'
                  }`}
                >
                  <span className="inline-block w-3 text-center">
                    {isSelected ? '●' : ''}
                  </span>
                  {opt.label}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
