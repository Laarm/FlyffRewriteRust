// Toasts (Plan/25 section 2.3): every mutation ends with one: success, or the error with its server code. A store of the
// visible toasts; the host component (`components/Toast.tsx`) renders them and dismisses them after a delay.
import { create } from 'zustand';
import { AdminApiError } from '../api/rest';

export type ToastKind = 'success' | 'error' | 'info';

export interface Toast {
  readonly id: number;
  readonly kind: ToastKind;
  readonly message: string;
  /** The server's error code (`target_outranks`, ...). */
  readonly code?: string;
}

interface ToastState {
  readonly toasts: readonly Toast[];
}

/** At most this many toasts stay on screen; the oldest go first. */
export const MAX_TOASTS = 5;

export const useToasts = create<ToastState>(() => ({ toasts: [] }));

let nextId = 1;

function push(kind: ToastKind, message: string, code?: string): number {
  const id = nextId++;
  useToasts.setState((s) => ({
    toasts: [...s.toasts, { id, kind, message, ...(code === undefined ? {} : { code }) }].slice(
      -MAX_TOASTS,
    ),
  }));
  return id;
}

/** The text of an error for a toast. */
export function errorMessage(error: unknown): { message: string; code?: string } {
  if (error instanceof AdminApiError) {
    const wp = error.missingWp;
    return {
      message: wp ? `${error.message} (needs ${wp})` : error.message,
      code: error.code,
    };
  }
  return { message: error instanceof Error ? error.message : String(error) };
}

export const toast = {
  success: (message: string): number => push('success', message),
  info: (message: string): number => push('info', message),
  /** An error toast; `prefix` says what failed ("Mute failed"). */
  error: (error: unknown, prefix?: string): number => {
    const { message, code } = errorMessage(error);
    return push('error', prefix ? `${prefix}: ${message}` : message, code);
  },
  dismiss: (id: number): void => {
    useToasts.setState((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
  },
  clear: (): void => {
    useToasts.setState({ toasts: [] });
  },
};
