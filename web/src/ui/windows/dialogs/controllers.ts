// The death and item-handling dialogs of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-22): the lazy classes of `lazy.ts`.
import type { WndCtor } from '../../catalog';
import { DIALOGS_LAZY } from './lazy';

export const DIALOG_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_REVIVAL: () => DIALOGS_LAZY.WndRevival.load(),
  APP_DROP_ITEM: () => DIALOGS_LAZY.WndDropItem.load(),
  APP_DROP_CONFIRM: () => DIALOGS_LAZY.WndDropConfirm.load(),
  APP_INVEN_REMOVE_ITEM: () => DIALOGS_LAZY.WndInvenRemoveItem.load(),
  APP_QUEITMWARNING: () => DIALOGS_LAZY.WndQueItmWarning.load(),
};
