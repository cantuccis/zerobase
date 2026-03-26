import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SettingsPage } from './SettingsPage';
import { ApiError } from '../../lib/api';
import type { Settings, ErrorResponseBody } from '../../lib/api/types';

// ── Test data ────────────────────────────────────────────────────────────────

const DEFAULT_SETTINGS: Settings = {
  smtp: {
    enabled: false,
    host: '',
    port: 587,
    username: '',
    password: '',
    tls: true,
  },
  meta: {
    appName: 'Zerobase',
    appUrl: '',
    senderName: 'Zerobase',
    senderAddress: '',
  },
  s3: {
    enabled: false,
    bucket: '',
    region: '',
    endpoint: '',
    accessKey: '',
    secretKey: '',
    forcePathStyle: false,
  },
};

const CONFIGURED_SETTINGS: Settings = {
  smtp: {
    enabled: true,
    host: 'smtp.example.com',
    port: 465,
    username: 'user@example.com',
    password: '', // write-only
    tls: true,
  },
  meta: {
    appName: 'MyApp',
    appUrl: 'https://example.com',
    senderName: 'MyApp',
    senderAddress: 'noreply@example.com',
  },
  s3: {
    enabled: false,
    bucket: '',
    region: '',
    endpoint: '',
    accessKey: '',
    secretKey: '',
    forcePathStyle: false,
  },
};

const S3_CONFIGURED_SETTINGS: Settings = {
  ...CONFIGURED_SETTINGS,
  s3: {
    enabled: true,
    bucket: 'my-bucket',
    region: 'us-east-1',
    endpoint: 'https://s3.amazonaws.com',
    accessKey: 'AKIAIOSFODNN7EXAMPLE',
    secretKey: '', // write-only
    forcePathStyle: false,
  },
};

// ── Mocks ────────────────────────────────────────────────────────────────────

const mockGetSettings = vi.fn();
const mockUpdateSettings = vi.fn();
const mockTestEmail = vi.fn();

vi.mock('../../lib/auth/client', () => ({
  client: {
    getSettings: (...args: unknown[]) => mockGetSettings(...args),
    updateSettings: (...args: unknown[]) => mockUpdateSettings(...args),
    testEmail: (...args: unknown[]) => mockTestEmail(...args),
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
  value: { href: '', pathname: '/_/', origin: 'http://localhost:8090' },
  writable: true,
});

// ── Helpers ──────────────────────────────────────────────────────────────────

function renderPage() {
  return render(<SettingsPage />);
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('SettingsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetSettings.mockResolvedValue(DEFAULT_SETTINGS);
    mockUpdateSettings.mockResolvedValue(DEFAULT_SETTINGS);
  });

  // ── Loading state ────────────────────────────────────────────────────────

  it('shows loading spinner while fetching settings', () => {
    mockGetSettings.mockReturnValue(new Promise(() => {}));
    renderPage();

    expect(screen.getByText('Loading settings...')).toBeInTheDocument();
  });

  // ── Default state (SMTP disabled) ────────────────────────────────────────

  it('renders SMTP Configuration heading after loading', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('02. Mail Settings')).toBeInTheDocument();
    });
  });

  it('shows Disabled status when SMTP is off', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('smtp-status')).toHaveTextContent('Disabled');
    });
  });

  it('hides SMTP fields when disabled', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('02. Mail Settings')).toBeInTheDocument();
    });

    expect(screen.queryByLabelText(/SMTP Host/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Port/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Username/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Password/)).not.toBeInTheDocument();
  });

  it('does not show test email section when SMTP is disabled', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('02. Mail Settings')).toBeInTheDocument();
    });

    expect(screen.queryByText('04. Test Email')).not.toBeInTheDocument();
  });

  // ── Toggling SMTP on ────────────────────────────────────────────────────

  it('shows SMTP fields after enabling', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable SMTP/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Enable SMTP/ }));

    expect(screen.getByLabelText(/SMTP Host/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Username/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Password/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Sender Address/)).toBeInTheDocument();
  });

  it('shows Not configured status when enabled without host', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable SMTP/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Enable SMTP/ }));

    expect(screen.getByTestId('smtp-status')).toHaveTextContent('Not configured');
  });

  it('shows Send Test Email section when enabled', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable SMTP/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Enable SMTP/ }));

    expect(screen.getByText('Verify your SMTP configuration by sending a test message.')).toBeInTheDocument();
  });

  // ── Loading configured settings ──────────────────────────────────────────

  it('populates fields from API response', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();

    await waitFor(() => {
      expect(screen.getByLabelText(/SMTP Host/)).toHaveValue('smtp.example.com');
    });

    expect(screen.getByLabelText(/Port/)).toHaveValue(465);
    expect(screen.getByLabelText(/Username/)).toHaveValue('user@example.com');
    expect(screen.getByLabelText(/Password/)).toHaveValue(''); // write-only
    expect(screen.getByLabelText(/Sender Address/)).toHaveValue('noreply@example.com');
  });

  it('shows Configured status when host is set and enabled', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('smtp-status')).toHaveTextContent('Configured');
    });
  });

  // ── Validation ────────────────────────────────────────────────────────────

  it('shows validation error when host is empty and SMTP enabled', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable SMTP/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Enable SMTP/ }));
    await user.click(screen.getByText('Save Settings'));

    expect(screen.getByText('SMTP host is required when enabled.')).toBeInTheDocument();
    expect(mockUpdateSettings).not.toHaveBeenCalled();
  });

  it('shows validation error when sender address is empty and SMTP enabled', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable SMTP/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Enable SMTP/ }));

    // Fill host but not sender address
    await user.type(screen.getByLabelText(/SMTP Host/), 'smtp.test.com');
    await user.click(screen.getByText('Save Settings'));

    expect(screen.getByText('Sender address is required when SMTP is enabled.')).toBeInTheDocument();
    expect(mockUpdateSettings).not.toHaveBeenCalled();
  });

  it('clears field error when user changes the field value', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable SMTP/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Enable SMTP/ }));
    await user.click(screen.getByText('Save Settings'));

    expect(screen.getByText('SMTP host is required when enabled.')).toBeInTheDocument();

    await user.type(screen.getByLabelText(/SMTP Host/), 's');

    expect(screen.queryByText('SMTP host is required when enabled.')).not.toBeInTheDocument();
  });

  // ── Saving ────────────────────────────────────────────────────────────────

  it('saves settings successfully', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    mockUpdateSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText(/SMTP Host/)).toHaveValue('smtp.example.com');
    });

    await user.click(screen.getByText('Save Settings'));

    await waitFor(() => {
      expect(screen.getByText('Settings saved successfully.')).toBeInTheDocument();
    });

    expect(mockUpdateSettings).toHaveBeenCalledTimes(1);
  });

  it('shows error when save fails with API error', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    const errorBody: ErrorResponseBody = {
      code: 400,
      message: 'Invalid SMTP configuration.',
      data: {},
    };
    mockUpdateSettings.mockRejectedValue(new ApiError(400, errorBody));
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText(/SMTP Host/)).toHaveValue('smtp.example.com');
    });

    await user.click(screen.getByText('Save Settings'));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Invalid SMTP configuration.');
    });
  });

  it('shows network error when save fails with non-API error', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    mockUpdateSettings.mockRejectedValue(new TypeError('Failed to fetch'));
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText(/SMTP Host/)).toHaveValue('smtp.example.com');
    });

    await user.click(screen.getByText('Save Settings'));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Unable to connect to the server.');
    });
  });

  it('does not send password if left blank', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    mockUpdateSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText(/SMTP Host/)).toHaveValue('smtp.example.com');
    });

    await user.click(screen.getByText('Save Settings'));

    await waitFor(() => {
      expect(mockUpdateSettings).toHaveBeenCalledTimes(1);
    });

    const callArg = mockUpdateSettings.mock.calls[0][0];
    expect(callArg.smtp).not.toHaveProperty('password');
  });

  it('sends password when user types one', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    mockUpdateSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText(/SMTP Host/)).toHaveValue('smtp.example.com');
    });

    await user.type(screen.getByLabelText(/Password/), 'secret123');
    await user.click(screen.getByText('Save Settings'));

    await waitFor(() => {
      expect(mockUpdateSettings).toHaveBeenCalledTimes(1);
    });

    const callArg = mockUpdateSettings.mock.calls[0][0];
    expect(callArg.smtp.password).toBe('secret123');
  });

  // ── Test email ────────────────────────────────────────────────────────────

  it('sends test email successfully', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    mockTestEmail.mockResolvedValue({ success: true });
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByPlaceholderText('recipient@example.com')).toBeInTheDocument();
    });

    await user.type(screen.getByPlaceholderText('recipient@example.com'), 'test@example.com');
    await user.click(screen.getByRole('button', { name: 'Send Test Email' }));

    await waitFor(() => {
      expect(screen.getByText('Test email sent to test@example.com.')).toBeInTheDocument();
    });

    expect(mockTestEmail).toHaveBeenCalledWith('test@example.com');
  });

  it('shows error for empty recipient', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByPlaceholderText('recipient@example.com')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: 'Send Test Email' }));

    expect(screen.getByText('Please enter a recipient email address.')).toBeInTheDocument();
    expect(mockTestEmail).not.toHaveBeenCalled();
  });

  it('shows error when test email API call fails', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    const errorBody: ErrorResponseBody = {
      code: 500,
      message: 'SMTP connection failed.',
      data: {},
    };
    mockTestEmail.mockRejectedValue(new ApiError(500, errorBody));
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByPlaceholderText('recipient@example.com')).toBeInTheDocument();
    });

    await user.type(screen.getByPlaceholderText('recipient@example.com'), 'test@example.com');
    await user.click(screen.getByRole('button', { name: 'Send Test Email' }));

    await waitFor(() => {
      expect(screen.getByText('SMTP connection failed.')).toBeInTheDocument();
    });
  });

  // ── Error state on load ──────────────────────────────────────────────────

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

  // ── TLS toggle ────────────────────────────────────────────────────────────

  it('toggles TLS switch', async () => {
    mockGetSettings.mockResolvedValue(CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Enable TLS/ })).toBeInTheDocument();
    });

    const tlsSwitch = screen.getByRole('switch', { name: /Enable TLS/ });
    expect(tlsSwitch).toHaveAttribute('aria-checked', 'true');

    await user.click(tlsSwitch);

    expect(tlsSwitch).toHaveAttribute('aria-checked', 'false');
  });

  // ══════════════════════════════════════════════════════════════════════════
  // File Storage settings
  // ══════════════════════════════════════════════════════════════════════════

  // ── Default state (S3 disabled) ──────────────────────────────────────────

  it('renders File Storage heading after loading', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('03. File Storage')).toBeInTheDocument();
    });
  });

  it('shows Local storage status when S3 is off', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('storage-status')).toHaveTextContent('Local storage');
    });
  });

  it('hides S3 fields when disabled', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('03. File Storage')).toBeInTheDocument();
    });

    expect(screen.queryByLabelText(/Bucket/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Region/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Endpoint/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Access Key/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Secret Key/)).not.toBeInTheDocument();
  });

  // ── Toggling S3 on ──────────────────────────────────────────────────────

  it('shows S3 fields after enabling', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Use S3 Storage/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Use S3 Storage/ }));

    expect(screen.getByLabelText(/Bucket/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Region/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Endpoint/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Access Key/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Secret Key/)).toBeInTheDocument();
  });

  it('shows Not configured status when S3 enabled without bucket', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Use S3 Storage/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Use S3 Storage/ }));

    expect(screen.getByTestId('storage-status')).toHaveTextContent('Not configured');
  });

  // ── Loading configured S3 settings ──────────────────────────────────────

  it('populates S3 fields from API response', async () => {
    mockGetSettings.mockResolvedValue(S3_CONFIGURED_SETTINGS);
    renderPage();

    await waitFor(() => {
      expect(screen.getByLabelText(/Bucket/)).toHaveValue('my-bucket');
    });

    expect(screen.getByLabelText(/Region/)).toHaveValue('us-east-1');
    expect(screen.getByLabelText(/Endpoint/)).toHaveValue('https://s3.amazonaws.com');
    expect(screen.getByLabelText(/Access Key/)).toHaveValue('AKIAIOSFODNN7EXAMPLE');
    expect(screen.getByLabelText(/Secret Key/)).toHaveValue(''); // write-only
  });

  it('shows S3 status when bucket and region are set and S3 is enabled', async () => {
    mockGetSettings.mockResolvedValue(S3_CONFIGURED_SETTINGS);
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('storage-status')).toHaveTextContent('S3');
    });
  });

  // ── Validation ──────────────────────────────────────────────────────────

  it('shows validation error when bucket is empty and S3 enabled', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Use S3 Storage/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Use S3 Storage/ }));
    await user.click(screen.getByText('Save Storage Settings'));

    expect(screen.getByText('Bucket is required when S3 is enabled.')).toBeInTheDocument();
    expect(mockUpdateSettings).not.toHaveBeenCalled();
  });

  it('shows validation error when region is empty and S3 enabled', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Use S3 Storage/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Use S3 Storage/ }));
    await user.type(screen.getByLabelText(/Bucket/), 'my-bucket');
    await user.click(screen.getByText('Save Storage Settings'));

    expect(screen.getByText('Region is required when S3 is enabled.')).toBeInTheDocument();
    expect(mockUpdateSettings).not.toHaveBeenCalled();
  });

  it('clears storage field error when user changes the field value', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Use S3 Storage/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('switch', { name: /Use S3 Storage/ }));
    await user.click(screen.getByText('Save Storage Settings'));

    expect(screen.getByText('Bucket is required when S3 is enabled.')).toBeInTheDocument();

    await user.type(screen.getByLabelText(/Bucket/), 'b');

    expect(screen.queryByText('Bucket is required when S3 is enabled.')).not.toBeInTheDocument();
  });

  it('does not validate S3 fields when S3 is disabled', async () => {
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByText('03. File Storage')).toBeInTheDocument();
    });

    await user.click(screen.getByText('Save Storage Settings'));

    await waitFor(() => {
      expect(mockUpdateSettings).toHaveBeenCalledTimes(1);
    });
  });

  // ── Saving ──────────────────────────────────────────────────────────────

  it('saves S3 settings successfully', async () => {
    mockGetSettings.mockResolvedValue(S3_CONFIGURED_SETTINGS);
    mockUpdateSettings.mockResolvedValue(S3_CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText(/Bucket/)).toHaveValue('my-bucket');
    });

    await user.click(screen.getByText('Save Storage Settings'));

    await waitFor(() => {
      expect(screen.getByText('Storage settings saved successfully.')).toBeInTheDocument();
    });

    expect(mockUpdateSettings).toHaveBeenCalledTimes(1);
  });

  it('shows error when storage save fails with API error', async () => {
    mockGetSettings.mockResolvedValue(S3_CONFIGURED_SETTINGS);
    const errorBody: ErrorResponseBody = {
      code: 400,
      message: 'Invalid S3 configuration.',
      data: {},
    };
    mockUpdateSettings.mockRejectedValue(new ApiError(400, errorBody));
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText(/Bucket/)).toHaveValue('my-bucket');
    });

    await user.click(screen.getByText('Save Storage Settings'));

    await waitFor(() => {
      expect(screen.getByText('Invalid S3 configuration.')).toBeInTheDocument();
    });
  });

  it('shows network error when storage save fails with non-API error', async () => {
    mockGetSettings.mockResolvedValue(S3_CONFIGURED_SETTINGS);
    mockUpdateSettings.mockRejectedValue(new TypeError('Failed to fetch'));
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText(/Bucket/)).toHaveValue('my-bucket');
    });

    await user.click(screen.getByText('Save Storage Settings'));

    await waitFor(() => {
      expect(screen.getByText('Unable to connect to the server.')).toBeInTheDocument();
    });
  });

  it('does not send secretKey if left blank', async () => {
    mockGetSettings.mockResolvedValue(S3_CONFIGURED_SETTINGS);
    mockUpdateSettings.mockResolvedValue(S3_CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText(/Bucket/)).toHaveValue('my-bucket');
    });

    await user.click(screen.getByText('Save Storage Settings'));

    await waitFor(() => {
      expect(mockUpdateSettings).toHaveBeenCalledTimes(1);
    });

    const callArg = mockUpdateSettings.mock.calls[0][0];
    expect(callArg.s3).not.toHaveProperty('secretKey');
  });

  it('sends secretKey when user types one', async () => {
    mockGetSettings.mockResolvedValue(S3_CONFIGURED_SETTINGS);
    mockUpdateSettings.mockResolvedValue(S3_CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText(/Bucket/)).toHaveValue('my-bucket');
    });

    await user.type(screen.getByLabelText(/Secret Key/), 'my-secret-key');
    await user.click(screen.getByText('Save Storage Settings'));

    await waitFor(() => {
      expect(mockUpdateSettings).toHaveBeenCalledTimes(1);
    });

    const callArg = mockUpdateSettings.mock.calls[0][0];
    expect(callArg.s3.secretKey).toBe('my-secret-key');
  });

  // ── Force path style toggle ─────────────────────────────────────────────

  it('toggles force path style switch', async () => {
    mockGetSettings.mockResolvedValue(S3_CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByRole('switch', { name: /Force path style/ })).toBeInTheDocument();
    });

    const pathStyleSwitch = screen.getByRole('switch', { name: /Force path style/ });
    expect(pathStyleSwitch).toHaveAttribute('aria-checked', 'false');

    await user.click(pathStyleSwitch);

    expect(pathStyleSwitch).toHaveAttribute('aria-checked', 'true');
  });

  // ── Storage save sends correct payload structure ─────────────────────────

  it('sends correct S3 payload structure on save', async () => {
    mockGetSettings.mockResolvedValue(S3_CONFIGURED_SETTINGS);
    mockUpdateSettings.mockResolvedValue(S3_CONFIGURED_SETTINGS);
    renderPage();
    const user = userEvent.setup();

    await waitFor(() => {
      expect(screen.getByLabelText(/Bucket/)).toHaveValue('my-bucket');
    });

    await user.click(screen.getByText('Save Storage Settings'));

    await waitFor(() => {
      expect(mockUpdateSettings).toHaveBeenCalledTimes(1);
    });

    const callArg = mockUpdateSettings.mock.calls[0][0];
    expect(callArg.s3).toEqual({
      enabled: true,
      bucket: 'my-bucket',
      region: 'us-east-1',
      endpoint: 'https://s3.amazonaws.com',
      accessKey: 'AKIAIOSFODNN7EXAMPLE',
      forcePathStyle: false,
    });
  });
});
