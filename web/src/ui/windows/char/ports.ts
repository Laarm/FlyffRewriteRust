// Ports of the character core windows (Plan/42 §4.5, §19.4 D, WP-UI-21; implemented by 45 WP-CFEAT-09 status, -10 inventory,
// -11 character, -51 motions). The READ MODELS (`*View`, read through a `ViewSource` and polled by the window's `process`
// at 10 Hz: the shell diffs the object it gets, so a store that returns the same object while nothing changed costs
// nothing) and the ACTIONS the shells call. Plain TS interfaces without protobuf types; `ui/` never imports `features/`.
// Every action names the legacy send function behind it (ref/99 App. B); the decision behind a call (gates, confirms, the
// message itself) is the feature's.

import type { Shortcut } from '../../dnd/shortcut';
import type { ItemView } from '../../tooltip/types';
import type { HonorPorts } from '../couple/ports';
import type { ViewSource } from '../login/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';

/** The cooldown of an item group: absolute `host.now()` ms of start and end. */
export interface ItemCooldown {
  readonly startMs: number;
  readonly endMs: number;
}

// ---- 3-D previews (41 §21.1 `PreviewRenderer`) ---------------------------------------------------------------------------------

/** What a shell asks of the preview renderer for the paper doll and the head portrait. */
export interface CorePreviewRequest {
  readonly canvas: HTMLCanvasElement;
  /** `inventory`: the paper doll of `WIDC_CUSTOM20`; `statusPortrait`: the head of `WIDC_CUSTOM6` of the status window. */
  readonly preset: 'inventory' | 'statusPortrait';
  /** The opaque subject (`MoverVisualState` of 41); the shell never looks inside. */
  readonly subject: unknown;
}

export interface CorePreviewHandle {
  /** `m_fRot` of the paper doll (degrees, dragged with the pointer). */
  setRotation?(degrees: number): void;
  invalidate(): void;
  close(): void;
}

/** `host.previews`: `null` when no renderer exists (the canvas stays empty). */
export interface CorePreviewPort {
  open(request: CorePreviewRequest): CorePreviewHandle | null;
}

// ---- inventory (APP_INVENTORY, ref/22 §17 D, 45 §9.1) ----------------------------------------------------------------------------

/** One item of the bag or of the equipment. The tooltip composers of 45 read the rest of their own record through `ItemView`. */
export interface InventoryItem extends ItemView {
  /** `m_dwObjId`: what crosses the wire and what the shortcut carries. */
  readonly objId: number;
  /** `m_dwItemId`. */
  readonly itemId: number;
  /** The icon file (`item/itm_x.dds`); `null` = the prop has none. */
  readonly icon: string | null;
  /** `m_nItemNum`. */
  readonly count: number;
  /** `dwPackMax > 1`: the stack count is drawn. */
  readonly stackable: boolean;
  /** `GetExtra()`: the part of the stack reserved by a trade, vendor or upgrade window; > 0 dims the item and forbids dragging it. */
  readonly extra: number;
  /** `IsUsingItem` (an item that is being used by a timed effect): dimmed. */
  readonly using: boolean;
  /** `IsFlag(expired)` or an item the player cannot use: drawn with (255, 100, 100). */
  readonly expired: boolean;
  /** The cooldown group of the item. */
  readonly cooldown: ItemCooldown | null;
  /** `dwHanded == HD_TWO`: a two-handed right weapon is drawn ghosted in the left-hand slot (alpha 100). */
  readonly twoHanded: boolean;
  /** `dwParts != NULL_ID`: the item can be worn. */
  readonly wearable: boolean;
}

export interface InventoryView {
  /** The bag by slot index (`null` = empty slot); its length is the container size (168, or 42 without `inventory_168`). */
  readonly bag: readonly (InventoryItem | null)[];
  /** The worn items by `PARTS_*` index (length `MAX_HUMAN_PARTS`). */
  readonly equip: readonly (InventoryItem | null)[];
  /** `GetGold()`. */
  readonly gold: number;
  /** `m_abHideCoat` of HAT, CLOTH, GLOVE, BOOTS, CLOAK. */
  readonly hideCoat: readonly [boolean, boolean, boolean, boolean, boolean];
  /** `g_Option.m_bViewMask`. */
  readonly viewMask: boolean;
  /** `features.inventory_sorting`: the sort button exists. */
  readonly sorting: boolean;
  /** `features.hide_fashion`: the five hide boxes and the mask box exist. */
  readonly hideFashion: boolean;
  /** `features.new_font_color`: the colour of stack counts. */
  readonly newFontColor: boolean;
  /** Bumped when the equipment changed: the item tooltips are cached per `(serial, version)`. */
  readonly version: number;
  /** `m_bIsUpgradeMode`: the pointer is the hammer over the window (the cursor itself is 43's). */
  readonly upgradeMode: boolean;
  /** The subject of the paper doll (`MoverVisualState`, opaque). */
  readonly subject?: unknown;
}

export interface InventoryActions {
  /** `WIN_DBLCLK` of the bag grid (`nID == 11`): the double-click dispatch of ref/99 §4.2.1 (45 §9.2). */
  useItem(item: InventoryItem, slot: number): void;
  /**
   * `OnLButtonDown` on an occupied bag slot (`CWndItemCtrl`, ref/99 §4.2.2): the dialogs that block item actions refuse it (a text
   * and `false`), an armed upgrade cursor completes with the pressed item. `false` = no selection, no drag.
   */
  press?(item: InventoryItem, slot: number): boolean;
  /** `OnRButtonDown` on the bag grid: the pending upgrade cursor is cancelled. */
  cancelCursor?(): void;
  /** `OnLButtonDblClk` on a worn item: `SendDoEquip(item, part)` (45 §9.1). */
  unequip(item: InventoryItem, part: number): void;
  /** A worn item dragged onto the bag: `SendDoUseItem(MAKELONG(ITYPE_ITEM, objId), 0, part)` (45 §9.4 first row). */
  unequipByDrag(objId: number, part: number): void;
  /** A bag item dropped onto the equipment area: `SendDoEquip(item)` (the equip-bind confirm is 45's). */
  equip(item: InventoryItem): void;
  /** Bag slot onto bag slot (`CWndItemCtrl::OnDropIcon`): `SendMoveItem` (45 §9.4). `to` is `-1` when the pointer is over no slot. */
  moveItem(item: InventoryItem, from: number, to: number): void;
  /**
   * A shortcut from ANOTHER window dropped onto the bag grid or the window (`WIN_ITEMDROP`, ref/99 §4.2.3): shop buy,
   * vendor, bank, pocket, mail, guild bank. `fromApp` is the APP id of the source frame. Returns whether it was accepted
   * (otherwise the forbid icon shows).
   */
  dropIn(sc: Shortcut, fromApp: number): boolean;
  /** A bag shortcut dropped on the trash slot (`WIDC_CUSTOM21`): the gates and `APP_INVEN_REMOVE_ITEM` (45 §9.4). */
  trash(sc: Shortcut): boolean;
  /** `SendHdr(PACKETTYPE_SORT_INVENTORY)`. */
  sort(): void;
  /** `SendHideCoat(index, hide)` with `index` 0..4 = HAT, CLOTH, GLOVE, BOOTS, CLOAK. */
  hideCoat(index: number, hide: boolean): void;
  /** `SendOptionEnableRenderMask(on)`. */
  viewMask(on: boolean): void;
  /** `PutString(prj.GetText(tid), NULL, prj.GetTextColor(tid))`: the refusals of the shell (trade limit, repair). */
  message?(tid: number): void;
}

// ---- status (APP_STATUS1, 45 §7.2) ----------------------------------------------------------------------------------------------

export interface StatusView {
  /** The title bar text (`TID_GAME_WND_STATUS_PLAYER_INFORMATION` with the name and the level: `Hero - Lv: 15`). */
  readonly title: string;
  readonly hp: number;
  readonly hpMax: number;
  readonly mp: number;
  readonly mpMax: number;
  readonly fp: number;
  readonly fpMax: number;
  /** `exp * 10000 / maxExp` (integer, `rules.expPercentX100`): the EXP gauge and its percent text. */
  readonly expPercentX100: number;
  /** The guild-house rest bonus in px of the AEXP gauge (`GuildHouse->m_nExtraExp`, 0 = none). */
  readonly restPoint: number;
  /** `features.remove_bt_exp = false`: the percent text of the rest bonus is drawn (the gauge always is). */
  readonly restText: boolean;
  /** `features.improved_status`: the HP/MP/FP numbers exist; `false` shows only the EXP percent. */
  readonly improved: boolean;
  /** `features.exp_stop`: the EXP stop button exists. */
  readonly expStopFeature: boolean;
  /** `selfStore.modeFlags & EXPUP_STOP`: the button shows its pressed frame. */
  readonly expStopped: boolean;
  /** The subject of the head portrait (`MoverVisualState`, opaque). */
  readonly subject?: unknown;
}

export interface StatusActions {
  /** `SendExpStop` (toggle). */
  expStop(): void;
}

// ---- character (APP_CHARACTER3, 45 §20) -----------------------------------------------------------------------------------------

/** The values of the "Detail" block (the numbers the original prints from `g_pPlayer->Get*`). */
export interface DerivedStats {
  /** `(min + max) / 2` with the attack bonuses of `RenderATK`. */
  readonly atk: number;
  readonly def: number;
  /** `GetCriticalProb()` in percent. */
  readonly critical: number;
  /** `int(GetAttackSpeed() * 100) / 2` in percent. */
  readonly attackSpeed: number;
  // `features.statistics`
  readonly speedRun?: number;
  readonly criticalRate?: number;
  readonly spellCastSpeed?: number;
  readonly pvmDmg?: number;
  readonly hitRate?: number;
  readonly blockMelee?: number;
  readonly blockRange?: number;
  readonly pvpDmg?: number;
}

/** The pending stat allocation (`m_nStrCount` ...). */
export interface StatDelta {
  readonly str: number;
  readonly sta: number;
  readonly dex: number;
  readonly int: number;
}

export interface StatPair {
  /** `GetStr()` (with buffs and equipment). */
  readonly total: number;
  /** `m_nStr` (the base): the colour is black when equal, blue when `total` is higher, red when lower. */
  readonly base: number;
}

export interface CharacterView {
  readonly name: string;
  /** `GetJobString()`. */
  readonly job: string;
  readonly level: number;
  /** `IsBaseJob()` (a vagrant): the stat confirm adds `TID_DIAG_0082`. */
  readonly baseJob: boolean;
  /** `GetLegendChar()`: 0 none, 1 master (the mark icon of the level range), 2 hero. */
  readonly legend: 0 | 1 | 2;
  /** `g_dpCertified.GetServerName(...)` and the channel name. */
  readonly server: string;
  readonly channel: string;
  readonly str: StatPair;
  readonly sta: StatPair;
  readonly dex: StatPair;
  readonly int: StatPair;
  /** `GetRemainGP()`. */
  readonly remainGp: number;
  readonly derived: DerivedStats;
  /** The derived values with a pending allocation applied (`rules.statPreview`); `null` = same as `derived`. */
  preview(pending: StatDelta): DerivedStats | null;
  /** `features.statistics`: the eight extra lines and the taller layout (`WndNewCharacter02.tga`). */
  readonly statistics: boolean;
  /** `GetFameName()` (`''` prints `-`), `m_nFame`, `GetPKValue()`, `GetPKPropensity()`. */
  readonly fameName: string;
  readonly fame: number;
  readonly pkValue: number;
  readonly pkPropensity: number;
}

export interface CharacterActions {
  /** `SendModifyStatus(str, sta, dex, int)` after the confirm window's Yes (45 §20.2). */
  modifyStatus(delta: StatDelta): void;
}

// ---- motion (APP_MOTION, 45 §19.4) ----------------------------------------------------------------------------------------------

export interface MotionEntry {
  /** `MotionProp::dwID` (`MOT_BASE_*`). */
  readonly id: number;
  /** `Icon/<szIconName>`. */
  readonly icon: string;
  /** `szDesc` (the tooltip text). */
  readonly description: string;
}

export interface EmoticonEntry {
  /** The `.inc` of the sprite sheet (`Theme/texChatEmoticon.inc`) and the sprite index. */
  readonly atlas: string;
  readonly sprite: number;
  /** `m_EmiticonCmd[i].m_szCommand`: the tooltip is `"/" + command`. */
  readonly command: string;
}

export interface MotionView {
  readonly motions: readonly MotionEntry[];
  readonly emoticons: readonly EmoticonEntry[];
  /** The tooltip of a motion; the cheer motion adds the live cheer points and wait time (`CWndMotion1::Process`). */
  tooltip(entry: MotionEntry): string;
}

export interface MotionActions {
  /** `ObjectExecutor(SHORTCUT_MOTION, id)` = `motions.execute(id)` (single click, 45 §19.4). */
  execute(id: number): void;
  /** `ObjectExecutor(SHORTCUT_EMOTICON, index)` = `chat.sendEmoticon(index)`. */
  sendEmoticon(index: number): void;
}

// ---- stat confirm (APP_STATE_CONFIRM) -------------------------------------------------------------------------------------------

/** What the stat-point confirm window needs; the character window builds it when Apply is pressed. */
export interface StateConfirmRequest {
  /** The window text (`TID_GAME_CHARSTATUS_APPLY_Q` plus, for a vagrant, `TID_DIAG_0082`). */
  readonly text: string;
  /** Yes (Enter): `SendModifyStatus` and clear the pending values. */
  yes(): void;
  /** No, the close button and Esc: the waiting flag is cleared. */
  no(): void;
}

// ---- the bundle of ports ------------------------------------------------------------------------------------------------------

export interface CoreWindowPorts {
  inventory: { view: ViewSource<InventoryView>; actions: InventoryActions };
  status: { view: ViewSource<StatusView>; actions: StatusActions };
  character: { view: ViewSource<CharacterView>; actions: CharacterActions };
  motion: { view: ViewSource<MotionView>; actions: MotionActions };
  previews?: CorePreviewPort | null;
  /** The honor tab of the character window (`HonorPage`, WP-UI-52, wired by 45 WP-CFEAT-40); without it the tab lists no title and sends nothing. */
  honor?: HonorPorts;
}
