import { render, screen, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { useState } from 'react';
import { AuthSettingsEditor, DEFAULT_AUTH_OPTIONS } from './AuthSettingsEditor';
import type { AuthOptions } from '../../lib/api/types';

/** Stateless render with a mock onChange — good for testing callbacks. */
function renderEditor(overrides: Partial<AuthOptions> = {}, onChange = vi.fn()) {
  const options = { ...DEFAULT_AUTH_OPTIONS, ...overrides };
  return { ...render(<AuthSettingsEditor authOptions={options} onChange={onChange} />), onChange, options };
}

/** Stateful wrapper — the component re-renders when onChange fires. */
function StatefulEditor({ initial, spy }: { initial: AuthOptions; spy?: (o: AuthOptions) => void }) {
  const [opts, setOpts] = useState(initial);
  return (
    <AuthSettingsEditor
      authOptions={opts}
      onChange={(o) => {
        setOpts(o);
        spy?.(o);
      }}
    />
  );
}

function renderStateful(overrides: Partial<AuthOptions> = {}) {
  const spy = vi.fn();
  const initial = { ...DEFAULT_AUTH_OPTIONS, ...overrides };
  return { ...render(<StatefulEditor initial={initial} spy={spy} />), spy };
}

describe('AuthSettingsEditor', () => {
  // ── Rendering ──────────────────────────────────────────────────────────

  it('renders the auth settings editor section', () => {
    renderEditor();
    expect(screen.getByTestId('auth-settings-editor')).toBeInTheDocument();
  });

  // ── Auth Methods ───────────────────────────────────────────────────────

  it('shows all four auth method toggles', () => {
    renderEditor();
    expect(screen.getByTestId('allow-email-auth')).toBeInTheDocument();
    expect(screen.getByTestId('allow-oauth2-auth')).toBeInTheDocument();
    expect(screen.getByTestId('allow-otp-auth')).toBeInTheDocument();
    expect(screen.getByTestId('mfa-enabled')).toBeInTheDocument();
  });

  it('shows email auth enabled by default', () => {
    renderEditor();
    expect(screen.getByTestId('allow-email-auth')).toHaveAttribute('aria-checked', 'true');
  });

  it('shows oauth2 disabled by default', () => {
    renderEditor();
    expect(screen.getByTestId('allow-oauth2-auth')).toHaveAttribute('aria-checked', 'false');
  });

  it('shows otp disabled by default', () => {
    renderEditor();
    expect(screen.getByTestId('allow-otp-auth')).toHaveAttribute('aria-checked', 'false');
  });

  it('shows mfa disabled by default', () => {
    renderEditor();
    expect(screen.getByTestId('mfa-enabled')).toHaveAttribute('aria-checked', 'false');
  });

  // ── Toggle interactions ────────────────────────────────────────────────

  it('toggles email auth off when clicked', async () => {
    const { onChange } = renderEditor();
    const user = userEvent.setup();

    await user.click(screen.getByTestId('allow-email-auth'));

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ allowEmailAuth: false }),
    );
  });

  it('toggles oauth2 on when clicked', async () => {
    const { onChange } = renderEditor();
    const user = userEvent.setup();

    await user.click(screen.getByTestId('allow-oauth2-auth'));

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ allowOauth2Auth: true }),
    );
  });

  it('toggles otp on when clicked', async () => {
    const { onChange } = renderEditor();
    const user = userEvent.setup();

    await user.click(screen.getByTestId('allow-otp-auth'));

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ allowOtpAuth: true }),
    );
  });

  it('toggles mfa on when clicked', async () => {
    const { onChange } = renderEditor();
    const user = userEvent.setup();

    await user.click(screen.getByTestId('mfa-enabled'));

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ mfaEnabled: true }),
    );
  });

  // ── MFA Duration ───────────────────────────────────────────────────────

  it('does not show MFA duration when MFA is disabled', () => {
    renderEditor();
    expect(screen.queryByTestId('mfa-duration-section')).not.toBeInTheDocument();
  });

  it('shows MFA duration section when MFA is enabled', () => {
    renderEditor({ mfaEnabled: true });
    expect(screen.getByTestId('mfa-duration-section')).toBeInTheDocument();
    expect(screen.getByTestId('mfa-duration')).toBeInTheDocument();
  });

  it('updates MFA duration value', () => {
    const { spy } = renderStateful({ mfaEnabled: true, mfaDuration: 0 });

    const input = screen.getByTestId('mfa-duration');
    fireEvent.change(input, { target: { value: '300' } });

    const lastCall = spy.mock.calls[spy.mock.calls.length - 1][0];
    expect(lastCall.mfaDuration).toBe(300);
  });

  // ── Password Requirements ──────────────────────────────────────────────

  it('shows minimum password length input', () => {
    renderEditor();
    expect(screen.getByTestId('min-password-length')).toBeInTheDocument();
  });

  it('shows default minimum password length of 8', () => {
    renderEditor();
    expect(screen.getByTestId('min-password-length')).toHaveValue(8);
  });

  it('updates minimum password length', () => {
    const { spy } = renderStateful();

    const input = screen.getByTestId('min-password-length');
    fireEvent.change(input, { target: { value: '12' } });

    const lastCall = spy.mock.calls[spy.mock.calls.length - 1][0];
    expect(lastCall.minPasswordLength).toBe(12);
  });

  it('enforces minimum password length of 1', () => {
    const { spy } = renderStateful();

    const input = screen.getByTestId('min-password-length');
    fireEvent.change(input, { target: { value: '0' } });

    // Should clamp to 1
    const lastCall = spy.mock.calls[spy.mock.calls.length - 1][0];
    expect(lastCall.minPasswordLength).toBeGreaterThanOrEqual(1);
  });

  // ── Email Policy ───────────────────────────────────────────────────────

  it('shows require email toggle', () => {
    renderEditor();
    expect(screen.getByTestId('require-email')).toBeInTheDocument();
  });

  it('shows require email enabled by default', () => {
    renderEditor();
    expect(screen.getByTestId('require-email')).toHaveAttribute('aria-checked', 'true');
  });

  it('toggles require email off', async () => {
    const { onChange } = renderEditor();
    const user = userEvent.setup();

    await user.click(screen.getByTestId('require-email'));

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ requireEmail: false }),
    );
  });

  // ── Identity Fields ────────────────────────────────────────────────────

  it('shows identity fields input', () => {
    renderEditor();
    expect(screen.getByTestId('identity-fields')).toBeInTheDocument();
  });

  it('shows default identity fields as "email"', () => {
    renderEditor();
    expect(screen.getByTestId('identity-fields')).toHaveValue('email');
  });

  it('updates identity fields from comma-separated input', () => {
    const { spy } = renderStateful();

    const input = screen.getByTestId('identity-fields');
    fireEvent.change(input, { target: { value: 'email, username' } });

    const lastCall = spy.mock.calls[spy.mock.calls.length - 1][0];
    expect(lastCall.identityFields).toEqual(['email', 'username']);
  });

  // ── Rendering with custom values ──────────────────────────────────────

  it('renders with custom auth options', () => {
    renderEditor({
      allowEmailAuth: false,
      allowOauth2Auth: true,
      allowOtpAuth: true,
      mfaEnabled: true,
      mfaDuration: 600,
      minPasswordLength: 12,
      requireEmail: false,
      identityFields: ['email', 'username'],
    });

    expect(screen.getByTestId('allow-email-auth')).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByTestId('allow-oauth2-auth')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('allow-otp-auth')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('mfa-enabled')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('mfa-duration')).toHaveValue(600);
    expect(screen.getByTestId('min-password-length')).toHaveValue(12);
    expect(screen.getByTestId('require-email')).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByTestId('identity-fields')).toHaveValue('email, username');
  });

  // ── Default values ─────────────────────────────────────────────────────

  it('has correct default auth options', () => {
    expect(DEFAULT_AUTH_OPTIONS).toEqual({
      allowEmailAuth: true,
      allowOauth2Auth: false,
      allowOtpAuth: false,
      requireEmail: true,
      mfaEnabled: false,
      mfaDuration: 0,
      minPasswordLength: 8,
      identityFields: ['email'],
      manageRule: null,
    });
  });
});
