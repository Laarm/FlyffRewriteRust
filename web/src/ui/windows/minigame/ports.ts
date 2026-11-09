// Ports of the NPC mini-game windows (Plan/42 §19.4 D / §24.0, 45 §28.0; WP-UI-63, wired by WP-CFEAT-45 in `features/events/minigame`): the five `APP_MINIGAME_*` windows and the three boxes
// of `src:_Interface/WndMiniGame.cpp` (Kawibawibo 17-318 game, 320-528 win, 531-610 confirm; find word 613-905; dice 908-1390, stake box 1392-1510, message box 1512-1596; puzzle 1598-1950).
// "Windows hold no rules": every window is opened with ONE request object; the rules (the spin of the hands and of the dice, which card goes into which slot, what Start sends) are the
// feature's machines (`features/events/minigame/{kawibawibo,dice,findWord,puzzle}.ts`), which the windows poll and drive:
//   * a machine that counts frames (Kawibawibo, dice) is advanced by the window with `frame()` once per 60 Hz frame that elapsed since its last `process` (`FrameClock`);
//   * `view()` is read every process and the window syncs its controls from it (identity does not matter, the controls compare before they write).
//
// Send table (ref/99 App. B; the feature carries the `// legacy-send:` markers, `features/events/minigame/senders.ts`):
//
//   window                        control / moment                    request member            message
//   -----------------------------------------------------------------------------------------------------------------------------------------------
//   WndKawiBawiBoGame (619)       BTN_START                           KawibawiboRequest.start   SendKawibawiboStart        KawibawiboStart
//   WndKawiBawiBoGameWin (620)    BTN_END                             WinRequest.end            SendKawibawiboGetItem      KawibawiboGetItem
//   WndFindWordGame (612)         opening                             (the feature)             SendAlphabetOpenWnd        AlphabetOpenWnd
//                                 BTN_START, five cards               FindWordRequest.start     SendAlphabetStart          AlphabetStart{question_id, item_obj_ids}
//   WndPuzzleGame (614)           opening                             (the feature)             SendReassembleOpenWnd      ReassembleOpenWnd
//                                 BTN_START, nine cards               PuzzleRequest.start       SendReassembleStart        ReassembleStart{item_obj_ids}
//   WndDiceGame (616)             opening                             (the feature)             SendFiveSystemOpenWnd      FiveSystemOpenWnd
//                                 the stake box OK                    DiceTenderRequest.bet     SendFiveSystemBet          FiveSystemBet{bet_num, bet_penya}
//                                 after the spin (phase 2, once)      DicePort.frame            SendFiveSystemStart        FiveSystemStart
//                                 closing                             (the feature)             SendFiveSystemDestroyWnd   FiveSystemDestroyWnd
//
// The confirm (`CWndKawiBawiBoGameConfirm`) and the dice message box (`CWndDiceGameMessage`) sit on the layout of `APP_MESSAGEBOX`, the stake box (`CWndDiceGameTender`) on that of `APP_TRADE_GOLD`.

import type { ItemView } from '../../tooltip/types';

/** What every window of the family needs of the rest of the client: the texts and the standard boxes. */
export interface MiniGameHost {
  /** `prj.GetText( tid )`; `''` while the texts bundle is missing. */
  text(tid: number): string;
  /** `str.Format( prj.GetText( tid ), ... )`. */
  fmt(tid: number, ...args: readonly (string | number | bigint)[]): string;
  /** `g_WndMng.OpenMessageBox( text )`. */
  box(text: string): void;
}

/** `CMiniGame::KAWI .. BO`. */
export const KAWI = 0;
export const BAWI = 1;
export const BO = 2;
/** The hands in the order of the buttons (`KAWI, BAWI, BO`) and of the texts `TID_GAME_KAWIBAWIBO_KAWI .. _BO`. */
export const HAND_NAMES = ['Kawi', 'Bawi', 'Bo'] as const;

// ---- Kawibawibo (619) -------------------------------------------------------------------------------------------------------------

/** What the game window shows. */
export interface KawibawiboView {
  /** The own hand (`m_nMyChoice`, 0..2). */
  readonly myChoice: number;
  /** The hand of the NPC (`m_nComChoice`, 0..2). */
  readonly comChoice: number;
  /** `m_nWinningCount`. */
  readonly winning: number;
  /** `BTN_LEFT`, `BTN_RIGHT` and `BTN_START` are enabled (`EnableAllBtn` / `DisableAllBtn`). */
  readonly buttonsEnabled: boolean;
}

/** The machine behind the game window (`features/events/minigame/kawibawibo.ts`). */
export interface KawibawiboRequest {
  view(): KawibawiboView;
  /** One 60 Hz frame of `CWndKawiBawiBoGame::Process`. */
  frame(): void;
  /** `BTN_LEFT` (-1) / `BTN_RIGHT` (+1): the own hand cycles. */
  cycle(dir: -1 | 1): void;
  /** `BTN_START`. */
  start(): void;
  /** `WTBID_CLOSE` is refused while this answers `false` (`m_nWinningCount > 0 || m_nStatus != 0 || m_nPrevResult == DRAW`). */
  canClose(): boolean;
  /** The window ended (destroyed by any path): the machine is dropped. */
  closed(): void;
}

/** One prize of the win window: the item and the number (`> 1` is printed at the icon's corner). */
export interface PrizeView {
  readonly itemId: number;
  readonly count: number;
  /** The icon file (`item/itm_x.dds`), `null` for an item without a prop (nothing is drawn, the original skipped a NULL prop). */
  readonly icon: string | null;
  /** The tooltip item (`g_WndMng.PutToolTip_Item( &itemElem )`), `null` for an unknown item. */
  readonly tooltip: ItemView | null;
}

/** What the win window shows and does (`CWndKawiBawiBoGameWin::SetInfo`). */
export interface KawibawiboWinRequest {
  readonly prize: PrizeView;
  readonly nextPrize: PrizeView;
  /** `nWinningCount >= nWinningMaxCount`: the Next button is disabled. */
  readonly nextDisabled: boolean;
  /** `BTN_END`: take the prize (`SendKawibawiboGetItem`; the coupon check is renewed). */
  end(): void;
  /** `BTN_NEXT`: go on (the coupon check is dropped). */
  next(): void;
  /** The window is gone (it re-enables the game's buttons: `OnDestroy`). */
  closed(): void;
}

/** The confirm of menu 44 (`CWndKawiBawiBoGameConfirm`). */
export interface KawibawiboConfirmRequest {
  /** OK: the game window opens (the old one is replaced). Cancel just closes. */
  ok(): void;
  closed?(): void;
}

// ---- find word (612) --------------------------------------------------------------------------------------------------------------

/** A card shown in a slot: its icon file. */
export interface CardView {
  readonly icon: string | null;
}

/** What the find-word window shows. */
export interface FindWordView {
  /** The five slots: the card in each, `null` = empty. */
  readonly slots: readonly (CardView | null)[];
  /** The hint from the server: the card at its slot, drawn dimmed (alpha 100). */
  readonly hint: { readonly slot: number; readonly card: CardView } | null;
  readonly startEnabled: boolean;
  /** The text of `TEXT_DESC` (the panel `minigamefindworddesc`, then the question appended). */
  readonly description: string;
}

export interface FindWordRequest {
  /** The window built its controls: `SendAlphabetOpenWnd`. */
  opened?(): void;
  view(): FindWordView;
  /** A drop of the bag item `objId` on `slot` (-1: not over a slot). The machine decides; the answer says whether it took the card. */
  drop(slot: number, objId: number): boolean;
  /** A double click on a slot: it is emptied. */
  empty(slot: number): void;
  /** `BTN_START`. */
  start(): void;
  closed(): void;
}

// ---- puzzle (614) -----------------------------------------------------------------------------------------------------------------

export interface PuzzleView {
  /** The nine slots: the card in each, `null` = empty. */
  readonly slots: readonly (CardView | null)[];
  readonly startEnabled: boolean;
  readonly description: string;
}

export interface PuzzleRequest {
  /** The window built its controls: `SendReassembleOpenWnd`. */
  opened?(): void;
  view(): PuzzleView;
  drop(objId: number): boolean;
  /** A double click on `slot`: every slot of that card is emptied. */
  empty(slot: number): void;
  start(): void;
  closed(): void;
}

// ---- dice (616) -------------------------------------------------------------------------------------------------------------------

/** The rolling die and the flashing result. */
export interface DiceView {
  /** The face drawn big (`m_nDiceNum`, 0..5). */
  readonly face: number;
  /** The six stakes in penya (`m_nPenya`). */
  readonly stakes: readonly number[];
  /** `m_nMultiple`: the payout is the stake times this. */
  readonly multiple: number;
  /** `BTN_START` is enabled. */
  readonly startEnabled: boolean;
  /** `BTN_NUM1..6` are enabled. */
  readonly buttonsEnabled: boolean;
  /** The face that flashes (`m_bEnd` and `m_nDiceChoiceNum`), `null` = none. */
  readonly flashFace: number | null;
  /** The alpha of the flash (`m_nAlpha`: 70 .. 230, 15 per frame). */
  readonly flashAlpha: number;
  readonly description: string;
}

export interface DicePort {
  /** The window built its controls: `SendFiveSystemOpenWnd`. */
  opened?(): void;
  view(): DiceView;
  /** One 60 Hz frame of `CWndDiceGame::Process`. */
  frame(): void;
  /** `BTN_NUM1..6` (`face` 0..5): the stake box opens. */
  openStake(face: number): void;
  /** `BTN_START`. */
  start(): void;
  closed(): void;
}

/** The stake box (`CWndDiceGameTender`). */
export interface DiceTenderRequest {
  /** The face the stake is for (0..5) and the stake now (the edit's preset). */
  readonly face: number;
  readonly preset: number;
  readonly min: number;
  readonly max: number;
  /** OK / Enter with a valid value: `SendFiveSystemBet( face, value )` (0 withdraws). */
  bet(value: number): void;
  closed?(): void;
}

/** The result box (`CWndDiceGameMessage`). */
export interface DiceMessageRequest {
  readonly text: string;
  /** OK: the stakes are reset and the dice window re-enables its buttons. */
  ok(): void;
  closed?(): void;
}
