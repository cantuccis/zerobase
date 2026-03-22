import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  ErrorBoundary,
  ErrorFallback,
  logError,
  getErrorLog,
  clearErrorLog,
} from './ErrorBoundary';

// ── Helpers ───────────────────────────────────────────────────────────────────

/** A component that throws on render when `shouldThrow` is true. */
function Bomb({ shouldThrow = true }: { shouldThrow?: boolean }) {
  if (shouldThrow) {
    throw new Error('Boom!');
  }
  return <div data-testid="child">All good</div>;
}

/** Controlled bomb that can be toggled after mount. */
function ToggleBomb() {
  const [shouldThrow, setShouldThrow] = useState(false);
  return (
    <div>
      <button onClick={() => setShouldThrow(true)}>Detonate</button>
      {shouldThrow && <Bomb />}
      {!shouldThrow && <div data-testid="safe">Safe</div>}
    </div>
  );
}

import { useState } from 'react';

// ── Tests: ErrorFallback ──────────────────────────────────────────────────────

describe('ErrorFallback', () => {
  it('renders the error message', () => {
    const error = new Error('Test failure');
    const resetError = vi.fn();

    render(<ErrorFallback error={error} resetError={resetError} />);

    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Something went wrong')).toBeInTheDocument();
    expect(screen.getByText('Test failure')).toBeInTheDocument();
  });

  it('renders a default message when error.message is empty', () => {
    const error = new Error('');
    const resetError = vi.fn();

    render(<ErrorFallback error={error} resetError={resetError} />);

    expect(screen.getByText('An unexpected error occurred.')).toBeInTheDocument();
  });

  it('calls resetError when "Try Again" is clicked', async () => {
    const user = userEvent.setup();
    const error = new Error('Oops');
    const resetError = vi.fn();

    render(<ErrorFallback error={error} resetError={resetError} />);

    await user.click(screen.getByText('Try Again'));
    expect(resetError).toHaveBeenCalledOnce();
  });

  it('reloads the page when "Reload Page" is clicked', async () => {
    const user = userEvent.setup();
    const error = new Error('Oops');
    const resetError = vi.fn();
    const reloadSpy = vi.fn();

    Object.defineProperty(window, 'location', {
      value: { ...window.location, reload: reloadSpy },
      writable: true,
    });

    render(<ErrorFallback error={error} resetError={resetError} />);

    await user.click(screen.getByText('Reload Page'));
    expect(reloadSpy).toHaveBeenCalledOnce();
  });
});

// ── Tests: ErrorBoundary ──────────────────────────────────────────────────────

describe('ErrorBoundary', () => {
  let consoleSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    clearErrorLog();
    // Suppress React error boundary console noise during tests
    consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
  });

  afterEach(() => {
    consoleSpy.mockRestore();
  });

  it('renders children when there is no error', () => {
    render(
      <ErrorBoundary>
        <div data-testid="child">Hello</div>
      </ErrorBoundary>,
    );

    expect(screen.getByTestId('child')).toHaveTextContent('Hello');
  });

  it('renders the default fallback when a child throws', () => {
    render(
      <ErrorBoundary>
        <Bomb />
      </ErrorBoundary>,
    );

    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Boom!')).toBeInTheDocument();
    expect(screen.queryByTestId('child')).not.toBeInTheDocument();
  });

  it('renders a custom fallback when provided', () => {
    const CustomFallback = ({ error, resetError }: { error: Error; resetError: () => void }) => (
      <div data-testid="custom-fallback">
        <span>Custom: {error.message}</span>
        <button onClick={resetError}>Reset</button>
      </div>
    );

    render(
      <ErrorBoundary fallback={CustomFallback}>
        <Bomb />
      </ErrorBoundary>,
    );

    expect(screen.getByTestId('custom-fallback')).toBeInTheDocument();
    expect(screen.getByText('Custom: Boom!')).toBeInTheDocument();
  });

  it('logs the error via logError', () => {
    render(
      <ErrorBoundary>
        <Bomb />
      </ErrorBoundary>,
    );

    const log = getErrorLog();
    expect(log).toHaveLength(1);
    expect(log[0].message).toBe('Boom!');
    expect(log[0].timestamp).toBeTruthy();
  });

  it('calls onError callback when an error is caught', () => {
    const onError = vi.fn();

    render(
      <ErrorBoundary onError={onError}>
        <Bomb />
      </ErrorBoundary>,
    );

    expect(onError).toHaveBeenCalledOnce();
    expect(onError).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'Boom!' }),
      expect.objectContaining({ componentStack: expect.any(String) }),
    );
  });

  it('recovers from error when resetError is called', async () => {
    const user = userEvent.setup();

    // We need a stateful wrapper so that after reset, the child doesn't throw again
    function RecoverableTest() {
      const [broken, setBroken] = useState(true);
      return (
        <ErrorBoundary
          fallback={({ error, resetError }) => (
            <div>
              <span>{error.message}</span>
              <button
                onClick={() => {
                  setBroken(false);
                  resetError();
                }}
              >
                Recover
              </button>
            </div>
          )}
        >
          <Bomb shouldThrow={broken} />
        </ErrorBoundary>
      );
    }

    render(<RecoverableTest />);

    // Should show fallback
    expect(screen.getByText('Boom!')).toBeInTheDocument();

    // Click recover
    await user.click(screen.getByText('Recover'));

    // Should now show the child content
    expect(screen.getByTestId('child')).toHaveTextContent('All good');
    expect(screen.queryByText('Boom!')).not.toBeInTheDocument();
  });

  it('catches errors thrown during event handlers that propagate to render', () => {
    // Event handler errors don't get caught by error boundaries on their own.
    // But errors in render phase (from state changes) do.
    function ThrowOnRender() {
      const [shouldThrow, setShouldThrow] = useState(false);
      if (shouldThrow) throw new Error('Render crash');
      return <button onClick={() => setShouldThrow(true)}>Trigger</button>;
    }

    render(
      <ErrorBoundary>
        <ThrowOnRender />
      </ErrorBoundary>,
    );

    act(() => {
      screen.getByText('Trigger').click();
    });

    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Render crash')).toBeInTheDocument();
  });
});

// ── Tests: Error logging utilities ────────────────────────────────────────────

describe('logError / getErrorLog / clearErrorLog', () => {
  let consoleSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    clearErrorLog();
    consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
  });

  afterEach(() => {
    consoleSpy.mockRestore();
  });

  it('logs an error with message and timestamp', () => {
    logError(new Error('fail'));

    const log = getErrorLog();
    expect(log).toHaveLength(1);
    expect(log[0].message).toBe('fail');
    expect(log[0].timestamp).toMatch(/^\d{4}-\d{2}-\d{2}T/);
  });

  it('logs optional componentStack', () => {
    logError(new Error('fail'), '\n    at Broken\n    at App');

    const log = getErrorLog();
    expect(log[0].componentStack).toBe('\n    at Broken\n    at App');
  });

  it('accumulates multiple errors', () => {
    logError(new Error('one'));
    logError(new Error('two'));
    logError(new Error('three'));

    expect(getErrorLog()).toHaveLength(3);
  });

  it('writes to console.error', () => {
    logError(new Error('console test'));

    expect(consoleSpy).toHaveBeenCalledWith(
      '[ErrorBoundary]',
      'console test',
      expect.objectContaining({ stack: expect.any(String) }),
    );
  });

  it('clearErrorLog empties the log', () => {
    logError(new Error('one'));
    logError(new Error('two'));
    clearErrorLog();

    expect(getErrorLog()).toHaveLength(0);
  });

  it('getErrorLog returns a copy (not the internal array)', () => {
    logError(new Error('test'));
    const copy = getErrorLog();
    clearErrorLog();

    // The copy should still have 1 entry even though we cleared
    expect(copy).toHaveLength(1);
    expect(getErrorLog()).toHaveLength(0);
  });
});
