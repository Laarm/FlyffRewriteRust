// Ports of the item-use dialog shells and of the buff strip (Plan/42 §19.4 D, Plan/45 §9.4, §9.6, §12; WP-UI-28, wired by 45
// WP-CFEAT-13 item windows and WP-CFEAT-30 buffs). Each dialog is opened with ONE request object: the data it shows and the
// callbacks of its buttons. The decisions behind the callbacks (the sends, the texts of the event rules) are 45's; the shells keep
// the control behaviour of the original.

import type { Rect } from '../../layout/geometry';
import type { RichRun } from '../../text/markup';
import type { ViewSource } from '../char/ports';
import type { DialogItem } from '../dialogs/ports';

// ---- APP_EQUIP_BIND_CONFIRM ----------------------------------------------------------------------------------------------------

/** `CWndEquipBindConfirm`: the soul-link confirm of an `IP_FLAG_EQUIP_BIND` item that is about to be worn. */
export interface EquipBindConfirmRequest {
  /** Yes / Enter: the caller sends (`DoUseItem` for the double click, `DoEquip` for the drag). */
  yes(): void;
  no?(): void;
}

// ---- APP_RANDOMSCROLL_CONFIRM / APP_SMELT_SAFETY_CONFIRM ----------------------------------------------------------------------

/** The text and Yes / No windows of the upgrade flows. */
export interface YesNoTextRequest {
  readonly text: string;
  yes(): void;
  no?(): void;
  /** The window went away by any route (Yes, No, the title bar's close): `OnDestroy` of the original. */
  closed?(): void;
}

// ---- APP_QUESTITEM_INFO --------------------------------------------------------------------------------------------------------

/** `CWndQuestItemInfo`: the picture, the name and the description of a quest item. */
export interface QuestItemInfoRequest {
  readonly name: string;
  readonly description: string;
  /** The icon file (`item/itm_x.dds`). */
  readonly icon: string | null;
}

// ---- APP_COMM_SELECTVIL --------------------------------------------------------------------------------------------------------

/** `CWndSelectVillage`: OK sends `ReturnScrollReq(i)` with the index of the checked radio (0 Flaris, 1 Saint Morning, 2 Darkon). */
export interface SelectVillageRequest {
  select(index: number): void;
}

// ---- APP_COMM_ITEM -------------------------------------------------------------------------------------------------------------

/** One row of the duration list: a premium item (a state-mode item or a charged item buff) with its remaining time. */
export interface CommItemRow {
  readonly key: string;
  /** The picture (`item/itm_x.dds`). */
  readonly icon: string | null;
  /** The name in blue (`#cff0000ff`). */
  readonly name: string;
  /** The line below: the remaining time, "usable on condition" or the ticket / pocket text. */
  readonly detail: string;
  /** The tooltip of the row (the item buff tooltip of `OnMouseWndSurface`). */
  readonly tooltip: readonly RichRun[];
}

export interface CommItemView {
  readonly rows: readonly CommItemRow[];
}

export interface CommItemRequest {
  readonly view: ViewSource<CommItemView>;
}

// ---- APP_SUMMON_ANGEL ----------------------------------------------------------------------------------------------------------

/** What the summon window reads about one inventory stack. */
export interface AngelStack {
  readonly objId: number;
  readonly itemId: number;
  /** `m_nItemNum`. */
  readonly count: number;
  /** `GetExtra()`: the units reserved by windows (this one included). */
  readonly extra: number;
  /** The material: `II_GEN_MAT_ORICHALCUM01(_1)` or `II_GEN_MAT_MOONSTONE(_1)`. */
  readonly kind: 'orichalcum' | 'moonstone' | null;
  readonly icon: string | null;
}

export interface SummonAngelRequest {
  /** The stack of an object id, `null` when it is gone. */
  stack(objId: number): AngelStack | null;
  /** `SetExtra(n)`: reserves units of the stack. */
  setExtra(objId: number, extra: number): void;
  /** `g_WndMng.m_pWndWorld->m_bShiftPushed`. */
  shiftDown(): boolean;
  /** The icons of the two dice on the lower left (`prj.GetItemProp(II_GEN_MAT_ORICHALCUM01)->szIcon` and the moonstone's). */
  readonly orichalcumIcon: string | null;
  readonly moonstoneIcon: string | null;
  /** `SendCreateAngel`: the (object id, count) pairs of the used stacks. */
  create(materials: readonly { objId: number; count: number }[]): void;
  /** `OpenMessageBox(TID_GAME_SUMMONANGEL_ERROR1 / ERROR2)`. */
  message(tid: number): void;
  /** The inventory window rect when it is open (the window opens beside it). */
  inventoryRect(): { l: number; t: number; r: number; b: number } | null;
}

// ---- buff icons ----------------------------------------------------------------------------------------------------------------

/** One buff icon as the strip and the status window draw it (`RenderBuffIcon`). */
export interface BuffIcon {
  /** `type:id` of the buff (stable across frames). */
  readonly key: string;
  /** The picture (`icon/skill_x.dds` for skills, `item/itm_x.dds` for items). */
  readonly icon: string | null;
  /** `0..255`: 192 for item buffs, 255 for skills. */
  readonly alpha: number;
  /** `< 20 s` left (the return scroll always flashes). */
  readonly flash: boolean;
  /** An evildoing skill: the picture is multiplied by (255, 120, 255). */
  readonly evil: boolean;
  /** The drawn size: 32 on the world strip, 38 (scale 1.2) in the status window. */
  readonly size: number;
  /** The side of the tooltip rect: 32 on the world strip, 28 in the status window. */
  readonly tipSize: number;
  /** The timer text under the icon, `''` when there is none. */
  readonly timer: string;
  /** `0xAARRGGBB` of the timer text. */
  readonly timerColor: number;
  /** Outlined title font (the status window with `better_buff_display`) or the plain font. */
  readonly timerFont: 'text' | 'title';
  /** The plain seconds of the status window are drawn with a black shadow one pixel right and down. */
  readonly timerShadow: boolean;
  /** Offset of the timer text from the icon's top-left. */
  readonly timerDx: number;
  readonly timerDy: number;
  /** More copies of the picture at a smaller size (the second and third scroll of an experience family). */
  readonly extras?: readonly { readonly dx: number; readonly dy: number; readonly size: number }[];
}

/** A buff icon of the world strip with its place (virtual pixels of the whole UI). */
export interface BuffHudIcon extends BuffIcon {
  readonly x: number;
  readonly y: number;
}

/** The cast bar of a timed item (`RenderCasting`: 200 px wide at the height / 2 of the world, white back, red fill). */
export interface CastBarView {
  /** `0..1`. */
  readonly fraction: number;
}

export interface BuffHudView {
  readonly icons: readonly BuffHudIcon[];
  readonly cast: CastBarView | null;
}

export interface BuffStatusView {
  /** At most 28 icons, in the order of the buff map. */
  readonly icons: readonly BuffIcon[];
}

export interface BuffActions {
  /**
   * The hud strip of a viewport width (`RenderBuff`: the start x, the wrap limit and the gap follow the width). `avoid`: rects in
   * the strip's client space (the open skill-buff window) that the party lane flows around (owner #004); absent = the original layout.
   */
  hud(width: number, avoid?: readonly Rect[]): BuffHudView;
  readonly status: ViewSource<BuffStatusView>;
  /** The tooltip runs of an icon (`OnMouseWndSurface` of the buff windows); `null` = none. */
  tooltip(key: string): readonly RichRun[] | null;
  /** The double click on an icon: return scroll / party summon / pet feed pocket specials; nothing for the rest. */
  activate(key: string): void;
  /** The single click on an icon (`OnLButtonDown`, 43 §4.4): the angel icons redeem the finished buff (`AngelBuff`); nothing for the rest. */
  click?(key: string): void;
  /** Layout mode of the status window: `0` 7 columns, `1` 7 rows (`g_Option.m_nBuffStatusMode`). */
  mode(): 0 | 1;
  setMode(mode: 0 | 1): void;
  /** The version of the hud icons changes (a buff was added, removed or the time text changed): polled by the window. */
  subscribe(fn: () => void): () => void;
}

// ---- the item-specific use windows of WP-CFEAT-15 ------------------------------------------------------------------------------
// `APP_REMOVE_ELEM`, `APP_RESTATE_CONFIRM`, `APP_FUNNYCOIN_CONFIRM` and `APP_CONFIRM_ENTER` take a `YesNoTextRequest` (a text and Yes / No:
// the feature decides what Yes sends). The windows below have more to show.

/** `CWndSelectCh`: the ticket layer list. `count` entries (`expand + 1`), entry `i` is `"<label>\t<i + 1>"`. */
export interface SelectChannelRequest {
  /** `nChCount + 1`: the base layer and the `expand` extra ones. */
  readonly count: number;
  /** `TID_GAME_CHAR_SERVERNAME` ("Channel"). */
  readonly label: string;
  /** The click on an entry: `SendDoUseItemInput(item, "0" ... "-n")`; the window closes. */
  choose(key: string): void;
  /** The window went away by any route (the choice, the close button, a program close). */
  closed?(): void;
}

/** `CWndChangeName` (the character rename scroll) and `CWndChangePetName` (the pet naming scroll): one edit, OK and Cancel. */
export interface ChangeNameRequest {
  /**
   * `IsValidPlayerName`, `IsInvalidName` and `IsAllowedLetter` of the typed (trimmed) name: the text id of the first refusal, `0` when the
   * name is fine. The window shows the refusal in a message box and keeps the edit focused.
   */
  validate(name: string): number;
  /** OK with a valid name: `SendDoUseItemInput(item, name)` (pet) or `SendQuerySetPlayerName` (character, M6); the window closes. */
  confirm(name: string): void;
  /** The message box of a refusal. */
  error(tid: number): void;
  closed?(): void;
}

/** `CWndChangeAttribute`: the element-change scroll with one target slot and five element icons. */
export interface ChangeAttributeRequest {
  /** The scroll (reserved by the feature while the window is open: `SetChangeItem` did `SetExtra(extra + 1)`). */
  readonly scroll: DialogItem;
  /** `ChangeAttribute.inc` (the `help` bundle's panel): the description box. */
  readonly description: string;
  /** The tooltips `TID_TOOLTIP_CA_INSLOT`, `_FIRE`, `_WATER`, `_ELEC`, `_WIND`, `_EARTH` (index 0 = the slot, 1..5 = the elements). */
  readonly tips: readonly string[];
  /** The sprites of the five elements in `icon/icon_monelemantkind.dds` (16 x 16 each): the texture and the cut-out `i` (0 = fire ... 4 = earth). */
  readonly elementIcons: { readonly path: string; cut(i: number): { x: number; y: number } };
  /** The item of an inventory object id, `null` when it is gone. */
  item(objId: number): DialogItem | null;
  /** `IsEleRefineryAble(prop) && m_nResistAbilityOption > 0`: the item can have its element changed. */
  eligible(objId: number): boolean;
  /** `SetExtra(GetExtra() + 1)` (`on`) / `SetExtra(0)` (`off`) of the item. */
  reserve(objId: number, on: boolean): void;
  /** `SendChangeAttribute(item, scroll, attribute 1..5)`. */
  send(itemObjId: number, scrollObjId: number, attribute: number): void;
  /** `PutString(TID_GAME_NOTELEMENT)`: the item has no element to change. */
  refused(): void;
}

/** `CWndRandomOptionRetry`: the awakening augmentation scroll: the item slot and three radio lines. */
export interface RandomOptionRetryRequest {
  /** The scroll (reserved while the window is open: `SetItemId` did `SetExtra(extra + 1)`). */
  readonly scroll: DialogItem;
  /** The item of an inventory object id, `null` when it is gone. */
  item(objId: number): DialogItem | null;
  /**
   * The awakening lines of an item as they are printed on its radio buttons (`"<name> +<n>"`, `"<name> -<n>%"`; `GetRandomOptionSize` > 1
   * and `eAwakening`), `null` when the item cannot be used (the caller says why with `refusedItem`). At most three.
   */
  lines(objId: number): readonly string[] | null;
  /** `SetExtra(GetExtra() + 1)` (`on`) / `SetExtra(0)` (`off`). */
  reserve(objId: number, on: boolean): void;
  /** The item list changed (an `UpdateItem` of the retried item): the radio lines are read again. Returns the unsubscribe. */
  subscribe(listener: () => void): () => void;
  /** Start: `SendRandomOptionRetry(item, scroll, line 1..3)`. */
  send(itemObjId: number, scrollObjId: number, line: number): void;
  /** `OpenMessageBox(TID_MMI_RANDOMOPTIONRETRY_ERRORTEXT03)`. */
  refusedItem(): void;
  /** `PutString(TID_GAME_EQUIPPUT)`: a worn item cannot be put in. */
  worn(objId: number): boolean;
  sayWorn(): void;
  /** The default radio titles `"Awake Option 1"` .. `"Awake Option 3"` of the layout. */
  readonly defaults: readonly string[];
}
