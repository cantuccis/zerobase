import type { ReactNode } from 'react';
import { AuthProvider, useAuth } from '../lib/auth';
import { ToastProvider, ToastContainer } from '../lib/toast';
import { ThemeProvider } from '../lib/theme';
import { ErrorBoundary } from '../lib/error-boundary';
import { AuthGuard } from './AuthGuard';
import { Sidebar, MobileSidebar } from './Sidebar';
import { ThemeToggle } from './ThemeToggle';

// ── Header ───────────────────────────────────────────────────────────────────

function DashboardHeader() {
  const { admin, logout } = useAuth();

  function handleLogout() {
    logout();
    window.location.href = '/_/login';
  }

  return (
    <header className="flex h-14 items-center justify-between border-b border-gray-200 bg-white px-4 sm:px-6 dark:border-gray-700 dark:bg-gray-800">
      <div className="flex items-center gap-3">
        <MobileSidebar currentPath={getCurrentPath()} />
        <h1 className="text-sm font-medium text-gray-500 md:hidden dark:text-gray-400">Zerobase</h1>
      </div>

      <div className="flex items-center gap-3">
        <ThemeToggle />
        {admin && (
          <span className="hidden text-sm text-gray-500 sm:inline dark:text-gray-400">{admin.email}</span>
        )}
        <button
          type="button"
          onClick={handleLogout}
          className="rounded-md px-3 py-1.5 text-sm font-medium text-gray-600 transition-colors hover:bg-gray-100 hover:text-gray-900 dark:text-gray-300 dark:hover:bg-gray-700 dark:hover:text-gray-100"
        >
          Sign Out
        </button>
      </div>
    </header>
  );
}

// ── Utility ──────────────────────────────────────────────────────────────────

function getCurrentPath(): string {
  if (typeof window !== 'undefined') {
    return window.location.pathname;
  }
  return '/_/';
}

// ── Layout content ───────────────────────────────────────────────────────────

interface DashboardLayoutContentProps {
  children: ReactNode;
  currentPath: string;
  pageTitle?: string;
}

function DashboardLayoutContent({ children, currentPath, pageTitle }: DashboardLayoutContentProps) {
  return (
    <div className="flex h-screen overflow-hidden bg-gray-50 dark:bg-gray-900">
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:fixed focus:left-4 focus:top-4 focus:z-[100] focus:rounded-md focus:bg-blue-600 focus:px-4 focus:py-2 focus:text-sm focus:font-medium focus:text-white focus:shadow-lg focus:outline-none"
      >
        Skip to main content
      </a>

      <Sidebar currentPath={currentPath} />

      <div className="flex flex-1 flex-col overflow-hidden">
        <DashboardHeader />

        <main id="main-content" className="flex-1 overflow-y-auto px-4 py-6 sm:px-6 lg:px-8" tabIndex={-1}>
          {pageTitle && (
            <h2 className="mb-6 text-2xl font-bold text-gray-900 dark:text-gray-100">{pageTitle}</h2>
          )}
          {children}
        </main>
      </div>
    </div>
  );
}

// ── Public component ─────────────────────────────────────────────────────────

export interface DashboardLayoutProps {
  children: ReactNode;
  currentPath: string;
  pageTitle?: string;
}

/**
 * Main dashboard layout wrapping content with AuthProvider, AuthGuard,
 * sidebar navigation, and header with user info.
 */
export function DashboardLayout({ children, currentPath, pageTitle }: DashboardLayoutProps) {
  return (
    <ThemeProvider>
      <AuthProvider>
        <ToastProvider>
          <ErrorBoundary>
            <AuthGuard>
              <DashboardLayoutContent currentPath={currentPath} pageTitle={pageTitle}>
                <ErrorBoundary>
                  {children}
                </ErrorBoundary>
              </DashboardLayoutContent>
              <ToastContainer />
            </AuthGuard>
          </ErrorBoundary>
        </ToastProvider>
      </AuthProvider>
    </ThemeProvider>
  );
}
