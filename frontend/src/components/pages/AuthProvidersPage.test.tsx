import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { AuthProvidersPage } from './AuthProvidersPage';
import { ApiError } from '../../lib/api';
import type { Settings, ErrorResponseBody } from '../../lib/api/types';

// ── Test data ────────────────────────────────────────────────────────────────

const DEFAULT_SETTINGS: Settings = {
  smtp: {},
  meta: {},
  s3: {},
  auth: {
    oauth2Providers: {},
  },
};

const CONFIGURED_SETTINGS: Settings = {
  smtp: {},
  meta: {},
  s3: {},
  auth: {
    oauth2Providers: {
      google: {
        enabled: true,
        clientId: 'google-client-id-123',
        clientSecret: '', // write-only
        displayName: 'Google',
      },
      microsoft: {
        enabled: false,
        clientId: '',
        clientSecret: '',
        displayName: 'Microsoft',
      },
    },
  },
};

const ALL_ENABLED_SETTINGS: Settings = {
  smtp: {},
  meta: {},
  s3: {},
  auth: {
    oauth2Providers: {
      google: {
        enabled: true,
        clientId: 'google-id',
        clientSecret: '',
        displayName: 'Google',
      },
      microsoft: {
        enabled: true,
        clientId: 'microsoft-id',
        clientSecret: '',
        displayName: 'Microsoft',
      },
    },
  },
};

// ── Mocks ────────────────────────────────────────────────────────────────────

const mockGetSettings = vi.fn();
const mockUpdateSettings = vi.fn();

vi.mock('../../lib/auth/client', () => ({
  client: {
    getSettings: (...args: unknown[]) => mockGetSettings(...args),
    updateSettings: (...args: unknown[]) => mockUpdateSettings(...args),
    get isAuthenticated() {
      return true;
    },
    get token() {
      return 'mock-token';
    },
    logout: vi.fn(),
  },
}));

Object.defineProperty(window, 'location', {
  value: { href: '', pathname: '/_/settings/auth-providers', origin: 'http://localhost:8090' },
  writable: true,
});

// ── Helpers ──────────────────────────────────────────────────────────────────

function renderPage() {
  return render(<AuthProvidersPage />);
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('AuthProvidersPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetSettings.mockResolvedValue(DEFAULT_SETTINGS);
    mockUpdateSettings.mockResolvedValue(DEFAULT_SETTINGS);
  });

  // ── Loading state ──────────────────────────────────────────────────────

  it('shows loading spinner while fetching settings', () => {
    mockGetSettings.mockReturnValue(new Promise(() => {}));
    renderPage();

    expect(screen.getByText('Loading auth providers...')).toBeInTheDocument();
  });

  // ── Default state (no providers configured) ────────────────────────────

  it('renders Google and Microsoft provider cards after loading', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('Google')).toBeInTheDocument();
    });

    expect(screen.getByText('Microsoft')).toBeInTheDocument();
  });

  it('shows Disabled status for both providers by default', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('google-status')).toHaveTextContent('Disabled');
    });

    expect(screen.getByTestId('microsoft-status')).toHaveTextContent('Disabled');
  });

  it('hides configuration fields when providers are disabled', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('Google')).toBeInTheDocument();
    });

    expect(screen.queryByLabelText(/Client ID/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Client Secret/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Redirect URL/)).not.toBeInTheDocument();
  });

  // ── Toggling provider on ───────────────────────────────────────────────

  it('shows configuration fields after enabling Google', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable Google/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Enable Google/ }));

    expect(screen.getByLabelText(/Client ID/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Client Secret/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Redirect URL/)).toBeInTheDocument();
  });

  it('shows Not configured status when enabled without client ID', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable Google/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Enable Google/ }));

    expect(screen.getByTestId('google-status')).toHaveTextContent('Not configured');
  });

  it('displays the correct redirect URL for the provider', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable Google/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Enable Google/ }));

    const redirectInput = screen.getByTestId('google-redirect-url');
    expect(redirectInput).toHaveValue('http://localhost:8090/api/oauth2/redirect/google');
  });

  // ── Loading configured settings ────────────────────────────────────────

  it('populates fields from API response', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('google-status')).toHaveTextContent('Enabled');
    });

    // Google is enabled and has client ID
    const googleSwitch = screen.getByRole('switch', { name: /Enable Google/ });
    expect(googleSwitch).toHaveAttribute('aria-checked', 'true');

    // Microsoft is disabled
    const microsoftSwitch = screen.getByRole('switch', { name: /Enable Microsoft/ });
    expect(microsoftSwitch).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByTestId('microsoft-status')).toHaveTextContent('Disabled');
  });

  it('shows client ID value from API for enabled provider', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();

    await waitFor(() => {
      expect(screen.getByDisplayValue('google-client-id-123')).toBeInTheDocument();
    });
  });

  it('shows Enabled status when client ID is set and provider is enabled', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('google-status')).toHaveTextContent('Enabled');
    });
  });

  // ── Toggling provider off ──────────────────────────────────────────────

  it('hides fields when disabling a configured provider', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByDisplayValue('google-client-id-123')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Enable Google/ }));

    expect(screen.queryByDisplayValue('google-client-id-123')).not.toBeInTheDocument();
    expect(screen.getByTestId('google-status')).toHaveTextContent('Disabled');
  });

  // ── Validation ─────────────────────────────────────────────────────────

  it('shows validation error when client ID is empty and provider is enabled', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable Google/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Enable Google/ }));
    await user.click(screen.getByText('Save Providers'));

    expect(screen.getByText('Client ID is required when provider is enabled.')).toBeInTheDocument();
    expect(mockUpdateSettings).not.toHaveBeenCalled();
  });

  it('does not validate disabled providers', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('Google')).toBeInTheDocument();
    });

    // Both providers are disabled, save should succeed without validation errors
    await user.click(screen.getByText('Save Providers'));

    await waitFor(() => {
      expect(mockUpdateSettings).toHaveBeenCalledTimes(1);
    });
  });

  it('clears field error when user changes the field value', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable Google/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Enable Google/ }));
    await user.click(screen.getByText('Save Providers'));

    expect(screen.getByText('Client ID is required when provider is enabled.')).toBeInTheDocument();

    await user.type(screen.getByLabelText(/Client ID/), 'a');

    expect(screen.queryByText('Client ID is required when provider is enabled.')).not.toBeInTheDocument();
  });

  // ── Saving ─────────────────────────────────────────────────────────────

  it('saves settings successfully', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    mockUpdateSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByDisplayValue('google-client-id-123')).toBeInTheDocument();
    });

    await user.click(screen.getByText('Save Providers'));

    await waitFor(() => {
      expect(screen.getByText('Auth provider settings saved successfully.')).toBeInTheDocument();
    });

    expect(mockUpdateSettings).toHaveBeenCalledTimes(1);
  });

  it('sends correct payload structure on save', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    mockUpdateSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByDisplayValue('google-client-id-123')).toBeInTheDocument();
    });

    await user.click(screen.getByText('Save Providers'));

    await waitFor(() => {
      expect(mockUpdateSettings).toHaveBeenCalledTimes(1);
    });

    const callArg = mockUpdateSettings.mock.calls[0][0];
    expect(callArg.auth.oauth2Providers.google).toEqual({
      enabled: true,
      clientId: 'google-client-id-123',
      displayName: 'Google',
    });
    // clientSecret should NOT be included when left blank
    expect(callArg.auth.oauth2Providers.google).not.toHaveProperty('clientSecret');
  });

  it('does not send clientSecret if left blank', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    mockUpdateSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByDisplayValue('google-client-id-123')).toBeInTheDocument();
    });

    await user.click(screen.getByText('Save Providers'));

    await waitFor(() => {
      expect(mockUpdateSettings).toHaveBeenCalledTimes(1);
    });

    const callArg = mockUpdateSettings.mock.calls[0][0];
    expect(callArg.auth.oauth2Providers.google).not.toHaveProperty('clientSecret');
    expect(callArg.auth.oauth2Providers.microsoft).not.toHaveProperty('clientSecret');
  });

  it('sends clientSecret when user types one', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    mockUpdateSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByDisplayValue('google-client-id-123')).toBeInTheDocument();
    });

    const secretInputs = screen.getAllByLabelText(/Client Secret/);
    await user.type(secretInputs[0], 'my-new-secret');
    await user.click(screen.getByText('Save Providers'));

    await waitFor(() => {
      expect(mockUpdateSettings).toHaveBeenCalledTimes(1);
    });

    const callArg = mockUpdateSettings.mock.calls[0][0];
    expect(callArg.auth.oauth2Providers.google.clientSecret).toBe('my-new-secret');
  });

  it('shows error when save fails with API error', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    const errorBody: ErrorResponseBody = {
      code: 400,
      message: 'Invalid OAuth2 configuration.',
      data: {},
    };
    mockUpdateSettings.mockRejectedValue(new ApiError(400, errorBody));
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByDisplayValue('google-client-id-123')).toBeInTheDocument();
    });

    await user.click(screen.getByText('Save Providers'));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Invalid OAuth2 configuration.');
    });
  });

  it('shows network error when save fails with non-API error', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    mockUpdateSettings.mockRejectedValue(new TypeError('Failed to fetch'));
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByDisplayValue('google-client-id-123')).toBeInTheDocument();
    });

    await user.click(screen.getByText('Save Providers'));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Unable to connect to the server.');
    });
  });

  // ── Error state on load ────────────────────────────────────────────────

  it('shows error when loading settings fails', async () => {
    const errorBody: ErrorResponseBody = {
      code: 500,
      message: 'Internal server error.',
      data: {},
    };
    mockGetSettings.mockRejectedValue(new ApiError(500, errorBody));
    renderPage();

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Internal server error.');
    });
  });

  it('shows network error on load failure', async () => {
    mockGetSettings.mockRejectedValue(new TypeError('Failed to fetch'));
    renderPage();

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Unable to connect to the server.');
    });
  });

  // ── Multiple providers ─────────────────────────────────────────────────

  it('can enable both providers independently', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable Google/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Enable Google/ }));
    await user.click(screen.getByRole('switch', { name: /Enable Microsoft/ }));

    expect(screen.getByRole('switch', { name: /Enable Google/ })).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByRole('switch', { name: /Enable Microsoft/ })).toHaveAttribute('aria-checked', 'true');

    // Both should show their redirect URLs
    expect(screen.getByTestId('google-redirect-url')).toHaveValue('http://localhost:8090/api/oauth2/redirect/google');
    expect(screen.getByTestId('microsoft-redirect-url')).toHaveValue('http://localhost:8090/api/oauth2/redirect/microsoft');
  });

  it('validates all enabled providers on save', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable Google/ })).toBeInTheDocument();
    });

    // Enable both but don't fill client IDs
    await user.click(screen.getByRole('switch', { name: /Enable Google/ }));
    await user.click(screen.getByRole('switch', { name: /Enable Microsoft/ }));
    await user.click(screen.getByText('Save Providers'));

    // Should show validation errors for both
    const errors = screen.getAllByText('Client ID is required when provider is enabled.');
    expect(errors).toHaveLength(2);
    expect(mockUpdateSettings).not.toHaveBeenCalled();
  });

  it('saves all enabled providers with correct payload', async () => {
    mockGetSettings.mockResolvedValue(ALL_ENABLED_SETTINGS);
    mockUpdateSettings.mockResolvedValue(ALL_ENABLED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByTestId('google-status')).toHaveTextContent('Enabled');
    });

    await user.click(screen.getByText('Save Providers'));

    await waitFor(() => {
      expect(mockUpdateSettings).toHaveBeenCalledTimes(1);
    });

    const callArg = mockUpdateSettings.mock.calls[0][0];
    expect(callArg.auth.oauth2Providers.google.enabled).toBe(true);
    expect(callArg.auth.oauth2Providers.google.clientId).toBe('google-id');
    expect(callArg.auth.oauth2Providers.microsoft.enabled).toBe(true);
    expect(callArg.auth.oauth2Providers.microsoft.clientId).toBe('microsoft-id');
  });

  // ── Redirect URL copy button ───────────────────────────────────────────

  it('has a copy button for the redirect URL', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable Google/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Enable Google/ }));

    expect(screen.getByRole('button', { name: /Copy Google redirect URL/ })).toBeInTheDocument();
  });
});
