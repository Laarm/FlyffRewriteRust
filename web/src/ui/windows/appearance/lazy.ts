// The appearance windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports the shells dynamically.
import { LazyWnd } from '../../wnd/lazy';
import type * as ChangeFace from './WndChangeFace';
import type * as ChangeSex from './WndChangeSex';
import type * as ItemTransy from './WndItemTransy';

export const APPEARANCE_LAZY = {
  WndChangeFace: new LazyWnd<typeof ChangeFace.WndChangeFace>(
    'WndChangeFace',
    async () => (await import('./WndChangeFace')).WndChangeFace,
  ),
  WndChangeSex: new LazyWnd<typeof ChangeSex.WndChangeSex>(
    'WndChangeSex',
    async () => (await import('./WndChangeSex')).WndChangeSex,
  ),
  WndItemTransy: new LazyWnd<typeof ItemTransy.WndItemTransy>(
    'WndItemTransy',
    async () => (await import('./WndItemTransy')).WndItemTransy,
  ),
} as const;
