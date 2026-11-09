// `MiniGameWindowsService` (`ui.minigame`, Plan/42 §19.4 D / P, 45 §28.0; WP-UI-63): the windows of the NPC mini-games.
//
//   ui.minigame.openKawibawibo( request )          APP_MINIGAME_KAWIBAWIBO (619): rock-paper-scissors      ui.minigame.openKawibawiboWin( request )     620: the won round
//   ui.minigame.openKawibawiboConfirm( request )   the question of menu 44 (on APP_MESSAGEBOX)
//   ui.minigame.openFindWord( request )            APP_MINIGAME_WORD (612): the letter cards
//   ui.minigame.openPuzzle( request )              APP_MINIGAME_PUZZLE (614): the 3 x 3 picture
//   ui.minigame.openDice( port )                   APP_MINIGAME_DICE (616): the dice game               ui.minigame.openDiceTender( request )        the stake box (on APP_TRADE_GOLD)
//   ui.minigame.openDiceMessage( request )         the result box (on APP_MESSAGEBOX)
//   ui.minigame.closeAll()                         every mini-game window goes (a world change, the move, the logout)
//
// Each `open*` replaces the open window of the same kind (the original: `SAFE_DELETE( m_pWndX ); new ...`) and answers `null` while the chunk of the window is still loading (the window then opens
// by itself). The three boxes share their layouts with other windows (`APP_MESSAGEBOX`, `APP_TRADE_GOLD`), so they live under a key of their own and are found by it.

import {
  APP_MESSAGEBOX,
  APP_MINIGAME_DICE,
  APP_MINIGAME_KAWIBAWIBO,
  APP_MINIGAME_KAWIBAWIBO_WIN,
  APP_MINIGAME_PUZZLE,
  APP_MINIGAME_WORD,
  APP_TRADE_GOLD,
} from '../../../gen/defs/resdata';
import { fmt } from '../siege/format';
import type { MessageBoxApi } from '../../msgbox/MessageBoxService';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import type { WndNeuz } from '../../wnd/WndNeuz';
import { MINIGAME_LAZY } from './lazy';
import type {
  DiceMessageRequest,
  DicePort,
  DiceTenderRequest,
  FindWordRequest,
  KawibawiboConfirmRequest,
  KawibawiboRequest,
  KawibawiboWinRequest,
  MiniGameHost,
  PuzzleRequest,
} from './ports';
import type { WndDiceGame } from './WndDiceGame';
import type { WndDiceGameMessage } from './WndDiceGameMessage';
import type { WndDiceGameTender } from './WndDiceGameTender';
import type { WndFindWordGame } from './WndFindWordGame';
import type { WndKawiBawiBoConfirm } from './WndKawiBawiBoConfirm';
import type { WndKawiBawiBoGame } from './WndKawiBawiBoGame';
import type { WndKawiBawiBoGameWin } from './WndKawiBawiBoGameWin';
import type { WndPuzzleGame } from './WndPuzzleGame';

export * from './ports';
export { FrameClock, MAX_CATCH_UP } from './frames';
export { slotAt } from './cardSlots';
export { DICE_IDS, PAYOUT_COLOR, STAKE_COLOR, ttoi } from './rows';
export type {
  WndDiceGame,
  WndDiceGameMessage,
  WndDiceGameTender,
  WndFindWordGame,
  WndKawiBawiBoConfirm,
  WndKawiBawiBoGame,
  WndKawiBawiBoGameWin,
  WndPuzzleGame,
};

export interface MiniGameWindowDeps {
  readonly wm: WindowManager;
  readonly msgbox: MessageBoxApi;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
  /** Thousands separators (`GetNumberFormatEx`). */
  readonly commas: (n: number) => string;
}

/** The `APP_*` of every window of the family (`closeAll`, the probe). */
export const MINIGAME_APPS: readonly number[] = [
  APP_MINIGAME_KAWIBAWIBO,
  APP_MINIGAME_KAWIBAWIBO_WIN,
  APP_MINIGAME_WORD,
  APP_MINIGAME_PUZZLE,
  APP_MINIGAME_DICE,
];

/** The keys of the boxes that share a layout with other windows. */
const CONFIRM_KEY = 'kawibawibo-confirm';
const TENDER_KEY = 'dice-tender';
const MESSAGE_KEY = 'dice-message';

export class MiniGameWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  readonly #commas: (n: number) => string;
  readonly #host: MiniGameHost;

  constructor(deps: MiniGameWindowDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    this.#commas = deps.commas;
    const { msgbox, getText } = deps;
    this.#host = {
      text: getText,
      fmt: (tid, ...args) => fmt(getText(tid), args),
      box: (text) => {
        msgbox.open(text);
      },
    };
  }

  /** The texts and boxes the windows use (tests). */
  get host(): MiniGameHost {
    return this.#host;
  }

  // ---- helpers -----------------------------------------------------------------------------------------------------------

  #live(app: number, key = ''): WndNeuz | null {
    const w = this.#wm.get(app, key);
    return w && !this.#wm.isDestroying(w) ? w : null;
  }

  /** `SAFE_DELETE( pWnd )`: the old window of the app goes at once. */
  #replace(app: number, key = ''): void {
    const old = this.#wm.get(app, key);
    if (!old) return;
    old.destroy();
    this.#wm.flushDestroyed();
  }

  #open<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    return openLazy(this.#wm, app, lazy, build, (a) => {
      this.#replace(a);
    });
  }

  /** The same for a window under a key of its own (a layout other windows use too). */
  #openKeyed<C extends WndClass>(
    app: number,
    key: string,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    const open = (ctor: C): InstanceType<C> | null => {
      this.#replace(app, key);
      return this.#wm.open(app, () => build(ctor), key);
    };
    const ctor = lazy.ctor;
    if (ctor) return open(ctor);
    lazy.load().then(
      (c) => {
        open(c);
      },
      () => undefined,
    );
    return null;
  }

  #get<C extends WndClass>(app: number, lazy: LazyWnd<C>, key = ''): InstanceType<C> | null {
    const w = this.#live(app, key);
    const ctor = lazy.ctor;
    return ctor && w instanceof ctor ? (w as InstanceType<C>) : null;
  }

  isOpen(app: number): boolean {
    return this.#live(app) !== null;
  }

  /** Is the confirm, the stake box or the result box open? */
  isBoxOpen(box: 'confirm' | 'tender' | 'message'): boolean {
    return box === 'confirm'
      ? this.kawibawiboConfirm() !== null
      : box === 'tender'
        ? this.diceTender() !== null
        : this.diceMessage() !== null;
  }

  /** The `APP_*` of the mini-game windows that are open now (the debug probe). */
  openApps(): number[] {
    return MINIGAME_APPS.filter((app) => this.isOpen(app));
  }

  // ---- the windows -------------------------------------------------------------------------------------------------------

  openKawibawibo(request: KawibawiboRequest): WndKawiBawiBoGame | null {
    return this.#open(
      APP_MINIGAME_KAWIBAWIBO,
      MINIGAME_LAZY.WndKawiBawiBoGame,
      (C) => new C(request, this.#host),
    );
  }
  kawibawibo(): WndKawiBawiBoGame | null {
    return this.#get(APP_MINIGAME_KAWIBAWIBO, MINIGAME_LAZY.WndKawiBawiBoGame);
  }

  openKawibawiboWin(request: KawibawiboWinRequest): WndKawiBawiBoGameWin | null {
    return this.#open(
      APP_MINIGAME_KAWIBAWIBO_WIN,
      MINIGAME_LAZY.WndKawiBawiBoGameWin,
      (C) => new C(request, { host: this.#host, tooltip: this.#tooltip }),
    );
  }
  kawibawiboWin(): WndKawiBawiBoGameWin | null {
    return this.#get(APP_MINIGAME_KAWIBAWIBO_WIN, MINIGAME_LAZY.WndKawiBawiBoGameWin);
  }

  openKawibawiboConfirm(request: KawibawiboConfirmRequest): WndKawiBawiBoConfirm | null {
    return this.#openKeyed(
      APP_MESSAGEBOX,
      CONFIRM_KEY,
      MINIGAME_LAZY.WndKawiBawiBoConfirm,
      (C) => new C(request, this.#host),
    );
  }
  kawibawiboConfirm(): WndKawiBawiBoConfirm | null {
    return this.#get(APP_MESSAGEBOX, MINIGAME_LAZY.WndKawiBawiBoConfirm, CONFIRM_KEY);
  }

  openFindWord(request: FindWordRequest): WndFindWordGame | null {
    return this.#open(APP_MINIGAME_WORD, MINIGAME_LAZY.WndFindWordGame, (C) => new C(request));
  }
  findWord(): WndFindWordGame | null {
    return this.#get(APP_MINIGAME_WORD, MINIGAME_LAZY.WndFindWordGame);
  }

  openPuzzle(request: PuzzleRequest): WndPuzzleGame | null {
    return this.#open(APP_MINIGAME_PUZZLE, MINIGAME_LAZY.WndPuzzleGame, (C) => new C(request));
  }
  puzzle(): WndPuzzleGame | null {
    return this.#get(APP_MINIGAME_PUZZLE, MINIGAME_LAZY.WndPuzzleGame);
  }

  openDice(port: DicePort): WndDiceGame | null {
    return this.#open(
      APP_MINIGAME_DICE,
      MINIGAME_LAZY.WndDiceGame,
      (C) => new C(port, this.#host, this.#commas),
    );
  }
  dice(): WndDiceGame | null {
    return this.#get(APP_MINIGAME_DICE, MINIGAME_LAZY.WndDiceGame);
  }

  openDiceTender(request: DiceTenderRequest): WndDiceGameTender | null {
    return this.#openKeyed(
      APP_TRADE_GOLD,
      TENDER_KEY,
      MINIGAME_LAZY.WndDiceGameTender,
      (C) => new C(request, this.#host),
    );
  }
  diceTender(): WndDiceGameTender | null {
    return this.#get(APP_TRADE_GOLD, MINIGAME_LAZY.WndDiceGameTender, TENDER_KEY);
  }

  openDiceMessage(request: DiceMessageRequest): WndDiceGameMessage | null {
    return this.#openKeyed(
      APP_MESSAGEBOX,
      MESSAGE_KEY,
      MINIGAME_LAZY.WndDiceGameMessage,
      (C) => new C(request),
    );
  }
  diceMessage(): WndDiceGameMessage | null {
    return this.#get(APP_MESSAGEBOX, MINIGAME_LAZY.WndDiceGameMessage, MESSAGE_KEY);
  }

  /** The five windows, the confirm and the two dice boxes (`CWndWorld::Process` when the avatar moves; a world change; the logout). */
  closeAll(): void {
    for (const app of MINIGAME_APPS) this.#wm.close(app);
    this.#wm.close(APP_MESSAGEBOX, CONFIRM_KEY);
    this.#wm.close(APP_TRADE_GOLD, TENDER_KEY);
    this.#wm.close(APP_MESSAGEBOX, MESSAGE_KEY);
  }
}
