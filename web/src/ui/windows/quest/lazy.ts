// The quest windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically. The list `WndQuest` is an APPLET (a registry factory: plain class, answered by
// `controllers.ts` with `Promise.resolve`); the detail, the quick info and the Heaven Tower are lazy. One class per line (the vitest transform `lazyWndSync` reads this file line by line).
import { LazyWnd } from '../../wnd/lazy';

import type * as Detail from './WndQuestDetail';
import type * as QuickInfo from './WndQuestQuickInfo';
import type * as HeavenTower from './WndHeavenTower';

export const QUEST_LAZY = {
  WndQuestDetail: new LazyWnd<typeof Detail.WndQuestDetail>(
    'WndQuestDetail',
    async () => (await import('./WndQuestDetail')).WndQuestDetail,
  ),
  WndQuestQuickInfo: new LazyWnd<typeof QuickInfo.WndQuestQuickInfo>(
    'WndQuestQuickInfo',
    async () => (await import('./WndQuestQuickInfo')).WndQuestQuickInfo,
  ),
  WndHeavenTower: new LazyWnd<typeof HeavenTower.WndHeavenTower>(
    'WndHeavenTower',
    async () => (await import('./WndHeavenTower')).WndHeavenTower,
  ),
} as const;
