// The quest windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-53): the four layouts become `built` - the list (an applet: a plain class), the detail, the quick info and the
// Heaven Tower (lazy classes of `lazy.ts`). Whether the quest applet is AVAILABLE is a different question (PL-148): `ui.quest.enableApplet()` registers its factory only when quest data is loaded.
import type { WndCtor } from '../../catalog';
import { QUEST_LAZY } from './lazy';
import { WndQuest } from './WndQuest';

export const QUEST_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_QUEST_EX_LIST: () => Promise.resolve(WndQuest),
  APP_QUEST_EX_DETAIL: () => QUEST_LAZY.WndQuestDetail.load(),
  APP_QUEST_QUICK_INFO: () => QUEST_LAZY.WndQuestQuickInfo.load(),
  APP_HEAVEN_TOWER: () => QUEST_LAZY.WndHeavenTower.load(),
};
