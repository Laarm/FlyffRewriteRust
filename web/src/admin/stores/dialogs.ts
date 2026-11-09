// The modal dialogs of the console: any view opens one with `openDialog(render)`; the host (`components/DialogHost.tsx`) shows
// them stacked, the newest on top. The grant dialog, confirmations and the password dialog all go through here, so a dialog can be
// opened from anywhere (map inspector, player rows, the palette) without the opener owning its place in the tree.
import type { ReactNode } from 'react';
import { create } from 'zustand';

export interface DialogEntry {
  readonly id: number;
  /** Renders the dialog; `close` removes it. */
  readonly render: (close: () => void) => ReactNode;
}

interface DialogState {
  readonly entries: readonly DialogEntry[];
}

export const useDialogs = create<DialogState>(() => ({ entries: [] }));

let nextId = 1;

export interface OpenedDialog {
  readonly id: number;
  close(): void;
}

/** Opens a dialog. `render(close)` returns the dialog element (usually a `Dialog` or `ConfirmDialog`). */
export function openDialog(render: (close: () => void) => ReactNode): OpenedDialog {
  const id = nextId++;
  const close = (): void => {
    useDialogs.setState((s) => ({ entries: s.entries.filter((e) => e.id !== id) }));
  };
  useDialogs.setState((s) => ({ entries: [...s.entries, { id, render }] }));
  return { id, close };
}

/** Closes every dialog (a lost session, a route change to the login). */
export function closeAllDialogs(): void {
  useDialogs.setState({ entries: [] });
}
