// The death and item-handling dialogs in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically.
import { LazyWnd } from '../../wnd/lazy';
import type * as WndDropConfirm from './WndDropConfirm';
import type * as WndDropItem from './WndDropItem';
import type * as WndInvenRemoveItem from './WndInvenRemoveItem';
import type * as WndQueItmWarning from './WndQueItmWarning';
import type * as WndRevival from './WndRevival';

export const DIALOGS_LAZY = {
  WndRevival: new LazyWnd<typeof WndRevival.WndRevival>(
    'WndRevival',
    async () => (await import('./WndRevival')).WndRevival,
  ),
  WndDropItem: new LazyWnd<typeof WndDropItem.WndDropItem>(
    'WndDropItem',
    async () => (await import('./WndDropItem')).WndDropItem,
  ),
  WndDropConfirm: new LazyWnd<typeof WndDropConfirm.WndDropConfirm>(
    'WndDropConfirm',
    async () => (await import('./WndDropConfirm')).WndDropConfirm,
  ),
  WndInvenRemoveItem: new LazyWnd<typeof WndInvenRemoveItem.WndInvenRemoveItem>(
    'WndInvenRemoveItem',
    async () => (await import('./WndInvenRemoveItem')).WndInvenRemoveItem,
  ),
  WndQueItmWarning: new LazyWnd<typeof WndQueItmWarning.WndQueItmWarning>(
    'WndQueItmWarning',
    async () => (await import('./WndQueItmWarning')).WndQueItmWarning,
  ),
} as const;
