import { useEffect, type ReactNode } from 'react';
import { useAuth } from '../lib/auth';
import { client } from '../lib/auth';

/**
 * Wraps dashboard content and redirects to the login page if unauthenticated.
 * Shows a loading spinner while the auth state is being resolved.
 */
export function AuthGuard({ children }: { children: ReactNode }) {
  const { admin, loading } = useAuth();

  useEffect(() => {
    if (!loading && !admin && !client.isAuthenticated) {
      window.location.href = '/_/login';
    }
  }, [loading, admin]);

  if (loading) {
    return (
      <div className="flex min-h-screen items-center justify-center">
        <svg className="h-8 w-8 animate-spin text-primary" viewBox="0 0 24 24" fill="none" aria-label="Loading">
          <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
          <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
        </svg>
      </div>
    );
  }

  // While redirecting, show nothing
  if (!admin && !client.isAuthenticated) {
    return null;
  }

  return <>{children}</>;
}
