// The requests of the rainbow-race windows (Plan/42 §19.4 P, 45 §28.7; WP-UI-60..62). A window only shows and reports: what the buttons send (App. B names below), the panel texts (read from the help bundle by the
// feature), the slice the progress window reads and the end boxes are the rainbow FEATURE's (`features/events/rainbow`, WP-CFEAT-44), which opens every window through `ui.rainbow` with ONE request object.
// The send table of CA3 (App. B names; `RMG` = the stored game, `SendMinigamePacket( RMG_MAX, MP_NONE, p1, p2 )`):
//
//   WndRainbowRaceOffer      OK                                   `apply()` = SendRainbowRaceApplication, then the window closes; CANCEL closes
//   WndRainbowRacePanel      OK                                   closes (768 info, 770 rule, 774 prize, 794 game information)
//   WndRainbowRaceRanking    OK                                   closes (the rows come from the player-data cache at draw time)
//   WndRainbowRaceButton     a press and a release, no move       `onClick()` toggles the progress window
//   WndRainbowRaceMiniGame   a click on a title                   `info( game )` opens 794 with that game's panel; OK closes
//   WndRRMiniGameKawiBawiBo  START                                `play()` = SendRainbowRaceMiniGameState { RMG, 0, 0, 0 } (the server rolls)
//   WndRRMiniGameDice        START                                `roll()` = SendRainbowRaceMiniGameState { RMG, 0, 0, 0 }
//   WndRRMiniGameArithmetic  the input button                     `answer( atoi( edit ) )` = SendRainbowRaceMiniGameState { RMG, 0, n, 0 }
//   WndRRMiniGameStopWatch   the second START press / 60 s        `send( ms )` = SendRainbowRaceMiniGameState { RMG, 0, ms, 0 }
//   WndRRMiniGameTyping      OK                                   `submit( [e1, e2, e3] )` = SendRainbowRaceMiniGameExtState { RMG, 0, 0, 0 } + the three strings
//   WndRRMiniGameCard        the second pick                      `pick( first, second )` = SendRainbowRaceMiniGameState { RMG, 0, first, second }
//   WndRRMiniGameLadder      OK                                   `roll()` = SendRainbowRaceMiniGameState { RMG, 0, 0, 0 }

import type { MiniGameCue } from './constants';
import type { Rng } from './rng';

/** The texts and boxes the windows use. */
export interface RainbowHost {
  text(tid: number): string;
  /** The text of `tid` with its `%s` / `%d` conversions filled. */
  fmt(tid: number, ...args: (string | number)[]): string;
  /** `g_WndMng.OpenMessageBox`. */
  box(text: string): void;
}

/** `APP_RAINBOWRACE_OFFER`: the application window. */
export interface OfferRequest {
  /** `TID_GAME_RAINBOWRACE_OFFERNUM( count )` in bold black above the panel. */
  readonly count: number;
  /** The panel `rainbowraceoffer` of the help bundle (the fee is in its text). */
  readonly text: string;
  /** OK: `SendRainbowRaceApplication`; the window closes after the call. */
  apply(): void;
}

/** `APP_RAINBOWRACE_INFO` / `_RULE` / `_PRIZE` / `_MINIGAME_INFO`: a window with one panel text. */
export interface PanelRequest {
  readonly text: string;
}

/** `APP_RAINBOWRACE_MINIGAME_INFO`: the panel of one game. */
export interface GameInfoRequest extends PanelRequest {
  /** The game (0..6) the panel belongs to. */
  readonly game: number;
}

/** One ranked player as the player-data cache knows him. */
export interface RankingPlayer {
  readonly name: string;
  /** `JTYPE_*` of the job. */
  readonly jobType: number;
  readonly job: number;
  readonly level: number;
}

/** `APP_RAINBOWRACE_RANKING`: five ranked character ids (0 = none), the rows are read at draw time. */
export interface RankingRequest {
  readonly ids: readonly number[];
  /** The cache entry of a character, `null` while unknown (the row stays blank). */
  player(id: number): RankingPlayer | null;
}

/** `APP_RAINBOWRACE_BUTTON`. */
export interface ButtonRequest {
  /** A press and a release without a window move between them. */
  onClick(): void;
}

/** The part of the slice the progress window shows. */
export interface ProgressState {
  /** `1 << game` of the current game, 0 for none. */
  readonly nowGame: number;
  /** The completed games (bit per game, 0x7F = all). */
  readonly completed: number;
}

/** `APP_RAINBOWRACE_MINIGAME`. */
export interface ProgressRequest {
  /** The live state (`CRainbowRace::GetInstance()`), read on every `process` unless the window is frozen. */
  state(): ProgressState;
  /** A title was clicked: open the information window of the game. */
  info(game: number): void;
}

/** What every game window shares. */
interface GameRequestBase {
  /** The panel of the game (`rrminigame*` of the help bundle). */
  readonly text: string;
  /** `PLAYSND( "InfOpen.wav" )` / `PlayMusic( BGM_IN_FITUP )`. */
  cue(e: MiniGameCue): void;
  /** The clock of the game (`ctx.now()`, ms). */
  now(): number;
}

/** Game 0. */
export interface KawiBawiBoRequest extends GameRequestBase {
  /** START: `SendMinigamePacket()`. */
  play(): void;
  /** FINISH: the end box of the game (the window closes itself). */
  end(): void;
}

/** Game 1. */
export interface DiceRequest extends GameRequestBase {
  /** The target number of the OPENWND (`p1`). */
  readonly target: number;
  roll(): void;
  end(): void;
  /** The faces of the roll (cosmetic); `Math.random` by default. */
  readonly rng?: Rng;
}

/** Game 2. */
export interface ArithmeticRequest extends GameRequestBase {
  /** The question and the correct answers of the OPENWND (`SetNextQuestion` right after the window was made); empty/0 when absent. */
  readonly question?: string;
  readonly count?: number;
  /** The input button: `SendMinigamePacket( RMG_MAX, MP_NONE, n )`. */
  answer(n: number): void;
}

/** Game 3. */
export interface StopwatchRequest extends GameRequestBase {
  /** The target time of the OPENWND (`SetTargetTime( p1 )` right after the window was made). */
  readonly target?: number;
  /** The time to send: the second press, or exactly 60 000 when the clock runs out. */
  send(ms: number): void;
  /** The test pin (`rainbowStopwatchPin`): the elapsed time a running clock reports instead of the real one. */
  pinned?(): number | null;
}

/** Game 4. */
export interface TypingRequest extends GameRequestBase {
  /** The questions of the OPENWND (`SetQuestion( i, .. )` right after the window was made). */
  readonly questions?: readonly string[];
  /** The ext packet with the three typed strings. */
  submit(answers: readonly string[]): void;
}

/** Game 5. */
export interface CardRequest extends GameRequestBase {
  /** The deck of the OPENWND (`SetQuestion( deck )` right after the window was made): byte `i` minus one is the kind of card `i`. */
  readonly deck?: string;
  /** The second pick: `SendMinigamePacket( RMG_MAX, MP_NONE, first, second )`. */
  pick(first: number, second: number): void;
}

/** Game 6. */
export interface LadderRequest extends GameRequestBase {
  roll(): void;
  end(): void;
  readonly rng?: Rng;
}
