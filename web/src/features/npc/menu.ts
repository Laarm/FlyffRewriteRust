// The click menus of players and NPCs (Plan/45 §11.1-§11.2, `CWndWorld::ShowMoverMenu`, `src:_Interface/WndWorld.cpp:6720-6905`;
// WP-CFEAT-18). Pure functions: which `MMI_*` ids a target offers and what a click on an NPC does (act at once or open the
// popup); the popup and the actions behind the ids are `menuModule.ts`.
//
// The NPC rule of the plan: the ids of `npcs[key].menus` minus the client-hidden ones; none -> the conversation (`MMI_DIALOG`),
// exactly one -> that action at once, two or more -> a popup in id order. (The original counted the hidden ids too; a listed
// `MMI_REPAIR` next to `MMI_TRADE` therefore made a popup of two there; the repair window is dropped, so now the shop opens.)

import {
  MMI_ARENA_ENTER,
  MMI_BANKING,
  MMI_BEAUTYSHOP,
  MMI_BEAUTYSHOP_SKIN,
  MMI_CHEER,
  MMI_DIALOG,
  MMI_DUEL,
  MMI_DUEL_PARTY,
  MMI_FIGHT,
  MMI_GUILDBANKING,
  MMI_GUILDCOMBAT_1TO1_OFFER,
  MMI_GUILDCOMBAT_1TO1_REPAYMENT,
  MMI_INPUT_REWARD,
  MMI_INVITE_CAMPUS,
  MMI_INVITE_COMPANY,
  MMI_INVITE_PARTY,
  MMI_ADD_MESSENGER,
  MMI_LORD_RAINBOW_LADDER,
  MMI_LORD_RAINBOWAPPLICATION,
  MMI_QUERYEQUIP,
  MMI_QUEST,
  MMI_RANK_GUILD,
  MMI_RANK_INFO,
  MMI_RANK_WAR,
  MMI_REMOVE_CAMPUS,
  MMI_REPAIR,
  MMI_SHOW_REWARD,
  MMI_TRACE,
  MMI_TRADE,
  MMI_UPGRADE,
} from '../../gen/defs/neuz';
import {
  TID_GAME_MENU_CAMPUS_INVITATION,
  TID_GAME_MENU_CAMPUS_SEVERANCE,
  TID_MMI_ADD_MESSENGER,
  TID_MMI_CHEER,
  TID_MMI_DIALOG,
  TID_MMI_INVITE_COMPANY,
  TID_MMI_INVITE_PARTY,
  TID_MMI_QUERYEQUIP,
  TID_MMI_TRACE,
  TID_MMI_TRADE,
} from '../../gen/defs/text';

/** The ids that no client lists (Plan/45 §11.2 "hidden"): the repair window is dropped, the rankings and the wanted board are off. */
export const HIDDEN_NPC_MENUS: ReadonlySet<number> = new Set([
  MMI_QUEST,
  MMI_FIGHT,
  MMI_REPAIR,
  MMI_RANK_GUILD,
  MMI_RANK_WAR,
  MMI_RANK_INFO,
  MMI_UPGRADE,
  MMI_INPUT_REWARD,
  MMI_SHOW_REWARD,
]);

/** The event flags the menus read (`g_eLocal.GetState`): `EVE_18` = 1, `ENABLE_GUILD_INVENTORY` = 2, the 1:1 guild combat = 22. */
export const EVENT = { EVE_18: 1, GUILD_INVENTORY: 2, GC_1TO1: 22, RAINBOW_RACE: 25 } as const;

export interface NpcMenuEnv {
  /** The own character is in a guild. */
  guilded(): boolean;
  /** The own character is a vagrant (no job yet): no arena. */
  vagrant(): boolean;
  /** `g_eLocal.GetState(id)`. */
  eventOn(id: number): boolean;
}

const isHiddenId = (id: number, env: NpcMenuEnv): boolean =>
  HIDDEN_NPC_MENUS.has(id) ||
  // the 1:1 guild combat entries are hidden while its event is off
  (id >= MMI_GUILDCOMBAT_1TO1_OFFER &&
    id <= MMI_GUILDCOMBAT_1TO1_REPAYMENT &&
    !env.eventOn(EVENT.GC_1TO1)) ||
  // ---- rainbow race (ui-rainbow): the entries 107-119 are hidden while the event (flag 25) is off
  (id >= MMI_LORD_RAINBOWAPPLICATION &&
    id <= MMI_LORD_RAINBOW_LADDER &&
    !env.eventOn(EVENT.RAINBOW_RACE));

/** The ids of an NPC that the client does not hide, ascending and without repeats (`AppendMenu` loops `i = 0 .. MAX`). */
export function visibleNpcMenus(menus: readonly number[], env: NpcMenuEnv): number[] {
  return [...new Set(menus)].filter((id) => !isHiddenId(id, env)).sort((a, b) => a - b);
}

/** The entries the popup lists: the ids of the NPC that are available to this player right now. */
export function popupMenus(ids: readonly number[], env: NpcMenuEnv): number[] {
  return ids.filter((id) => {
    if (id === MMI_GUILDBANKING) return env.guilded() && env.eventOn(EVENT.GUILD_INVENTORY);
    if (id === MMI_ARENA_ENTER) return !env.vagrant();
    return true;
  });
}

export type NpcClick =
  /** Run the action of this id at once (`OnCommand(nMenu)`). */
  | { readonly kind: 'action'; readonly id: number }
  /** Show the popup with these ids. */
  | { readonly kind: 'popup'; readonly ids: readonly number[] }
  /** Nothing to show or do. */
  | { readonly kind: 'none' };

/** What a click on an NPC with these menus does. */
export function resolveNpcClick(menus: readonly number[], env: NpcMenuEnv): NpcClick {
  const ids = visibleNpcMenus(menus, env);
  if (ids.length === 0) return { kind: 'action', id: MMI_DIALOG };
  if (ids.length === 1) return { kind: 'action', id: ids[0] ?? MMI_DIALOG };
  const shown = popupMenus(ids, env);
  return shown.length === 0 ? { kind: 'none' } : { kind: 'popup', ids: shown };
}

// ---- players -----------------------------------------------------------------------------------------------------------------------

export interface PlayerMenuEnv {
  /** The target has a personal shop open (`m_vtInfo.IsVendorOpen()`): only the equipment can be looked at. */
  targetVendorOpen: boolean;
  /** The own guild rank has `PF_INVITATION`. */
  canInviteToGuild: boolean;
  /** `g_eLocal.GetState(EVE_18)`. */
  eve18: boolean;
  /** The target is alive and both stand in a fight region (`RA_FIGHT`). */
  duelAllowed: boolean;
  /** The own character leads a party. */
  partyLeader: boolean;
  /** The own character belongs to a campus, and whether the target is a member of it. */
  campus: 'none' | 'member' | 'other';
}

/** The menu of a player target, in the order of the original. */
export function playerMenuIds(env: PlayerMenuEnv): number[] {
  if (env.targetVendorOpen) return [MMI_QUERYEQUIP];
  const ids: number[] = [MMI_TRADE, MMI_ADD_MESSENGER, MMI_INVITE_PARTY];
  if (env.canInviteToGuild) ids.push(MMI_INVITE_COMPANY);
  if (env.eve18 && env.duelAllowed) {
    ids.push(MMI_DUEL);
    if (env.partyLeader) ids.push(MMI_DUEL_PARTY);
  }
  ids.push(MMI_TRACE, MMI_QUERYEQUIP, MMI_CHEER);
  // `CCampusHelper`: without a campus the invitation; a member of the own campus the severance; else the invitation
  ids.push(env.campus === 'member' ? MMI_REMOVE_CAMPUS : MMI_INVITE_CAMPUS);
  return ids;
}

/** The label of an id: `TID_MMI_*` of the texts bundle (`TID_MMI_DIALOG + id`), the legacy English duel labels, the campus texts. */
export function menuLabel(id: number, text: (tid: number) => string): string {
  switch (id) {
    case MMI_DUEL:
      return 'Duel - 1 vs 1';
    case MMI_DUEL_PARTY:
      return 'Duel - Party vs Party';
    case MMI_INVITE_CAMPUS:
      return text(TID_GAME_MENU_CAMPUS_INVITATION);
    case MMI_REMOVE_CAMPUS:
      return text(TID_GAME_MENU_CAMPUS_SEVERANCE);
    case MMI_TRADE:
      return text(TID_MMI_TRADE);
    case MMI_ADD_MESSENGER:
      return text(TID_MMI_ADD_MESSENGER);
    case MMI_INVITE_PARTY:
      return text(TID_MMI_INVITE_PARTY);
    case MMI_INVITE_COMPANY:
      return text(TID_MMI_INVITE_COMPANY);
    case MMI_TRACE:
      return text(TID_MMI_TRACE);
    case MMI_QUERYEQUIP:
      return text(TID_MMI_QUERYEQUIP);
    case MMI_CHEER:
      return text(TID_MMI_CHEER);
    default:
      return text(TID_MMI_DIALOG + id);
  }
}

export { MMI_BANKING, MMI_BEAUTYSHOP, MMI_BEAUTYSHOP_SKIN };
