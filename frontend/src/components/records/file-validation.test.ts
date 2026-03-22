import { describe, it, expect } from 'vitest';
import {
  formatFileSize,
  isAllowedMimeType,
  isImageFile,
  validateFiles,
  type FileValidationOptions,
} from './file-validation';

// ── Helpers ──────────────────────────────────────────────────────────────────

function createFile(name: string, size: number, type: string): File {
  const buffer = new ArrayBuffer(size);
  return new File([buffer], name, { type });
}

// ── formatFileSize ───────────────────────────────────────────────────────────

describe('formatFileSize', () => {
  it('formats 0 bytes', () => {
    expect(formatFileSize(0)).toBe('0 B');
  });

  it('formats bytes under 1 KB', () => {
    expect(formatFileSize(512)).toBe('512 B');
  });

  it('formats kilobytes', () => {
    expect(formatFileSize(1024)).toBe('1.0 KB');
    expect(formatFileSize(2560)).toBe('2.5 KB');
  });

  it('formats megabytes', () => {
    expect(formatFileSize(1048576)).toBe('1.0 MB');
    expect(formatFileSize(5242880)).toBe('5.0 MB');
  });
});

// ── isAllowedMimeType ────────────────────────────────────────────────────────

describe('isAllowedMimeType', () => {
  it('allows any type when list is empty', () => {
    const file = createFile('test.pdf', 100, 'application/pdf');
    expect(isAllowedMimeType(file, [])).toBe(true);
  });

  it('matches exact MIME type', () => {
    const file = createFile('photo.png', 100, 'image/png');
    expect(isAllowedMimeType(file, ['image/png'])).toBe(true);
    expect(isAllowedMimeType(file, ['image/jpeg'])).toBe(false);
  });

  it('matches wildcard MIME type (e.g., image/*)', () => {
    const png = createFile('photo.png', 100, 'image/png');
    const jpg = createFile('photo.jpg', 100, 'image/jpeg');
    const pdf = createFile('doc.pdf', 100, 'application/pdf');

    expect(isAllowedMimeType(png, ['image/*'])).toBe(true);
    expect(isAllowedMimeType(jpg, ['image/*'])).toBe(true);
    expect(isAllowedMimeType(pdf, ['image/*'])).toBe(false);
  });

  it('matches file extension', () => {
    const pdf = createFile('document.pdf', 100, 'application/pdf');
    expect(isAllowedMimeType(pdf, ['.pdf'])).toBe(true);
    expect(isAllowedMimeType(pdf, ['.doc'])).toBe(false);
  });

  it('matches extension case-insensitively', () => {
    const file = createFile('PHOTO.PNG', 100, 'image/png');
    expect(isAllowedMimeType(file, ['.png'])).toBe(true);
  });

  it('matches any of multiple allowed types', () => {
    const file = createFile('photo.png', 100, 'image/png');
    expect(isAllowedMimeType(file, ['image/jpeg', 'image/png', 'image/gif'])).toBe(true);
  });
});

// ── isImageFile ──────────────────────────────────────────────────────────────

describe('isImageFile', () => {
  it('returns true for image types', () => {
    expect(isImageFile(createFile('a.png', 100, 'image/png'))).toBe(true);
    expect(isImageFile(createFile('a.jpg', 100, 'image/jpeg'))).toBe(true);
    expect(isImageFile(createFile('a.gif', 100, 'image/gif'))).toBe(true);
    expect(isImageFile(createFile('a.webp', 100, 'image/webp'))).toBe(true);
  });

  it('returns false for non-image types', () => {
    expect(isImageFile(createFile('a.pdf', 100, 'application/pdf'))).toBe(false);
    expect(isImageFile(createFile('a.txt', 100, 'text/plain'))).toBe(false);
  });
});

// ── validateFiles ────────────────────────────────────────────────────────────

describe('validateFiles', () => {
  const defaultOptions: FileValidationOptions = {
    maxSize: 5242880, // 5 MB
    maxSelect: 3,
    mimeTypes: [],
  };

  it('accepts valid files', () => {
    const files = [
      createFile('a.txt', 1024, 'text/plain'),
      createFile('b.txt', 2048, 'text/plain'),
    ];
    const result = validateFiles(files, defaultOptions);
    expect(result.valid).toHaveLength(2);
    expect(result.errors).toHaveLength(0);
  });

  it('rejects files exceeding max size', () => {
    const bigFile = createFile('big.bin', 10 * 1048576, 'application/octet-stream');
    const smallFile = createFile('small.txt', 100, 'text/plain');
    const result = validateFiles([bigFile, smallFile], defaultOptions);

    expect(result.valid).toHaveLength(1);
    expect(result.valid[0].name).toBe('small.txt');
    expect(result.errors).toHaveLength(1);
    expect(result.errors[0].reason).toBe('size');
    expect(result.errors[0].file.name).toBe('big.bin');
  });

  it('rejects files with disallowed MIME types', () => {
    const opts: FileValidationOptions = {
      maxSize: 5242880,
      maxSelect: 5,
      mimeTypes: ['image/png', 'image/jpeg'],
    };
    const png = createFile('photo.png', 100, 'image/png');
    const pdf = createFile('doc.pdf', 100, 'application/pdf');
    const result = validateFiles([png, pdf], opts);

    expect(result.valid).toHaveLength(1);
    expect(result.valid[0].name).toBe('photo.png');
    expect(result.errors).toHaveLength(1);
    expect(result.errors[0].reason).toBe('type');
  });

  it('rejects files exceeding max count', () => {
    const opts: FileValidationOptions = { maxSize: 0, maxSelect: 2, mimeTypes: [] };
    const files = [
      createFile('a.txt', 100, 'text/plain'),
      createFile('b.txt', 100, 'text/plain'),
      createFile('c.txt', 100, 'text/plain'),
    ];
    const result = validateFiles(files, opts);

    expect(result.valid).toHaveLength(2);
    expect(result.errors).toHaveLength(1);
    expect(result.errors[0].reason).toBe('count');
    expect(result.errors[0].file.name).toBe('c.txt');
  });

  it('accounts for existing files in count check', () => {
    const opts: FileValidationOptions = { maxSize: 0, maxSelect: 2, mimeTypes: [] };
    const files = [createFile('new.txt', 100, 'text/plain')];
    const result = validateFiles(files, opts, 2); // already at max

    expect(result.valid).toHaveLength(0);
    expect(result.errors).toHaveLength(1);
    expect(result.errors[0].reason).toBe('count');
  });

  it('allows unlimited size when maxSize is 0', () => {
    const opts: FileValidationOptions = { maxSize: 0, maxSelect: 5, mimeTypes: [] };
    const bigFile = createFile('huge.bin', 100 * 1048576, 'application/octet-stream');
    const result = validateFiles([bigFile], opts);

    expect(result.valid).toHaveLength(1);
    expect(result.errors).toHaveLength(0);
  });

  it('allows unlimited count when maxSelect is 0', () => {
    const opts: FileValidationOptions = { maxSize: 0, maxSelect: 0, mimeTypes: [] };
    const files = Array.from({ length: 20 }, (_, i) =>
      createFile(`file${i}.txt`, 100, 'text/plain'),
    );
    const result = validateFiles(files, opts);

    expect(result.valid).toHaveLength(20);
    expect(result.errors).toHaveLength(0);
  });

  it('reports size errors before type errors', () => {
    const opts: FileValidationOptions = {
      maxSize: 100,
      maxSelect: 5,
      mimeTypes: ['image/png'],
    };
    // File is too big AND wrong type — size check comes first
    const file = createFile('big.pdf', 1000, 'application/pdf');
    const result = validateFiles([file], opts);

    expect(result.errors).toHaveLength(1);
    expect(result.errors[0].reason).toBe('size');
  });
});
