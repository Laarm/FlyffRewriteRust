// Pure row, sort and time-text helpers of the housing and guild-house windows (Plan/42 §19.4 N, 45 §28.10, §28.11; WP-UI-58, WP-UI-59). Not lazy: the windows, the features and the tests all read them.
// The originals (`_Interface/WndHousing.cpp`) keep these as comparators and `OnDraw` snippets; here they are functions of plain data, so the tables of the tests are the goldens.

import {
  IK2_GUILDHOUSE_FURNITURE,
  IK2_GUILDHOUSE_NPC,
  IK2_GUILDHOUSE_PAPERING,
  IK3_BED,
  IK3_CABINET,
  IK3_CARPET,
  IK3_CASE,
  IK3_CHAIR,
  IK3_CLOSET,
  IK3_DESK,
  IK3_PROPS,
  IK3_SOFA,
  IK3_TABLE,
  IK3_TELEPORTER,
  IK3_WALLPAPER,
  IK3_WARDROBE,
} from '../../../gen/defs/itemkind';
import {
  TID_GAME_GUILDHOUSE_FURKIND,
  TID_GAME_HOUSING_BED,
  TID_GAME_HOUSING_CABINET,
  TID_GAME_HOUSING_CARPET,
  TID_GAME_HOUSING_CLOSET,
  TID_GAME_HOUSING_PROPS,
  TID_GAME_HOUSING_SOFA,
  TID_GAME_HOUSING_TABLE,
  TID_GAME_HOUSING_WALLPAPER,
  TID_GAME_HOUSING_WARDROBE,
  TID_GAME_REMAIN_TIME,
  TID_GAME_TIME_1HOUR,
  TID_GAME_TOOLTIP_NEWFURNITURE,
  TID_MMI_GHOUSE_BED,
  TID_MMI_GHOUSE_CARPET,
  TID_MMI_GHOUSE_CASE,
  TID_MMI_GHOUSE_CHAIR,
  TID_MMI_GHOUSE_CLOSET,
  TID_MMI_GHOUSE_DESK,
  TID_MMI_GHOUSE_SOFA,
  TID_MMI_GHOUSE_TABLE,
  TID_MMI_GHOUSE_WALLPAPER,
  TID_MMI_GHOUSE_WARDROBE,
  TID_GAME_GUILDHOUSE_EXPIRATION,
  TID_PK_LIMIT_DAY,
  TID_PK_LIMIT_HOUR,
  TID_PK_LIMIT_MINUTE,
  TID_PK_LIMIT_SECOND,
} from '../../../gen/defs/text';

export { IK2_GUILDHOUSE_FURNITURE, IK2_GUILDHOUSE_NPC, IK2_GUILDHOUSE_PAPERING };
export { IK3_CARPET, IK3_TELEPORTER, IK3_WALLPAPER };

/** The instance key of the upkeep window (`APP_CONFIRM_BUYEX` is the shop's quantity box too). */
export const UPKEEP_KEY = 'ghUpkeep';
/** The rows of the housing list that fit (`nDrawCount >= 10`). */
export const HOUSING_PAGE_ROWS = 10;
/** `GH_MAX_VIEW_CAPACITY`: the rows of the guild storage that fit, and the row buttons. */
export const GH_MAX_VIEW_CAPACITY = 7;
/** The pitch of a storage row (`nListFontHeight = 40`). */
export const GH_ROW_PITCH = 40;
/** `NULL_ID`: the object id of a furniture that is not installed. */
export const NULL_OBJ_ID = 0xffff_ffff;
/** `PF_GUILDHOUSE_FURNITURE` and `PF_GUILDHOUSE_UPKEEP` of the guild power words (`guild.h:44-45`). */
export const PF_GUILDHOUSE_FURNITURE = 0x20;
export const PF_GUILDHOUSE_UPKEEP = 0x40;
/** `MAX_UPKEEP_DAY - 1`: the most days the upkeep window offers; `UPKEEP_DAY_PENYA_SMALL` per day. */
export const MAX_UPKEEP_DAY_CLIENT = 34;
export const UPKEEP_DAY_PENYA = 2_400_000;
/** The colours of the lists (`D3DCOLOR_ARGB`). */
export const COLOR_BLACK = 0xff000000;
export const COLOR_SELECTED_BLUE = 0xff0000ff;
export const COLOR_ROOM_SELECT = 0xffff0000;
/** `RenderFillRect( ..., 0x55ff0000 )`: the selected storage row and the blinking SHOWONE bar. */
export const FILL_RED = 'rgba(255, 0, 0, 0.333)';
/** `tmOld` of `CWndGHouseShowOneUnit::OnDraw`: the bar toggles every 600 ms. */
export const SHOWONE_BLINK_MS = 600;

/** A text of the bundle by id. */
export type TextOf = (tid: number) => string;

// ---- the housing list (APP_HOUSING) ---------------------------------------------------------------------------------------------

/** One furniture or paper of the own mini room as the window shows it (`HOUSING_ITEM`). */
export interface HousingItemView {
  readonly itemId: number;
  readonly name: string;
  /** `szCommand`: the text of the information box. */
  readonly description: string;
  /** `dwItemKind3`. */
  readonly kind3: number;
  /** Unix seconds when the item expires. */
  readonly expireAt: number;
  readonly installed: boolean;
}

/** The type texts of `OnDraw` by `IK3_*` (a kind outside the nine draws nothing). */
export function housingTypeTid(kind3: number): number | null {
  switch (kind3) {
    case IK3_BED:
      return TID_GAME_HOUSING_BED;
    case IK3_SOFA:
      return TID_GAME_HOUSING_SOFA;
    case IK3_WARDROBE:
      return TID_GAME_HOUSING_WARDROBE;
    case IK3_CLOSET:
      return TID_GAME_HOUSING_CLOSET;
    case IK3_TABLE:
      return TID_GAME_HOUSING_TABLE;
    case IK3_CABINET:
      return TID_GAME_HOUSING_CABINET;
    case IK3_PROPS:
      return TID_GAME_HOUSING_PROPS;
    case IK3_WALLPAPER:
      return TID_GAME_HOUSING_WALLPAPER;
    case IK3_CARPET:
      return TID_GAME_HOUSING_CARPET;
    default:
      return null;
  }
}

/** `CTimeSpan( t )` of a whole number of seconds >= 0. */
export function timeSpan(seconds: number): {
  days: number;
  hours: number;
  minutes: number;
  seconds: number;
} {
  const t = Math.max(0, Math.trunc(seconds));
  return {
    days: Math.floor(t / 86400),
    hours: Math.floor(t / 3600) % 24,
    minutes: Math.floor(t / 60) % 60,
    seconds: t % 60,
  };
}

/** `printf` with the conversions `%s %d %u %.1d` of the legacy templates, in order. */
export function fmt(template: string, args: readonly (string | number)[]): string {
  let i = 0;
  return template.replace(/%(?:%|[-+ #0]*\d*(?:\.\d+)?(?:I64|ll|l|h)?[sducfxXi])/g, (m) => {
    if (m === '%%') return '%';
    const a = args[i];
    i += 1;
    return a === undefined ? '' : String(a);
  });
}

/**
 * The time column of the housing list: `t = expireAt - now` clamped at 0; less than one hour (days and hours both zero) reads `TID_GAME_TIME_1HOUR`, else `TID_GAME_REMAIN_TIME( days, hours )`.
 */
export function housingTimeText(expireAt: number, now: number, text: TextOf): string {
  const ts = timeSpan(expireAt - now);
  if (ts.days <= 0 && ts.hours <= 0) return fmt(text(TID_GAME_TIME_1HOUR), []);
  return fmt(text(TID_GAME_REMAIN_TIME), [ts.days, ts.hours]);
}

/** The columns a header click sorts by (`WIDC_STATIC2..5`). */
export type HousingColumn = 'type' | 'name' | 'time' | 'installed';

/** `strcmp` order of two names. */
export function compareNames(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/**
 * The comparator key of a column in the "greater" direction: type descending, name ascending, time descending, installed first (`CompType_Greater` ... `CompDeploy_Greater`). Negative = `a` first.
 */
function greaterOrder(column: HousingColumn, a: HousingItemView, b: HousingItemView): number {
  switch (column) {
    case 'type':
      return b.kind3 - a.kind3;
    case 'name':
      return compareNames(a.name, b.name);
    case 'time':
      return b.expireAt - a.expireAt;
    case 'installed':
      return Number(b.installed) - Number(a.installed);
  }
}

/** The rows in display order: a STABLE sort (FIX: `std::sort` ordered equal rows arbitrarily); `greater = false` is the exact reverse comparison, equal rows keep the list order. */
export function sortHousing(
  rows: readonly HousingItemView[],
  column: HousingColumn,
  greater: boolean,
): HousingItemView[] {
  if (rows.length < 2) return [...rows];
  return rows
    .map((row, index) => ({ row, index }))
    .sort((x, y) => {
      const c = greaterOrder(column, x.row, y.row) * (greater ? 1 : -1);
      return c !== 0 ? c : x.index - y.index;
    })
    .map((e) => e.row);
}

/** A header click: the same column flips the direction, a new column starts "greater". */
export function nextSortState<C>(
  cur: { column: C | null; greater: boolean },
  column: C,
): { column: C; greater: boolean } {
  if (cur.column === column) return { column, greater: !cur.greater };
  return { column, greater: true };
}

// ---- the guild storage (APP_GH_FURNITURE_STORAGE) --------------------------------------------------------------------------------

/** The two sections of the storage window (`GH_SECTION`). */
export type GhSection = 'furniture' | 'teleporter';

/** One furniture of the guild house as the storage window shows it (`HOUSING_ITEM` built by `RefreshItemList`). */
export interface GhRowView {
  /** The index in the furniture list of the house (`m_nSlotIndex`). */
  readonly slot: number;
  readonly itemId: number;
  readonly name: string;
  readonly description: string;
  readonly icon: string;
  /** `dwItemKind2` / `dwItemKind3`. */
  readonly kind2: number;
  readonly kind3: number;
  readonly installed: boolean;
  /** Unix seconds when the item expires, 0 = not started (`tKeepTime`). */
  readonly expireAt: number;
  /** `dwAbilityMin`: the duration in minutes of an item that was never installed. */
  readonly abilityMin: number;
  /** The object id of the installed furniture, `NULL_OBJ_ID` for none. */
  readonly ctrlObjId: number;
  readonly pos: readonly [number, number, number];
  readonly angle: number;
}

/** The combo of the furniture section: the text id and the kind it filters (0 = all). */
export const GH_COMBO_FURNITURE: readonly { readonly tid: number; readonly kind3: number }[] = [
  { tid: TID_GAME_GUILDHOUSE_FURKIND, kind3: 0 },
  { tid: TID_MMI_GHOUSE_BED, kind3: IK3_BED },
  { tid: TID_MMI_GHOUSE_TABLE, kind3: IK3_TABLE },
  { tid: TID_MMI_GHOUSE_SOFA, kind3: IK3_SOFA },
  { tid: TID_MMI_GHOUSE_DESK, kind3: IK3_DESK },
  { tid: TID_MMI_GHOUSE_CHAIR, kind3: IK3_CHAIR },
  { tid: TID_MMI_GHOUSE_CASE, kind3: IK3_CASE },
  { tid: TID_MMI_GHOUSE_CLOSET, kind3: IK3_CLOSET },
  { tid: TID_MMI_GHOUSE_WARDROBE, kind3: IK3_WARDROBE },
  { tid: TID_MMI_GHOUSE_CARPET, kind3: IK3_CARPET },
  { tid: TID_MMI_GHOUSE_WALLPAPER, kind3: IK3_WALLPAPER },
];

/** The only entry of the combo in the teleporter section (a literal in the original). */
export const GH_COMBO_TELEPORTER = 'TELEPORTER';

/**
 * The rows of a section, filtered by the combo kind (`m_dwComboCurrIK3`, 0 = all): the furniture section skips `IK2_GUILDHOUSE_NPC`, the teleporter section skips furniture and papering; the `slot` of each row stays
 * the index in the house list.
 */
export function filterGhRows(
  rows: readonly GhRowView[],
  section: GhSection,
  comboKind3: number,
): GhRowView[] {
  return rows.filter((r) => {
    if (comboKind3 !== 0 && r.kind3 !== comboKind3) return false;
    if (section === 'furniture') return r.kind2 !== IK2_GUILDHOUSE_NPC;
    return r.kind2 !== IK2_GUILDHOUSE_FURNITURE && r.kind2 !== IK2_GUILDHOUSE_PAPERING;
  });
}

/** The columns of the storage headers (`WIDC_STATIC1..3`). */
export type GhColumn = 'name' | 'time' | 'installed';

/** The seconds a storage row shows: the remaining time of an installed item, else the duration of the prop (`abilityMin * 60`), 0 for an expiry in the past. */
export function ghRowSeconds(row: GhRowView, now: number): { seconds: number; started: boolean } {
  if (row.expireAt <= 0) return { seconds: row.abilityMin * 60, started: false };
  return { seconds: Math.max(0, row.expireAt - now), started: true };
}

/** The sort time of a row (`m_dwTime`: `tKeepTime`, or `ability_min * 60` while it never ran). */
export function ghSortTime(row: GhRowView): number {
  return row.expireAt > 0 ? row.expireAt : row.abilityMin * 60;
}

/** The stable sort of the storage list (same comparators as the housing list). */
export function sortGhRows(
  rows: readonly GhRowView[],
  column: GhColumn,
  greater: boolean,
): GhRowView[] {
  if (rows.length < 2) return [...rows];
  return rows
    .map((row, index) => ({ row, index }))
    .sort((x, y) => {
      let c: number;
      if (column === 'name') c = compareNames(x.row.name, y.row.name);
      else if (column === 'time') c = ghSortTime(y.row) - ghSortTime(x.row);
      else c = Number(y.row.installed) - Number(x.row.installed);
      c *= greater ? 1 : -1;
      return c !== 0 ? c : x.index - y.index;
    })
    .map((e) => e.row);
}

/** The time column of a storage row: `TID_PK_LIMIT_DAY | _HOUR | _MINUTE` of the largest nonzero unit, then `(NEW)` while the item never ran. */
export function ghTimeText(row: GhRowView, now: number, text: TextOf): string {
  const { seconds, started } = ghRowSeconds(row, now);
  const ts = timeSpan(seconds);
  let out: string;
  if (ts.days > 0) out = fmt(text(TID_PK_LIMIT_DAY), [ts.days]);
  else if (ts.hours > 0) out = fmt(text(TID_PK_LIMIT_HOUR), [ts.hours]);
  else out = fmt(text(TID_PK_LIMIT_MINUTE), [ts.minutes]);
  return started ? out : out + text(TID_GAME_TOOLTIP_NEWFURNITURE);
}

/** The line of the SHOWONE window: the same time, then ` (NEW)` (never ran) or ` (▶)` (the time runs). */
export function showOneTimeText(row: GhRowView, now: number, text: TextOf): string {
  const { seconds, started } = ghRowSeconds(row, now);
  const ts = timeSpan(seconds);
  let out: string;
  if (ts.days > 0) out = fmt(text(TID_PK_LIMIT_DAY), [ts.days]);
  else if (ts.hours > 0) out = fmt(text(TID_PK_LIMIT_HOUR), [ts.hours]);
  else out = fmt(text(TID_PK_LIMIT_MINUTE), [ts.minutes]);
  return out + (started ? ' (▶)' : ' (NEW)');
}

/** The upkeep countdown of the main menu and the storage window: `TID_GAME_GUILDHOUSE_EXPIRATION` once it is over, else the nonzero parts joined by blanks. */
export function upkeepCountdownText(upkeepAt: number, now: number, text: TextOf): string {
  const left = upkeepAt - now;
  if (left < 0) return text(TID_GAME_GUILDHOUSE_EXPIRATION);
  const ts = timeSpan(left);
  const parts: string[] = [];
  if (ts.days > 0) parts.push(fmt(text(TID_PK_LIMIT_DAY), [ts.days]));
  if (ts.hours > 0) parts.push(fmt(text(TID_PK_LIMIT_HOUR), [ts.hours]));
  if (ts.minutes > 0) parts.push(fmt(text(TID_PK_LIMIT_MINUTE), [ts.minutes]));
  if (ts.seconds > 0) parts.push(fmt(text(TID_PK_LIMIT_SECOND), [ts.seconds]));
  return parts.join(' ');
}

/** The row button of the storage list: `Buttlockerinstall.bmp` + the dismantle tooltip for an installed row, else `Buttlockerdismantle.bmp` + the install tooltip (KEEP the original's cross-over). */
export const GH_BUTTON_INSTALLED = 'Buttlockerinstall.bmp';
export const GH_BUTTON_FREE = 'Buttlockerdismantle.bmp';

/** The rows of the room list: `"%d. %s"` over the owners whose name is known (an unknown one has no row). */
export function roomListRows(
  owners: readonly number[],
  nameOf: (id: number) => string | null,
): { id: number; text: string }[] {
  const out: { id: number; text: string }[] = [];
  for (const id of owners) {
    const name = nameOf(id);
    if (name === null || name === '') continue;
    out.push({ id, text: `${String(out.length + 1)}. ${name}` });
  }
  return out;
}

/** The days of the upkeep edit: a number in `[1, 34]` (FIX: the original stopped updating above 34 and could send 0). */
export function clampUpkeepDays(n: number): number {
  if (!Number.isFinite(n)) return 1;
  return Math.min(MAX_UPKEEP_DAY_CLIENT, Math.max(1, Math.trunc(n)));
}

/** The cost of `days` of upkeep (`days * UPKEEP_DAY_PENYA_SMALL`). */
export function upkeepCost(days: number): number {
  return clampUpkeepDays(days) * UPKEEP_DAY_PENYA;
}
