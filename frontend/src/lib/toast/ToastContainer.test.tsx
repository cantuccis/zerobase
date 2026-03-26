import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { ToastProvider, useToast } from './ToastContext';
import { ToastContainer } from './ToastContainer';

// ── Test helper ──────────────────────────────────────────────────────────────

function TestHarness() {
  const toast = useToast();

  return (
    <div>
      <button onClick={() => toast.success('Saved successfully')}>Add Success</button>
      <button onClick={() => toast.error('Something went wrong')}>Add Error</button>
      <button onClick={() => toast.warning('Rate limit approaching')}>Add Warning</button>
      <button onClick={() => toast.info('New version available')}>Add Info</button>
      <ToastContainer />
    </div>
  );
}

function renderWithToasts() {
  return render(
    <ToastProvider>
      <TestHarness />
    </ToastProvider>,
  );
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('ToastContainer', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders nothing when there are no toasts', () => {
    render(
      <ToastProvider>
        <ToastContainer />
      </ToastProvider>,
    );

    expect(screen.queryByLabelText('Notifications')).not.toBeInTheDocument();
  });

  it('renders the notification container when toasts exist', () => {
    renderWithToasts();
    fireEvent.click(screen.getByText('Add Success'));

    expect(screen.getByLabelText('Notifications')).toBeInTheDocument();
  });

  it('renders success toast with correct content and role', () => {
    renderWithToasts();
    fireEvent.click(screen.getByText('Add Success'));

    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent('Saved successfully');
    expect(alert).toHaveAttribute('data-toast-type', 'success');
  });

  it('renders error toast with correct styling', () => {
    renderWithToasts();
    fireEvent.click(screen.getByText('Add Error'));

    const alert = screen.getByRole('alert');
    expect(alert).toHaveAttribute('data-toast-type', 'error');
    expect(alert).toHaveTextContent('Something went wrong');
  });

  it('renders warning toast correctly', () => {
    renderWithToasts();
    fireEvent.click(screen.getByText('Add Warning'));

    const alert = screen.getByRole('alert');
    expect(alert).toHaveAttribute('data-toast-type', 'warning');
  });

  it('renders info toast correctly', () => {
    renderWithToasts();
    fireEvent.click(screen.getByText('Add Info'));

    const alert = screen.getByRole('alert');
    expect(alert).toHaveAttribute('data-toast-type', 'info');
  });

  it('stacks multiple toasts visually', () => {
    renderWithToasts();

    fireEvent.click(screen.getByText('Add Success'));
    fireEvent.click(screen.getByText('Add Error'));
    fireEvent.click(screen.getByText('Add Warning'));

    const alerts = screen.getAllByRole('alert');
    expect(alerts).toHaveLength(3);
    expect(alerts[0]).toHaveTextContent('Saved successfully');
    expect(alerts[1]).toHaveTextContent('Something went wrong');
    expect(alerts[2]).toHaveTextContent('Rate limit approaching');
  });

  it('dismiss button removes the toast', () => {
    renderWithToasts();

    fireEvent.click(screen.getByText('Add Success'));
    fireEvent.click(screen.getByText('Add Error'));

    const dismissButtons = screen.getAllByLabelText('Dismiss notification');
    expect(dismissButtons).toHaveLength(2);

    // Dismiss the first toast
    fireEvent.click(dismissButtons[0]);

    const alerts = screen.getAllByRole('alert');
    expect(alerts).toHaveLength(1);
    expect(alerts[0]).toHaveTextContent('Something went wrong');
  });

  it('has accessible dismiss button', () => {
    renderWithToasts();
    fireEvent.click(screen.getByText('Add Success'));

    const dismissBtn = screen.getByLabelText('Dismiss notification');
    expect(dismissBtn).toBeInTheDocument();
    expect(dismissBtn.tagName).toBe('BUTTON');
  });

  it('each toast has aria-live attribute', () => {
    renderWithToasts();
    fireEvent.click(screen.getByText('Add Info'));

    const alert = screen.getByRole('alert');
    expect(alert).toHaveAttribute('aria-live', 'polite');
  });

  it('different toast types have different styling classes', () => {
    renderWithToasts();

    fireEvent.click(screen.getByText('Add Success'));
    fireEvent.click(screen.getByText('Add Error'));

    const alerts = screen.getAllByRole('alert');
    // Success uses default primary border; error uses error-colored border
    expect(alerts[0].className).toContain('border-primary');
    expect(alerts[1].className).toContain('border-error');
  });
});
