// The 1:1 guild combat entries of the NPC menu (Plan/45 §11.2, §28.5; `CWndWorld::OnCommand`, `src:_Interface/WndWorld.cpp:4343-4489`, `5063-5130`, `6118-6123`): ids 60-70 and the arena pair 80 / 81.
// `MaFl_Amos` lists `MMI_DIALOG` + 60 / 62 / 63 / 64 / 67 / 68 / 69, `MaFl_Annie` `MMI_DIALOG` + 61 / 65 / 66 / 70. `features/npc/menu.ts` already hides 60-70 while event 22 is off and the module makes
// every action a no-op then (80 / 81 are not gated by the client: the server refuses a vagrant, a missing NPC in range and an exit outside the arena).
//
//   60 OFFER         Gc1to1TenderOpenWndReq          61 CANCEL     the cancel confirm (YES: Gc1to1TenderCancel)     62 OFFERSTATE    Gc1to1TenderViewReq
//   63 SELECTION     Gc1to1MemberLineUpOpenWndReq    64 ENTRANCE   the join confirm (YES: Gc1to1TeleportToStage)    65..69 GUIDE_*   the board with `guildcombat1to1_text<4|5|1|2|3>_eng`
//   70 REPAYMENT     Gc1to1TenderFailed              80 ARENA_ENTER  ArenaEnter                                      81 ARENA_EXIT    ArenaExit

import {
  MMI_ARENA_ENTER,
  MMI_ARENA_EXIT,
  MMI_GUILDCOMBAT_1TO1_CANCEL,
  MMI_GUILDCOMBAT_1TO1_ENTRANCE,
  MMI_GUILDCOMBAT_1TO1_GUIDE_ENTRY,
  MMI_GUILDCOMBAT_1TO1_GUIDE_PRIZE,
  MMI_GUILDCOMBAT_1TO1_GUIDE_RULE,
  MMI_GUILDCOMBAT_1TO1_GUIDE_TEX,
  MMI_GUILDCOMBAT_1TO1_GUIDE_WIN,
  MMI_GUILDCOMBAT_1TO1_OFFER,
  MMI_GUILDCOMBAT_1TO1_OFFERSTATE,
  MMI_GUILDCOMBAT_1TO1_REPAYMENT,
  MMI_GUILDCOMBAT_1TO1_SELECTION,
} from '../../../gen/defs/neuz';

/** The panel of each guide entry (`GuildCombat1to1_TEXT<n>_ENG.inc`, lower-case stems). */
export const GUIDE_PANELS: Readonly<Record<number, string>> = {
  [MMI_GUILDCOMBAT_1TO1_GUIDE_TEX]: 'guildcombat1to1_text4_eng',
  [MMI_GUILDCOMBAT_1TO1_GUIDE_PRIZE]: 'guildcombat1to1_text5_eng',
  [MMI_GUILDCOMBAT_1TO1_GUIDE_ENTRY]: 'guildcombat1to1_text1_eng',
  [MMI_GUILDCOMBAT_1TO1_GUIDE_RULE]: 'guildcombat1to1_text2_eng',
  [MMI_GUILDCOMBAT_1TO1_GUIDE_WIN]: 'guildcombat1to1_text3_eng',
};

/** The ids 60-70 (the entries the event flag gates). */
export const GC1TO1_MENU_IDS = [
  MMI_GUILDCOMBAT_1TO1_OFFER,
  MMI_GUILDCOMBAT_1TO1_CANCEL,
  MMI_GUILDCOMBAT_1TO1_OFFERSTATE,
  MMI_GUILDCOMBAT_1TO1_SELECTION,
  MMI_GUILDCOMBAT_1TO1_ENTRANCE,
  MMI_GUILDCOMBAT_1TO1_GUIDE_TEX,
  MMI_GUILDCOMBAT_1TO1_GUIDE_PRIZE,
  MMI_GUILDCOMBAT_1TO1_GUIDE_ENTRY,
  MMI_GUILDCOMBAT_1TO1_GUIDE_RULE,
  MMI_GUILDCOMBAT_1TO1_GUIDE_WIN,
  MMI_GUILDCOMBAT_1TO1_REPAYMENT,
] as const;

/** What the menu actions call: the module's senders and window openers. */
export interface Gc1to1MenuActions {
  tenderOpenWnd(): void;
  openCancelConfirm(): void;
  tenderView(): void;
  memberLineUpOpenWnd(): void;
  openJoinConfirm(): void;
  openBoard(panel: string): void;
  tenderFailed(): void;
  arenaEnter(): void;
  arenaExit(): void;
}

/** The action of each `MMI_*` id (`registerAction( id, () => table.get( id )() )`). */
export function menuTable(a: Gc1to1MenuActions): ReadonlyMap<number, () => void> {
  const table = new Map<number, () => void>([
    [
      MMI_GUILDCOMBAT_1TO1_OFFER,
      () => {
        a.tenderOpenWnd();
      },
    ],
    [
      MMI_GUILDCOMBAT_1TO1_CANCEL,
      () => {
        a.openCancelConfirm();
      },
    ],
    [
      MMI_GUILDCOMBAT_1TO1_OFFERSTATE,
      () => {
        a.tenderView();
      },
    ],
    [
      MMI_GUILDCOMBAT_1TO1_SELECTION,
      () => {
        a.memberLineUpOpenWnd();
      },
    ],
    [
      MMI_GUILDCOMBAT_1TO1_ENTRANCE,
      () => {
        a.openJoinConfirm();
      },
    ],
    [
      MMI_GUILDCOMBAT_1TO1_REPAYMENT,
      () => {
        a.tenderFailed();
      },
    ],
    [
      MMI_ARENA_ENTER,
      () => {
        a.arenaEnter();
      },
    ],
    [
      MMI_ARENA_EXIT,
      () => {
        a.arenaExit();
      },
    ],
  ]);
  for (const [id, panel] of Object.entries(GUIDE_PANELS)) {
    table.set(Number(id), () => {
      a.openBoard(panel);
    });
  }
  return table;
}
