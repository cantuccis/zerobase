import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { AuthFieldsDisplay, AUTH_SYSTEM_FIELDS } from './AuthFieldsDisplay';

describe('AuthFieldsDisplay', () => {
  it('renders nothing when collection type is base', () => {
    const { container } = render(<AuthFieldsDisplay collectionType="base" />);
    expect(container.innerHTML).toBe('');
  });

  it('renders nothing when collection type is view', () => {
    const { container } = render(<AuthFieldsDisplay collectionType="view" />);
    expect(container.innerHTML).toBe('');
  });

  it('renders auth fields display when collection type is auth', () => {
    render(<AuthFieldsDisplay collectionType="auth" />);
    expect(screen.getByTestId('auth-fields-display')).toBeInTheDocument();
  });

  it('shows the "Auth System Fields" heading', () => {
    render(<AuthFieldsDisplay collectionType="auth" />);
    expect(screen.getByText('Auth System Fields')).toBeInTheDocument();
  });

  it('shows the "Auto-included" badge', () => {
    render(<AuthFieldsDisplay collectionType="auth" />);
    expect(screen.getByText('Auto-included')).toBeInTheDocument();
  });

  it('shows explanatory text about auto-managed fields', () => {
    render(<AuthFieldsDisplay collectionType="auth" />);
    expect(
      screen.getByText(/automatically managed by the auth system and cannot be removed/),
    ).toBeInTheDocument();
  });

  it('renders all auth system fields', () => {
    render(<AuthFieldsDisplay collectionType="auth" />);
    for (const field of AUTH_SYSTEM_FIELDS) {
      expect(screen.getByTestId(`auth-field-${field.name}`)).toBeInTheDocument();
    }
  });

  it('shows field names for all auth fields', () => {
    render(<AuthFieldsDisplay collectionType="auth" />);
    // Use getByTestId + text content since 'email' appears as both name and type
    for (const field of AUTH_SYSTEM_FIELDS) {
      const row = screen.getByTestId(`auth-field-${field.name}`);
      expect(row).toHaveTextContent(field.name);
    }
  });

  it('shows type badges for each auth field', () => {
    render(<AuthFieldsDisplay collectionType="auth" />);
    // email type, password type, bool type (emailVisibility, verified), text type (tokenKey)
    const emailField = screen.getByTestId('auth-field-email');
    expect(emailField).toHaveTextContent('email');

    const passwordField = screen.getByTestId('auth-field-password');
    expect(passwordField).toHaveTextContent('password');
  });

  it('shows lock icons for all auth fields', () => {
    render(<AuthFieldsDisplay collectionType="auth" />);
    for (const field of AUTH_SYSTEM_FIELDS) {
      expect(screen.getByTestId(`auth-field-lock-${field.name}`)).toBeInTheDocument();
    }
  });

  it('shows descriptions for auth fields', () => {
    render(<AuthFieldsDisplay collectionType="auth" />);
    expect(screen.getByText('User email address')).toBeInTheDocument();
    expect(screen.getByText(/Controls whether email is visible/)).toBeInTheDocument();
    expect(screen.getByText(/Whether the email has been verified/)).toBeInTheDocument();
    expect(screen.getByText(/never returned in API responses/)).toBeInTheDocument();
    expect(screen.getByText(/Per-user token invalidation key/)).toBeInTheDocument();
  });

  it('contains exactly 5 auth system fields', () => {
    expect(AUTH_SYSTEM_FIELDS).toHaveLength(5);
  });
});
