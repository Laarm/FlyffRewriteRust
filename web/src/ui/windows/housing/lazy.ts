// The housing and guild-house windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically. The two applets (`WndHousing` 811, `WndGuildHousing` 853) are registry factories and
// stay plain classes (`controllers.ts` answers them with `Promise.resolve`); the room list, the quit box, the guild-house menu, the SHOWONE strip, the teleporter shell and the upkeep box are lazy. One class per line
// (the vitest transform `lazyWndSync` reads this file line by line).
import { LazyWnd } from '../../wnd/lazy';

import type * as MainMenu from './WndGHMainMenu';
import type * as ShowOne from './WndGHouseShowOneUnit';
import type * as Teleporter from './WndGHTeleporter';
import type * as Upkeep from './WndGHUpkeep';
import type * as QuitRoom from './WndQuitRoom';
import type * as RoomList from './WndRoomList';

export const HOUSING_LAZY = {
  WndRoomList: new LazyWnd<typeof RoomList.WndRoomList>(
    'WndRoomList',
    async () => (await import('./WndRoomList')).WndRoomList,
  ),
  WndQuitRoom: new LazyWnd<typeof QuitRoom.WndQuitRoom>(
    'WndQuitRoom',
    async () => (await import('./WndQuitRoom')).WndQuitRoom,
  ),
  WndGHMainMenu: new LazyWnd<typeof MainMenu.WndGHMainMenu>(
    'WndGHMainMenu',
    async () => (await import('./WndGHMainMenu')).WndGHMainMenu,
  ),
  WndGHouseShowOneUnit: new LazyWnd<typeof ShowOne.WndGHouseShowOneUnit>(
    'WndGHouseShowOneUnit',
    async () => (await import('./WndGHouseShowOneUnit')).WndGHouseShowOneUnit,
  ),
  WndGHTeleporter: new LazyWnd<typeof Teleporter.WndGHTeleporter>(
    'WndGHTeleporter',
    async () => (await import('./WndGHTeleporter')).WndGHTeleporter,
  ),
  WndGHUpkeep: new LazyWnd<typeof Upkeep.WndGHUpkeep>(
    'WndGHUpkeep',
    async () => (await import('./WndGHUpkeep')).WndGHUpkeep,
  ),
} as const;
