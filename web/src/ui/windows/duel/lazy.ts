// The duel windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically.
import { LazyWnd } from '../../wnd/lazy';
import type * as WndDuelConfirm from './WndDuelConfirm';
import type * as WndDuelResult from './WndDuelResult';
import type * as WndResurrectionConfirm from './WndResurrectionConfirm';

export const DUEL_LAZY = {
  WndDuelConfirm: new LazyWnd<typeof WndDuelConfirm.WndDuelConfirm>(
    'WndDuelConfirm',
    async () => (await import('./WndDuelConfirm')).WndDuelConfirm,
  ),
  WndDuelResult: new LazyWnd<typeof WndDuelResult.WndDuelResult>(
    'WndDuelResult',
    async () => (await import('./WndDuelResult')).WndDuelResult,
  ),
  WndResurrectionConfirm: new LazyWnd<typeof WndResurrectionConfirm.WndResurrectionConfirm>(
    'WndResurrectionConfirm',
    async () => (await import('./WndResurrectionConfirm')).WndResurrectionConfirm,
  ),
} as const;
