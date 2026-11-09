// The appearance windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-35): the lazy classes of `lazy.ts`.
import type { WndCtor } from '../../catalog';
import { APPEARANCE_LAZY } from './lazy';

export const APPEARANCE_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_CHANGEFACE: () => APPEARANCE_LAZY.WndChangeFace.load(),
  APP_CHANGESEX: () => APPEARANCE_LAZY.WndChangeSex.load(),
  APP_ITEM_TRANSY: () => APPEARANCE_LAZY.WndItemTransy.load(),
};
