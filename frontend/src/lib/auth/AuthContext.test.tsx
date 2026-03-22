import { render, screen, waitFor, act } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { AuthProvider, useAuth } from './AuthContext';
import { ApiError } from '../api';
import type { ErrorResponseBody, AdminAuthResponse } from '../api';

// ── Mocks ────────────────────────────────────────────────────────────────────

const mockAdminAuthWithPassword = vi.fn();
const mockLogout = vi.fn();
let mockIsAuthenticated = false;

vi.mock('./client', () => ({
  client: {
    adminAuthWithPassword: (...args: unknown[]) => mockAdminAuthWithPassword(...args),
    logout: () => mockLogout(),
    get isAuthenticated() {
      return mockIsAuthenticated;
    },
    get token() {
      return mockIsAuthenticated ? 'fake-token' : null;
    },
  },
}));

// ── Test consumer component ──────────────────────────────────────────────────

function TestConsumer({ onLogin }: { onLogin?: (email: string, pass: string) => void }) {
  const { admin, loading, login, logout } = useAuth();

  return (
    <div>
      <span data-testid="loading">{String(loading)}</span>
      <span data-testid="admin">{admin ? admin.email : 'null'}</span>
      <button
        onClick={async () => {
          try {
            await login('admin@test.com', 'pass');
            onLogin?.('admin@test.com', 'pass');
          } catch {
            // handled in test
          }
        }}
      >
        Login
      </button>
      <button onClick={logout}>Logout</button>
    </div>
  );
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('AuthContext', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockIsAuthenticated = false;
  });

  it('provides loading=true initially then resolves to false', async () => {
    render(
      <AuthProvider>
        <TestConsumer />
      </AuthProvider>,
    );

    // After mount effect runs, loading should be false
    await waitFor(() => {
      expect(screen.getByTestId('loading')).toHaveTextContent('false');
    });
    expect(screen.getByTestId('admin')).toHaveTextContent('null');
  });

  it('login sets admin on success', async () => {
    const mockAdmin = {
      id: 'admin1',
      email: 'admin@test.com',
      collectionId: '_pbc_superusers',
      collectionName: '_superusers',
      emailVisibility: true,
      verified: true,
      created: '2025-01-01',
      updated: '2025-01-01',
    };
    mockAdminAuthWithPassword.mockResolvedValue({
      token: 'jwt-token',
      admin: mockAdmin,
    } satisfies AdminAuthResponse);

    render(
      <AuthProvider>
        <TestConsumer />
      </AuthProvider>,
    );

    await waitFor(() => {
      expect(screen.getByTestId('loading')).toHaveTextContent('false');
    });

    await act(async () => {
      screen.getByText('Login').click();
    });

    await waitFor(() => {
      expect(screen.getByTestId('admin')).toHaveTextContent('admin@test.com');
    });

    expect(mockAdminAuthWithPassword).toHaveBeenCalledWith('admin@test.com', 'pass');
  });

  it('login propagates ApiError', async () => {
    const errorBody: ErrorResponseBody = {
      code: 400,
      message: 'Invalid credentials.',
      data: {},
    };
    mockAdminAuthWithPassword.mockRejectedValue(new ApiError(400, errorBody));

    let caughtError: unknown;

    function ErrorTestConsumer() {
      const { login } = useAuth();
      return (
        <button
          onClick={async () => {
            try {
              await login('bad@email.com', 'wrong');
            } catch (e) {
              caughtError = e;
            }
          }}
        >
          Login
        </button>
      );
    }

    render(
      <AuthProvider>
        <ErrorTestConsumer />
      </AuthProvider>,
    );

    await act(async () => {
      screen.getByText('Login').click();
    });

    expect(caughtError).toBeInstanceOf(ApiError);
    expect((caughtError as ApiError).response.message).toBe('Invalid credentials.');
  });

  it('logout clears admin state and calls client.logout', async () => {
    const mockAdmin = {
      id: 'admin1',
      email: 'admin@test.com',
      collectionId: '_pbc_superusers',
      collectionName: '_superusers',
      emailVisibility: true,
      verified: true,
      created: '2025-01-01',
      updated: '2025-01-01',
    };
    mockAdminAuthWithPassword.mockResolvedValue({
      token: 'jwt-token',
      admin: mockAdmin,
    });

    render(
      <AuthProvider>
        <TestConsumer />
      </AuthProvider>,
    );

    // Login first
    await act(async () => {
      screen.getByText('Login').click();
    });

    await waitFor(() => {
      expect(screen.getByTestId('admin')).toHaveTextContent('admin@test.com');
    });

    // Now logout
    await act(async () => {
      screen.getByText('Logout').click();
    });

    expect(mockLogout).toHaveBeenCalled();
    expect(screen.getByTestId('admin')).toHaveTextContent('null');
  });

  it('throws when useAuth is used outside AuthProvider', () => {
    // Suppress React error boundary console output
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    expect(() => {
      render(<TestConsumer />);
    }).toThrow('useAuth must be used within an AuthProvider');

    consoleSpy.mockRestore();
  });
});
