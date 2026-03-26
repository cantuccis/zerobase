import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { DashboardLayout } from './DashboardLayout';

// ── Mocks ────────────────────────────────────────────────────────────────────

const mockLogout = vi.fn();
let mockIsAuthenticated = true;

vi.mock('../lib/auth/client', () => ({
  client: {
    adminAuthWithPassword: vi.fn(),
    logout: () => mockLogout(),
    get isAuthenticated() {
      return mockIsAuthenticated;
    },
    get token() {
      return mockIsAuthenticated ? 'mock-token' : null;
    },
  },
}));

// Mock window.location
Object.defineProperty(window, 'location', {
  value: { href: '', pathname: '/_/', origin: 'http://localhost:8090' },
  writable: true,
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe('DashboardLayout', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockIsAuthenticated = true;
    window.location.href = '';
    window.location.pathname = '/_/';
  });

  it('renders the sidebar with all navigation items', async () => {
    render(
      <DashboardLayout currentPath="/_/">
        <div>Test content</div>
      </DashboardLayout>,
    );

    await waitFor(() => {
      expect(screen.getByLabelText('Main navigation')).toBeInTheDocument();
    });

    // Check nav items exist as links in sidebar
    const nav = screen.getByLabelText('Main navigation');
    expect(nav).toBeInTheDocument();

    // All four nav items should be present
    expect(screen.getByRole('link', { name: /Collections/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Settings/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Logs/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Backups/ })).toBeInTheDocument();
  });

  it('renders the header with Sign Out button', async () => {
    render(
      <DashboardLayout currentPath="/_/">
        <div>Test content</div>
      </DashboardLayout>,
    );

    await waitFor(() => {
      expect(screen.getByText('SIGN OUT')).toBeInTheDocument();
    });
  });

  it('renders the page title as h2 when provided', async () => {
    render(
      <DashboardLayout currentPath="/_/settings" pageTitle="Settings">
        <div>Test content</div>
      </DashboardLayout>,
    );

    await waitFor(() => {
      expect(screen.getByRole('heading', { level: 2, name: 'Settings' })).toBeInTheDocument();
    });
  });

  it('does not render a page title heading when not provided', async () => {
    render(
      <DashboardLayout currentPath="/_/">
        <div>Test content</div>
      </DashboardLayout>,
    );

    await waitFor(() => {
      expect(screen.getByText('Test content')).toBeInTheDocument();
    });

    expect(screen.queryByRole('heading', { level: 2 })).not.toBeInTheDocument();
  });

  it('renders children in the content area', async () => {
    render(
      <DashboardLayout currentPath="/_/">
        <div data-testid="child-content">Hello Dashboard</div>
      </DashboardLayout>,
    );

    await waitFor(() => {
      expect(screen.getByTestId('child-content')).toBeInTheDocument();
    });

    expect(screen.getByText('Hello Dashboard')).toBeInTheDocument();
  });

  it('calls logout and redirects when Sign Out is clicked', async () => {
    const user = userEvent.setup();
    render(
      <DashboardLayout currentPath="/_/">
        <div>Content</div>
      </DashboardLayout>,
    );

    await waitFor(() => {
      expect(screen.getByText('SIGN OUT')).toBeInTheDocument();
    });

    await user.click(screen.getByText('SIGN OUT'));

    expect(mockLogout).toHaveBeenCalled();
    expect(window.location.href).toBe('/_/login');
  });

  it('highlights the correct sidebar item based on currentPath', async () => {
    render(
      <DashboardLayout currentPath="/_/settings">
        <div>Settings content</div>
      </DashboardLayout>,
    );

    await waitFor(() => {
      expect(screen.getByRole('link', { name: /Settings/ })).toBeInTheDocument();
    });

    const settingsLink = screen.getByRole('link', { name: /Settings/ });
    expect(settingsLink).toHaveAttribute('aria-current', 'page');

    const collectionsLink = screen.getByRole('link', { name: /Collections/ });
    expect(collectionsLink).not.toHaveAttribute('aria-current');
  });

  it('renders mobile hamburger button', async () => {
    render(
      <DashboardLayout currentPath="/_/">
        <div>Content</div>
      </DashboardLayout>,
    );

    await waitFor(() => {
      expect(screen.getByLabelText('Open navigation menu')).toBeInTheDocument();
    });
  });

  it('has proper layout structure with main content area', async () => {
    render(
      <DashboardLayout currentPath="/_/">
        <div>Content</div>
      </DashboardLayout>,
    );

    await waitFor(() => {
      expect(screen.getByRole('main')).toBeInTheDocument();
    });
  });

  it('renders the Zerobase brand in sidebar', async () => {
    render(
      <DashboardLayout currentPath="/_/">
        <div>Content</div>
      </DashboardLayout>,
    );

    await waitFor(() => {
      // Brand appears in sidebar as "ADMIN" and mobile header as "ZEROBASE"
      expect(screen.getByText('ZEROBASE')).toBeInTheDocument();
    });
  });
});

// ── Auth guard integration ───────────────────────────────────────────────────

describe('DashboardLayout auth guard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.location.href = '';
  });

  it('redirects to login when not authenticated', async () => {
    mockIsAuthenticated = false;

    render(
      <DashboardLayout currentPath="/_/">
        <div>Protected content</div>
      </DashboardLayout>,
    );

    await waitFor(() => {
      expect(window.location.href).toBe('/_/login');
    });
  });

  it('does not render content when not authenticated', () => {
    mockIsAuthenticated = false;

    render(
      <DashboardLayout currentPath="/_/">
        <div>Protected content</div>
      </DashboardLayout>,
    );

    expect(screen.queryByText('Protected content')).not.toBeInTheDocument();
  });
});
