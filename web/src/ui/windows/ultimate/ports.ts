// Ports of the ultimate-weapon windows (Plan/42 §19.4 L, WP-UI-32, WP-UI-33 jewel removal; wired by the `ultimate` feature of 45, WP-CFEAT-26):
// the sunstone / moonstone mixing window and its confirmation, the gem extraction window, the jewel setting window, the weapon change
// window (general -> unique, unique -> ultimate), the jewel removal window and its confirmation. Each window is opened with ONE request
// object: the facts it reads about the bag items and the callbacks of its buttons. The decisions behind the callbacks (the sends, the texts of
// the results, the item tables) are the feature's; the shells keep the control behaviour of the original (`src:_Interface/WndField.cpp`,
// `CWndMixJewel` ... `CWndRemoveJewel`).

import type { DialogItem } from '../dialogs/ports';

/** What the item id of a bag item means to the ultimate windows. */
export type UltimateRole =
  /** `II_GEN_MAT_ORICHALCUM01` / `_1`: the five light units of the mix. */
  | 'orichalcum'
  /** `II_GEN_MAT_MOONSTONE` / `_1`: the five dark units of the mix and the price of a jewel removal. */
  | 'moonstone'
  /** `II_GEN_MAT_ORICHALCUM02`, the shining Orichalcum. */
  | 'shining'
  /** One of the five jewels (diamond, emerald, sapphire, ruby, topaz). */
  | 'jewel';

/** `dwReferStat1` of a weapon: `WEAPON_GENERAL`, `WEAPON_UNIQUE`, `WEAPON_ULTIMATE`. */
export type WeaponTier = 'general' | 'unique' | 'ultimate' | 'other';

/** What the windows read of a weapon. */
export interface WeaponFacts {
  readonly tier: WeaponTier;
  /** `dwLimitLevel1`. */
  readonly level: number;
  /** `GetAbilityOption()`: the +N. */
  readonly ao: number;
  /** `IK2_WEAPON_DIRECT` or `IK2_WEAPON_MAGIC`: the item kinds an ultimate weapon has. */
  readonly directOrMagic: boolean;
  /** `GetUltimatePiercingSize()`: the sockets of the weapon. */
  readonly sockets: number;
  /** `GetUltimatePiercingItem( i )`: the ability item of each socket, `0` = empty (`sockets` entries). */
  readonly gems: readonly number[];
}

/** A bag item as the ultimate windows see it (the feature builds it from the inventory and the item table). */
export interface UltimateItem {
  /** The tooltip view, the icon and the object id. */
  readonly view: DialogItem;
  readonly objId: number;
  readonly itemId: number;
  /** `m_nItemNum`. */
  readonly num: number;
  /** `GetExtra()`: the units a window, a trade or a stall reserved. */
  readonly extra: number;
  readonly role: UltimateRole | null;
  /** `IK2_MATERIAL`. */
  readonly material: boolean;
  /** The weapon facts, `null` for anything that is not `IK1_WEAPON`. */
  readonly weapon: WeaponFacts | null;
}

/** The members every request of this file shares. */
export interface UltimateBase {
  /** The bag record of an object id with its CURRENT reservation; `null` when the bag does not have it. */
  item(objId: number): UltimateItem | null;
  /** `SetExtra( n )`: reserves `n` units of the stack for the window (dims it, forbids dragging it). */
  setExtra(objId: number, extra: number): void;
  /** `OpenMessageBox( prj.GetText( tid ) )`. */
  messageBox(tid: number): void;
  /** `PutString( prj.GetText( tid ) )`: a system line in the chat, with the text arguments of the format. */
  say(tid: number, args?: readonly (string | number)[]): void;
  /** The window was destroyed. */
  closed?(): void;
}

// ---- APP_SMELT_MIXJEWEL ------------------------------------------------------------------------------------------------------------

/** The mixing window: five Orichalcum and five moonstones become one shining Orichalcum (`UltimateMakeItem`). */
export interface MixJewelRequest extends UltimateBase {
  /** `SmeltMixJewel.inc`: the description in the text box. */
  readonly description: string;
  /** The icons of the faint placeholders (`II_GEN_MAT_ORICHALCUM01`, `II_GEN_MAT_MOONSTONE`) and of the confirmation (`II_GEN_MAT_ORICHALCUM02`). */
  readonly icons: {
    readonly orichalcum: string | null;
    readonly moonstone: string | null;
    readonly shining: string | null;
  };
  /** The Shift key (`m_bShiftPushed`): a drop fills every free slot of its five with the stack. */
  shiftDown(): boolean;
  /** Start: `SendUltimateMakeItem( objids[10] )`, the first five are Orichalcum stacks, the last five moonstone stacks. */
  make(objIds: readonly number[]): void;
}

// ---- APP_SMELT_EXTRACTION ----------------------------------------------------------------------------------------------------------

/** The extraction window: one general or unique weapon is destroyed for jewels (`UltimateMakeGem`). */
export interface ExtractionRequest extends UltimateBase {
  /** `SmeltExtraction.inc`. */
  readonly description: string;
  /** The name of the jewel a weapon of this level makes (`GetGemKind( limit_level )`), `''` when none. */
  gemName(level: number): string;
  /** Start: `SendUltimateMakeGem( weapon )`. */
  make(weaponObjId: number): void;
}

// ---- APP_SMELT_JEWEL ---------------------------------------------------------------------------------------------------------------

/** The jewel setting window: one jewel into the first free socket of an ultimate weapon (`UltimateSetGem`). */
export interface SmeltJewelRequest extends UltimateBase {
  /** `SmeltJewel.inc`. */
  readonly description: string;
  /** The picture of an item id: the ability item of a socketed jewel or the jewel in the free socket (`item/itm_x.dds`), `null` when unknown. */
  iconOf(itemId: number): string | null;
  /** Start: after the turn of the weapon, `SendUltimateSetGem( weapon, jewel )`. */
  set(weaponObjId: number, jewelObjId: number): void;
  /** The Start button was pressed (`PLAYSND( "PcSkillD-Counter01.wav" )`, T30). */
  started?(): void;
}

// ---- APP_SMELT_CHANGEWEAPON --------------------------------------------------------------------------------------------------------

/** The weapon change window of `type`: a general weapon becomes unique, a unique one ultimate (`UltimateTransWeapon`). */
export interface ChangeWeaponRequest extends UltimateBase {
  readonly type: 'general' | 'unique';
  /** `SmeltChangeUniqueWeapon.inc` / `SmeltChangeUltimateWeapon.inc` (the left text) and `ChangeUniqueWeaponInfo.inc` / `ChangeUltimateWeaponInfo.inc`. */
  readonly description: string;
  readonly info: string;
  /** `GetGemKind( limit_level )`: the item id of the jewel a weapon of this level needs, `null` when none. */
  gemKind(level: number): number | null;
  /** Start: `SendUltimateTransWeapon( weapon, jewel, shining )`. */
  transform(weaponObjId: number, jewelObjId: number, shiningObjId: number): void;
}

// ---- APP_SMELT_REMOVE_JEWEL / APP_SMELT_REMOVE_CONFIRM -----------------------------------------------------------------------------

/** What the remove window prints for the jewel in one socket: the text and its colour (`0xAARRGGBB`), `null` = nothing. */
export interface JewelLabel {
  readonly text: string;
  readonly color: number;
}

/** The jewel removal window: the last jewel of an ultimate weapon goes for a moonstone (`UltimateRemoveGem`). */
export interface RemoveJewelRequest extends UltimateBase {
  /** `SmeltRemoveJewel.inc`. */
  readonly description: string;
  /** The icon of the faint moonstone in its slot. */
  readonly moonstoneIcon: string | null;
  /** The icon of a socketed jewel (`item/itm_x.dds`), `null` when unknown. */
  jewelIcon(abilityItemId: number): string | null;
  /**
   * The line of a socketed jewel: its attribute name in the added-option colour of that attribute. `features.improved_ulti_gems` hides the
   * five old attributes (hit points ... PvP damage): a jewel that gives one is listed without a text.
   */
  jewelLabel(abilityItemId: number): JewelLabel | null;
  /**
   * Start asks `TID_GAME_REMOVEJEWEL` first (`APP_SMELT_REMOVE_CONFIRM`); Yes sends `SendUltimateRemoveGem( weapon, moonstone )`.
   * The confirmation is Plan/45 §18.6's: the original's window sends at once.
   */
  remove(weaponObjId: number, moonstoneObjId: number): void;
  /** The text of the confirmation (`TID_GAME_REMOVEJEWEL`). */
  readonly confirmText: string;
}
