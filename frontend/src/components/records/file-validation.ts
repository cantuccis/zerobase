/**
 * Client-side file validation utilities for the FileUpload component.
 *
 * Validates file size, MIME type, and count constraints defined in
 * the collection schema's file field options.
 */

export interface FileValidationOptions {
  /** Maximum file size in bytes. 0 means no limit. */
  maxSize: number;
  /** Maximum number of files allowed. */
  maxSelect: number;
  /** Allowed MIME types. Empty array means all types allowed. */
  mimeTypes: string[];
}

export interface FileValidationError {
  file: File;
  reason: 'size' | 'type' | 'count';
  message: string;
}

export interface FileValidationResult {
  valid: File[];
  errors: FileValidationError[];
}

/** Format byte count to human-readable string. */
export function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 B';
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

/** Check if a file's MIME type matches the allowed list. */
export function isAllowedMimeType(file: File, allowedTypes: string[]): boolean {
  if (allowedTypes.length === 0) return true;

  return allowedTypes.some((allowed) => {
    // Exact match (e.g., "image/png")
    if (allowed === file.type) return true;

    // Wildcard match (e.g., "image/*")
    if (allowed.endsWith('/*')) {
      const category = allowed.slice(0, -2);
      return file.type.startsWith(category + '/');
    }

    // Extension match (e.g., ".pdf")
    if (allowed.startsWith('.')) {
      return file.name.toLowerCase().endsWith(allowed.toLowerCase());
    }

    return false;
  });
}

/** Check if a file is an image based on its MIME type. */
export function isImageFile(file: File): boolean {
  return file.type.startsWith('image/');
}

/**
 * Validate a list of files against the field options.
 *
 * @param files - Files to validate
 * @param options - Validation constraints from the field schema
 * @param existingCount - Number of already-attached files (for count check)
 * @returns Object with valid files and validation errors
 */
export function validateFiles(
  files: File[],
  options: FileValidationOptions,
  existingCount: number = 0,
): FileValidationResult {
  const valid: File[] = [];
  const errors: FileValidationError[] = [];

  for (const file of files) {
    // Size check
    if (options.maxSize > 0 && file.size > options.maxSize) {
      errors.push({
        file,
        reason: 'size',
        message: `"${file.name}" exceeds the maximum size of ${formatFileSize(options.maxSize)}.`,
      });
      continue;
    }

    // Type check
    if (!isAllowedMimeType(file, options.mimeTypes)) {
      const allowed = options.mimeTypes.join(', ');
      errors.push({
        file,
        reason: 'type',
        message: `"${file.name}" is not an allowed file type. Allowed: ${allowed}.`,
      });
      continue;
    }

    // Count check
    if (options.maxSelect > 0 && existingCount + valid.length >= options.maxSelect) {
      errors.push({
        file,
        reason: 'count',
        message: `Maximum of ${options.maxSelect} file${options.maxSelect > 1 ? 's' : ''} allowed.`,
      });
      continue;
    }

    valid.push(file);
  }

  return { valid, errors };
}
