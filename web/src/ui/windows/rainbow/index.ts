// `RainbowWindowsService` (`ui.rainbow`, Plan/42 §19.4 P, 45 §28.7; WP-UI-60, -61, -62): the rainbow-race windows. The FEATURE (the store slice, the menus 107-119, the reducers of the nine S2C cases, the race button
// lifecycle, the end boxes) is `features/events/rainbow` (WP-CFEAT-44), which calls exactly these methods:
//
//   ui.rainbow.openOffer( request )                      766: the application window            ui.rainbow.openPanel( app, request )   768 / 770 / 774: info, rules, prizes
//   ui.rainbow.openRanking( request )                    772: the previous ranking                ui.rainbow.openInfo( request )         794: the panel of one game, beside the progress list
//   ui.rainbow.openButton( request ) / closeButton()     803: the race button                     ui.rainbow.openProgress( request )     792: the progress list of the seven games
//   ui.rainbow.freezeProgress() / unfreezeProgress()     a FINISH arrived / the end box went      ui.rainbow.closeProgress() / isProgressOpen()
//   ui.rainbow.openKawiBawiBo / openDice / openArithmetic / openStopwatch / openTyping / openCard / openLadder( request )      795-801, with the getters of the same names without `open`
//   ui.rainbow.openEndBox( game, onClosed )              the end box of a game (its own key, next to the main box)
//   ui.rainbow.closeGame( game ) / closeGames() / closeAll()
//
// Each `open*` replaces the open window of its kind (the original: `SAFE_DELETE( m_pWndX ); new ...`) and answers `null` while the chunk of the window is still loading (the window then opens by itself).

import {
  APP_MESSAGEBOX,
  APP_RAINBOWRACE_BUTTON,
  APP_RAINBOWRACE_MINIGAME,
  APP_RAINBOWRACE_MINIGAME_INFO,
  APP_RAINBOWRACE_OFFER,
  APP_RAINBOWRACE_RANKING,
} from '../../../gen/defs/resdata';
import { TID_GAME_RAINBOWRACE_END, TID_GAME_RRMINIGAME_LADDER_FAIL } from '../../../gen/defs/text';
import type { MessageBoxApi } from '../../msgbox/MessageBoxService';
import type { UiAtlases } from '../../skin/atlas';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import type { WndNeuz } from '../../wnd/WndNeuz';
import { fmt } from '../siege/format';
import { LadderFailBox, RainbowGameEndBox } from './boxes';
import {
  END_BOX_KEY,
  GAME_APPS,
  GAME_TITLE_TIDS,
  LADDER_FAIL_BOX_KEY,
  RAINBOW_APPS,
} from './constants';
import { RAINBOW_LAZY } from './lazy';
import type {
  ArithmeticRequest,
  ButtonRequest,
  CardRequest,
  DiceRequest,
  GameInfoRequest,
  KawiBawiBoRequest,
  LadderRequest,
  OfferRequest,
  PanelRequest,
  ProgressRequest,
  RainbowHost,
  RankingRequest,
  StopwatchRequest,
  TypingRequest,
} from './ports';
import type { WndRainbowRaceButton } from './WndRainbowRaceButton';
import type { WndRainbowRaceMiniGame } from './WndRainbowRaceMiniGame';
import type { WndRainbowRaceOffer } from './WndRainbowRaceOffer';
import type { WndRainbowRacePanel } from './WndRainbowRacePanel';
import type { WndRainbowRaceRanking } from './WndRainbowRaceRanking';
import type { WndRRMiniGameArithmetic } from './WndRRMiniGameArithmetic';
import type { WndRRMiniGameCard } from './WndRRMiniGameCard';
import type { WndRRMiniGameDice } from './WndRRMiniGameDice';
import type { WndRRMiniGameKawiBawiBo } from './WndRRMiniGameKawiBawiBo';
import type { WndRRMiniGameLadder } from './WndRRMiniGameLadder';
import type { WndRRMiniGameStopWatch } from './WndRRMiniGameStopWatch';
import type { WndRRMiniGameTyping } from './WndRRMiniGameTyping';

export * from './ports';
export * from './constants';
export { atoi } from './atoi';
export { buttonPosition, infoPosition, progressPosition } from './geometry';
export { RainbowGameEndBox, LadderFailBox } from './boxes';
export type {
  WndRainbowRaceButton,
  WndRainbowRaceMiniGame,
  WndRainbowRaceOffer,
  WndRainbowRacePanel,
  WndRainbowRaceRanking,
  WndRRMiniGameArithmetic,
  WndRRMiniGameCard,
  WndRRMiniGameDice,
  WndRRMiniGameKawiBawiBo,
  WndRRMiniGameLadder,
  WndRRMiniGameStopWatch,
  WndRRMiniGameTyping,
};

export interface RainbowWindowDeps {
  readonly wm: WindowManager;
  readonly msgbox: MessageBoxApi;
  readonly atlases: UiAtlases;
  readonly getText: (tid: number) => string;
}

export class RainbowWindowsService {
  readonly #wm: WindowManager;
  readonly #atlases: UiAtlases;
  readonly #host: RainbowHost;
  readonly #getText: (tid: number) => string;

  constructor(deps: RainbowWindowDeps) {
    this.#wm = deps.wm;
    this.#atlases = deps.atlases;
    this.#getText = deps.getText;
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
  get host(): RainbowHost {
    return this.#host;
  }

  // ---- helpers -----------------------------------------------------------------------------------------------------------

  /** `GetApplet( app )` that is not on its way out. */
  #live(app: number, key = ''): WndNeuz | null {
    const w = this.#wm.get(app, key);
    return w && !this.#wm.isDestroying(w) ? w : null;
  }

  /** `SAFE_DELETE( pWnd )`: the old window of the app goes at once. */
  #replaceApp(app: number, key = ''): void {
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
      this.#replaceApp(a);
    });
  }

  #get<C extends WndClass>(app: number, lazy: LazyWnd<C>): InstanceType<C> | null {
    const w = this.#live(app);
    const ctor = lazy.ctor;
    return ctor && w instanceof ctor ? (w as InstanceType<C>) : null;
  }

  isOpen(app: number): boolean {
    return this.#live(app) !== null;
  }

  /** The `APP_*` of the rainbow windows that are open now (the debug probe). */
  openApps(): number[] {
    return RAINBOW_APPS.filter((app) => this.isOpen(app));
  }

  // ---- the application, the panels and the ranking ------------------------------------------------------------------------

  openOffer(request: OfferRequest): WndRainbowRaceOffer | null {
    return this.#open(
      APP_RAINBOWRACE_OFFER,
      RAINBOW_LAZY.WndRainbowRaceOffer,
      (C) => new C(request, { host: this.#host }),
    );
  }
  offer(): WndRainbowRaceOffer | null {
    return this.#get(APP_RAINBOWRACE_OFFER, RAINBOW_LAZY.WndRainbowRaceOffer);
  }

  /** 768 (info), 770 (rules) or 774 (prizes). */
  openPanel(app: number, request: PanelRequest): WndRainbowRacePanel | null {
    return this.#open(app, RAINBOW_LAZY.WndRainbowRacePanel, (C) => new C(app, request));
  }
  panel(app: number): WndRainbowRacePanel | null {
    return this.#get(app, RAINBOW_LAZY.WndRainbowRacePanel);
  }

  /** 794: the panel of one game; the progress window it opens beside is read at creation. */
  openInfo(request: GameInfoRequest): WndRainbowRacePanel | null {
    return this.#open(
      APP_RAINBOWRACE_MINIGAME_INFO,
      RAINBOW_LAZY.WndRainbowRacePanel,
      (C) => new C(APP_RAINBOWRACE_MINIGAME_INFO, request),
    );
  }
  info(): WndRainbowRacePanel | null {
    return this.#get(APP_RAINBOWRACE_MINIGAME_INFO, RAINBOW_LAZY.WndRainbowRacePanel);
  }

  openRanking(request: RankingRequest): WndRainbowRaceRanking | null {
    return this.#open(
      APP_RAINBOWRACE_RANKING,
      RAINBOW_LAZY.WndRainbowRaceRanking,
      (C) => new C(request, { atlases: this.#atlases }),
    );
  }
  ranking(): WndRainbowRaceRanking | null {
    return this.#get(APP_RAINBOWRACE_RANKING, RAINBOW_LAZY.WndRainbowRaceRanking);
  }

  // ---- the race button and the progress list ------------------------------------------------------------------------------

  openButton(request: ButtonRequest): WndRainbowRaceButton | null {
    return this.#open(
      APP_RAINBOWRACE_BUTTON,
      RAINBOW_LAZY.WndRainbowRaceButton,
      (C) => new C(request, { atlases: this.#atlases }),
    );
  }
  button(): WndRainbowRaceButton | null {
    return this.#get(APP_RAINBOWRACE_BUTTON, RAINBOW_LAZY.WndRainbowRaceButton);
  }
  closeButton(): void {
    this.#live(APP_RAINBOWRACE_BUTTON)?.destroy();
  }

  openProgress(request: ProgressRequest): WndRainbowRaceMiniGame | null {
    return this.#open(
      APP_RAINBOWRACE_MINIGAME,
      RAINBOW_LAZY.WndRainbowRaceMiniGame,
      (C) => new C(request, { host: this.#host }),
    );
  }
  progress(): WndRainbowRaceMiniGame | null {
    return this.#get(APP_RAINBOWRACE_MINIGAME, RAINBOW_LAZY.WndRainbowRaceMiniGame);
  }
  isProgressOpen(): boolean {
    return this.isOpen(APP_RAINBOWRACE_MINIGAME);
  }
  closeProgress(): void {
    this.#live(APP_RAINBOWRACE_MINIGAME)?.destroy();
  }
  /** `m_bGameInfoRefresh = FALSE` of an open progress window (a FINISH arrived). */
  freezeProgress(): void {
    this.progress()?.freeze();
  }
  /** `m_bGameInfoRefresh = TRUE`. */
  unfreezeProgress(): void {
    this.progress()?.unfreeze();
  }

  // ---- the seven games ---------------------------------------------------------------------------------------------------

  openKawiBawiBo(request: KawiBawiBoRequest): WndRRMiniGameKawiBawiBo | null {
    return this.#open(
      GAME_APPS[0] ?? 0,
      RAINBOW_LAZY.WndRRMiniGameKawiBawiBo,
      (C) => new C(request, { host: this.#host }),
    );
  }
  kawiBawiBo(): WndRRMiniGameKawiBawiBo | null {
    return this.#get(GAME_APPS[0] ?? 0, RAINBOW_LAZY.WndRRMiniGameKawiBawiBo);
  }

  openDice(request: DiceRequest): WndRRMiniGameDice | null {
    return this.#open(
      GAME_APPS[1] ?? 0,
      RAINBOW_LAZY.WndRRMiniGameDice,
      (C) => new C(request, { atlases: this.#atlases }),
    );
  }
  dice(): WndRRMiniGameDice | null {
    return this.#get(GAME_APPS[1] ?? 0, RAINBOW_LAZY.WndRRMiniGameDice);
  }

  openArithmetic(request: ArithmeticRequest): WndRRMiniGameArithmetic | null {
    return this.#open(
      GAME_APPS[2] ?? 0,
      RAINBOW_LAZY.WndRRMiniGameArithmetic,
      (C) => new C(request),
    );
  }
  arithmetic(): WndRRMiniGameArithmetic | null {
    return this.#get(GAME_APPS[2] ?? 0, RAINBOW_LAZY.WndRRMiniGameArithmetic);
  }

  openStopwatch(request: StopwatchRequest): WndRRMiniGameStopWatch | null {
    return this.#open(
      GAME_APPS[3] ?? 0,
      RAINBOW_LAZY.WndRRMiniGameStopWatch,
      (C) => new C(request, { atlases: this.#atlases }),
    );
  }
  stopwatch(): WndRRMiniGameStopWatch | null {
    return this.#get(GAME_APPS[3] ?? 0, RAINBOW_LAZY.WndRRMiniGameStopWatch);
  }

  openTyping(request: TypingRequest): WndRRMiniGameTyping | null {
    return this.#open(GAME_APPS[4] ?? 0, RAINBOW_LAZY.WndRRMiniGameTyping, (C) => new C(request));
  }
  typing(): WndRRMiniGameTyping | null {
    return this.#get(GAME_APPS[4] ?? 0, RAINBOW_LAZY.WndRRMiniGameTyping);
  }

  openCard(request: CardRequest): WndRRMiniGameCard | null {
    return this.#open(
      GAME_APPS[5] ?? 0,
      RAINBOW_LAZY.WndRRMiniGameCard,
      (C) => new C(request, { atlases: this.#atlases }),
    );
  }
  card(): WndRRMiniGameCard | null {
    return this.#get(GAME_APPS[5] ?? 0, RAINBOW_LAZY.WndRRMiniGameCard);
  }

  openLadder(request: LadderRequest): WndRRMiniGameLadder | null {
    return this.#open(
      GAME_APPS[6] ?? 0,
      RAINBOW_LAZY.WndRRMiniGameLadder,
      (C) =>
        new C(request, {
          openFail: (onGone) => {
            this.#openLadderFail(onGone);
          },
        }),
    );
  }
  ladder(): WndRRMiniGameLadder | null {
    return this.#get(GAME_APPS[6] ?? 0, RAINBOW_LAZY.WndRRMiniGameLadder);
  }

  /** Whether the window of a game (0..6) is open. */
  isGameOpen(game: number): boolean {
    const app = GAME_APPS[game];
    return app !== undefined && this.isOpen(app);
  }

  /** Closes the window of a game. */
  closeGame(game: number): void {
    const app = GAME_APPS[game];
    if (app !== undefined) this.#live(app)?.destroy();
  }

  /** Closes the seven game windows. */
  closeGames(): void {
    for (const app of GAME_APPS) this.#live(app)?.destroy();
  }

  // ---- the boxes ------------------------------------------------------------------------------------------------------------

  /**
   * `new CWndRainbowRaceMiniGameEnd; SetGameID( game ); Initialize( NULL )`: "Congratulations. You are the winner of <game>!" on its own key (a second end box replaces the first). OK refreshes the
   * progress list (`SetCurrentGameInfo`); when the box goes, by OK, Esc or the logout sweep, the list is unfrozen and `onClosed` runs (the feature sets `m_bCheckEnd`).
   */
  openEndBox(game: number, onClosed?: () => void): RainbowGameEndBox {
    const text = this.#host.fmt(
      TID_GAME_RAINBOWRACE_END,
      this.#host.text(GAME_TITLE_TIDS[game] ?? 0),
    );
    this.#replaceApp(APP_MESSAGEBOX, END_BOX_KEY);
    const box = new RainbowGameEndBox(text, {
      onOk: () => {
        this.progress()?.refreshNow();
      },
      onGone: () => {
        this.unfreezeProgress();
        onClosed?.();
      },
    });
    this.#wm.open(APP_MESSAGEBOX, () => box, END_BOX_KEY);
    return box;
  }

  /** Whether an end box is open. */
  isEndBoxOpen(): boolean {
    return this.#live(APP_MESSAGEBOX, END_BOX_KEY) !== null;
  }

  /** The text of the open end box, `null` for none (the probe). */
  endBoxText(): string | null {
    const w = this.#live(APP_MESSAGEBOX, END_BOX_KEY);
    return w instanceof RainbowGameEndBox ? w.message : null;
  }

  /** The text of the open ladder fail box, `null` for none (the probe). */
  ladderFailText(): string | null {
    const w = this.#live(APP_MESSAGEBOX, LADDER_FAIL_BOX_KEY);
    return w instanceof LadderFailBox ? w.message : null;
  }

  #openLadderFail(onGone: () => void): void {
    this.#replaceApp(APP_MESSAGEBOX, LADDER_FAIL_BOX_KEY);
    const box = new LadderFailBox(this.#getText(TID_GAME_RRMINIGAME_LADDER_FAIL), {
      onGone,
    });
    this.#wm.open(APP_MESSAGEBOX, () => box, LADDER_FAIL_BOX_KEY);
  }

  /** Whether the ladder fail box is open. */
  isLadderFailBoxOpen(): boolean {
    return this.#live(APP_MESSAGEBOX, LADDER_FAIL_BOX_KEY) !== null;
  }

  /** Every rainbow window and box goes (the session left the world). */
  closeAll(): void {
    for (const app of RAINBOW_APPS) this.#live(app)?.destroy();
    this.#live(APP_MESSAGEBOX, END_BOX_KEY)?.destroy();
    this.#live(APP_MESSAGEBOX, LADDER_FAIL_BOX_KEY)?.destroy();
  }
}
