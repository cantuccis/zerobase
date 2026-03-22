import {
  createContext,
  useContext,
  useState,
  useCallback,
  useRef,
  type ReactNode,
} from 'react';

// ── Types ────────────────────────────────────────────────────────────────────

export type ToastType = 'success' | 'error' | 'warning' | 'info';

export interface Toast {
  /** Unique identifier for the toast. */
  id: string;
  /** The message to display. */
  message: string;
  /** Visual type controlling colour and icon. */
  type: ToastType;
  /** Duration in ms before auto-dismiss. 0 = persist until manual close. */
  duration: number;
}

export interface ToastOptions {
  /** Duration in ms before auto-dismiss. Defaults to 5000. */
  duration?: number;
}

export interface ToastContextValue {
  /** Current visible toasts (newest last). */
  toasts: Toast[];
  /** Show a success toast. */
  success: (message: string, options?: ToastOptions) => string;
  /** Show an error toast. */
  error: (message: string, options?: ToastOptions) => string;
  /** Show a warning toast. */
  warning: (message: string, options?: ToastOptions) => string;
  /** Show an info toast. */
  info: (message: string, options?: ToastOptions) => string;
  /** Dismiss a specific toast by id. */
  dismiss: (id: string) => void;
  /** Dismiss all toasts. */
  dismissAll: () => void;
}

// ── Defaults ─────────────────────────────────────────────────────────────────

const DEFAULT_DURATION: Record<ToastType, number> = {
  success: 4000,
  error: 6000,
  warning: 5000,
  info: 5000,
};

// ── Context ──────────────────────────────────────────────────────────────────

const ToastContext = createContext<ToastContextValue | null>(null);

// ── Provider ─────────────────────────────────────────────────────────────────

let nextId = 0;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const timersRef = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());

  const dismiss = useCallback((id: string) => {
    const timer = timersRef.current.get(id);
    if (timer) {
      clearTimeout(timer);
      timersRef.current.delete(id);
    }
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const dismissAll = useCallback(() => {
    for (const timer of timersRef.current.values()) {
      clearTimeout(timer);
    }
    timersRef.current.clear();
    setToasts([]);
  }, []);

  const addToast = useCallback(
    (type: ToastType, message: string, options?: ToastOptions): string => {
      const id = `toast-${++nextId}`;
      const duration = options?.duration ?? DEFAULT_DURATION[type];

      const toast: Toast = { id, message, type, duration };
      setToasts((prev) => [...prev, toast]);

      if (duration > 0) {
        const timer = setTimeout(() => {
          timersRef.current.delete(id);
          setToasts((prev) => prev.filter((t) => t.id !== id));
        }, duration);
        timersRef.current.set(id, timer);
      }

      return id;
    },
    [],
  );

  const success = useCallback(
    (message: string, options?: ToastOptions) => addToast('success', message, options),
    [addToast],
  );
  const error = useCallback(
    (message: string, options?: ToastOptions) => addToast('error', message, options),
    [addToast],
  );
  const warning = useCallback(
    (message: string, options?: ToastOptions) => addToast('warning', message, options),
    [addToast],
  );
  const info = useCallback(
    (message: string, options?: ToastOptions) => addToast('info', message, options),
    [addToast],
  );

  return (
    <ToastContext.Provider value={{ toasts, success, error, warning, info, dismiss, dismissAll }}>
      {children}
    </ToastContext.Provider>
  );
}

// ── Hook ─────────────────────────────────────────────────────────────────────

/** Access the toast notification system. Must be used within a ToastProvider. */
export function useToast(): ToastContextValue {
  const ctx = useContext(ToastContext);
  if (!ctx) {
    throw new Error('useToast must be used within a ToastProvider');
  }
  return ctx;
}
