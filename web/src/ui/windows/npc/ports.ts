// Ports of the NPC dialog and shop window shells (Plan/42 §4.5, WP-UI-24): the READ MODELS (`*View`, observed through a
// `ViewSource`) and the ACTIONS the shells call. Plain TS interfaces without protobuf types; the implementations are the
// feature controllers of 45 (WP-CFEAT-18 `features/npc/dialog`, WP-CFEAT-19 `features/npc/shop`), the shells are tested with
// mock ports only. Each action names the legacy send it covers (ref/99 App. B).

import type { RichRun } from '../../text/markup';
import type { ItemView } from '../../tooltip/types';
import type { PreviewPort } from '../login/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';

// ---- the dialog (`CWndDialog`, APP_DIALOG_EX; Plan/45 §11.3) -----------------------------------------------------------

/** One key button, answer or quest-list entry the server offered (`WORDBUTTON`): sent back exactly as offered. */
export interface DialogButton {
  readonly word: string;
  readonly key: string;
  readonly param: number;
  readonly questId: number;
}

/** A quest-list entry: the button plus the formatted list text and the icon rule inputs. */
export interface DialogQuestEntry extends DialogButton {
  /** `"[#b<min>#nb~#b<max>#nb ] #b<title>#nb"` (markup), or the bare word when the quest is unknown. */
  readonly text: string;
  /** `false` for `QUEST_NEXT_LEVEL` (red icon, not clickable). */
  readonly valid: boolean;
}

/**
 * One page of the dialog text (`m_strArray[i]`), already parsed (`CWndDialog::ParsingString`): the display runs (a link
 * word is bold), a mark per character (1 first / 2 middle / 3 last / 4 single-character link word, 0 plain) and the
 * `word -> key` map (`m_mapWordToOriginal`).
 */
export interface DialogPage {
  readonly runs: readonly RichRun[];
  readonly marks: readonly number[];
  readonly links: ReadonlyMap<string, string>;
  /** `m_dwQuest` of the page (the `Say` that created it). */
  readonly questId: number;
}

/** What the dialog window shows (`dialogStore`, Plan/45 §2.3). */
export interface DialogView {
  /** Bumps with every change of the state. */
  readonly version: number;
  /** The NPC object id; the window closes when it disappears (the feature does that). */
  readonly npcId: number;
  /** The portrait file (`char/char_x.tga`) or `null` when the NPC has none. */
  readonly portrait: string | null;
  /** The text of the group box above the text area (`WIDC_GROUP_BOX_TITLE`): the clicked key word. */
  readonly title: string;
  /** `m_bSay`: the page text is drawn (a quest-list key click clears it). */
  readonly say: boolean;
  /** `m_nSelectKeyButton` (purple), -1 for none. */
  readonly selectedKey: number;
  /** The key buttons (at most 32). */
  readonly keys: readonly DialogButton[];
  /** `m_aWordButton`: the answers of the response (image buttons and text links), at most 32. */
  readonly answers: readonly DialogButton[];
  /** `m_strArray.GetSize()` and `m_nCurArray`. */
  readonly pageCount: number;
  readonly page: number;
  /** `m_string`: the page that is shown (it stays after a click until the next `say`, the answer links are gone). */
  readonly shown: DialogPage | null;
  readonly newQuests: readonly DialogQuestEntry[];
  readonly currentQuests: readonly DialogQuestEntry[];
  /** Quests whose end condition holds (`__IsEndQuestCondition`): the yellow icon. */
  readonly completable: ReadonlySet<number>;
}

/** A click on something the server offered. */
export type DialogClick =
  | { readonly kind: 'key'; readonly index: number }
  | { readonly kind: 'answer'; readonly index: number }
  /** A `[Word][Key]` link of the text: sent as `(key, 0, 0)`. */
  | { readonly kind: 'link'; readonly key: string }
  | { readonly kind: 'quest'; readonly list: 'new' | 'current'; readonly index: number };

export interface DialogActions {
  /**
   * A click on a key button, a link, an answer or a quest-list entry. Returns `false` when it is ignored (less than one
   * second after the previous request, `m_timer`); `true` plays the click sound. Clears pages, answers and links and sends
   * `ScriptDlg` (`legacy-send: SendScriptDialogReq`).
   */
  click(click: DialogClick): boolean;
  /** The Next button (`WIDC_BUTTON1`): the next page. */
  next(): void;
  /** The window was destroyed (Esc, `exit`, the NPC left): the dialog state is dropped. */
  closed(): void;
}

// ---- the shop (`CWndShop`, APP_SHOPEX; Plan/45 §12) ---------------------------------------------------------------------

/** One stock entry of a shop tab (`CItemElem` of `m_ShopInventory[tab]`). */
export interface ShopCell extends ItemView {
  /** The storage id inside the tab container (the `obj_id` of `BuyItem`). */
  readonly objId: number;
  readonly itemId: number;
  readonly icon: string | null;
  /** The stack the stock holds (the most that one purchase can take). */
  readonly count: number;
  /** Red tint: the player cannot use the item (sex, job, level, flight level) or, in a chip shop, cannot pay it. */
  readonly tint: boolean;
  /** `dwItemSex` vs the player's sex: `true` match, `false` mismatch, `null` no restriction (the sex sort box). */
  readonly sexMatch: boolean | null;
  /** The level requirement is met (`!IsLimitLevel`): `null` without a requirement (the level sort box). */
  readonly levelOk: boolean | null;
  /** The price of one piece shown in `WIDC_COST` (penya with the tax, or red chips). */
  readonly price: number;
  /** The record of the stock item for the tooltip composers (`InvItem` of 45). */
  readonly item?: unknown;
}

export interface ShopTabView {
  /** `m_venderSlot[i]`: the tab title. A tab without a title is not created. */
  readonly title: string;
  /** The index of the container (what `BuyItem.tab` carries). */
  readonly index: number;
  readonly cells: readonly ShopCell[];
}

export interface ShopView {
  readonly version: number;
  readonly npcId: number;
  /** `m_nVenderType`: 0 penya shop, 1 Red Chip shop. */
  readonly vendorType: 0 | 1;
  readonly tabs: readonly ShopTabView[];
  /** `features.quick_sell`: the Quick Sell button. */
  readonly quickSell: boolean;
  readonly newFontColor: boolean;
}

/** A shop stock item picked for a purchase. */
export interface ShopPick {
  readonly tab: number;
  readonly cell: ShopCell;
}

export interface ShopActions {
  /**
   * A stock item was dragged to the inventory grid or double-clicked: the feature opens `APP_CONFIRM_BUYEX` (the quantity
   * box lives there, never in the shell) after the gates (`TID_DIAG_0048` not enough penya, the chip gate).
   */
  buy(pick: ShopPick): void;
  /**
   * An inventory item was dropped on the shop (`WIN_ITEMDROP` of the tab grids): equipped and quest items are refused, a
   * charged item opens `APP_WARNING` first, then `APP_CONFIRM_SELL`. Returns whether the drop was accepted.
   */
  sell(objId: number): boolean;
  /** `WIDC_BUTTON_QUICK_SELL`: opens `APP_QUICK_SELL`. */
  quickSell(): void;
  /** The tooltip numbers context of a stock item (`PutToolTip_Item(.., APP_SHOP_)`): nothing but the flag. */
  hovered?(cell: ShopCell | null): void;
  /** The window was destroyed (close button, Esc, death, teleport): `CloseShopWnd` is sent. */
  closed(): void;
}

// ---- the buy / sell confirmations --------------------------------------------------------------------------------------

export interface ConfirmBuyRequest {
  readonly name: string;
  readonly icon: string | null;
  /** The pieces of the stock entry (`m_nItemNum`): a purchase cannot take more. */
  readonly stock: number;
  /** The most the quantity box accepts: 99 (9999 for arrows, charms and Red Chips, 21 for the Perin). */
  readonly maxQuantity: number;
  /** 0 penya shop (the price is in penya), 1 chip shop (in Red Chips). */
  readonly buyType: 0 | 1;
  /** The price of `n` pieces (`OnChangeBuyCount`), `0` when the item has no price (it was sold to somebody else). */
  total(n: number): number;
  /**
   * OK: the checks of `OnOK` (`TID_DIAG_0086` / `0087` quantity, `TID_DIAG_0009` / `TID_GAME_CANNTBUY_REDCHIP` payment, each a
   * message box), then `BuyItem` / `BuyChipItem` (`legacy-send: SendBuyItem`, `SendBuyChipItem`). The window closes either way.
   */
  confirm(n: number): void;
}

export interface ConfirmSellRequest {
  readonly name: string;
  readonly icon: string | null;
  /** The pieces of the stack (`m_nItemNum`). */
  count(): number;
  /** The reservation (`GetExtra()`): the window closes itself when the item becomes reserved (`__DROP_CONFIRM_BUG`). */
  reserved(): boolean;
  /** The pieces that remain (the item may have been sold or moved meanwhile). */
  gone(): boolean;
  /** What `n` pieces pay (`rules.sellUnitPrice` x `n`, at least 1 per piece). */
  total(n: number): number;
  confirm(n: number): void;
}

export interface WarningRequest {
  /** The window title text is the layout's; OK continues to `APP_CONFIRM_SELL`. */
  ok(): void;
}

// ---- the quick sell (`CWndQuickSell`, APP_QUICK_SELL; Plan/45 §12.4) ------------------------------------------------

export interface QuickSellEntry {
  readonly objId: number;
  readonly icon: string | null;
  /** The stack count shown in the cell. */
  readonly count: number;
  readonly showCount: boolean;
  readonly item?: unknown;
  readonly name: string;
  readonly kind2: number;
  readonly parts: number;
  readonly serial?: number | string;
}

export interface QuickSellView {
  readonly version: number;
  readonly entries: readonly QuickSellEntry[];
  /** `Σ rules.quickSellUnitPrice` (PL-168: bound items count 1). */
  readonly total: number;
}

export interface QuickSellActions {
  /** OK: `QuickSell` is sent (`legacy-send: SendQuickSell`); the window closes (the server removes the items). */
  confirm(): void;
  /** The window was destroyed (OK, Cancel, the close button, the shop closed): the reservations are restored (`RestoreItem`). */
  closed(): void;
}

// ---- the beauty shops (`APP_BEAUTY_SHOP_EX`, `APP_BEAUTY_SHOP_SKIN`, Plan/45 §17; WP-UI-35, WP-CFEAT-19) ------------------------------

/**
 * The 3-D previews of a beauty shop (doc 41 presets `beautyHair`, `hairKind`, `faceKind`; `CWndBeautyShop::OnDraw`,
 * `DrawHairKind`, `CWndFaceShop::OnDraw`, `DrawFaces`). The port opens one preview per `WTYPE_CUSTOM` placeholder; the SUBJECTS are the
 * feature's (a `MoverVisualState` of the own mover with one thing changed), opaque to the window. A window without previews (no WebGL2,
 * tests) keeps the placeholder cards.
 */
export interface BeautyPreviews<S> {
  readonly port: PreviewPort;
  readonly subjects: S;
}

/** The subjects of the hair shop: the left model, the turning right model and the four head cards. */
export interface HairPreviewSubjects {
  /** `WIDC_CUSTOM5`: the own look as it is now. */
  before(): unknown;
  /** `WIDC_CUSTOM6`: the draft style and colour (the original turns it with `RotationY(g_tmCurrent / 1000)`). */
  after(view: HairShopView): unknown;
  /** `WIDC_CUSTOM1..4`: the head with hair style `mesh` in the colour the player has now. */
  card(mesh: number): unknown;
}

/** The subjects of the face shop. */
export interface FacePreviewSubjects {
  before(): unknown;
  after(view: FaceShopView): unknown;
  /** `WIDC_CUSTOM1..4`, `WIDC_CUSTOM7..10`: the head with face `face` (0-based head mesh). */
  card(face: number): unknown;
}

/**
 * What the hair shop window shows: the draft look and what it costs. The four style cards page with the arrows (`step`), the selected
 * style (a click on a card) is `mesh` and is framed in yellow on its card.
 */
export interface HairShopView {
  readonly version: number;
  /** The selected style (`m_dwSelectHairMesh - 1`, hair mesh 0..`meshCount - 1`). */
  readonly mesh: number;
  readonly meshCount: number;
  /** The draft colour, red / green / blue 0..255. */
  readonly rgb: readonly [number, number, number];
  /** The four style cards (`WIDC_CUSTOM1..4`): mesh numbers, consecutive from the page (`m_dwHairMesh`), wrapping at `meshCount`. */
  readonly cards: readonly number[];
  /** The look the player has now (the left preview and what `reset` returns to). */
  readonly currentMesh: number;
  readonly currentRgb: readonly [number, number, number];
  /** `rules.hairCost`: the price of the style change alone, of the colour change alone and of both (`WIDC_COST`). */
  readonly stylePrice: number;
  readonly colorPrice: number;
  readonly total: number;
  /** Coupon mode (`II_SYS_SYS_SCR_HAIRCHANGE`): everything is free and the title says so (`TID_GAME_NOWUSING_COUPON`). */
  readonly coupon: boolean;
}

export interface HairShopActions {
  /** `WIDC_HAIRSTYLE_LEFT` / `RIGHT`: the four cards page by one style (wraps); the selected style does not change. */
  step(delta: -1 | 1): void;
  /** A click on a style card: the style becomes the selected one. */
  pick(mesh: number): void;
  /** A colour channel changed (slider or edit): 0 red, 1 green, 2 blue. */
  setChannel(channel: 0 | 1 | 2, value: number): void;
  /** `WIDC_BUTTON1`: back to the current look. */
  reset(): void;
  /** OK: the feature opens the confirmation (`CWndBeautyShopConfirm`, or `CWndUseCouponConfirm` in coupon mode), or closes when nothing changes. */
  ok(): void;
  /** The window was destroyed (Cancel, Esc, the NPC left): the draft is dropped. */
  closed(): void;
}

/** What the face shop window shows. */
export interface FaceShopView {
  readonly version: number;
  /** The selected face (head mesh 0..`faceCount - 1`). */
  readonly face: number;
  readonly currentFace: number;
  readonly faceCount: number;
  /** The two rows of four face cards: `WIDC_CUSTOM1..4` (friendly faces, all heads) and `WIDC_CUSTOM7..10` (new faces, the heads from 5). */
  readonly friendly: readonly number[];
  readonly fresh: readonly number[];
  /** `CHANGE_FACE_COST`, 0 in coupon mode. */
  readonly cost: number;
  readonly coupon: boolean;
}

export interface FaceShopActions {
  stepFriendly(delta: -1 | 1): void;
  stepNew(delta: -1 | 1): void;
  pick(face: number): void;
  reset(): void;
  ok(): void;
  closed(): void;
}

/**
 * `APP_BEAUTY_SHOP_EX_CONFIRM` (288 x 128): the confirmation of a paid change (`CWndBeautyShopConfirm`: "Selected Change your Hair Price is
 * 2,000,000 Penya. Do you want to change your hair?"), and the coupon confirmations (`CWndUseCouponConfirm`: a title of its own, the question
 * and a red warning line). Yes / No.
 */
export interface BeautyConfirmRequest {
  readonly text: string;
  /** The window title when it differs from the layout's ("Use Coupon", `TID_GAME_USE_CHANGE_COUPON`). */
  readonly title?: string;
  /** A second paragraph in red (`TID_GAME_WARNNING_USE_COUPON`), added on a line of its own. */
  readonly warning?: string;
  /** Yes: the request is sent (`SetHairReq` / `ChangeFaceReq`) or the coupon shop opens. */
  yes(): void;
  /** No or the close button. */
  no(): void;
}
