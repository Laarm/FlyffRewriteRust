// The guild-house entries of the NPC menu (Plan/45 §11.2, §28.11; `CWndWorld::OnCommand`, `src:_Interface/WndWorld.cpp:6286-6338`): ids 147-150 of `MMI_*`; 146 `MMI_TELEPORTER` stays the navigator's (WP-CFEAT-28).
//
//   147 MMI_GUILDHOUSE_SALE     `SendBuyGuildHouse`
//   148 MMI_GUILDHOUSE_CHARGE   no `PF_GUILDHOUSE_UPKEEP` power: chat `TID_GAME_GUILDHOUSE_TAX_LEVEL`; else the upkeep window (an open one is replaced: `SAFE_DELETE`, then new)
//   149 MMI_GUILDHOUSE_ENTER    `SendGuildHouseEnter`
//   150 MMI_GUILDHOUSE_OUT      `SendGuildHouseGoOut` (the door NPC inside: its only entry)

import {
  MMI_GUILDHOUSE_CHARGE,
  MMI_GUILDHOUSE_ENTER,
  MMI_GUILDHOUSE_OUT,
  MMI_GUILDHOUSE_SALE,
} from '../../../gen/defs/neuz';

export interface GuildHouseMenuActions {
  buy(): void;
  /** The power check and the upkeep window (replaces an open one). */
  charge(): void;
  enter(): void;
  goOut(): void;
}

export function guildHouseMenuTable(a: GuildHouseMenuActions): ReadonlyMap<number, () => void> {
  return new Map<number, () => void>([
    [
      MMI_GUILDHOUSE_SALE,
      () => {
        a.buy();
      },
    ],
    [
      MMI_GUILDHOUSE_CHARGE,
      () => {
        a.charge();
      },
    ],
    [
      MMI_GUILDHOUSE_ENTER,
      () => {
        a.enter();
      },
    ],
    [
      MMI_GUILDHOUSE_OUT,
      () => {
        a.goOut();
      },
    ],
  ]);
}
