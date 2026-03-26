import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect } from 'vitest';
import { Sidebar, MobileSidebar, isNavItemActive } from './Sidebar';

// ── isNavItemActive unit tests ───────────────────────────────────────────────

describe('isNavItemActive', () => {
  describe('Overview (root) item', () => {
    const href = '/_/';

    it('is active for exact root path', () => {
      expect(isNavItemActive(href, '/_/')).toBe(true);
    });

    it('is active for root path without trailing slash', () => {
      expect(isNavItemActive(href, '/_')).toBe(true);
    });

    it('is not active for /collections', () => {
      expect(isNavItemActive(href, '/_/collections')).toBe(false);
    });

    it('is not active for /settings', () => {
      expect(isNavItemActive(href, '/_/settings')).toBe(false);
    });

    it('is not active for /logs', () => {
      expect(isNavItemActive(href, '/_/logs')).toBe(false);
    });

    it('is not active for /backups', () => {
      expect(isNavItemActive(href, '/_/backups')).toBe(false);
    });
  });

  describe('Collections item', () => {
    const href = '/_/collections';

    it('is active for exact path', () => {
      expect(isNavItemActive(href, '/_/collections')).toBe(true);
    });

    it('is active for sub-path', () => {
      expect(isNavItemActive(href, '/_/collections/users')).toBe(true);
      expect(isNavItemActive(href, '/_/collections/col123/edit')).toBe(true);
    });

    it('is not active for root', () => {
      expect(isNavItemActive(href, '/_/')).toBe(false);
    });

    it('is not active for /settings', () => {
      expect(isNavItemActive(href, '/_/settings')).toBe(false);
    });
  });

  describe('Settings item', () => {
    const href = '/_/settings';

    it('is active for exact path', () => {
      expect(isNavItemActive(href, '/_/settings')).toBe(true);
    });

    it('is active for sub-path', () => {
      expect(isNavItemActive(href, '/_/settings/email')).toBe(true);
    });

    it('is not active for root', () => {
      expect(isNavItemActive(href, '/_/')).toBe(false);
    });

    it('is not active for /settings-extra (no false prefix match)', () => {
      expect(isNavItemActive(href, '/_/settings-extra')).toBe(false);
    });
  });

  describe('Logs item', () => {
    const href = '/_/logs';

    it('is active for exact path', () => {
      expect(isNavItemActive(href, '/_/logs')).toBe(true);
    });

    it('is active for sub-path', () => {
      expect(isNavItemActive(href, '/_/logs/abc123')).toBe(true);
    });

    it('is not active for root', () => {
      expect(isNavItemActive(href, '/_/')).toBe(false);
    });
  });

  describe('Backups item', () => {
    const href = '/_/backups';

    it('is active for exact path', () => {
      expect(isNavItemActive(href, '/_/backups')).toBe(true);
    });

    it('is not active for root', () => {
      expect(isNavItemActive(href, '/_/')).toBe(false);
    });
  });

  it('handles trailing slashes', () => {
    expect(isNavItemActive('/_/settings', '/_/settings/')).toBe(true);
  });
});

// ── Sidebar rendering ────────────────────────────────────────────────────────

describe('Sidebar', () => {
  it('renders all navigation items', () => {
    render(<Sidebar currentPath="/_/" />);

    expect(screen.getByText('Overview')).toBeInTheDocument();
    expect(screen.getByText('Collections')).toBeInTheDocument();
    expect(screen.getByText('Settings')).toBeInTheDocument();
    expect(screen.getByText('Logs')).toBeInTheDocument();
    expect(screen.getByText('Backups')).toBeInTheDocument();
  });

  it('renders the brand link', () => {
    render(<Sidebar currentPath="/_/" />);

    const brandLink = screen.getByText('ADMIN');
    expect(brandLink).toBeInTheDocument();
    expect(brandLink.closest('a')).toHaveAttribute('href', '/_/');
  });

  it('has correct navigation links', () => {
    render(<Sidebar currentPath="/_/" />);

    expect(screen.getByText('Overview').closest('a')).toHaveAttribute('href', '/_/');
    expect(screen.getByText('Collections').closest('a')).toHaveAttribute('href', '/_/collections');
    expect(screen.getByText('Settings').closest('a')).toHaveAttribute('href', '/_/settings');
    expect(screen.getByText('Logs').closest('a')).toHaveAttribute('href', '/_/logs');
    expect(screen.getByText('Backups').closest('a')).toHaveAttribute('href', '/_/backups');
  });

  it('marks the active item with aria-current="page"', () => {
    render(<Sidebar currentPath="/_/settings" />);

    const settingsLink = screen.getByText('Settings').closest('a');
    const overviewLink = screen.getByText('Overview').closest('a');

    expect(settingsLink).toHaveAttribute('aria-current', 'page');
    expect(overviewLink).not.toHaveAttribute('aria-current');
  });

  it('marks Overview as active for root path', () => {
    render(<Sidebar currentPath="/_/" />);

    const overviewLink = screen.getByText('Overview').closest('a');
    expect(overviewLink).toHaveAttribute('aria-current', 'page');
    expect(overviewLink?.className).toContain('bg-primary');
    expect(overviewLink?.className).toContain('text-on-primary');
  });

  it('marks Collections as active for collections path', () => {
    render(<Sidebar currentPath="/_/collections" />);

    const collectionsLink = screen.getByText('Collections').closest('a');
    const overviewLink = screen.getByText('Overview').closest('a');

    expect(collectionsLink).toHaveAttribute('aria-current', 'page');
    expect(overviewLink).not.toHaveAttribute('aria-current');
  });

  it('applies inactive styles to non-current pages', () => {
    render(<Sidebar currentPath="/_/" />);

    const settingsLink = screen.getByText('Settings').closest('a');
    expect(settingsLink?.className).toContain('text-outline');
    expect(settingsLink?.className).not.toContain('bg-primary');
  });

  it('has navigation landmark with accessible label', () => {
    render(<Sidebar currentPath="/_/" />);

    expect(screen.getByRole('navigation')).toBeInTheDocument();
    expect(screen.getByLabelText('Main navigation')).toBeInTheDocument();
  });

  it('renders navigation as a list', () => {
    render(<Sidebar currentPath="/_/" />);

    const list = screen.getByRole('list');
    const items = within(list).getAllByRole('listitem');
    expect(items).toHaveLength(8);
  });

  it('renders SVG icons with aria-hidden', () => {
    const { container } = render(<Sidebar currentPath="/_/" />);

    const icons = container.querySelectorAll('svg[aria-hidden="true"]');
    expect(icons.length).toBeGreaterThanOrEqual(8);
  });
});

// ── MobileSidebar ────────────────────────────────────────────────────────────

describe('MobileSidebar', () => {
  it('renders the hamburger button', () => {
    render(<MobileSidebar currentPath="/_/" />);

    expect(screen.getByLabelText('Open navigation menu')).toBeInTheDocument();
  });

  it('does not show the drawer initially', () => {
    render(<MobileSidebar currentPath="/_/" />);

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('opens the drawer when hamburger is clicked', async () => {
    const user = userEvent.setup();
    render(<MobileSidebar currentPath="/_/" />);

    await user.click(screen.getByLabelText('Open navigation menu'));

    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('Overview')).toBeInTheDocument();
    expect(screen.getByText('Collections')).toBeInTheDocument();
    expect(screen.getByText('Settings')).toBeInTheDocument();
    expect(screen.getByText('Logs')).toBeInTheDocument();
    expect(screen.getByText('Backups')).toBeInTheDocument();
  });

  it('closes the drawer when close button is clicked', async () => {
    const user = userEvent.setup();
    render(<MobileSidebar currentPath="/_/" />);

    await user.click(screen.getByLabelText('Open navigation menu'));
    expect(screen.getByRole('dialog')).toBeInTheDocument();

    await user.click(screen.getByLabelText('Close navigation menu'));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('closes the drawer when backdrop is clicked', async () => {
    const user = userEvent.setup();
    const { container } = render(<MobileSidebar currentPath="/_/" />);

    await user.click(screen.getByLabelText('Open navigation menu'));
    expect(screen.getByRole('dialog')).toBeInTheDocument();

    // Click the backdrop (the div with bg-primary/50)
    const backdrop = container.querySelector('.bg-primary\\/50');
    expect(backdrop).toBeTruthy();
    await user.click(backdrop!);

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('shows active state for current page in drawer', async () => {
    const user = userEvent.setup();
    render(<MobileSidebar currentPath="/_/logs" />);

    await user.click(screen.getByLabelText('Open navigation menu'));

    const logsLink = screen.getByText('Logs').closest('a');
    expect(logsLink).toHaveAttribute('aria-current', 'page');
    expect(logsLink?.className).toContain('bg-primary');
  });

  it('drawer has proper ARIA attributes', async () => {
    const user = userEvent.setup();
    render(<MobileSidebar currentPath="/_/" />);

    await user.click(screen.getByLabelText('Open navigation menu'));

    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(dialog).toHaveAttribute('aria-label', 'Navigation menu');
  });
});
