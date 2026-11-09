// The 1:1 guild combat application status window in a chunk of its own (`ui/wnd/lazy.ts`): the only place that imports this shell dynamically. One class per line (the vitest transform
// `lazyWndSync` reads this file line by line).
import { LazyWnd } from '../../wnd/lazy';

import type * as OfferState from './WndGuildCombat1to1OfferState';

export const GC1TO1_LAZY = {
  WndGuildCombat1to1OfferState: new LazyWnd<typeof OfferState.WndGuildCombat1to1OfferState>(
    'WndGuildCombat1to1OfferState',
    async () => (await import('./WndGuildCombat1to1OfferState')).WndGuildCombat1to1OfferState,
  ),
} as const;
