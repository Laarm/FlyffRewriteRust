// The siege entries of the NPC menu (Plan/45 §11.2, §28.4; `CWndWorld::OnCommand`, `src:_Interface/WndWorld.cpp:4124-4350`): ids 26-41 of `MMI_*`. The popup lists whatever the NPC offers
// (`MaFl_GuildWar`, Madrigal): no client gate and no config key (flag 15 is not read by the client, 45 §28.0).
//
//   26 MMI_GUILDWAR_APP          InGuildCombat{gcInWindow}          27 MMI_GUILDWAR_STATE         RequestStatus
//   28 MMI_GUILDWAR_CANCEL       the cancel confirm                  29 MMI_GUILDWAR_JOIN          the join confirm
//   30 MMI_GUILDCOMBAT_RANKING   PlayerPointGuildCombat              33 MMI_GUILDCOMBAT_SELECTION  SelectPlayerGuildCombat{window}
//   34 MMI_GUILDCOMBAT_JACKPOT   GetPenyaGuildGuildCombat, ONLY for the guild master (else silent)
//   36 MMI_GUILDCOMBAT_BESTPLAYER GetPenyaPlayerGuildCombat
//   35 JACKPOT2, 37 INFO_BOARD1, 38 INFO_BOARD2, 39 INFO_BOARD3, 40 INFO_TEX      the board with the panel `GuildCombatTEXT_<5|1|2|3|6>_ENG`
//   41 MMI_GUILDCOMBAT_PENYA_RETURN                                  nothing (the legacy handler is empty)

import {
  MMI_GUILDCOMBAT_BESTPLAYER,
  MMI_GUILDCOMBAT_INFO_BOARD1,
  MMI_GUILDCOMBAT_INFO_BOARD2,
  MMI_GUILDCOMBAT_INFO_BOARD3,
  MMI_GUILDCOMBAT_INFO_TEX,
  MMI_GUILDCOMBAT_JACKPOT,
  MMI_GUILDCOMBAT_JACKPOT2,
  MMI_GUILDCOMBAT_PENYA_RETURN,
  MMI_GUILDCOMBAT_RANKING,
  MMI_GUILDCOMBAT_SELECTION,
  MMI_GUILDWAR_APP,
  MMI_GUILDWAR_CANCEL,
  MMI_GUILDWAR_JOIN,
  MMI_GUILDWAR_STATE,
} from '../../../gen/defs/neuz';

/** The panel of the help bundle of each board entry (`GuildCombatTEXT_<n>_ENG.inc`, lower-case stems; panel 4 is unused). */
export const BOARD_PANELS: Readonly<Record<number, string>> = {
  [MMI_GUILDCOMBAT_INFO_BOARD1]: 'guildcombattext_1_eng',
  [MMI_GUILDCOMBAT_INFO_BOARD2]: 'guildcombattext_2_eng',
  [MMI_GUILDCOMBAT_INFO_BOARD3]: 'guildcombattext_3_eng',
  [MMI_GUILDCOMBAT_JACKPOT2]: 'guildcombattext_5_eng',
  [MMI_GUILDCOMBAT_INFO_TEX]: 'guildcombattext_6_eng',
};

/** What the menu actions call: the module's senders and window openers. */
export interface SiegeMenuActions {
  window(): void;
  requestStatus(): void;
  openCancelConfirm(): void;
  openJoinConfirm(): void;
  playerPoint(): void;
  selectPlayerWindow(): void;
  /** `pGuild->IsMaster( g_pPlayer->m_idPlayer )` decides inside. */
  getPenyaGuild(): void;
  getPenyaPlayer(): void;
  openBoard(panel: string): void;
}

/** The action of each `MMI_*` id (`registerAction( id, () => actions[ id ]() )`). */
export function menuTable(a: SiegeMenuActions): ReadonlyMap<number, () => void> {
  const table = new Map<number, () => void>([
    [
      MMI_GUILDWAR_APP,
      () => {
        a.window();
      },
    ],
    [
      MMI_GUILDWAR_STATE,
      () => {
        a.requestStatus();
      },
    ],
    [
      MMI_GUILDWAR_CANCEL,
      () => {
        a.openCancelConfirm();
      },
    ],
    [
      MMI_GUILDWAR_JOIN,
      () => {
        a.openJoinConfirm();
      },
    ],
    [
      MMI_GUILDCOMBAT_RANKING,
      () => {
        a.playerPoint();
      },
    ],
    [
      MMI_GUILDCOMBAT_SELECTION,
      () => {
        a.selectPlayerWindow();
      },
    ],
    [
      MMI_GUILDCOMBAT_JACKPOT,
      () => {
        a.getPenyaGuild();
      },
    ],
    [
      MMI_GUILDCOMBAT_BESTPLAYER,
      () => {
        a.getPenyaPlayer();
      },
    ],
    // the empty handler of the original
    [MMI_GUILDCOMBAT_PENYA_RETURN, () => undefined],
  ]);
  for (const [id, panel] of Object.entries(BOARD_PANELS)) {
    table.set(Number(id), () => {
      a.openBoard(panel);
    });
  }
  return table;
}
