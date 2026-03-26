import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { FileUpload, type FileUploadProps } from './FileUpload';

// ── Helpers ──────────────────────────────────────────────────────────────────

function createFile(name: string, size: number, type: string): File {
  const buffer = new ArrayBuffer(size);
  return new File([buffer], name, { type });
}

function createImageFile(name: string = 'photo.png', size: number = 1024): File {
  return createFile(name, size, 'image/png');
}

const defaultOptions = {
  maxSize: 5242880,
  maxSelect: 5,
  mimeTypes: [] as string[],
};

function renderFileUpload(overrides: Partial<FileUploadProps> = {}) {
  const onChange = vi.fn();
  const props: FileUploadProps = {
    name: 'avatar',
    multiple: false,
    options: defaultOptions,
    value: [],
    onChange,
    ...overrides,
  };

  const result = render(<FileUpload {...props} />);
  return { ...result, onChange };
}

// ── Stub URL.createObjectURL / revokeObjectURL for JSDOM ─────────────────────

beforeEach(() => {
  vi.stubGlobal('URL', {
    ...globalThis.URL,
    createObjectURL: vi.fn(() => 'blob:mock-url'),
    revokeObjectURL: vi.fn(),
  });
});

// ── Rendering ────────────────────────────────────────────────────────────────

describe('FileUpload rendering', () => {
  it('renders the drop zone', () => {
    renderFileUpload();
    expect(screen.getByTestId('file-upload-dropzone-avatar')).toBeInTheDocument();
    expect(screen.getByText('Click to browse')).toBeInTheDocument();
    expect(screen.getByText(/drag and drop/)).toBeInTheDocument();
  });

  it('shows max size info', () => {
    renderFileUpload({ options: { maxSize: 2097152, maxSelect: 1, mimeTypes: [] } });
    expect(screen.getByText(/Max 2\.0 MB/)).toBeInTheDocument();
  });

  it('shows allowed file types', () => {
    renderFileUpload({ options: { maxSize: 0, maxSelect: 1, mimeTypes: ['image/png', 'image/jpeg'] } });
    expect(screen.getByText(/image\/png, image\/jpeg/)).toBeInTheDocument();
  });

  it('shows "All file types accepted" when no mimeTypes restriction', () => {
    renderFileUpload();
    expect(screen.getByText(/All file types accepted/)).toBeInTheDocument();
  });

  it('shows max file count for multi-file mode', () => {
    renderFileUpload({ multiple: true, options: { maxSize: 0, maxSelect: 3, mimeTypes: [] } });
    expect(screen.getByText(/Up to 3 files/)).toBeInTheDocument();
  });

  it('displays existing files', () => {
    renderFileUpload({ existingFiles: ['report.pdf', 'data.csv'] });
    expect(screen.getByTestId('existing-file-report.pdf')).toBeInTheDocument();
    expect(screen.getByTestId('existing-file-data.csv')).toBeInTheDocument();
  });

  it('shows file count indicator for multiple mode', () => {
    renderFileUpload({
      multiple: true,
      options: { maxSize: 0, maxSelect: 5, mimeTypes: [] },
      existingFiles: ['old.txt'],
      value: [createFile('new.txt', 100, 'text/plain')],
    });
    expect(screen.getByTestId('file-upload-count-avatar')).toHaveTextContent('2/5 files');
  });
});

// ── Click to browse ──────────────────────────────────────────────────────────

describe('FileUpload click-to-browse', () => {
  it('triggers file input on drop zone click', async () => {
    renderFileUpload();
    const input = screen.getByTestId('file-input-hidden-avatar') as HTMLInputElement;
    const clickSpy = vi.spyOn(input, 'click');

    await userEvent.click(screen.getByTestId('file-upload-dropzone-avatar'));
    expect(clickSpy).toHaveBeenCalled();
  });

  it('adds files from file input', async () => {
    const { onChange } = renderFileUpload();
    const input = screen.getByTestId('file-input-hidden-avatar');

    const file = createFile('test.txt', 500, 'text/plain');
    await userEvent.upload(input, file);

    expect(onChange).toHaveBeenCalledWith([file]);
  });

  it('supports multiple files in multi mode', async () => {
    const { onChange } = renderFileUpload({ multiple: true });
    const input = screen.getByTestId('file-input-hidden-avatar');

    const files = [
      createFile('a.txt', 100, 'text/plain'),
      createFile('b.txt', 200, 'text/plain'),
    ];
    await userEvent.upload(input, files);

    expect(onChange).toHaveBeenCalledWith(files);
  });

  it('does not open file picker when disabled', async () => {
    renderFileUpload({ disabled: true });
    const input = screen.getByTestId('file-input-hidden-avatar') as HTMLInputElement;
    const clickSpy = vi.spyOn(input, 'click');

    await userEvent.click(screen.getByTestId('file-upload-dropzone-avatar'));
    expect(clickSpy).not.toHaveBeenCalled();
  });
});

// ── Drag and drop ────────────────────────────────────────────────────────────

describe('FileUpload drag and drop', () => {
  it('shows drag-over state on drag enter', () => {
    renderFileUpload();
    const dropzone = screen.getByTestId('file-upload-dropzone-avatar');

    fireEvent.dragEnter(dropzone, { dataTransfer: { files: [] } });
    expect(screen.getByTestId('drop-active-text')).toBeInTheDocument();
    expect(screen.getByTestId('drop-active-text')).toHaveTextContent('Drop file here');
  });

  it('shows plural text in multi mode', () => {
    renderFileUpload({ multiple: true });
    const dropzone = screen.getByTestId('file-upload-dropzone-avatar');

    fireEvent.dragEnter(dropzone, { dataTransfer: { files: [] } });
    expect(screen.getByTestId('drop-active-text')).toHaveTextContent('Drop files here');
  });

  it('removes drag-over state on drag leave', () => {
    renderFileUpload();
    const dropzone = screen.getByTestId('file-upload-dropzone-avatar');

    fireEvent.dragEnter(dropzone, { dataTransfer: { files: [] } });
    expect(screen.getByTestId('drop-active-text')).toBeInTheDocument();

    fireEvent.dragLeave(dropzone, { dataTransfer: { files: [] } });
    expect(screen.queryByTestId('drop-active-text')).not.toBeInTheDocument();
  });

  it('processes dropped files', () => {
    const { onChange } = renderFileUpload();
    const dropzone = screen.getByTestId('file-upload-dropzone-avatar');
    const file = createFile('dropped.txt', 500, 'text/plain');

    fireEvent.drop(dropzone, {
      dataTransfer: { files: [file] },
    });

    expect(onChange).toHaveBeenCalledWith([file]);
  });

  it('ignores drops when disabled', () => {
    const { onChange } = renderFileUpload({ disabled: true });
    const dropzone = screen.getByTestId('file-upload-dropzone-avatar');
    const file = createFile('dropped.txt', 500, 'text/plain');

    fireEvent.drop(dropzone, {
      dataTransfer: { files: [file] },
    });

    expect(onChange).not.toHaveBeenCalled();
  });
});

// ── Validation ───────────────────────────────────────────────────────────────

describe('FileUpload validation', () => {
  it('shows error for oversized files', async () => {
    renderFileUpload({
      options: { maxSize: 1024, maxSelect: 5, mimeTypes: [] },
    });
    const input = screen.getByTestId('file-input-hidden-avatar');
    const bigFile = createFile('huge.bin', 2048, 'application/octet-stream');

    await userEvent.upload(input, bigFile);

    expect(screen.getByTestId('file-upload-errors-avatar')).toBeInTheDocument();
    expect(screen.getByText(/exceeds the maximum size/)).toBeInTheDocument();
  });

  it('shows error for disallowed file types', () => {
    renderFileUpload({
      options: { maxSize: 0, maxSelect: 5, mimeTypes: ['image/png'] },
    });
    const dropzone = screen.getByTestId('file-upload-dropzone-avatar');
    const pdf = createFile('doc.pdf', 100, 'application/pdf');

    // Use drop instead of file input to bypass the accept attribute filtering
    fireEvent.drop(dropzone, {
      dataTransfer: { files: [pdf] },
    });

    expect(screen.getByTestId('file-upload-errors-avatar')).toBeInTheDocument();
    expect(screen.getByText(/not an allowed file type/)).toBeInTheDocument();
  });

  it('shows error when exceeding max file count', async () => {
    const { onChange } = renderFileUpload({
      multiple: true,
      options: { maxSize: 0, maxSelect: 1, mimeTypes: [] },
    });
    const input = screen.getByTestId('file-input-hidden-avatar');
    const files = [
      createFile('a.txt', 100, 'text/plain'),
      createFile('b.txt', 100, 'text/plain'),
    ];

    await userEvent.upload(input, files);

    // Only the first file should be accepted
    expect(onChange).toHaveBeenCalledWith([files[0]]);
    expect(screen.getByText(/Maximum of 1 file allowed/)).toBeInTheDocument();
  });

  it('can dismiss validation errors', async () => {
    renderFileUpload({
      options: { maxSize: 100, maxSelect: 5, mimeTypes: [] },
    });
    const input = screen.getByTestId('file-input-hidden-avatar');
    await userEvent.upload(input, createFile('big.bin', 1000, 'text/plain'));

    expect(screen.getByTestId('file-upload-errors-avatar')).toBeInTheDocument();

    await userEvent.click(screen.getByTestId('file-upload-clear-errors-avatar'));
    expect(screen.queryByTestId('file-upload-errors-avatar')).not.toBeInTheDocument();
  });
});

// ── File previews ────────────────────────────────────────────────────────────

describe('FileUpload previews', () => {
  it('shows image preview for image files', () => {
    const imageFile = createImageFile('photo.png');
    renderFileUpload({ value: [imageFile] });

    expect(screen.getByTestId('file-preview-0')).toBeInTheDocument();
    expect(screen.getByTestId('file-preview-image-0')).toBeInTheDocument();
    expect(screen.getByTestId('file-preview-image-0')).toHaveAttribute('src', 'blob:mock-url');
  });

  it('shows file icon for non-image files', () => {
    const file = createFile('readme.txt', 100, 'text/plain');
    renderFileUpload({ value: [file] });

    expect(screen.getByTestId('file-preview-0')).toBeInTheDocument();
    expect(screen.getByTestId('file-preview-icon-0')).toBeInTheDocument();
  });

  it('shows file name and size in preview', () => {
    const file = createFile('document.pdf', 2048, 'application/pdf');
    renderFileUpload({ value: [file] });

    expect(screen.getByText('document.pdf')).toBeInTheDocument();
    expect(screen.getByText('2.0 KB')).toBeInTheDocument();
  });

  it('shows multiple previews', () => {
    const files = [
      createImageFile('a.png'),
      createFile('b.txt', 100, 'text/plain'),
    ];
    renderFileUpload({ value: files, multiple: true });

    expect(screen.getByTestId('file-preview-0')).toBeInTheDocument();
    expect(screen.getByTestId('file-preview-1')).toBeInTheDocument();
  });
});

// ── File removal ─────────────────────────────────────────────────────────────

describe('FileUpload file removal', () => {
  it('removes a new file on remove button click', async () => {
    const files = [
      createFile('a.txt', 100, 'text/plain'),
      createFile('b.txt', 200, 'text/plain'),
    ];
    const { onChange } = renderFileUpload({ value: files, multiple: true });

    await userEvent.click(screen.getByTestId('remove-file-0'));
    expect(onChange).toHaveBeenCalledWith([files[1]]);
  });

  it('calls onRemoveExisting when removing an existing file', async () => {
    const onRemoveExisting = vi.fn();
    renderFileUpload({
      existingFiles: ['old.txt'],
      onRemoveExisting,
    });

    await userEvent.click(screen.getByTestId('remove-existing-old.txt'));
    expect(onRemoveExisting).toHaveBeenCalledWith('old.txt');
  });
});

// ── Progress indicator ───────────────────────────────────────────────────────

describe('FileUpload progress indicator', () => {
  it('does not show progress bar when uploadProgress is null', () => {
    renderFileUpload({ uploadProgress: null });
    expect(screen.queryByTestId('file-upload-progress-avatar')).not.toBeInTheDocument();
  });

  it('shows progress bar with percentage', () => {
    renderFileUpload({ uploadProgress: 45 });
    expect(screen.getByTestId('file-upload-progress-avatar')).toBeInTheDocument();
    expect(screen.getByText('45%')).toBeInTheDocument();
    expect(screen.getByText('Uploading…')).toBeInTheDocument();
  });

  it('renders progress bar at correct width', () => {
    renderFileUpload({ uploadProgress: 75 });
    const bar = screen.getByTestId('file-upload-progress-bar-avatar');
    expect(bar).toHaveStyle({ width: '75%' });
  });

  it('clamps progress bar between 0 and 100', () => {
    const { rerender } = render(
      <FileUpload
        name="avatar"
        options={defaultOptions}
        value={[]}
        onChange={() => {}}
        uploadProgress={150}
      />,
    );
    const bar = screen.getByTestId('file-upload-progress-bar-avatar');
    expect(bar).toHaveStyle({ width: '100%' });

    rerender(
      <FileUpload
        name="avatar"
        options={defaultOptions}
        value={[]}
        onChange={() => {}}
        uploadProgress={-10}
      />,
    );
    expect(bar).toHaveStyle({ width: '0%' });
  });

  it('progress bar has correct ARIA attributes', () => {
    renderFileUpload({ uploadProgress: 60 });
    const bar = screen.getByRole('progressbar');
    expect(bar).toHaveAttribute('aria-valuenow', '60');
    expect(bar).toHaveAttribute('aria-valuemin', '0');
    expect(bar).toHaveAttribute('aria-valuemax', '100');
  });
});

// ── Accessibility ────────────────────────────────────────────────────────────

describe('FileUpload accessibility', () => {
  it('drop zone has correct aria-label', () => {
    renderFileUpload();
    expect(screen.getByRole('button', { name: /Upload file for avatar/ })).toBeInTheDocument();
  });

  it('drop zone is keyboard accessible', () => {
    const { onChange } = renderFileUpload();
    const dropzone = screen.getByTestId('file-upload-dropzone-avatar');
    expect(dropzone).toHaveAttribute('tabindex', '0');
  });

  it('drop zone is not focusable when disabled', () => {
    renderFileUpload({ disabled: true });
    const dropzone = screen.getByTestId('file-upload-dropzone-avatar');
    expect(dropzone).toHaveAttribute('tabindex', '-1');
  });

  it('remove buttons have aria-labels', () => {
    const file = createFile('test.txt', 100, 'text/plain');
    renderFileUpload({ value: [file] });
    expect(screen.getByLabelText('Remove test.txt')).toBeInTheDocument();
  });

  it('validation errors have alert role', async () => {
    renderFileUpload({
      options: { maxSize: 100, maxSelect: 5, mimeTypes: [] },
    });
    const input = screen.getByTestId('file-input-hidden-avatar');
    await userEvent.upload(input, createFile('big.bin', 1000, 'text/plain'));

    const alerts = screen.getAllByRole('alert');
    expect(alerts.length).toBeGreaterThan(0);
  });
});

// ── Error state ──────────────────────────────────────────────────────────────

describe('FileUpload error state', () => {
  it('applies error styling when hasError is true', () => {
    renderFileUpload({ hasError: true });
    const dropzone = screen.getByTestId('file-upload-dropzone-avatar');
    expect(dropzone.className).toContain('border-error');
  });
});
