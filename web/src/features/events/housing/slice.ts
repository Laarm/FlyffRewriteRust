// The housing slice of the events store (Plan/45 §28.0, §28.10; WP-CFEAT-46): the personal mini room. Reset when the character leaves the world (the join batch of the next login starts empty: the server sends
// `HousingAllInfo` only for a room with an item or a visitor).
//
//   items       the furniture list of the own room in the order received (`CHousing::m_vecFurnitureList`)
//   visitors    the friends allowed to visit (`m_setVisitAllow`)
//   visitable   the rooms the player may visit (`m_vecVisitable`, cleared when the room list opens)
//   papering    the wallpaper and the carpet installed in the room, as item ids (the textures of the room)
//   myRoom      `CDeployManager::IsMyRoom`: a client guess as in the original (the wire has no layer): set by the visit request, cleared on a `Replace` into any world but the mini room

/** `WI_WORLD_MINIROOM`. */
export const WORLD_MINIROOM = 209;
/** `NULL_ID`: `HousingPaperingInfo{ NULL_ID, false }` resets both papers. */
export const NULL_ITEM = 0xffff_ffff;

/** `HOUSINGINFO` of the list. */
export interface HousingRow {
  readonly itemId: number;
  /** Unix seconds when the item expires. */
  readonly expireAt: number;
  /** Installed in the room. */
  readonly setup: boolean;
  readonly pos: readonly [number, number, number];
  readonly angle: number;
}

export interface HousingSlice {
  readonly items: readonly HousingRow[];
  readonly visitors: ReadonlySet<number>;
  readonly visitable: readonly number[];
  readonly papering: { readonly wall: number | null; readonly floor: number | null };
  readonly myRoom: boolean;
}

export function initialHousing(): HousingSlice {
  return {
    items: [],
    visitors: new Set(),
    visitable: [],
    papering: { wall: null, floor: null },
    myRoom: false,
  };
}
