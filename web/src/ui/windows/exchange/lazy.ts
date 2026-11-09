// The exchange window in a chunk of its own (`ui/wnd/lazy.ts`): the only place that imports the shell dynamically.
import { LazyWnd } from '../../wnd/lazy';
import type * as DialogEvent from './WndDialogEvent';

export const EXCHANGE_LAZY = {
  WndDialogEvent: new LazyWnd<typeof DialogEvent.WndDialogEvent>(
    'WndDialogEvent',
    async () => (await import('./WndDialogEvent')).WndDialogEvent,
  ),
} as const;
