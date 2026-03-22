import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ThemeToggle } from './ThemeToggle';
import { ThemeProvider } from '../lib/theme';

// ── Mock matchMedia ──────────────────────────────────────────────────────────

function setupMatchMedia(prefersDark: boolean) {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: prefersDark,
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  });
}

function renderToggle() {
  return render(
    <ThemeProvider>
      <ThemeToggle />
    </ThemeProvider>,
  );
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('ThemeToggle', () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.classList.remove('dark');
    setupMatchMedia(false);
  });

  it('renders the icon button and select dropdown', () => {
    renderToggle();

    expect(screen.getByRole('button', { name: /theme/i })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: /select theme/i })).toBeInTheDocument();
  });

  it('shows Light, Dark, and System options in select', () => {
    renderToggle();

    const select = screen.getByRole('combobox', { name: /select theme/i });
    const options = select.querySelectorAll('option');

    expect(options).toHaveLength(3);
    expect(options[0]).toHaveTextContent('Light');
    expect(options[1]).toHaveTextContent('Dark');
    expect(options[2]).toHaveTextContent('System');
  });

  it('defaults to system theme', () => {
    renderToggle();

    const select = screen.getByRole('combobox', { name: /select theme/i }) as HTMLSelectElement;
    expect(select.value).toBe('system');
  });

  it('switches to dark when selecting dark from dropdown', async () => {
    const user = userEvent.setup();
    renderToggle();

    const select = screen.getByRole('combobox', { name: /select theme/i });
    await user.selectOptions(select, 'dark');

    expect((select as HTMLSelectElement).value).toBe('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
    expect(localStorage.getItem('zerobase-theme')).toBe('dark');
  });

  it('switches to light when selecting light from dropdown', async () => {
    const user = userEvent.setup();
    localStorage.setItem('zerobase-theme', 'dark');

    renderToggle();

    const select = screen.getByRole('combobox', { name: /select theme/i });
    await user.selectOptions(select, 'light');

    expect((select as HTMLSelectElement).value).toBe('light');
    expect(document.documentElement.classList.contains('dark')).toBe(false);
    expect(localStorage.getItem('zerobase-theme')).toBe('light');
  });

  it('cycles theme when clicking the icon button', async () => {
    const user = userEvent.setup();
    renderToggle();

    const button = screen.getByRole('button', { name: /theme/i });
    const select = screen.getByRole('combobox', { name: /select theme/i }) as HTMLSelectElement;

    // Default is system → clicking should go to next (light)
    // system -> light -> dark -> system
    await user.click(button);
    expect(select.value).toBe('light');

    await user.click(button);
    expect(select.value).toBe('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);

    await user.click(button);
    expect(select.value).toBe('system');
  });

  it('persists preference after selecting from dropdown', async () => {
    const user = userEvent.setup();
    renderToggle();

    const select = screen.getByRole('combobox', { name: /select theme/i });
    await user.selectOptions(select, 'dark');

    expect(localStorage.getItem('zerobase-theme')).toBe('dark');

    await user.selectOptions(select, 'light');
    expect(localStorage.getItem('zerobase-theme')).toBe('light');

    await user.selectOptions(select, 'system');
    expect(localStorage.getItem('zerobase-theme')).toBe('system');
  });
});
