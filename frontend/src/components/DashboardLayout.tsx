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
    <header className="fixed top-0 right-0 left-0 z-30 flex h-16 items-center justify-between border-b border-primary bg-background px-4 sm:px-6 md:left-64">
      <div className="flex items-center gap-3">
        <MobileSidebar currentPath={getCurrentPath()} />
        <h1 className="text-label-md text-primary md:hidden">ZEROBASE</h1>
      </div>

      <div className="flex items-center gap-4">
        <ThemeToggle />
        {admin && (
          <span className="hidden text-xs font-medium text-secondary sm:inline">{admin.email}</span>
        )}
        <button
          type="button"
          onClick={handleLogout}
          className="px-3 py-1.5 text-label-md text-primary hover:bg-surface-container-high transition-colors-fast"
        >
          SIGN OUT
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
    <div className="h-screen overflow-hidden bg-background">
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:fixed focus:left-4 focus:top-4 focus:z-[100] focus:bg-primary focus:px-4 focus:py-2 focus:text-sm focus:font-bold focus:text-on-primary focus:outline-none"
      >
        Skip to main content
      </a>

      <Sidebar currentPath={currentPath} />
      <DashboardHeader />

      <main
        id="main-content"
        className="h-full overflow-y-auto pt-16 md:ml-64"
        tabIndex={-1}
      >
        <div className="px-4 py-8 sm:px-6 lg:px-8 animate-fade-in">
          {pageTitle && (
            <h2 className="mb-8 text-headline-lg text-primary">{pageTitle}</h2>
          )}
          {children}
        </div>
      </main>
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
