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
    <div className="w-full max-w-sm mx-auto animate-slide-up">
      <div className="mb-10 text-center">
        <h1 className="text-display-lg text-on-background">Sign In</h1>
        <p className="mt-3 text-label-md text-secondary">Zerobase Admin Console</p>
      </div>

      <form onSubmit={handleSubmit} noValidate className="space-y-6">
        {error && (
          <div
            role="alert"
            className="border border-error px-4 py-3 text-sm text-error"
          >
            {error}
          </div>
        )}

        <div className="space-y-2">
          <label htmlFor="login-email" className="text-label-md text-on-surface block">
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
            className={`block w-full border bg-background text-on-background px-4 py-3 text-sm
              focus-visible:outline-none focus-visible:border-primary focus-visible:ring-1 focus-visible:ring-primary
              disabled:cursor-not-allowed disabled:opacity-50
              ${fieldErrors.identity ? 'border-error' : 'border-primary'}`}
            placeholder="admin@example.com"
          />
          {fieldErrors.identity && (
            <p id="login-email-error" className="text-xs text-error">{fieldErrors.identity}</p>
          )}
        </div>

        <div className="space-y-2">
          <label htmlFor="login-password" className="text-label-md text-on-surface block">
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
            className={`block w-full border bg-background text-on-background px-4 py-3 text-sm
              focus-visible:outline-none focus-visible:border-primary focus-visible:ring-1 focus-visible:ring-primary
              disabled:cursor-not-allowed disabled:opacity-50
              ${fieldErrors.password ? 'border-error' : 'border-primary'}`}
            placeholder="Enter your password"
          />
          {fieldErrors.password && (
            <p id="login-password-error" className="text-xs text-error">{fieldErrors.password}</p>
          )}
        </div>

        <button
          type="submit"
          disabled={submitting}
          className="flex w-full items-center justify-center bg-primary text-on-primary px-[1.4rem] py-[0.85rem] text-label-md
            focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary
            disabled:cursor-not-allowed disabled:opacity-50
            active:scale-[0.98] cursor-pointer"
        >
          {submitting ? (
            <>
              <svg className="mr-2 h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
              </svg>
              Signing In...
            </>
          ) : (
            'Sign In'
          )}
        </button>
      </form>
    </div>
  );
}
