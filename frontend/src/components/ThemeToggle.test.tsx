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

/** Click the trigger button to open the dropdown, then click an option by name. */
async function selectThemeOption(user: ReturnType<typeof userEvent.setup>, name: string) {
  const trigger = screen.getByRole('button', { name: /theme/i });
  await user.click(trigger);
  const option = screen.getByRole('option', { name: new RegExp(name, 'i') });
  await user.click(option.querySelector('button')!);
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('ThemeToggle', () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.classList.remove('dark');
    setupMatchMedia(false);
  });

  it('renders the trigger button with theme label', () => {
    renderToggle();

    const trigger = screen.getByRole('button', { name: /theme/i });
    expect(trigger).toBeInTheDocument();
    expect(trigger).toHaveAttribute('aria-haspopup', 'listbox');
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
  });

  it('shows Light, Dark, and System options when opened', async () => {
    const user = userEvent.setup();
    renderToggle();

    const trigger = screen.getByRole('button', { name: /theme/i });
    await user.click(trigger);

    const listbox = screen.getByRole('listbox', { name: /select theme/i });
    expect(listbox).toBeInTheDocument();

    const options = screen.getAllByRole('option');
    expect(options).toHaveLength(3);
    expect(options[0]).toHaveTextContent('Light');
    expect(options[1]).toHaveTextContent('Dark');
    expect(options[2]).toHaveTextContent('System');
  });

  it('defaults to system theme', () => {
    renderToggle();

    const trigger = screen.getByRole('button', { name: /theme:.*system/i });
    expect(trigger).toBeInTheDocument();
  });

  it('switches to dark when selecting dark from dropdown', async () => {
    const user = userEvent.setup();
    renderToggle();

    await selectThemeOption(user, 'dark');

    expect(screen.getByRole('button', { name: /theme:.*dark/i })).toBeInTheDocument();
    expect(document.documentElement.classList.contains('dark')).toBe(true);
    expect(localStorage.getItem('zerobase-theme')).toBe('dark');
  });

  it('switches to light when selecting light from dropdown', async () => {
    const user = userEvent.setup();
    localStorage.setItem('zerobase-theme', 'dark');

    renderToggle();

    await selectThemeOption(user, 'light');

    expect(screen.getByRole('button', { name: /theme:.*light/i })).toBeInTheDocument();
    expect(document.documentElement.classList.contains('dark')).toBe(false);
    expect(localStorage.getItem('zerobase-theme')).toBe('light');
  });

  it('closes dropdown after selecting an option', async () => {
    const user = userEvent.setup();
    renderToggle();

    const trigger = screen.getByRole('button', { name: /theme/i });
    await user.click(trigger);

    // Listbox should be visible
    expect(screen.getByRole('listbox')).toBeInTheDocument();

    // Select an option
    const darkOption = screen.getByRole('option', { name: /dark/i });
    await user.click(darkOption.querySelector('button')!);

    // Listbox should be closed
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
  });

  it('persists preference after selecting from dropdown', async () => {
    const user = userEvent.setup();
    renderToggle();

    await selectThemeOption(user, 'dark');
    expect(localStorage.getItem('zerobase-theme')).toBe('dark');

    await selectThemeOption(user, 'light');
    expect(localStorage.getItem('zerobase-theme')).toBe('light');

    await selectThemeOption(user, 'system');
    expect(localStorage.getItem('zerobase-theme')).toBe('system');
  });
});
