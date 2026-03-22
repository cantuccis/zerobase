import { useState } from 'react';
import { useAuth } from '../lib/auth';
import { ApiError } from '../lib/api';

export function LoginForm() {
  const { login } = useAuth();

  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(e: React.SyntheticEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    setFieldErrors({});

    // Client-side validation
    const errors: Record<string, string> = {};
    if (!email.trim()) errors.identity = 'Email is required.';
    if (!password) errors.password = 'Password is required.';
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }

    setSubmitting(true);
    try {
      await login(email.trim(), password);
      // On success, redirect to the dashboard
      window.location.href = '/_/';
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.isValidation && err.response.data) {
          const mapped: Record<string, string> = {};
          for (const [key, val] of Object.entries(err.response.data)) {
            mapped[key] = val.message;
          }
          setFieldErrors(mapped);
        }
        setError(err.response.message || 'Invalid credentials.');
      } else {
        setError('Unable to connect to the server. Please try again.');
      }
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="w-full max-w-sm mx-auto">
      <div className="mb-8 text-center">
        <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Zerobase Admin</h1>
        <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">Sign in to your superuser account</p>
      </div>

      <form onSubmit={handleSubmit} noValidate className="space-y-5">
        {error && (
          <div
            role="alert"
            className="rounded-md border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700 dark:border-red-800 dark:bg-red-900/30 dark:text-red-400"
          >
            {error}
          </div>
        )}

        <div className="space-y-1.5">
          <label htmlFor="login-email" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
            Email
          </label>
          <input
            id="login-email"
            name="email"
            type="email"
            autoComplete="email"
            spellCheck={false}
            required
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            disabled={submitting}
            aria-invalid={!!fieldErrors.identity}
            aria-describedby={fieldErrors.identity ? 'login-email-error' : undefined}
            className={`block w-full rounded-md border px-3 py-2 text-sm shadow-sm transition-colors
              focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
              disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700
              dark:bg-gray-800 dark:text-gray-100
              ${fieldErrors.identity ? 'border-red-400 dark:border-red-600' : 'border-gray-300 dark:border-gray-600'}`}
            placeholder="admin@example.com"
          />
          {fieldErrors.identity && (
            <p id="login-email-error" className="text-xs text-red-600 dark:text-red-400">{fieldErrors.identity}</p>
          )}
        </div>

        <div className="space-y-1.5">
          <label htmlFor="login-password" className="block text-sm font-medium text-gray-700 dark:text-gray-300">
            Password
          </label>
          <input
            id="login-password"
            name="password"
            type="password"
            autoComplete="current-password"
            required
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            disabled={submitting}
            aria-invalid={!!fieldErrors.password}
            aria-describedby={fieldErrors.password ? 'login-password-error' : undefined}
            className={`block w-full rounded-md border px-3 py-2 text-sm shadow-sm transition-colors
              focus-visible:outline-none focus-visible:ring-2 focus:ring-blue-500
              disabled:cursor-not-allowed disabled:bg-gray-100 dark:disabled:bg-gray-700
              dark:bg-gray-800 dark:text-gray-100
              ${fieldErrors.password ? 'border-red-400 dark:border-red-600' : 'border-gray-300 dark:border-gray-600'}`}
            placeholder="Enter your password"
          />
          {fieldErrors.password && (
            <p id="login-password-error" className="text-xs text-red-600 dark:text-red-400">{fieldErrors.password}</p>
          )}
        </div>

        <button
          type="submit"
          disabled={submitting}
          className="flex w-full items-center justify-center rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white shadow-sm
            transition-colors hover:bg-blue-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2
            disabled:cursor-not-allowed disabled:opacity-60 dark:focus-visible:ring-offset-gray-900"
        >
          {submitting ? (
            <>
              <svg className="mr-2 h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
              </svg>
              Signing in...
            </>
          ) : (
            'Sign In'
          )}
        </button>
      </form>
    </div>
  );
}
