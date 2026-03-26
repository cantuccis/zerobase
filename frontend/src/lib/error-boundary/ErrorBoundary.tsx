import { Component, type ErrorInfo, type ReactNode } from 'react';

// ── Error logging ─────────────────────────────────────────────────────────────

export interface ErrorLogEntry {
  message: string;
  stack?: string;
  componentStack?: string;
  timestamp: string;
}

/** In-memory log for the current session; also writes to the browser console. */
const errorLog: ErrorLogEntry[] = [];

export function logError(error: Error, componentStack?: string): void {
  const entry: ErrorLogEntry = {
    message: error.message,
    stack: error.stack,
    componentStack,
    timestamp: new Date().toISOString(),
  };
  errorLog.push(entry);

  // eslint-disable-next-line no-console
  console.error('[ErrorBoundary]', error.message, {
    stack: error.stack,
    componentStack,
  });
}

/** Returns a shallow copy of all logged errors for this session. */
export function getErrorLog(): ReadonlyArray<ErrorLogEntry> {
  return [...errorLog];
}

/** Clears the in-memory error log (useful for tests). */
export function clearErrorLog(): void {
  errorLog.length = 0;
}

// ── Fallback UI ───────────────────────────────────────────────────────────────

export interface ErrorFallbackProps {
  error: Error;
  resetError: () => void;
}

/**
 * Default fallback UI shown when an unhandled error crashes a subtree.
 * Provides the error message and a retry button.
 */
export function ErrorFallback({ error, resetError }: ErrorFallbackProps) {
  return (
    <div
      role="alert"
      className="mx-auto mt-12 max-w-lg border border-primary bg-background p-8 text-center animate-fade-in"
    >
      <svg
        className="mx-auto mb-4 h-8 w-8 text-error"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        strokeWidth={2}
        aria-hidden="true"
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"
        />
      </svg>

      <h3 className="text-title-md mb-2 text-on-background">
        Something went wrong
      </h3>

      <p className="mb-6 text-sm text-secondary">
        {error.message || 'An unexpected error occurred.'}
      </p>

      <div className="flex items-center justify-center gap-3">
        <button
          type="button"
          onClick={resetError}
          className="border border-primary bg-primary px-4 py-2 text-sm font-medium text-on-primary hover:opacity-80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary transition-opacity-fast"
        >
          Try Again
        </button>

        <button
          type="button"
          onClick={() => { window.location.reload(); }}
          className="border border-primary bg-background px-4 py-2 text-sm font-medium text-on-background hover:bg-surface-container focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary transition-colors-fast"
        >
          Reload Page
        </button>
      </div>
    </div>
  );
}

// ── Error Boundary class component ────────────────────────────────────────────

export interface ErrorBoundaryProps {
  children: ReactNode;
  /** Custom fallback renderer. Receives the error and a reset callback. */
  fallback?: (props: ErrorFallbackProps) => ReactNode;
  /** Called when an error is caught, before rendering the fallback. */
  onError?: (error: Error, errorInfo: ErrorInfo) => void;
}

interface ErrorBoundaryState {
  error: Error | null;
}

/**
 * React error boundary that catches render-time errors in its subtree,
 * logs them, and renders a fallback UI with retry support.
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo): void {
    logError(error, errorInfo.componentStack ?? undefined);
    this.props.onError?.(error, errorInfo);
  }

  resetError = (): void => {
    this.setState({ error: null });
  };

  render(): ReactNode {
    const { error } = this.state;
    if (error) {
      const FallbackComponent = this.props.fallback ?? ErrorFallback;
      return <FallbackComponent error={error} resetError={this.resetError} />;
    }
    return this.props.children;
  }
}
