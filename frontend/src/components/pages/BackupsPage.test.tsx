import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { BackupsPage } from './BackupsPage';
import { ApiError } from '../../lib/api';
import type { BackupEntry } from '../../lib/api/types';

// ── Test data ────────────────────────────────────────────────────────────────

function makeBackup(overrides: Partial<BackupEntry> = {}): BackupEntry {
  return {
    name: 'backup_2026-03-21_120000.zip',
    size: 1048576, // 1 MB
    created: '2026-03-21T12:00:00Z',
    ...overrides,
  };
}

const SAMPLE_BACKUPS: BackupEntry[] = [
  makeBackup({ name: 'backup_2026-03-21_120000.zip', size: 2097152, created: '2026-03-21T12:00:00Z' }),
  makeBackup({ name: 'backup_2026-03-20_080000.zip', size: 1048576, created: '2026-03-20T08:00:00Z' }),
  makeBackup({ name: 'backup_2026-03-19_150000.zip', size: 524288, created: '2026-03-19T15:00:00Z' }),
];

const EMPTY_BACKUPS: BackupEntry[] = [];

// ── Mocks ────────────────────────────────────────────────────────────────────

const mockListBackups = vi.fn();
const mockCreateBackup = vi.fn();
const mockDownloadBackup = vi.fn();
const mockDeleteBackup = vi.fn();
const mockRestoreBackup = vi.fn();

vi.mock('../../lib/auth/client', () => ({
  client: {
    listBackups: (...args: unknown[]) => mockListBackups(...args),
    createBackup: (...args: unknown[]) => mockCreateBackup(...args),
    downloadBackup: (...args: unknown[]) => mockDownloadBackup(...args),
    deleteBackup: (...args: unknown[]) => mockDeleteBackup(...args),
    restoreBackup: (...args: unknown[]) => mockRestoreBackup(...args),
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
  value: { href: '', pathname: '/_/backups', origin: 'http://localhost:8090', reload: vi.fn() },
  writable: true,
});

// Mock URL.createObjectURL and URL.revokeObjectURL
const mockCreateObjectURL = vi.fn().mockReturnValue('blob:mock-url');
const mockRevokeObjectURL = vi.fn();
URL.createObjectURL = mockCreateObjectURL;
URL.revokeObjectURL = mockRevokeObjectURL;

// ── Helpers ──────────────────────────────────────────────────────────────────

function renderPage() {
  return render(<BackupsPage />);
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('BackupsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    mockListBackups.mockResolvedValue([...SAMPLE_BACKUPS]);
    mockCreateBackup.mockResolvedValue(undefined);
    mockDeleteBackup.mockResolvedValue(undefined);
    mockRestoreBackup.mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  // ── Loading states ──────────────────────────────────────────────────────

  it('shows loading skeletons while fetching data', () => {
    mockListBackups.mockReturnValue(new Promise(() => {}));
    renderPage();

    expect(screen.getByTestId('loading-skeleton')).toBeInTheDocument();
    const pulseElements = document.querySelectorAll('.animate-pulse');
    expect(pulseElements.length).toBeGreaterThan(0);
  });

  it('hides loading skeleton after data loads', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.queryByTestId('loading-skeleton')).not.toBeInTheDocument();
    });
  });

  // ── Backup list rendering ───────────────────────────────────────────────

  it('renders backup list with name, size, and date', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backups-list')).toBeInTheDocument();
    });

    // Check all backup names are displayed (mobile + desktop layouts render each name twice)
    expect(screen.getAllByText('backup_2026-03-21_120000.zip').length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText('backup_2026-03-20_080000.zip').length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText('backup_2026-03-19_150000.zip').length).toBeGreaterThanOrEqual(1);

    // Check sizes are formatted (may appear in both mobile + desktop layouts)
    expect(screen.getAllByText('2.0 MB').length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText('1.0 MB').length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText('512.0 KB').length).toBeGreaterThanOrEqual(1);
  });

  it('shows backup count and total size', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backup-count')).toBeInTheDocument();
    });

    const countText = screen.getByTestId('backup-count').textContent;
    expect(countText).toContain('3 backups');
    expect(countText).toContain('3.5 MB');
  });

  it('renders action buttons for each backup', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backups-list')).toBeInTheDocument();
    });

    for (const backup of SAMPLE_BACKUPS) {
      expect(screen.getByTestId(`download-${backup.name}`)).toBeInTheDocument();
      expect(screen.getByTestId(`delete-${backup.name}`)).toBeInTheDocument();
      expect(screen.getByTestId(`restore-${backup.name}`)).toBeInTheDocument();
    }
  });

  // ── Empty state ─────────────────────────────────────────────────────────

  it('shows empty state when no backups exist', async () => {
    mockListBackups.mockResolvedValue(EMPTY_BACKUPS);
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('empty-state')).toBeInTheDocument();
    });

    expect(screen.getByText('No Backups Yet')).toBeInTheDocument();
    expect(screen.getByText('Create your first backup to protect your data.')).toBeInTheDocument();
    expect(screen.getByTestId('empty-create-btn')).toBeInTheDocument();
  });

  // ── Error handling ──────────────────────────────────────────────────────

  it('shows error banner when loading fails', async () => {
    mockListBackups.mockRejectedValue(new ApiError(500, { code: 500, message: 'Internal error' }));
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('error-banner')).toBeInTheDocument();
    });

    expect(screen.getByText(/Failed to load backups/)).toBeInTheDocument();
  });

  it('shows error banner for non-API errors', async () => {
    mockListBackups.mockRejectedValue(new Error('Network failure'));
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('error-banner')).toBeInTheDocument();
    });

    expect(screen.getByText('Failed to load backups. Please try again.')).toBeInTheDocument();
  });

  it('retries fetching when retry button is clicked', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    mockListBackups.mockRejectedValueOnce(new Error('fail'));
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('error-banner')).toBeInTheDocument();
    });

    mockListBackups.mockResolvedValue([...SAMPLE_BACKUPS]);
    await user.click(screen.getByText('Retry'));

    await waitFor(() => {
      expect(screen.getByTestId('backups-list')).toBeInTheDocument();
    });

    expect(mockListBackups).toHaveBeenCalledTimes(2);
  });

  // ── Create backup ──────────────────────────────────────────────────────

  it('creates a backup and shows success message', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('create-backup-btn')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId('create-backup-btn'));

    await waitFor(() => {
      expect(mockCreateBackup).toHaveBeenCalledTimes(1);
    });

    await waitFor(() => {
      expect(screen.getByTestId('success-message')).toBeInTheDocument();
    });

    expect(screen.getByText('Backup created successfully.')).toBeInTheDocument();
    // Should refresh the list after creation
    expect(mockListBackups).toHaveBeenCalledTimes(2);
  });

  it('shows spinner during backup creation', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    let resolveCreate: () => void;
    mockCreateBackup.mockReturnValue(new Promise<void>((resolve) => { resolveCreate = resolve; }));
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('create-backup-btn')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId('create-backup-btn'));

    // Button should show creating state
    expect(screen.getByTestId('create-backup-btn')).toHaveTextContent(/Creating/);
    expect(screen.getByTestId('create-backup-btn')).toBeDisabled();

    // Resolve the creation
    resolveCreate!();
    await waitFor(() => {
      expect(screen.getByTestId('create-backup-btn')).toHaveTextContent('Create Backup');
    });
  });

  it('shows error when backup creation fails', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    mockCreateBackup.mockRejectedValue(new ApiError(500, { code: 500, message: 'Disk full' }));
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('create-backup-btn')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId('create-backup-btn'));

    await waitFor(() => {
      expect(screen.getByTestId('error-banner')).toBeInTheDocument();
    });

    expect(screen.getByText(/Failed to create backup/)).toBeInTheDocument();
  });

  it('creates backup from empty state button', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    mockListBackups.mockResolvedValueOnce(EMPTY_BACKUPS);
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('empty-create-btn')).toBeInTheDocument();
    });

    // After creation, mock returns some backups
    mockListBackups.mockResolvedValue([...SAMPLE_BACKUPS]);
    await user.click(screen.getByTestId('empty-create-btn'));

    await waitFor(() => {
      expect(mockCreateBackup).toHaveBeenCalledTimes(1);
    });
  });

  // ── Download backup ─────────────────────────────────────────────────────

  it('downloads a backup file', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    const mockBlob = new Blob(['backup-data'], { type: 'application/zip' });
    mockDownloadBackup.mockResolvedValue({ blob: () => Promise.resolve(mockBlob) });
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backups-list')).toBeInTheDocument();
    });

    const backupName = SAMPLE_BACKUPS[0].name;
    await user.click(screen.getByTestId(`download-${backupName}`));

    await waitFor(() => {
      expect(mockDownloadBackup).toHaveBeenCalledWith(backupName);
    });

    expect(mockCreateObjectURL).toHaveBeenCalledWith(mockBlob);
    expect(mockRevokeObjectURL).toHaveBeenCalledWith('blob:mock-url');
  });

  it('shows error when download fails', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    mockDownloadBackup.mockRejectedValue(new ApiError(404, { code: 404, message: 'Not found' }));
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backups-list')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId(`download-${SAMPLE_BACKUPS[0].name}`));

    await waitFor(() => {
      expect(screen.getByTestId('error-banner')).toBeInTheDocument();
    });

    expect(screen.getByText(/Failed to download backup/)).toBeInTheDocument();
  });

  // ── Delete backup ───────────────────────────────────────────────────────

  it('shows confirmation modal before deleting', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backups-list')).toBeInTheDocument();
    });

    const backupName = SAMPLE_BACKUPS[0].name;
    await user.click(screen.getByTestId(`delete-${backupName}`));

    expect(screen.getByTestId('confirm-modal')).toBeInTheDocument();
    expect(screen.getByText('Delete Backup')).toBeInTheDocument();
    expect(screen.getByText(/Are you sure you want to delete/)).toBeInTheDocument();
  });

  it('deletes backup after confirmation', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backups-list')).toBeInTheDocument();
    });

    const backupName = SAMPLE_BACKUPS[0].name;
    await user.click(screen.getByTestId(`delete-${backupName}`));
    await user.click(screen.getByTestId('confirm-action'));

    await waitFor(() => {
      expect(mockDeleteBackup).toHaveBeenCalledWith(backupName);
    });

    await waitFor(() => {
      expect(screen.getByTestId('success-message')).toBeInTheDocument();
    });

    expect(screen.getByText(/deleted successfully/)).toBeInTheDocument();
    // Should refresh the list
    expect(mockListBackups).toHaveBeenCalledTimes(2);
  });

  it('cancels delete when cancel button is clicked', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backups-list')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId(`delete-${SAMPLE_BACKUPS[0].name}`));
    expect(screen.getByTestId('confirm-modal')).toBeInTheDocument();

    await user.click(screen.getByTestId('confirm-cancel'));
    expect(screen.queryByTestId('confirm-modal')).not.toBeInTheDocument();
    expect(mockDeleteBackup).not.toHaveBeenCalled();
  });

  it('shows error when delete fails', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    mockDeleteBackup.mockRejectedValue(new ApiError(500, { code: 500, message: 'Cannot delete' }));
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backups-list')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId(`delete-${SAMPLE_BACKUPS[0].name}`));
    await user.click(screen.getByTestId('confirm-action'));

    await waitFor(() => {
      expect(screen.getByTestId('error-banner')).toBeInTheDocument();
    });

    expect(screen.getByText(/Failed to delete backup/)).toBeInTheDocument();
  });

  // ── Restore backup ──────────────────────────────────────────────────────

  it('shows confirmation modal before restoring', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backups-list')).toBeInTheDocument();
    });

    const backupName = SAMPLE_BACKUPS[0].name;
    await user.click(screen.getByTestId(`restore-${backupName}`));

    expect(screen.getByTestId('confirm-modal')).toBeInTheDocument();
    expect(screen.getByText('Restore from Backup')).toBeInTheDocument();
    expect(screen.getByText(/replace the current database/)).toBeInTheDocument();
  });

  it('restores backup after confirmation', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backups-list')).toBeInTheDocument();
    });

    const backupName = SAMPLE_BACKUPS[0].name;
    await user.click(screen.getByTestId(`restore-${backupName}`));
    await user.click(screen.getByTestId('confirm-action'));

    await waitFor(() => {
      expect(mockRestoreBackup).toHaveBeenCalledWith(backupName);
    });

    await waitFor(() => {
      expect(screen.getByTestId('success-message')).toBeInTheDocument();
    });

    expect(screen.getByText(/restored from/)).toBeInTheDocument();
  });

  it('cancels restore when cancel button is clicked', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backups-list')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId(`restore-${SAMPLE_BACKUPS[0].name}`));
    expect(screen.getByTestId('confirm-modal')).toBeInTheDocument();

    await user.click(screen.getByTestId('confirm-cancel'));
    expect(screen.queryByTestId('confirm-modal')).not.toBeInTheDocument();
    expect(mockRestoreBackup).not.toHaveBeenCalled();
  });

  it('shows error when restore fails', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    mockRestoreBackup.mockRejectedValue(new ApiError(500, { code: 500, message: 'Corrupt backup' }));
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backups-list')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId(`restore-${SAMPLE_BACKUPS[0].name}`));
    await user.click(screen.getByTestId('confirm-action'));

    await waitFor(() => {
      expect(screen.getByTestId('error-banner')).toBeInTheDocument();
    });

    expect(screen.getByText(/Failed to restore backup/)).toBeInTheDocument();
  });

  // ── Header and description ─────────────────────────────────────────────

  it('renders header with create button and description', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backups-header')).toBeInTheDocument();
    });

    expect(screen.getByText('DATABASE BACKUPS')).toBeInTheDocument();
    expect(screen.getByTestId('create-backup-btn')).toBeInTheDocument();
    expect(screen.getByTestId('create-backup-btn')).toHaveTextContent('Create Backup');
  });

  // ── Sorting ────────────────────────────────────────────────────────────

  it('displays backups sorted by date descending (newest first)', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backups-list')).toBeInTheDocument();
    });

    // The redesigned component uses CSS grid rows instead of a table
    const backupNames = screen.getByTestId('backups-list').querySelectorAll('[data-testid^="backup-row-"]');
    expect(backupNames).toHaveLength(3);

    // First row should be newest backup (getAllByText because mobile+desktop layouts both render the name)
    expect(within(backupNames[0] as HTMLElement).getAllByText('backup_2026-03-21_120000.zip').length).toBeGreaterThanOrEqual(1);
    expect(within(backupNames[2] as HTMLElement).getAllByText('backup_2026-03-19_150000.zip').length).toBeGreaterThanOrEqual(1);
  });

  // ── Success message auto-dismiss ───────────────────────────────────────

  it('auto-dismisses success message after 5 seconds', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('create-backup-btn')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId('create-backup-btn'));

    await waitFor(() => {
      expect(screen.getByTestId('success-message')).toBeInTheDocument();
    });

    // Advance time by 5 seconds
    vi.advanceTimersByTime(5000);

    await waitFor(() => {
      expect(screen.queryByTestId('success-message')).not.toBeInTheDocument();
    });
  });

  // ── Single backup plural handling ──────────────────────────────────────

  it('shows singular "backup" for single entry', async () => {
    mockListBackups.mockResolvedValue([SAMPLE_BACKUPS[0]]);
    renderPage();

    await waitFor(() => {
      expect(screen.getByTestId('backup-count')).toBeInTheDocument();
    });

    expect(screen.getByTestId('backup-count').textContent).toContain('1 backup');
    expect(screen.getByTestId('backup-count').textContent).not.toContain('1 backups');
  });
});
