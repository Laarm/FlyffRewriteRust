// The siege windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-49, -50, -51): the lazy classes of `lazy.ts`, plus the three tab pages (plain classes that their hosts import).
// 16 layouts become `built`: 545, 675, 547, 551, 557, 559, 563, 565, 561, 567, 569, 571, 575, 555, 521, 519; the join confirm sits on `APP_CLOSE_EXISTING_CONNECTION`, which the login family
// already lists (its controller stays the login window; the join confirm is opened through `ui.siege.openJoinConfirm`).
import type { WndCtor } from '../../catalog';
import { SIEGE_LAZY } from './lazy';
import { WndGuildCombatRankClass } from './WndGuildCombatRankClass';
import { WndGuildCombatTabResultLog } from './WndGuildCombatTabResultLog';
import { WndGuildCombatTabResultRate } from './WndGuildCombatTabResultRate';

export const SIEGE_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_GUILDCOMBAT_1TO1_OFFER: () => SIEGE_LAZY.WndGuildCombatOffer.load(),
  APP_GUILDCOMBAT_OFFER: () => SIEGE_LAZY.WndGuildCombatOffer.load(),
  APP_GUILDCOMBAT_OFFERSTATE: () => SIEGE_LAZY.WndGuildCombatState.load(),
  APP_GUILDCOMBAT_SELECTION: () => SIEGE_LAZY.WndGuildCombatSelection.load(),
  APP_GUILDCOMBAT_SELSTART: () => SIEGE_LAZY.WndGuildCombatJoinSelection.load(),
  APP_GUILDCOMBAT_BOARD: () => SIEGE_LAZY.WndGuildCombatBoard.load(),
  APP_GUILDCOMBAT_MSG: () => SIEGE_LAZY.WndGuildCombatInfoBox.load(),
  APP_GUILDCOMBAT_MSG2: () => SIEGE_LAZY.WndGuildCombatInfoBox2.load(),
  APP_GUILDCOMBAT_CANCEL_CONFIRM: () => SIEGE_LAZY.WndGuildWarCancelConfirm.load(),
  APP_GUILDCOMBAT_RESULT_POINT: () => SIEGE_LAZY.WndGuildCombatResult.load(),
  APP_GUILDCOMBAT_TAB_RESULT: () => Promise.resolve(WndGuildCombatTabResultRate),
  APP_GUILDCOMBAT_TAB_RESULT_LOG: () => Promise.resolve(WndGuildCombatTabResultLog),
  APP_GUILDCOMBAT_RANK_P: () => SIEGE_LAZY.WndGuildCombatRankPerson.load(),
  APP_GUILDCOMBAT_RANKINGCLASS: () => Promise.resolve(WndGuildCombatRankClass),
  APP_GUILDCOMBAT_RANKING: () => SIEGE_LAZY.WndGuildCombatRanking.load(),
  APP_GUILD_WAR_STATE: () => SIEGE_LAZY.WndGuildWarState.load(),
};
