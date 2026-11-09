// Ports of the upgrade windows (Plan/42 §19.4 L, WP-UI-29, WP-UI-30, WP-UI-31; wired by 45 WP-CFEAT-24 / -25 in `features/upgrade/`): the safe
// upgrade window, the element windows (removal, commercial stones), the awakening windows (awakening, the protected choice, blessing and
// level-down cancel, the pet awakening cancel) and the piercing windows. Each window is opened with ONE request object: the data it shows and
// the callbacks of its buttons. The decisions behind the callbacks (the sends, the refusal texts, what an item may do) are the feature's; the
// shells keep the control behaviour of the originals (`src:_Interface/WndNewUpgrade.cpp`, `WndField.cpp`, `WndAwakening.cpp`, ...).
//
// Rules shared by every item window (45 §18.1): an item can only be dropped from the inventory (anything else gets the forbid cursor), the
// window RESERVES the item it holds (`SetExtra + 1`, the stack is dimmed and cannot be dragged) and releases it when the window closes or the
// slot is emptied, and a window that stays open after a send blocks a second send until the answer arrived (`pending`).

import type { DialogItem } from '../dialogs/ports';
import type { ViewSource } from '../char/ports';

/** The part every single-slot item window needs of the client. */
export interface SlotRequest {
  /** The item of an inventory object id, `null` when it is gone (or is not in the bag). */
  item(objId: number): DialogItem | null;
  /** `SetExtra(GetExtra() + 1)` (`on`) / `SetExtra(0)` (`off`) of the item. */
  reserve(objId: number, on: boolean): void;
  /**
   * May this item be put into the slot? The request says why not (a message box or a chat line of the original) and answers `false`. A
   * worn item or one the inventory does not hold is refused before this is asked.
   */
  accept(objId: number): boolean;
  /** The window went away by any route (the button, the title bar's close, a program close). */
  closed?(): void;
}

// ---- APP_REMOVE_ATTRIBUTE / APP_REMOVE_ATTRIBUTE_CONFIRM -----------------------------------------------------------------------

/**
 * `CWndRemoveAttribute` (272 x 256): a weapon or suit that carries an element in the slot, Start asks `APP_REMOVE_ATTRIBUTE_CONFIRM`; Yes sends
 * `RemoveAttributeReq{item}` (the window waits for `RemoveAttributeNtf`, which closes it) and the confirm blocks a second send meanwhile.
 */
export interface RemoveAttributeRequest extends SlotRequest {
  /** `RemoveAttribute.inc` (the `help` bundle's panel). */
  readonly description: string;
  /** `TID_GAME_REMOVE_ATTRIBUTE`: the text of the confirm window. */
  readonly confirmText: string;
  /** A removal was sent and the answer has not arrived: the confirm window ignores Yes. */
  pending(): boolean;
  /** Yes of the confirm: `SendRemoveAttribute(item)`. */
  send(objId: number): void;
}

// ---- APP_COMMERCIAL_ELEM --------------------------------------------------------------------------------------------------------

/** `CWndCommercialElem` (176 x 176): the weapon or suit (`WIDC_CUSTOM1`) and the element stone (`WIDC_CUSTOM2`). */
export interface CommercialElemRequest {
  item(objId: number): DialogItem | null;
  reserve(objId: number, on: boolean): void;
  /** The first slot: a refinable item with an element and no stone applied (`IsRestrictionItem` and `IsUpgradeItem`, the chat lines included). */
  acceptItem(objId: number): boolean;
  /** The second slot, with the first one filled: a stone that fits the item (`IsSMItem`). */
  acceptStone(targetObjId: number, objId: number): boolean;
  /** OK with both slots filled: `SendCommercialElem(item, stone)`. */
  send(targetObjId: number, stoneObjId: number): void;
  closed?(): void;
}

// ---- APP_AWAKENING / APP_CANCEL_BLESSING / APP_LVREQDOWN / APP_PET_AWAK_CANCEL -----------------------------------------------

/**
 * The four "item, description, Start" windows: `CWndAwakening` (an item that awakens and has no lines yet; 100 000 penya at the NPC),
 * `CWndBlessingCancel` (a blessed item with lines), `CWndLvReqDown` (an item whose required level is lowered) and `CWndPetAwakCancel` (a pick-up
 * pet). Start sends the request with the item and closes the window.
 */
export interface StartItemRequest extends SlotRequest {
  /** The `help` panel of the window (`ItemAwakening.inc`, `ItemBlessingCancel.inc`, `ReqLvDown.inc`, `PetAwakCancel.inc`). */
  readonly description: string;
  /** Start: `SendAwakening` / `SendBlessednessCancel` / `SendRemoveItemLevelDown` / `SendPickupPetAwakeningCancel`. */
  send(objId: number): void;
}

// ---- APP_AWAKE_SELECTCASE -------------------------------------------------------------------------------------------------------

/** `CWndSelectAwakeCase` (400 x 256, modal): the old and the new option set of a protected awakening and a gauge of the 60 s to choose. */
export interface SelectAwakeRequest {
  /** The awakened item (its icon is drawn at the left). */
  readonly item: DialogItem;
  /** The lines of the current option set (`"\n%s %c%d"` as the original prints them, one per line). */
  readonly oldLines: readonly string[];
  /** The lines of the offered set. */
  readonly newLines: readonly string[];
  /** `AWAKE_KEEP_TIME` (60 s): the time to choose; over it the old set is kept. */
  readonly durationMs: number;
  /** The client clock in ms. */
  now(): number;
  /** One of the two buttons: `SendSelectedAwakeningValue(item, serial, _AWAKE_OLD_VALUE (1) | _AWAKE_NEW_VALUE (2))` and the chat line "selected". */
  choose(flag: 1 | 2): void;
  /** The time ran out: the old set is sent and the chat line "time is up" printed. */
  timeout(): void;
  closed?(): void;
}

// ---- APP_PIERCING --------------------------------------------------------------------------------------------------------------

/**
 * `CWndPiercing` (192 x 256): the item (`WIDC_STATIC5`), the moonstone (`WIDC_STATIC6`) and the socket protection scroll (`WIDC_STATIC7`). OK with
 * the first two asks `TID_PIERCING_ERROR_NOTICE` (OK / Cancel); OK sends `PiercingSize{item, moonstone, protection | NULL_ID}` and closes the window.
 */
export interface PiercingRequest {
  item(objId: number): DialogItem | null;
  reserve(objId: number, on: boolean): void;
  /** What a drop on slot `slot` (0 item, 1 moonstone, 2 protection) may do; the request prints the refusal. `target` is the item of slot 0. */
  accept(slot: 0 | 1 | 2, objId: number, target: DialogItem | null): boolean;
  /** The cost shown under the slots (`100000` with `__EXT_PIERCING`). */
  cost(target: DialogItem | null): number;
  /** OK with the item and the moonstone in place: the OK / Cancel box `TID_PIERCING_ERROR_NOTICE`; `yes` runs when the player confirms. */
  ask(yes: () => void): void;
  /** The confirmed OK: `SendPiercingSize(item, moonstone, protection)`. */
  send(target: number, moonstone: number, protection: number | null): void;
  /** The enchant effect on the player while the window is open (`CreateSfx(XI_INT_INCHANT)`); returns what removes it. */
  sfx(): (() => void) | null;
  closed?(): void;
}

// ---- APP_SMELT_REMOVE_PIERCING / APP_SMELT_REMOVE_PIERCING_EX ---------------------------------------------------------------

/** What the removal window shows of the item in its slot: the sockets and the text of the card in each. */
export interface PiercingView {
  /** The sockets the item may use (`4` for a suit, else `10`). */
  readonly max: number;
  /** The option text of the card in socket `i` (`"Attack+5  HP+10  "`), `''` for an empty socket. */
  readonly lines: readonly string[];
}

/**
 * `CWndRemovePiercing` (272 x 208, `__CSC_VER12_4`: `APP_SMELT_REMOVE_PIERCING_EX` 272 x 480 with the ten socket lines): a pierced item in the
 * slot, Start sends `PiercingRemove{item}` and closes the window (the LAST card is removed).
 */
export interface RemovePiercingRequest extends SlotRequest {
  readonly description: string;
  /** `true` for the extended window (`APP_SMELT_REMOVE_PIERCING_EX`). */
  readonly extended: boolean;
  /** The sockets of the item in the slot. */
  view(objId: number): PiercingView;
  /** `TID_GAME_TOOLTIP_PIERCINGITEM`: the tooltip of the slot (the extended window). */
  readonly slotTip: string;
  /** Start: `SendPiercingRemove(item)`. */
  send(objId: number): void;
}

// ---- APP_TEST (the upgrade base) -------------------------------------------------------------------------------------------------

/** One of the six slots of `CWndUpgradeBase`: 0 the weapon or armour, 1 the card or dice, 2 / 4 the first supplement stone, 3 / 5 the second. */
export type UpgradeBaseSlot = 0 | 1 | 2 | 3 | 4 | 5;

/** The statics of `CWndUpgradeBase` that follow the slots: the cost, the two stone counts and the two stone names. */
export interface UpgradeBaseInfo {
  /** `WIDC_STATIC5`. */
  readonly cost: string;
  /** `WIDC_STATIC6` and `WIDC_STATIC4`. */
  readonly count0: string;
  readonly count1: string;
  /** `WIDC_STATIC10` and `WIDC_STATIC11`. */
  readonly name0: string;
  readonly name1: string;
}

/**
 * `CWndUpgradeBase` (240 x 280, applet id `APP_TEST`): the old supplement-stone upgrade. Slot 0 takes the weapon or armour, slot 1 the card or dice,
 * slots 2/4 and 3/5 the two supplement stones (two stacks each). The shell keeps the slot book-keeping; the rules (what fits where, the stones a
 * card needs, the costs) are the request's. The window is hidden in the port (`MMI_UPGRADE` dropped, 45 OQ6) and nothing opens it.
 */
export interface UpgradeBaseRequest {
  item(objId: number): DialogItem | null;
  /** `SetExtra(n)`: reserves `n` units of the stack (`0` releases). */
  setExtra(objId: number, extra: number): void;
  /** May `objId` go into `slot`? `items` are the objects in the six slots. The units to reserve (`SetExtra( n )`), `0` = refused (the request prints why). */
  accept(slot: UpgradeBaseSlot, objId: number, items: readonly (number | null)[]): number;
  /** The statics for the items in the slots. */
  info(items: readonly (number | null)[]): UpgradeBaseInfo;
  /** OK with the item and the card in place: `SendUpgradeBase(...)`; `false` = refused (the request says why), the window stays. */
  send(items: readonly (number | null)[], extras: readonly number[]): boolean;
  closed?(): void;
}

// ---- APP_SMELT_SAFETY (the safe upgrade window) ----------------------------------------------------------------------------------

/** `EUpgradeSetting` of `CWndNewUpgrade`. */
export type SmeltSetting = 'none' | 'normal' | 'ultimate' | 'jewel' | 'element' | 'piercing';

/** One material slot: the icon and the units left (the number under the icon). */
export interface SmeltSlotView {
  readonly icon: string | null;
  /** The units under the icon (`0` draws none). */
  readonly count: number;
  /** A translucent hint of what to drop here (alpha 75/255 of `DrawListItem`), not an item. */
  readonly ghost?: boolean;
}

/** The slots a click (not while running) empties. */
export type SmeltSlot = 'target' | 'material' | 'protection' | 'scroll';

/** What the safe-upgrade window shows, read every frame (`CWndNewUpgrade::OnDraw`, `CWndSmeltSafety::OnDraw`). */
export interface SmeltSafetyView {
  /** The title of the window: the mode name or the tries line `"Tries: N\tSuccess: S   Failure: F"`. */
  readonly title: string;
  /** `WIDC_TITLE_NOW_GRADE` ("Upgrade", "General Upgrading", "Piercing" ...). */
  readonly nowCaption: string;
  /** `WIDC_TITLE_MAX_GRADE` ("Maximum"). */
  readonly maxCaption: string;
  /** `WIDC_NOW_GRADE`: the level, element level or sockets of the target (`'--'` without a target). */
  readonly nowValue: string;
  /** `WIDC_EDIT_MAX_GRADE`: the level to stop at. */
  readonly maxValue: string;
  readonly target: DialogItem | null;
  readonly material: SmeltSlotView | null;
  readonly protection: SmeltSlotView | null;
  readonly scroll: SmeltSlotView | null;
  /** The upgrade runs: the enchant glow on the target, the gauge fills. */
  readonly running: boolean;
  /** The fill of the wait gauge `0..1` (only drawn while `running`). */
  readonly gauge: number;
  /** The result image: `0` none, `1` success, `2` failure. */
  readonly result: 0 | 1 | 2;
  /** The enable states of `Start`, `Stop`, `Reset` and the two max buttons. */
  readonly canStart: boolean;
  readonly canStop: boolean;
  readonly canReset: boolean;
  readonly canAdjust: boolean;
  /** The tooltip over the empty target slot (`TID_GAME_TOOLTIP_SMELT_SAFETY_ITEM`, the one-shot window), `null` = none. */
  readonly emptyTip: string | null;
}

export interface SmeltSafetyActions {
  /** A drop from the inventory (or a double click fed by the dispatch): `SetItem(item)`. */
  drop(objId: number): void;
  /** A click on a slot (`OnLButtonUp`). */
  clear(slot: SmeltSlot): void;
  start(): void;
  stop(): void;
  reset(): void;
  /** The `+` and `-` of the max level. */
  adjust(delta: 1 | -1): void;
  /** The window closed: every reservation is released. */
  closed(): void;
}

export interface SmeltSafetyRequest {
  readonly view: ViewSource<SmeltSafetyView>;
  readonly actions: SmeltSafetyActions;
}
