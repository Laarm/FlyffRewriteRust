// The guild-house slice of the events store (Plan/45 §28.0, §28.11; WP-CFEAT-46; server half `event/guild_house/`): the house of the own guild, its furniture and the rest point. Reset when the character leaves the world and
// whenever the own guild id changes (a new guild starts without the house of the old one).
//
//   furnitureChannel   `GuildHouseAllInfo.bank_enabled` = event flag 2 (`IsSetFurnitureChannel`): the furniture can be installed on this channel
//   house              the house of the guild with its furniture list (`null` = the guild has none; FIX: the original kept a previous guild's house)
//   restPoint          `GuildHouseRestPoint.rest_point` 0 .. 1 000 000; `restPercent` = whole percent (the rest gauge of the status window)
//   selectedObjId      `GuildHouse->m_dwSelectedObjID`: the furniture of the world that the storage window and the furniture menu act on (`null` = `NULL_ID`)
//   mode               `GuildHouse->m_iMode`: 0 idle, 2 SETUP placement, 4 RESET (reinstall) placement

/** `GUILDHOUSE_PCKTTYPE_*`. */
export const GH_OP = {
  ListUp: 0,
  ListDrop: 1,
  Setup: 2,
  Remove: 3,
  Reset: 4,
  Refresh: 5,
  Upkeep: 6,
  Expired: 7,
} as const;

/** `NULL_ID`. */
export const NULL_OBJ = 0xffff_ffff;

/** `GH_Fntr_Info` of the house (the wire row with the numbers normalised). */
export interface GhRow {
  readonly itemId: number;
  readonly setup: boolean;
  readonly pos: readonly [number, number, number];
  readonly angle: number;
  /** The object id of the installed furniture, `NULL_OBJ` for none. */
  readonly ctrlObjId: number;
  /** Unix seconds when the item expires, 0 = not started. */
  readonly expireAt: number;
}

export interface GhHouse {
  readonly idGuild: number;
  readonly worldId: number;
  /** Unix seconds until the upkeep is paid, 0 = expired. */
  readonly upkeepAt: number;
  readonly rows: readonly GhRow[];
}

export interface GuildHouseSlice {
  readonly furnitureChannel: boolean;
  readonly house: GhHouse | null;
  readonly restPoint: number;
  readonly restPercent: number;
  readonly selectedObjId: number | null;
  readonly mode: 0 | 2 | 4;
}

export function initialGuildHouse(): GuildHouseSlice {
  return {
    furnitureChannel: false,
    house: null,
    restPoint: 0,
    restPercent: 0,
    selectedObjId: null,
    mode: 0,
  };
}
