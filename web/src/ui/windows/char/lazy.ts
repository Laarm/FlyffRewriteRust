// The stat confirm window in a chunk of its own (`ui/wnd/lazy.ts`): the only place that imports it dynamically. The four core windows (inventory, status,
// character, motion) are applets built synchronously by the applet registry, so their classes stay plain imports.
import { LazyWnd } from '../../wnd/lazy';
import type * as StateConfirm from './WndStateConfirm';

export const CHAR_LAZY = {
  WndStateConfirm: new LazyWnd<typeof StateConfirm.WndStateConfirm>(
    'WndStateConfirm',
    async () => (await import('./WndStateConfirm')).WndStateConfirm,
  ),
} as const;
