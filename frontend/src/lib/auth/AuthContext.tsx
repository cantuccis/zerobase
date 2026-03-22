import { createContext, useContext, useState, useCallback, useEffect, type ReactNode } from 'react';
import type { AuthRecord } from '../api';
import { client } from './client';

// ── Types ────────────────────────────────────────────────────────────────────

export interface AuthState {
  /** The authenticated admin record, or null when logged out. */
  admin: AuthRecord | null;
  /** Whether the initial token check is still in progress. */
  loading: boolean;
}

export interface AuthContextValue extends AuthState {
  /** Authenticate with email and password. Returns the admin record on success. */
  login: (email: string, password: string) => Promise<AuthRecord>;
  /** Clear the stored token and reset state. */
  logout: () => void;
}

// ── Context ──────────────────────────────────────────────────────────────────

const AuthContext = createContext<AuthContextValue | null>(null);

// ── Provider ─────────────────────────────────────────────────────────────────

export function AuthProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AuthState>({ admin: null, loading: true });

  // On mount, check if a stored token exists and is still valid.
  useEffect(() => {
    if (!client.isAuthenticated) {
      setState({ admin: null, loading: false });
      return;
    }

    // We have a token but need to verify it's still valid.
    // For now, we assume a stored token is valid (the API will return 401 on
    // the first request if it's expired and the dashboard will redirect).
    // A dedicated /auth-refresh endpoint for admins could be added later.
    setState({ admin: null, loading: false });
  }, []);

  const login = useCallback(async (email: string, password: string): Promise<AuthRecord> => {
    const response = await client.adminAuthWithPassword(email, password);
    setState({ admin: response.admin, loading: false });
    return response.admin;
  }, []);

  const logout = useCallback(() => {
    client.logout();
    setState({ admin: null, loading: false });
  }, []);

  return (
    <AuthContext.Provider value={{ ...state, login, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

// ── Hook ─────────────────────────────────────────────────────────────────────

/** Access admin auth state and actions. Must be used within an AuthProvider. */
export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) {
    throw new Error('useAuth must be used within an AuthProvider');
  }
  return ctx;
}
