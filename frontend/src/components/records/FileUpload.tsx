/**
 * Reusable file upload component with drag-and-drop, click-to-browse,
 * image previews, progress indicator, multiple file support, and
 * client-side file size/type validation.
 *
 * Designed to be used standalone or inside the record form as a field input.
 */

import { useState, useCallback, useRef, useEffect } from 'react';
import {
  validateFiles,
  formatFileSize,
  isImageFile,
  type FileValidationOptions,
  type FileValidationError,
} from './file-validation';

// ── Types ────────────────────────────────────────────────────────────────────

export interface FileUploadProps {
  /** Field name for form binding. */
  name: string;
  /** Whether multiple files can be selected. */
  multiple?: boolean;
  /** Validation constraints. */
  options: FileValidationOptions;
  /** Currently attached files (new uploads). */
  value: File[];
  /** Existing file names from a saved record. */
  existingFiles?: string[];
  /** Called when files change (new uploads added/removed). */
  onChange: (files: File[]) => void;
  /** Called when an existing file is removed. */
  onRemoveExisting?: (filename: string) => void;
  /** Whether the field is in an error state. */
  hasError?: boolean;
  /** Whether the component is disabled. */
  disabled?: boolean;
  /** Simulated upload progress (0–100). Null = no upload in progress. */
  uploadProgress?: number | null;
}

interface FilePreview {
  file: File;
  url: string | null;
}

// ── Component ────────────────────────────────────────────────────────────────

export function FileUpload({
  name,
  multiple = false,
  options,
  value,
  existingFiles = [],
  onChange,
  onRemoveExisting,
  hasError = false,
  disabled = false,
  uploadProgress = null,
}: FileUploadProps) {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [isDragOver, setIsDragOver] = useState(false);
  const [validationErrors, setValidationErrors] = useState<FileValidationError[]>([]);
  const [previews, setPreviews] = useState<FilePreview[]>([]);
  const dragCounter = useRef(0);

  // Generate image previews when value changes
  useEffect(() => {
    const newPreviews: FilePreview[] = value.map((file) => ({
      file,
      url: isImageFile(file) ? URL.createObjectURL(file) : null,
    }));
    setPreviews(newPreviews);

    return () => {
      newPreviews.forEach((p) => {
        if (p.url) URL.revokeObjectURL(p.url);
      });
    };
  }, [value]);

  const processFiles = useCallback(
    (fileList: FileList | File[]) => {
      const files = Array.from(fileList);
      if (files.length === 0) return;

      const existingCount = existingFiles.length + value.length;
      const result = validateFiles(files, options, existingCount);

      setValidationErrors(result.errors);

      if (result.valid.length > 0) {
        if (multiple) {
          onChange([...value, ...result.valid]);
        } else {
          onChange(result.valid.slice(0, 1));
        }
      }
    },
    [options, existingFiles.length, value, multiple, onChange],
  );

  const handleInputChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const files = e.target.files;
      if (files) processFiles(files);
      // Reset input so the same file can be re-selected
      e.target.value = '';
    },
    [processFiles],
  );

  const handleDragEnter = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (disabled) return;
      dragCounter.current += 1;
      if (dragCounter.current === 1) {
        setIsDragOver(true);
      }
    },
    [disabled],
  );

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCounter.current -= 1;
    if (dragCounter.current === 0) {
      setIsDragOver(false);
    }
  }, []);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
  }, []);

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      dragCounter.current = 0;
      setIsDragOver(false);
      if (disabled) return;
      processFiles(e.dataTransfer.files);
    },
    [disabled, processFiles],
  );

  const handleBrowseClick = useCallback(() => {
    if (!disabled) {
      fileInputRef.current?.click();
    }
  }, [disabled]);

  const handleBrowseKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        handleBrowseClick();
      }
    },
    [handleBrowseClick],
  );

  const removeFile = useCallback(
    (index: number) => {
      const next = value.filter((_, i) => i !== index);
      onChange(next);
      // Clear errors related to count since we're removing
      setValidationErrors((prev) => prev.filter((e) => e.reason !== 'count'));
    },
    [value, onChange],
  );

  const removeExistingFile = useCallback(
    (filename: string) => {
      onRemoveExisting?.(filename);
    },
    [onRemoveExisting],
  );

  const clearErrors = useCallback(() => {
    setValidationErrors([]);
  }, []);

  const acceptMime = options.mimeTypes.length > 0 ? options.mimeTypes.join(',') : undefined;
  const totalFiles = existingFiles.length + value.length;
  const canAddMore = options.maxSelect === 0 || totalFiles < options.maxSelect;

  return (
    <div className="space-y-2" data-testid={`file-upload-${name}`}>
      {/* Drop zone */}
      <div
        role="button"
        tabIndex={disabled ? -1 : 0}
        aria-label={`Upload file${multiple ? 's' : ''} for ${name}`}
        aria-disabled={disabled || !canAddMore}
        onDragEnter={handleDragEnter}
        onDragLeave={handleDragLeave}
        onDragOver={handleDragOver}
        onDrop={handleDrop}
        onClick={canAddMore ? handleBrowseClick : undefined}
        onKeyDown={canAddMore ? handleBrowseKeyDown : undefined}
        className={`
          relative flex flex-col items-center justify-center gap-2 border border-dashed
          px-6 py-8 text-center
          ${disabled ? 'cursor-not-allowed bg-surface-dim dark:bg-surface-dim opacity-60' : canAddMore ? 'cursor-pointer' : 'cursor-default bg-surface-dim dark:bg-surface-dim'}
          ${isDragOver ? 'border-primary dark:border-on-primary bg-surface-container-low dark:bg-surface-container-low' : hasError ? 'border-error dark:border-error' : 'border-primary dark:border-on-primary hover:bg-surface-container-low dark:hover:bg-surface-container-low'}
        `}
        data-testid={`file-upload-dropzone-${name}`}
      >
        {/* Upload icon */}
        <svg
          className={`h-10 w-10 ${isDragOver ? 'text-primary dark:text-on-primary' : 'text-primary dark:text-on-primary'}`}
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          strokeWidth={1.5}
          aria-hidden="true"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            d="M12 16.5V9.75m0 0l3 3m-3-3l-3 3M6.75 19.5a4.5 4.5 0 01-1.41-8.775 5.25 5.25 0 0110.233-2.33 3 3 0 013.758 3.848A3.752 3.752 0 0118 19.5H6.75z"
          />
        </svg>

        {isDragOver ? (
          <p className="text-sm font-medium text-on-surface dark:text-on-surface" data-testid="drop-active-text">
            Drop file{multiple ? 's' : ''} here
          </p>
        ) : (
          <>
            <p className="text-sm text-on-surface-variant dark:text-on-surface-variant">
              <span className="font-medium text-on-surface dark:text-on-surface underline">Click to browse</span>
              {' '}or drag and drop
            </p>
            <p className="text-xs text-secondary dark:text-secondary">
              {options.mimeTypes.length > 0
                ? options.mimeTypes.join(', ')
                : 'All file types accepted'}
              {options.maxSize > 0 && ` · Max ${formatFileSize(options.maxSize)}`}
              {multiple && options.maxSelect > 0 && ` · Up to ${options.maxSelect} files`}
            </p>
          </>
        )}
      </div>

      {/* Hidden file input */}
      <input
        ref={fileInputRef}
        type="file"
        accept={acceptMime}
        multiple={multiple}
        onChange={handleInputChange}
        className="hidden"
        disabled={disabled}
        data-testid={`file-input-hidden-${name}`}
        aria-hidden="true"
        tabIndex={-1}
      />

      {/* Upload progress bar */}
      {uploadProgress !== null && (
        <div className="space-y-1" data-testid={`file-upload-progress-${name}`}>
          <div className="flex items-center justify-between text-xs text-secondary dark:text-secondary">
            <span>Uploading…</span>
            <span>{Math.round(uploadProgress)}%</span>
          </div>
          <div className="h-1 w-full overflow-hidden bg-surface-container dark:bg-surface-container">
            <div
              className="h-full bg-primary dark:bg-on-primary"
              style={{ width: `${Math.min(100, Math.max(0, uploadProgress))}%` }}
              role="progressbar"
              aria-valuenow={Math.round(uploadProgress)}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-label="Upload progress"
              data-testid={`file-upload-progress-bar-${name}`}
            />
          </div>
        </div>
      )}

      {/* Validation errors */}
      {validationErrors.length > 0 && (
        <div className="space-y-1" data-testid={`file-upload-errors-${name}`}>
          {validationErrors.map((err, i) => (
            <div
              key={`${err.file.name}-${err.reason}-${i}`}
              className="flex items-start gap-2 border border-error dark:border-error px-3 py-2 text-xs text-error dark:text-error"
              role="alert"
            >
              <svg className="mt-0.5 h-3.5 w-3.5 shrink-0" fill="currentColor" viewBox="0 0 20 20" aria-hidden="true">
                <path
                  fillRule="evenodd"
                  d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.28 7.22a.75.75 0 00-1.06 1.06L8.94 10l-1.72 1.72a.75.75 0 101.06 1.06L10 11.06l1.72 1.72a.75.75 0 101.06-1.06L11.06 10l1.72-1.72a.75.75 0 00-1.06-1.06L10 8.94 8.28 7.22z"
                  clipRule="evenodd"
                />
              </svg>
              <span>{err.message}</span>
            </div>
          ))}
          <button
            type="button"
            onClick={clearErrors}
            className="text-xs text-secondary dark:text-secondary underline hover:text-on-surface dark:hover:text-on-surface"
            data-testid={`file-upload-clear-errors-${name}`}
          >
            Dismiss errors
          </button>
        </div>
      )}

      {/* Existing files */}
      {existingFiles.length > 0 && (
        <div className="space-y-1" data-testid={`file-upload-existing-${name}`}>
          <p className="text-label-sm font-bold uppercase tracking-[0.05em] text-secondary dark:text-secondary">Existing files</p>
          <div className="flex flex-wrap gap-2">
            {existingFiles.map((filename) => (
              <span
                key={filename}
                className="inline-flex items-center gap-1.5 border border-outline-variant dark:border-outline-variant bg-surface-container-low dark:bg-surface-container-low px-2.5 py-1.5 text-xs text-on-surface dark:text-on-surface"
                data-testid={`existing-file-${filename}`}
              >
                <svg className="h-3.5 w-3.5 text-secondary dark:text-secondary" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2} aria-hidden="true">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" />
                </svg>
                {filename}
                {onRemoveExisting && (
                  <button
                    type="button"
                    onClick={() => removeExistingFile(filename)}
                    className="ml-0.5 p-0.5 text-secondary dark:text-secondary hover:bg-surface-container dark:hover:bg-surface-container hover:text-on-surface dark:hover:text-on-surface"
                    aria-label={`Remove ${filename}`}
                    data-testid={`remove-existing-${filename}`}
                  >
                    <svg className="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2} aria-hidden="true">
                      <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </button>
                )}
              </span>
            ))}
          </div>
        </div>
      )}

      {/* New file previews */}
      {previews.length > 0 && (
        <div className="space-y-1" data-testid={`file-upload-previews-${name}`}>
          <p className="text-label-sm font-bold uppercase tracking-[0.05em] text-secondary dark:text-secondary">
            New file{previews.length > 1 ? 's' : ''} ({previews.length})
          </p>
          <div className="flex flex-wrap gap-3">
            {previews.map((preview, index) => (
              <div
                key={`${preview.file.name}-${index}`}
                className="group relative flex flex-col items-center"
                data-testid={`file-preview-${index}`}
              >
                {preview.url ? (
                  <img
                    src={preview.url}
                    alt={`Preview of ${preview.file.name}`}
                    className="h-20 w-20 border border-primary dark:border-on-primary object-cover"
                    data-testid={`file-preview-image-${index}`}
                  />
                ) : (
                  <div
                    className="flex h-20 w-20 items-center justify-center border border-primary dark:border-on-primary bg-surface-container-low dark:bg-surface-container-low"
                    data-testid={`file-preview-icon-${index}`}
                  >
                    <svg className="h-8 w-8 text-secondary dark:text-secondary" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5} aria-hidden="true">
                      <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" />
                    </svg>
                  </div>
                )}
                {/* Remove button */}
                <button
                  type="button"
                  onClick={() => removeFile(index)}
                  className="absolute -right-1.5 -top-1.5 flex h-5 w-5 items-center justify-center bg-primary dark:bg-on-primary text-on-primary dark:text-primary opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                  aria-label={`Remove ${preview.file.name}`}
                  data-testid={`remove-file-${index}`}
                >
                  <svg className="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3} aria-hidden="true">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </button>
                {/* File info */}
                <p className="mt-1 max-w-[80px] truncate text-[10px] text-on-surface-variant dark:text-on-surface-variant" title={preview.file.name}>
                  {preview.file.name}
                </p>
                <p className="text-[10px] text-secondary dark:text-secondary">
                  {formatFileSize(preview.file.size)}
                </p>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* File count indicator */}
      {multiple && options.maxSelect > 0 && (
        <p className="text-xs text-secondary dark:text-secondary" data-testid={`file-upload-count-${name}`}>
          {totalFiles}/{options.maxSelect} file{options.maxSelect > 1 ? 's' : ''}
        </p>
      )}
    </div>
  );
}
