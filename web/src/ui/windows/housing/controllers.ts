// The housing and guild-house windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-58, WP-UI-59): the seven layouts become `built` - the two applets (`APP_HOUSING`, `APP_GH_FURNITURE_STORAGE`) as plain classes,
// the room list, the quit box, the guild-house menu, the teleporter shell and the SHOWONE strip as lazy classes. The upkeep box sits on `APP_CONFIRM_BUYEX` (306), which stays in `NPC_CONTROLLERS`: no second entry.
import type { WndCtor } from '../../catalog';
import { HOUSING_LAZY } from './lazy';
import { WndGuildHousing } from './WndGuildHousing';
import { WndHousing } from './WndHousing';

export const HOUSING_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_HOUSING: () => Promise.resolve(WndHousing),
  APP_MINIROOM_LIST: () => HOUSING_LAZY.WndRoomList.load(),
  APP_QUIT_ROOM: () => HOUSING_LAZY.WndQuitRoom.load(),
  APP_GH_MAIN: () => HOUSING_LAZY.WndGHMainMenu.load(),
  APP_GH_FURNITURE_STORAGE: () => Promise.resolve(WndGuildHousing),
  APP_GH_TELEPORTER: () => HOUSING_LAZY.WndGHTeleporter.load(),
  APP_GH_SHOWONE: () => HOUSING_LAZY.WndGHouseShowOneUnit.load(),
};
