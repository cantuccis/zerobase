import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { LoginForm } from './LoginForm';
import { AuthProvider } from '../lib/auth';
import { ApiError } from '../lib/api';
import type { ErrorResponseBody } from '../lib/api';

// ── Mocks ────────────────────────────────────────────────────────────────────

// Mock the auth client module so we can control adminAuthWithPassword
const mockAdminAuthWithPassword = vi.fn();
const mockLogout = vi.fn();
const mockIsAuthenticated = false;

vi.mock('../lib/auth/client', () => ({
  client: {
    adminAuthWithPassword: (...args: unknown[]) => mockAdminAuthWithPassword(...args),
    logout: () => mockLogout(),
    get isAuthenticated() {
      return mockIsAuthenticated;
    },
    get token() {
      return null;
    },
  },
}));

// Mock window.location
Object.defineProperty(window, 'location', {
  value: { href: '', origin: 'http://localhost:8090' },
  writable: true,
});

function renderLoginForm() {
  return render(
    <AuthProvider>
      <LoginForm />
    </AuthProvider>,
  );
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('LoginForm', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.location.href = '';
  });

  it('renders email and password fields with labels', () => {
    renderLoginForm();

    expect(screen.getByLabelText('Email')).toBeInTheDocument();
    expect(screen.getByLabelText('Password')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sign In' })).toBeInTheDocument();
  });

  it('renders heading and subtext', () => {
    renderLoginForm();

    expect(screen.getByText('Zerobase Admin')).toBeInTheDocument();
    expect(screen.getByText('Sign in to your superuser account')).toBeInTheDocument();
  });

  it('shows validation errors when submitting empty form', async () => {
    const user = userEvent.setup();
    renderLoginForm();

    await user.click(screen.getByRole('button', { name: 'Sign In' }));

    expect(screen.getByText('Email is required.')).toBeInTheDocument();
    expect(screen.getByText('Password is required.')).toBeInTheDocument();
    expect(mockAdminAuthWithPassword).not.toHaveBeenCalled();
  });

  it('shows validation error for empty email only', async () => {
    const user = userEvent.setup();
    renderLoginForm();

    await user.type(screen.getByLabelText('Password'), 'somepassword');
    await user.click(screen.getByRole('button', { name: 'Sign In' }));

    expect(screen.getByText('Email is required.')).toBeInTheDocument();
    expect(screen.queryByText('Password is required.')).not.toBeInTheDocument();
  });

  it('shows validation error for empty password only', async () => {
    const user = userEvent.setup();
    renderLoginForm();

    await user.type(screen.getByLabelText('Email'), 'admin@test.com');
    await user.click(screen.getByRole('button', { name: 'Sign In' }));

    expect(screen.queryByText('Email is required.')).not.toBeInTheDocument();
    expect(screen.getByText('Password is required.')).toBeInTheDocument();
  });

  it('calls login and redirects on successful authentication', async () => {
    const user = userEvent.setup();
    mockAdminAuthWithPassword.mockResolvedValue({
      token: 'jwt.token.here',
      admin: {
        id: 'admin1',
        email: 'admin@test.com',
        collectionId: '_pbc_superusers',
        collectionName: '_superusers',
        emailVisibility: true,
        verified: true,
        created: '2025-01-01',
        updated: '2025-01-01',
      },
    });

    renderLoginForm();

    await user.type(screen.getByLabelText('Email'), 'admin@test.com');
    await user.type(screen.getByLabelText('Password'), 'securepass');
    await user.click(screen.getByRole('button', { name: 'Sign In' }));

    await waitFor(() => {
      expect(mockAdminAuthWithPassword).toHaveBeenCalledWith('admin@test.com', 'securepass');
    });

    await waitFor(() => {
      expect(window.location.href).toBe('/_/');
    });
  });

  it('displays API error message on failed authentication', async () => {
    const user = userEvent.setup();
    const errorBody: ErrorResponseBody = {
      code: 400,
      message: 'Failed to authenticate.',
      data: {},
    };
    mockAdminAuthWithPassword.mockRejectedValue(new ApiError(400, errorBody));

    renderLoginForm();

    await user.type(screen.getByLabelText('Email'), 'admin@test.com');
    await user.type(screen.getByLabelText('Password'), 'wrongpass');
    await user.click(screen.getByRole('button', { name: 'Sign In' }));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Failed to authenticate.');
    });
  });

  it('displays field-level validation errors from API', async () => {
    const user = userEvent.setup();
    const errorBody: ErrorResponseBody = {
      code: 400,
      message: 'Validation failed.',
      data: {
        identity: { code: 'validation_required', message: 'Missing email or username.' },
        password: { code: 'validation_required', message: 'Missing password.' },
      },
    };
    mockAdminAuthWithPassword.mockRejectedValue(new ApiError(400, errorBody));

    renderLoginForm();

    await user.type(screen.getByLabelText('Email'), 'x');
    await user.type(screen.getByLabelText('Password'), 'y');
    await user.click(screen.getByRole('button', { name: 'Sign In' }));

    await waitFor(() => {
      expect(screen.getByText('Missing email or username.')).toBeInTheDocument();
      expect(screen.getByText('Missing password.')).toBeInTheDocument();
    });
  });

  it('shows generic error on network failure', async () => {
    const user = userEvent.setup();
    mockAdminAuthWithPassword.mockRejectedValue(new TypeError('Failed to fetch'));

    renderLoginForm();

    await user.type(screen.getByLabelText('Email'), 'admin@test.com');
    await user.type(screen.getByLabelText('Password'), 'pass');
    await user.click(screen.getByRole('button', { name: 'Sign In' }));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent(
        'Unable to connect to the server. Please try again.',
      );
    });
  });

  it('shows loading state while submitting', async () => {
    const user = userEvent.setup();
    // Create a promise that we control
    let resolveLogin!: (value: unknown) => void;
    mockAdminAuthWithPassword.mockReturnValue(
      new Promise((resolve) => {
        resolveLogin = resolve;
      }),
    );

    renderLoginForm();

    await user.type(screen.getByLabelText('Email'), 'admin@test.com');
    await user.type(screen.getByLabelText('Password'), 'pass');
    await user.click(screen.getByRole('button', { name: 'Sign In' }));

    // Button should show loading text
    expect(screen.getByText('Signing in...')).toBeInTheDocument();
    expect(screen.getByRole('button')).toBeDisabled();

    // Inputs should be disabled
    expect(screen.getByLabelText('Email')).toBeDisabled();
    expect(screen.getByLabelText('Password')).toBeDisabled();

    // Resolve the login to clean up
    resolveLogin({
      token: 'tok',
      admin: {
        id: 'a',
        email: 'a@b.com',
        collectionId: 'c',
        collectionName: 'n',
        emailVisibility: true,
        verified: true,
        created: '',
        updated: '',
      },
    });
  });

  it('clears previous errors when resubmitting', async () => {
    const user = userEvent.setup();

    // First: fail
    const errorBody: ErrorResponseBody = {
      code: 400,
      message: 'Bad credentials.',
      data: {},
    };
    mockAdminAuthWithPassword.mockRejectedValueOnce(new ApiError(400, errorBody));

    renderLoginForm();

    await user.type(screen.getByLabelText('Email'), 'admin@test.com');
    await user.type(screen.getByLabelText('Password'), 'wrong');
    await user.click(screen.getByRole('button', { name: 'Sign In' }));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Bad credentials.');
    });

    // Second: succeed
    mockAdminAuthWithPassword.mockResolvedValueOnce({
      token: 'tok',
      admin: {
        id: 'a',
        email: 'admin@test.com',
        collectionId: 'c',
        collectionName: 'n',
        emailVisibility: true,
        verified: true,
        created: '',
        updated: '',
      },
    });

    await user.clear(screen.getByLabelText('Password'));
    await user.type(screen.getByLabelText('Password'), 'correct');
    await user.click(screen.getByRole('button', { name: 'Sign In' }));

    // The error alert should be cleared immediately on resubmit
    await waitFor(() => {
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    });
  });

  it('has correct autocomplete attributes', () => {
    renderLoginForm();

    expect(screen.getByLabelText('Email')).toHaveAttribute('autocomplete', 'email');
    expect(screen.getByLabelText('Password')).toHaveAttribute('autocomplete', 'current-password');
  });

  it('trims email whitespace before submitting', async () => {
    const user = userEvent.setup();
    mockAdminAuthWithPassword.mockResolvedValue({
      token: 'tok',
      admin: {
        id: 'a',
        email: 'admin@test.com',
        collectionId: 'c',
        collectionName: 'n',
        emailVisibility: true,
        verified: true,
        created: '',
        updated: '',
      },
    });

    renderLoginForm();

    await user.type(screen.getByLabelText('Email'), '  admin@test.com  ');
    await user.type(screen.getByLabelText('Password'), 'pass');
    await user.click(screen.getByRole('button', { name: 'Sign In' }));

    await waitFor(() => {
      expect(mockAdminAuthWithPassword).toHaveBeenCalledWith('admin@test.com', 'pass');
    });
  });
});
