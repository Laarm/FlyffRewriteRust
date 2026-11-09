// What the inventory dispatch (Plan/45 §9.2 D1-D8, §9.3 the upgrade cursor, §9.5 the router R0-R31; WP-CFEAT-13/14) reads from the
// client and what it does to it. The inventory module builds one `DispatchEnv`; the tests build theirs by hand. Everything the
// dispatch decides on is a pure function of this environment, the item and the dispatcher's own state.

import type { ClientItem } from '../../../data/clientData';
import type { InvItem } from '../items';
import type { InventoryState } from '../state';

/** The sends of the dispatch (each names the legacy `Send*` it replaces). */
export interface DispatchSenders {
  /** `SendDoUseItem(MAKELONG(ITYPE_ITEM, objId), target, part)`. */
  doUseItem(objId: number, target: number, part: number): void;
  /** `SendDoUseItemInput(MAKELONG(ITYPE_ITEM, objId), text)`. */
  doUseItemInput(objId: number, input: string): void;
  /** `SendDoEquip(item, part)` (`part = -1` wears the item). */
  doEquip(objId: number, part: number): void;
  /** `SendEnchant(item, material)`. */
  enchant(itemObjId: number, materialObjId: number): void;
  /** `SendPiercing(item, card)`. */
  piercing(itemObjId: number, cardObjId: number): void;
  /** `SendUltimateEnchantWeapon(weapon, material)`. */
  ultimateEnchantWeapon(weaponObjId: number, materialObjId: number): void;
  /** `SendDoUseItemTarget(material, target)`. */
  doUseItemTarget(materialObjId: number, targetObjId: number): void;
  /** `SendRandomScroll(item, scroll)`. */
  randomScroll(itemObjId: number, scrollObjId: number): void;
  /** `SendReturnScroll(select)`. */
  returnScrollReq(select: number): void;
  /** `SendOpenBankWnd(objId, itemId)`. */
  openBankWnd(objId: number, itemId: number): void;
  /** `SendUsePetFeed(feed)`. */
  usePetFeed(feedObjId: number): void;
  /** `SendCreateMonster(MAKELONG(ITYPE_ITEM, objId), pos)`. */
  createMonster(objId: number, pos: readonly [number, number, number]): void;
}

/** The facts about the own mover the dispatch asks. */
export interface SelfFacts {
  /** `g_pPlayer != NULL`. */
  readonly present: boolean;
  /** `IsDie()`. */
  readonly dead: boolean;
  /** `m_pActMover->IsFly()`. */
  readonly flying: boolean;
  /** `m_pActMover->m_bGround != 0`. */
  readonly onGround: boolean;
  /** `IsMode(TRANSPARENT_MODE)`. */
  readonly transparent: boolean;
  /** `IsBaseJob()`: a vagrant. */
  readonly vagrant: boolean;
  /** `m_nSkillPoint`. */
  readonly skillPoints: number;
  /** `GetCurrentMaxSkillPoint()`, `null` while the rules cannot tell (the skill-point rules are M5). */
  readonly maxSkillPoints: number | null;
  /** `IsSMMode(SM_REVIVAL)`. */
  readonly smRevival: boolean;
  /** The party leader (`g_Party.IsLeader(idPlayer)`). */
  readonly partyLeader: boolean;
  /** `GetWorld()->GetID()`. */
  readonly worldId: number;
  /** `GetWorld()->m_szWorldName`. */
  readonly worldName: string;
}

/** The windows the dispatch opens (`CWndMgr` and the `new CWnd...` of the original). */
export interface DispatchWindows {
  /** `CWndCommItemDlg(tid)` on `APP_COMMITEM_DIALOG`: the generic "use the item?" confirm (Yes runs `yes`). */
  commItemConfirm(tid: number, yes: () => void): void;
  /** `CWndEquipBindConfirm`: the soul-link confirm (Yes runs `yes`). */
  equipBindConfirm(yes: () => void): void;
  /** `CReturnScrollMsgBox` + `CWndSelectVillage`: the confirm, then the choice of the village (`ReturnScrollReq`). */
  returnScroll(onSelect: (select: number) => void): void;
  /** `OpenMessageBoxUpper`. */
  upperBox(tid: number): void;
  /** `CWndMgr::OpenMap(file)`: the picture map window. */
  pictureMap(file: string): void;
  /** `CWndRandomScrollConfirm`: names the target and warns when it already has a random option; Yes sends. */
  randomScrollConfirm(target: InvItem, hasRandomOption: boolean, yes: () => void): void;
  /** `CWndSmeltSafetyConfirm` (WND_ERROR1/2/3): no protection scroll is active; Yes continues, No cancels the cursor. */
  smeltSafetyConfirm(mode: 1 | 2 | 3, yes: () => void, no: () => void): void;
  /** The weapon (object id) of an open `APP_SMELT_JEWEL` window, `null` when none: it cannot be enchanted meanwhile. */
  readonly smeltJewelItem?: (() => number | null) | undefined;
  /** Windows of WP-CFEAT-15 and later: absent = not built, the dispatch refuses with `TID_SBEVE_NOTUSEITEM`. */
  /**
   * `CWndRemoveElem` (`APP_REMOVE_ELEM`): replacing a weapon or suit that carries an element stone asks first. OK runs `proceed` (the use or
   * the equip again without the confirms, `bResult = FALSE`), Cancel does nothing.
   */
  readonly removeElem?:
    ((item: InvItem, target: number, part: number, proceed: () => void) => void) | undefined;
  readonly restateConfirm?: ((item: InvItem, target: number, part: number) => void) | undefined;
  readonly changeAttribute?: ((item: InvItem) => void) | undefined;
  readonly randomOptionRetry?: ((item: InvItem) => void) | undefined;
  readonly changeSex?: ((item: InvItem, target: number) => void) | undefined;
  readonly itemTransy?: ((item: InvItem) => void) | undefined;
  readonly changeName?: ((item: InvItem, mode: 'character' | 'pet') => void) | undefined;
  readonly confirmEnter?:
    ((kind: 'vis' | 'viskey' | 'pickupToBuff' | 'protectAwake', item: InvItem) => void) | undefined;
  readonly funnyCoinConfirm?: ((item: InvItem) => void) | undefined;
  readonly petMiracle?: ((item: InvItem) => void) | undefined;
  /** `CWndPetFoodMill` for the feed-maker item `item` (it names the tool of `MakePetFeed`); `false` = not built. */
  readonly petFoodMill?: ((item: InvItem) => boolean) | undefined;
  readonly petLifeConfirm?: ((item: InvItem) => void) | undefined;
  /** `APP_SMELT_CHANGEWEAPON` is open for a unique weapon (the only case that takes the unique-weapon protection scroll). */
  readonly changeWeaponUnique?: (() => boolean) | undefined;
  /** `g_WndMng.m_pWndWorld->m_wndMenuMover.SetVisible(FALSE)`. */
  readonly hideMoverMenu?: (() => void) | undefined;
  /** The cursor sfx of a pending create-monster item (`XI_CHR_CURSOR1`). */
  readonly cursorSfx?: (() => void) | undefined;
  readonly selectChannel?:
    ((item: InvItem, ticketWorld: number, expandedLayers: number) => void) | undefined;
  readonly beautyCoupon?: ((app: number) => void) | undefined;
  readonly summonParty?: ((item: InvItem) => void) | undefined;
  readonly guildName?: ((item: InvItem) => void) | undefined;
  readonly sealChar?: ((item: InvItem) => void) | undefined;
  /** A buff pet is out (`HasActivatedVisPet`): vis items ask before they are socketed. */
  readonly hasActivatedVisPet?: (() => boolean) | undefined;
  /** The class-change windows (`APP_CHANGECLASS_1/2`, WP-CFEAT-33); absent = not built. */
  readonly changeClass?: (() => void) | undefined;
  /** The window-feeding protocol (D3): the open window `app` takes the item. Absent = the window does not exist. */
  readonly feed?: ((app: number, item: InvItem) => boolean) | undefined;
}

/** The upgrade rules the cursor validates with (the wasm exports `isPierceAble` etc. once they exist; the TS port until then). */
export interface UpgradeRules {
  /** `CItemElem::IsPierceAble(kind3)` of the target. */
  isPierceAble(target: InvItem, targetProp: ClientItem, cardKind3: number): boolean;
  /** `CItemElem::IsEleRefineryAble(prop)`. */
  isEleRefineryAble(prop: ClientItem): boolean;
  /** `CItemElem::IsDiceRefineryAble(prop)`. */
  isDiceRefineryAble(prop: ClientItem): boolean;
}

export interface DispatchEnv {
  state(): InventoryState;
  prop(itemId: number): ClientItem | undefined;
  readonly self: () => SelfFacts;
  /** The id of the focused object, `null` = none or a static object. */
  focusId(): number | null;
  /** `PutString(prj.GetText(tid), NULL, prj.GetTextColor(tid))`. */
  say(tid: number, args?: readonly (string | number)[]): void;
  /** The client clock in ms. */
  now(): number;
  isOpen(app: number): boolean;
  /** `VendorIsVendor`: the own personal shop is open. */
  ownVendorOpen(): boolean;
  /** `HasBuff(type, id)` of the own buffs. */
  hasBuff(type: number, id: number): boolean;
  /** `HasBuffByIk3(ik3)`. */
  hasBuffByIk3(ik3: number): boolean;
  /** `IsUseItemReadyTime`: `false` = the cast time of this item has not been reached. */
  ready(item: InvItem, prop: ClientItem): boolean;
  /** The sfx at the player (`CreateSfx(XI_INT_INCHANT ...)`); returns a handle that `stop` removes. */
  playUpgradeSfx(): (() => void) | null;
  /** `gameplay.enchant_time_s * 1000`. */
  enchantTimeMs(): number;
  /** Is the quiz world current (`WI_WORLD_QUIZ`)? */
  quizWorld(): boolean;
  readonly features: () => {
    readonly better_safe_upgrades: boolean;
    readonly randomoption_retry: boolean;
  };
  readonly rules: UpgradeRules;
  readonly send: DispatchSenders;
  readonly windows: DispatchWindows;
  /** The ticket tables (`CTicketProperty`): the world a ticket opens and its expanded layers; `null` = no such ticket. */
  ticketOf?(itemId: number): { world: number; expandedLayers: number } | null;
  /** The pet in the world of the own mover (`GetPet()`); default: none. */
  hasPet?(): boolean;
}
