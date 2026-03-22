import { render, screen, act, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { ToastProvider, useToast } from './ToastContext';

// ── Test helper ──────────────────────────────────────────────────────────────

function TestHarness() {
  const toast = useToast();

  return (
    <div>
      <button onClick={() => toast.success('Success message')}>Add Success</button>
      <button onClick={() => toast.error('Error message')}>Add Error</button>
      <button onClick={() => toast.warning('Warning message')}>Add Warning</button>
      <button onClick={() => toast.info('Info message')}>Add Info</button>
      <button onClick={() => toast.success('Custom duration', { duration: 1000 })}>
        Add Custom Duration
      </button>
      <button onClick={() => toast.success('No auto-dismiss', { duration: 0 })}>
        Add Persistent
      </button>
      <button onClick={() => toast.dismissAll()}>Dismiss All</button>
      <div data-testid="toast-count">{toast.toasts.length}</div>
      <ul>
        {toast.toasts.map((t) => (
          <li key={t.id} data-testid={`toast-${t.id}`}>
            <span>{t.message}</span>
            <span data-testid={`type-${t.id}`}>{t.type}</span>
            <button onClick={() => toast.dismiss(t.id)}>Dismiss {t.id}</button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function renderHarness() {
  return render(
    <ToastProvider>
      <TestHarness />
    </ToastProvider>,
  );
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('ToastContext', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('throws when useToast is used outside a provider', () => {
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<TestHarness />)).toThrow(
      'useToast must be used within a ToastProvider',
    );
    consoleSpy.mockRestore();
  });

  it('starts with no toasts', () => {
    renderHarness();
    expect(screen.getByTestId('toast-count')).toHaveTextContent('0');
  });

  it('adds a success toast', () => {
    renderHarness();
    fireEvent.click(screen.getByText('Add Success'));

    expect(screen.getByTestId('toast-count')).toHaveTextContent('1');
    expect(screen.getByText('Success message')).toBeInTheDocument();
  });

  it('adds toasts of each type with correct type property', () => {
    renderHarness();

    fireEvent.click(screen.getByText('Add Success'));
    fireEvent.click(screen.getByText('Add Error'));
    fireEvent.click(screen.getByText('Add Warning'));
    fireEvent.click(screen.getByText('Add Info'));

    expect(screen.getByTestId('toast-count')).toHaveTextContent('4');
    expect(screen.getByText('Success message')).toBeInTheDocument();
    expect(screen.getByText('Error message')).toBeInTheDocument();
    expect(screen.getByText('Warning message')).toBeInTheDocument();
    expect(screen.getByText('Info message')).toBeInTheDocument();
  });

  it('auto-dismisses after default duration', () => {
    renderHarness();

    fireEvent.click(screen.getByText('Add Success'));
    expect(screen.getByTestId('toast-count')).toHaveTextContent('1');

    // Default success duration is 4000ms
    act(() => {
      vi.advanceTimersByTime(4000);
    });

    expect(screen.getByTestId('toast-count')).toHaveTextContent('0');
  });

  it('auto-dismisses with custom duration', () => {
    renderHarness();

    fireEvent.click(screen.getByText('Add Custom Duration'));
    expect(screen.getByTestId('toast-count')).toHaveTextContent('1');

    act(() => {
      vi.advanceTimersByTime(999);
    });
    expect(screen.getByTestId('toast-count')).toHaveTextContent('1');

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.getByTestId('toast-count')).toHaveTextContent('0');
  });

  it('does not auto-dismiss when duration is 0', () => {
    renderHarness();

    fireEvent.click(screen.getByText('Add Persistent'));
    expect(screen.getByTestId('toast-count')).toHaveTextContent('1');

    act(() => {
      vi.advanceTimersByTime(60000);
    });
    expect(screen.getByTestId('toast-count')).toHaveTextContent('1');
  });

  it('dismisses a specific toast by id', () => {
    renderHarness();

    fireEvent.click(screen.getByText('Add Success'));
    fireEvent.click(screen.getByText('Add Error'));
    expect(screen.getByTestId('toast-count')).toHaveTextContent('2');

    // Dismiss the first toast
    const dismissButtons = screen.getAllByText(/^Dismiss toast-/);
    fireEvent.click(dismissButtons[0]);

    expect(screen.getByTestId('toast-count')).toHaveTextContent('1');
    expect(screen.getByText('Error message')).toBeInTheDocument();
    expect(screen.queryByText('Success message')).not.toBeInTheDocument();
  });

  it('dismisses all toasts at once', () => {
    renderHarness();

    fireEvent.click(screen.getByText('Add Success'));
    fireEvent.click(screen.getByText('Add Error'));
    fireEvent.click(screen.getByText('Add Warning'));
    expect(screen.getByTestId('toast-count')).toHaveTextContent('3');

    fireEvent.click(screen.getByText('Dismiss All'));
    expect(screen.getByTestId('toast-count')).toHaveTextContent('0');
  });

  it('stacks multiple toasts in order', () => {
    renderHarness();

    fireEvent.click(screen.getByText('Add Success'));
    fireEvent.click(screen.getByText('Add Error'));
    fireEvent.click(screen.getByText('Add Info'));

    const items = screen.getAllByRole('listitem');
    expect(items).toHaveLength(3);
    expect(items[0]).toHaveTextContent('Success message');
    expect(items[1]).toHaveTextContent('Error message');
    expect(items[2]).toHaveTextContent('Info message');
  });

  it('each toast type has different default duration', () => {
    renderHarness();

    // Success: 4000ms, Error: 6000ms
    fireEvent.click(screen.getByText('Add Success'));
    fireEvent.click(screen.getByText('Add Error'));
    expect(screen.getByTestId('toast-count')).toHaveTextContent('2');

    // After 4000ms, success should be gone, error should remain
    act(() => {
      vi.advanceTimersByTime(4000);
    });
    expect(screen.getByTestId('toast-count')).toHaveTextContent('1');
    expect(screen.getByText('Error message')).toBeInTheDocument();

    // After another 2000ms, error should be gone
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(screen.getByTestId('toast-count')).toHaveTextContent('0');
  });

  it('returns toast id from add methods', () => {
    let capturedId = '';

    function IdCapture() {
      const toast = useToast();
      return (
        <button
          onClick={() => {
            capturedId = toast.success('test');
          }}
        >
          Add
        </button>
      );
    }

    render(
      <ToastProvider>
        <IdCapture />
      </ToastProvider>,
    );

    fireEvent.click(screen.getByText('Add'));
    expect(capturedId).toMatch(/^toast-\d+$/);
  });
});
