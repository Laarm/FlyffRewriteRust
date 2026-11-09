// The siege windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically. The tab pages (`WndGuildCombatTabResultRate`, `WndGuildCombatTabResultLog`,
// `WndGuildCombatRankClass`) are plain classes: the hosts import them, and the catalog answers them with `Promise.resolve`. One class per line (the vitest transform `lazyWndSync` reads this
// file line by line).
import { LazyWnd } from '../../wnd/lazy';

import type * as Offer from './WndGuildCombatOffer';
import type * as State from './WndGuildCombatState';
import type * as Selection from './WndGuildCombatSelection';
import type * as JoinSelection from './WndGuildCombatJoinSelection';
import type * as Board from './WndGuildCombatBoard';
import type * as InfoBox from './WndGuildCombatInfoBox';
import type * as InfoBox2 from './WndGuildCombatInfoBox2';
import type * as CancelConfirm from './WndGuildWarCancelConfirm';
import type * as JoinConfirm from './WndGuildWarJoinConfirm';
import type * as Result from './WndGuildCombatResult';
import type * as RankPerson from './WndGuildCombatRankPerson';
import type * as Ranking from './WndGuildCombatRanking';
import type * as WarState from './WndGuildWarState';

export const SIEGE_LAZY = {
  WndGuildCombatOffer: new LazyWnd<typeof Offer.WndGuildCombatOffer>(
    'WndGuildCombatOffer',
    async () => (await import('./WndGuildCombatOffer')).WndGuildCombatOffer,
  ),
  WndGuildCombatState: new LazyWnd<typeof State.WndGuildCombatState>(
    'WndGuildCombatState',
    async () => (await import('./WndGuildCombatState')).WndGuildCombatState,
  ),
  WndGuildCombatSelection: new LazyWnd<typeof Selection.WndGuildCombatSelection>(
    'WndGuildCombatSelection',
    async () => (await import('./WndGuildCombatSelection')).WndGuildCombatSelection,
  ),
  WndGuildCombatJoinSelection: new LazyWnd<typeof JoinSelection.WndGuildCombatJoinSelection>(
    'WndGuildCombatJoinSelection',
    async () => (await import('./WndGuildCombatJoinSelection')).WndGuildCombatJoinSelection,
  ),
  WndGuildCombatBoard: new LazyWnd<typeof Board.WndGuildCombatBoard>(
    'WndGuildCombatBoard',
    async () => (await import('./WndGuildCombatBoard')).WndGuildCombatBoard,
  ),
  WndGuildCombatInfoBox: new LazyWnd<typeof InfoBox.WndGuildCombatInfoBox>(
    'WndGuildCombatInfoBox',
    async () => (await import('./WndGuildCombatInfoBox')).WndGuildCombatInfoBox,
  ),
  WndGuildCombatInfoBox2: new LazyWnd<typeof InfoBox2.WndGuildCombatInfoBox2>(
    'WndGuildCombatInfoBox2',
    async () => (await import('./WndGuildCombatInfoBox2')).WndGuildCombatInfoBox2,
  ),
  WndGuildWarCancelConfirm: new LazyWnd<typeof CancelConfirm.WndGuildWarCancelConfirm>(
    'WndGuildWarCancelConfirm',
    async () => (await import('./WndGuildWarCancelConfirm')).WndGuildWarCancelConfirm,
  ),
  WndGuildWarJoinConfirm: new LazyWnd<typeof JoinConfirm.WndGuildWarJoinConfirm>(
    'WndGuildWarJoinConfirm',
    async () => (await import('./WndGuildWarJoinConfirm')).WndGuildWarJoinConfirm,
  ),
  WndGuildCombatResult: new LazyWnd<typeof Result.WndGuildCombatResult>(
    'WndGuildCombatResult',
    async () => (await import('./WndGuildCombatResult')).WndGuildCombatResult,
  ),
  WndGuildCombatRankPerson: new LazyWnd<typeof RankPerson.WndGuildCombatRankPerson>(
    'WndGuildCombatRankPerson',
    async () => (await import('./WndGuildCombatRankPerson')).WndGuildCombatRankPerson,
  ),
  WndGuildCombatRanking: new LazyWnd<typeof Ranking.WndGuildCombatRanking>(
    'WndGuildCombatRanking',
    async () => (await import('./WndGuildCombatRanking')).WndGuildCombatRanking,
  ),
  WndGuildWarState: new LazyWnd<typeof WarState.WndGuildWarState>(
    'WndGuildWarState',
    async () => (await import('./WndGuildWarState')).WndGuildWarState,
  ),
} as const;
