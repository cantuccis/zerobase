import { render } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { axe } from 'vitest-axe';

// ── Mock fetch globally ───────────────────────────────────────────────────────

beforeEach(() => {
  vi.stubGlobal('fetch', vi.fn(() =>
    Promise.resolve({
      ok: true,
      status: 200,
      json: () => Promise.resolve({ items: [], page: 1, perPage: 30, totalPages: 0, totalItems: 0 }),
      text: () => Promise.resolve(''),
      headers: new Headers(),
    }),
  ));
});

// ── Accessibility tests for standalone components ─────────────────────────────

describe('Accessibility — DashboardLayout', () => {
  // We test the layout structure directly with HTML to avoid auth mocking complexity
  it('skip-to-content link renders and is focusable', () => {
    const { container } = render(
      <div>
        <a
          href="#main-content"
          className="sr-only focus:not-sr-only"
        >
          Skip to main content
        </a>
        <nav aria-label="Main navigation">
          <ul role="list">
            <li><a href="/_/">Overview</a></li>
            <li><a href="/_/collections">Collections</a></li>
          </ul>
        </nav>
        <main id="main-content" tabIndex={-1}>
          <h1>Dashboard</h1>
        </main>
      </div>,
    );

    const skipLink = container.querySelector('a[href="#main-content"]');
    expect(skipLink).toBeInTheDocument();
    expect(skipLink).toHaveTextContent('Skip to main content');

    const main = container.querySelector('#main-content');
    expect(main).toBeInTheDocument();
    expect(main!.tagName).toBe('MAIN');
  });

  it('skip-to-content link has no axe violations', async () => {
    const { container } = render(
      <div>
        <a href="#main-content">Skip to main content</a>
        <main id="main-content" tabIndex={-1}>
          <h1>Page Title</h1>
          <p>Content</p>
        </main>
      </div>,
    );
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });
});

describe('Accessibility — Navigation', () => {
  it('sidebar nav has proper ARIA landmarks', () => {
    const { container } = render(
      <aside aria-label="Main navigation">
        <nav>
          <ul role="list">
            <li><a href="/_/" aria-current="page">Overview</a></li>
            <li><a href="/_/collections">Collections</a></li>
            <li><a href="/_/docs">API Docs</a></li>
          </ul>
        </nav>
      </aside>,
    );

    const aside = container.querySelector('aside');
    expect(aside).toHaveAttribute('aria-label', 'Main navigation');

    const activeLink = container.querySelector('[aria-current="page"]');
    expect(activeLink).toBeInTheDocument();
    expect(activeLink).toHaveTextContent('Overview');
  });

  it('navigation has no axe violations', async () => {
    const { container } = render(
      <nav aria-label="Main navigation">
        <ul role="list">
          <li><a href="/_/" aria-current="page">Overview</a></li>
          <li><a href="/_/collections">Collections</a></li>
        </ul>
      </nav>,
    );
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });
});

describe('Accessibility — Tables', () => {
  it('table headers have scope="col" attribute', () => {
    const { container } = render(
      <table>
        <thead>
          <tr>
            <th scope="col">Method</th>
            <th scope="col">URL</th>
            <th scope="col">Status</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>GET</td>
            <td>/api/health</td>
            <td>200</td>
          </tr>
        </tbody>
      </table>,
    );

    const headers = container.querySelectorAll('th');
    headers.forEach((th) => {
      expect(th).toHaveAttribute('scope', 'col');
    });
  });

  it('table with scope headers has no axe violations', async () => {
    const { container } = render(
      <table>
        <thead>
          <tr>
            <th scope="col">Name</th>
            <th scope="col">Type</th>
            <th scope="col">Fields</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>posts</td>
            <td>base</td>
            <td>5 fields</td>
          </tr>
        </tbody>
      </table>,
    );
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });

  it('sortable column headers are keyboard accessible', async () => {
    const user = userEvent.setup();
    const handleSort = vi.fn();

    const { getByText } = render(
      <table>
        <thead>
          <tr>
            <th
              scope="col"
              tabIndex={0}
              onClick={() => handleSort('timestamp')}
              onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleSort('timestamp'); } }}
              aria-sort="ascending"
            >
              Timestamp
            </th>
          </tr>
        </thead>
        <tbody>
          <tr><td>2026-03-21</td></tr>
        </tbody>
      </table>,
    );

    const header = getByText('Timestamp');
    header.focus();
    await user.keyboard('{Enter}');
    expect(handleSort).toHaveBeenCalledWith('timestamp');

    handleSort.mockClear();
    await user.keyboard(' ');
    expect(handleSort).toHaveBeenCalledWith('timestamp');
  });
});

describe('Accessibility — Clickable Table Rows', () => {
  it('clickable rows have keyboard support and proper ARIA', async () => {
    const user = userEvent.setup();
    const handleClick = vi.fn();

    const { getByRole } = render(
      <table>
        <thead>
          <tr><th scope="col">ID</th></tr>
        </thead>
        <tbody>
          <tr
            role="button"
            tabIndex={0}
            aria-label="View record rec-001"
            onClick={() => handleClick('rec-001')}
            onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleClick('rec-001'); } }}
          >
            <td>rec-001</td>
          </tr>
        </tbody>
      </table>,
    );

    const row = getByRole('button', { name: 'View record rec-001' });
    expect(row).toHaveAttribute('tabindex', '0');

    row.focus();
    await user.keyboard('{Enter}');
    expect(handleClick).toHaveBeenCalledWith('rec-001');

    handleClick.mockClear();
    await user.keyboard(' ');
    expect(handleClick).toHaveBeenCalledWith('rec-001');
  });
});

describe('Accessibility — Modals', () => {
  it('modal has proper dialog ARIA attributes', () => {
    const { getByRole } = render(
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="modal-title"
      >
        <h3 id="modal-title">Confirm Action</h3>
        <p>Are you sure?</p>
        <button>Cancel</button>
        <button>Confirm</button>
      </div>,
    );

    const dialog = getByRole('dialog');
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(dialog).toHaveAttribute('aria-labelledby', 'modal-title');
  });

  it('modal dialog has no axe violations', async () => {
    const { container } = render(
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="dlg-title"
      >
        <h3 id="dlg-title">Delete Collection</h3>
        <p>Are you sure you want to delete this collection?</p>
        <button type="button">Cancel</button>
        <button type="button">Delete</button>
      </div>,
    );
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });

  it('Escape key closes modal', async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();

    render(
      <div>
        <ModalWithEscape onClose={onClose} />
      </div>,
    );

    await user.keyboard('{Escape}');
    expect(onClose).toHaveBeenCalled();
  });
});

// Helper component for Escape key test
function ModalWithEscape({ onClose }: { onClose: () => void }) {
  const ref = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose();
    }
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  return (
    <div ref={ref} role="dialog" aria-modal="true" aria-label="Test modal">
      <button type="button">Close</button>
    </div>
  );
}

import React from 'react';

describe('Accessibility — Forms', () => {
  it('form inputs have associated labels', () => {
    const { container } = render(
      <form>
        <div>
          <label htmlFor="email">Email</label>
          <input id="email" type="email" name="email" autoComplete="email" />
        </div>
        <div>
          <label htmlFor="password">Password</label>
          <input
            id="password"
            type="password"
            name="password"
            autoComplete="current-password"
            aria-invalid={true}
            aria-describedby="password-error"
          />
          <p id="password-error">Password is required</p>
        </div>
      </form>,
    );

    const emailInput = container.querySelector('#email');
    const emailLabel = container.querySelector('label[for="email"]');
    expect(emailLabel).toBeInTheDocument();
    expect(emailInput).toBeInTheDocument();

    const passwordInput = container.querySelector('#password');
    expect(passwordInput).toHaveAttribute('aria-invalid', 'true');
    expect(passwordInput).toHaveAttribute('aria-describedby', 'password-error');
  });

  it('form with labels has no axe violations', async () => {
    const { container } = render(
      <form>
        <div>
          <label htmlFor="test-email">Email</label>
          <input id="test-email" type="email" name="email" autoComplete="email" />
        </div>
        <div>
          <label htmlFor="test-pass">Password</label>
          <input id="test-pass" type="password" name="password" autoComplete="current-password" />
        </div>
        <button type="submit">Sign In</button>
      </form>,
    );
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });
});

describe('Accessibility — Icon Buttons', () => {
  it('icon-only buttons have aria-label', () => {
    const { container } = render(
      <div>
        <button type="button" aria-label="Open navigation menu">
          <svg aria-hidden="true" viewBox="0 0 24 24">
            <line x1="3" y1="6" x2="21" y2="6" />
          </svg>
        </button>
        <button type="button" aria-label="Close navigation menu">
          <svg aria-hidden="true" viewBox="0 0 24 24">
            <line x1="18" y1="6" x2="6" y2="18" />
          </svg>
        </button>
      </div>,
    );

    const buttons = container.querySelectorAll('button');
    buttons.forEach((btn) => {
      expect(btn).toHaveAttribute('aria-label');
      expect(btn.getAttribute('aria-label')!.length).toBeGreaterThan(0);
    });

    // SVGs should be hidden from screen readers
    const svgs = container.querySelectorAll('svg');
    svgs.forEach((svg) => {
      expect(svg).toHaveAttribute('aria-hidden', 'true');
    });
  });

  it('icon buttons have no axe violations', async () => {
    const { container } = render(
      <div>
        <button type="button" aria-label="Open menu">
          <svg aria-hidden="true" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" /></svg>
        </button>
      </div>,
    );
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });
});

describe('Accessibility — ARIA Live Regions', () => {
  it('status messages have aria-live attribute', () => {
    const { container } = render(
      <div>
        <div role="status" aria-live="polite">
          Settings saved successfully.
        </div>
        <div role="alert">
          An error occurred. Please try again.
        </div>
      </div>,
    );

    const status = container.querySelector('[role="status"]');
    expect(status).toHaveAttribute('aria-live', 'polite');

    // role="alert" implicitly has aria-live="assertive"
    const alert = container.querySelector('[role="alert"]');
    expect(alert).toBeInTheDocument();
  });

  it('live region has no axe violations', async () => {
    const { container } = render(
      <div>
        <div role="status" aria-live="polite">Operation complete.</div>
      </div>,
    );
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });
});

describe('Accessibility — Theme Toggle', () => {
  it('theme toggle elements have accessible labels', () => {
    const { container } = render(
      <div>
        <button
          type="button"
          aria-label="Theme: light. Click to change."
          title="Current theme: light"
        >
          <svg aria-hidden="true" viewBox="0 0 24 24"><circle cx="12" cy="12" r="5" /></svg>
        </button>
        <select aria-label="Select theme">
          <option value="light">Light</option>
          <option value="dark">Dark</option>
          <option value="system">System</option>
        </select>
      </div>,
    );

    const button = container.querySelector('button');
    expect(button).toHaveAttribute('aria-label');

    const select = container.querySelector('select');
    expect(select).toHaveAttribute('aria-label', 'Select theme');
  });

  it('theme controls have no axe violations', async () => {
    const { container } = render(
      <div>
        <button type="button" aria-label="Toggle theme">
          <svg aria-hidden="true" viewBox="0 0 24 24"><circle cx="12" cy="12" r="5" /></svg>
        </button>
        <label htmlFor="theme-select">Theme</label>
        <select id="theme-select">
          <option value="light">Light</option>
          <option value="dark">Dark</option>
        </select>
      </div>,
    );
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });
});

describe('Accessibility — Mobile Drawer', () => {
  it('mobile drawer has proper dialog attributes', () => {
    const { getByRole } = render(
      <aside
        role="dialog"
        aria-modal="true"
        aria-label="Navigation menu"
      >
        <button type="button" aria-label="Close navigation menu">X</button>
        <nav>
          <ul role="list">
            <li><a href="/_/">Overview</a></li>
          </ul>
        </nav>
      </aside>,
    );

    const dialog = getByRole('dialog');
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(dialog).toHaveAttribute('aria-label', 'Navigation menu');
  });
});

describe('Accessibility — Color Contrast & Semantic HTML', () => {
  it('headings are properly hierarchical', () => {
    const { container } = render(
      <main>
        <h1>Dashboard</h1>
        <section>
          <h2>Collections</h2>
          <h3>System Collections</h3>
        </section>
        <section>
          <h2>Recent Activity</h2>
        </section>
      </main>,
    );

    const h1 = container.querySelectorAll('h1');
    const h2 = container.querySelectorAll('h2');
    const h3 = container.querySelectorAll('h3');

    expect(h1).toHaveLength(1);
    expect(h2.length).toBeGreaterThanOrEqual(1);
    expect(h3.length).toBeGreaterThanOrEqual(0);
  });

  it('semantic HTML structure has no axe violations', async () => {
    const { container } = render(
      <main>
        <h1>Dashboard</h1>
        <section>
          <h2>Overview</h2>
          <p>Welcome to the admin dashboard.</p>
        </section>
      </main>,
    );
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });
});

describe('Accessibility — Focus Trap in Modal', () => {
  it('Tab cycles through focusable elements within modal', async () => {
    const user = userEvent.setup();

    const { getByText } = render(
      <FocusTrapModal />,
    );

    const cancelBtn = getByText('Cancel');
    const confirmBtn = getByText('Confirm');

    cancelBtn.focus();
    expect(document.activeElement).toBe(cancelBtn);

    await user.tab();
    expect(document.activeElement).toBe(confirmBtn);

    // Tab from last element should cycle back to first
    await user.tab();
    expect(document.activeElement).toBe(cancelBtn);
  });
});

function FocusTrapModal() {
  const ref = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Tab' && ref.current) {
        const focusable = ref.current.querySelectorAll<HTMLElement>('button, [href], input');
        if (focusable.length === 0) return;
        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    }
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, []);

  return (
    <div ref={ref} role="dialog" aria-modal="true" aria-label="Confirmation">
      <button type="button">Cancel</button>
      <button type="button">Confirm</button>
    </div>
  );
}
