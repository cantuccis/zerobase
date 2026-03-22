import { AuthProvider } from '../lib/auth';
import { ThemeProvider } from '../lib/theme';
import { LoginForm } from './LoginForm';

/**
 * Top-level React island for the login page.
 * Wraps LoginForm in AuthProvider so useAuth is available.
 */
export function LoginPage() {
  return (
    <ThemeProvider>
      <AuthProvider>
        <div className="flex min-h-screen items-center justify-center bg-gray-50 px-4 dark:bg-gray-900">
          <LoginForm />
        </div>
      </AuthProvider>
    </ThemeProvider>
  );
}
