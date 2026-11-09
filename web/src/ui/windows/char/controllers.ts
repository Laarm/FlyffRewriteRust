// The character core windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-21): `APP_*` name -> loader. The four applets are plain
// classes (the applet registry builds them synchronously); the stat confirm is a lazy class (`lazy.ts`).
import type { WndCtor } from '../../catalog';
import { CHAR_LAZY } from './lazy';
import { WndCharacter } from './WndCharacter';
import { WndInventory } from './WndInventory';
import { WndMotion } from './WndMotion';
import { WndStatus } from './WndStatus';

export const CHAR_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_INVENTORY: () => Promise.resolve(WndInventory),
  APP_STATUS1: () => Promise.resolve(WndStatus),
  APP_CHARACTER3: () => Promise.resolve(WndCharacter),
  APP_MOTION: () => Promise.resolve(WndMotion),
  APP_STATE_CONFIRM: () => CHAR_LAZY.WndStateConfirm.load(),
};
