// The constants of the rainbow-race windows (Plan/42 §19.4 P, 45 §28.7; WP-UI-60..62; `src:_Common/RainbowRace.h:6-8, 22`, `_Common/MiniGameBase.h:5`, `_Interface/WndRainbowRace.cpp:341-440`): the game ids and
// states, the colours, the panel keys, the pictures and the atlas paths. Pure data; nothing here imports a window.

import {
  APP_RAINBOWRACE_BUTTON,
  APP_RAINBOWRACE_INFO,
  APP_RAINBOWRACE_MINIGAME,
  APP_RAINBOWRACE_MINIGAME_INFO,
  APP_RAINBOWRACE_OFFER,
  APP_RAINBOWRACE_PRIZE,
  APP_RAINBOWRACE_RANKING,
  APP_RAINBOWRACE_RULE,
  APP_RR_MINIGAME_ARITHMETIC,
  APP_RR_MINIGAME_CARD,
  APP_RR_MINIGAME_DICE,
  APP_RR_MINIGAME_KAWIBAWIBO,
  APP_RR_MINIGAME_LADDER,
  APP_RR_MINIGAME_STOPWATCH,
  APP_RR_MINIGAME_TYPING,
} from '../../../gen/defs/resdata';
import {
  TID_MMI_LORD_RAINBOW_ARITHMETIC,
  TID_MMI_LORD_RAINBOW_CARD,
  TID_MMI_LORD_RAINBOW_DICE,
  TID_MMI_LORD_RAINBOW_KAWIBAWIBO,
  TID_MMI_LORD_RAINBOW_LADDER,
  TID_MMI_LORD_RAINBOW_STOPWATCH,
  TID_MMI_LORD_RAINBOW_TYPING,
} from '../../../gen/defs/text';

/** `RMG_*`: the seven mini-games in the order of the race. */
export const RMG = {
  KAWIBAWIBO: 0,
  DICE: 1,
  ARITHMETIC: 2,
  STOPWATCH: 3,
  TYPING: 4,
  CARD: 5,
  LADDER: 6,
} as const;
/** `RMG_MAX`: also "the stored game" in `SendMinigamePacket( RMG_MAX, .. )`. */
export const RMG_MAX = 7;
export type RmgId = (typeof RMG)[keyof typeof RMG];

/** `IsAllCompleted`: `m_wGameState == 0x7F`. */
export const ALL_COMPLETED = 0x7f;

/** `MP_*` (`MiniGameBase.h`): the state of a mini-game packet. */
export const MP = { NONE: 0, OPENWND: 1, TRUE: 2, FALSE: 3, FINISH: 4, FAIL: 5 } as const;
export type MpState = (typeof MP)[keyof typeof MP];

/** The wire names of the mini-game cues (`PLAYSND( "InfOpen.wav" )` = `tick`, `PlayMusic( BGM_IN_FITUP )` = `win`; Plan/44 T39). */
export type MiniGameCue = 'tick' | 'win';

/** The `APP_*` of the seven game windows, by game id. */
export const GAME_APPS: readonly number[] = [
  APP_RR_MINIGAME_KAWIBAWIBO,
  APP_RR_MINIGAME_DICE,
  APP_RR_MINIGAME_ARITHMETIC,
  APP_RR_MINIGAME_STOPWATCH,
  APP_RR_MINIGAME_TYPING,
  APP_RR_MINIGAME_CARD,
  APP_RR_MINIGAME_LADDER,
];

/** Every window of the family (`closeAll`). The end box and the ladder box sit on `APP_MESSAGEBOX` under keys of their own. */
export const RAINBOW_APPS: readonly number[] = [
  APP_RAINBOWRACE_OFFER,
  APP_RAINBOWRACE_INFO,
  APP_RAINBOWRACE_RULE,
  APP_RAINBOWRACE_RANKING,
  APP_RAINBOWRACE_PRIZE,
  APP_RAINBOWRACE_MINIGAME,
  APP_RAINBOWRACE_MINIGAME_INFO,
  APP_RAINBOWRACE_BUTTON,
  ...GAME_APPS,
];

/** The title text of each game (`m_nTextID`, the same ids the NPC menu rows 113-119 use). */
export const GAME_TITLE_TIDS: readonly number[] = [
  TID_MMI_LORD_RAINBOW_KAWIBAWIBO,
  TID_MMI_LORD_RAINBOW_DICE,
  TID_MMI_LORD_RAINBOW_ARITHMETIC,
  TID_MMI_LORD_RAINBOW_STOPWATCH,
  TID_MMI_LORD_RAINBOW_TYPING,
  TID_MMI_LORD_RAINBOW_CARD,
  TID_MMI_LORD_RAINBOW_LADDER,
];

/** The colour of each game (`m_dwColor`: red, salmon, yellow, green, blue, indigo, violet), `[r, g, b]`. */
export const GAME_COLORS: readonly (readonly [number, number, number])[] = [
  [255, 0, 0],
  [250, 128, 114],
  [255, 255, 0],
  [0, 128, 0],
  [0, 0, 255],
  [75, 0, 100],
  [128, 0, 128],
];

/** The help panels of the game information window (`m_strMiniGameInfo`, lower-case stems of the help bundle). */
export const GAME_PANELS: readonly string[] = [
  'rrminigamekawibawibo',
  'rrminigamedice',
  'rrminigamearithmetic',
  'rrminigamestopwatch',
  'rrminigametyping',
  'rrminigamecard',
  'rrminigameladder',
];

/** The panels of the other windows. */
export const PANEL_OFFER = 'rainbowraceoffer';
export const PANEL_INFO = 'rainbowraceinfo';
export const PANEL_RULE = 'rainbowracerule';
export const PANEL_PRIZE = 'rainbowraceprize';

/** The check mark of a completed game (`m_strPathCheck`, `Theme/`). */
export const CHECK_PICTURES: readonly string[] = [
  'CheckRed.bmp',
  'CheckOrange.bmp',
  'CheckYellow.bmp',
  'CheckGreen.bmp',
  'CheckBlue.bmp',
  'CheckIndigo.bmp',
  'CheckViolet.bmp',
];

/** The text colours of the progress list (`0xff858585` for a completed game, `0xff7878c8` otherwise). */
export const TITLE_COLOR_DONE = 0xff858585;
export const TITLE_COLOR_OPEN = 0xff7878c8;

/** The atlases (`ui-atlases` keys). */
export const RAINBOW_BUTTON_ATLAS = 'theme/texmaprainbowbutton.inc';
export const DICE_NUMBER_ATLAS = 'theme/rainbowdicenumber.inc';
export const STOPWATCH_NUMBER_ATLAS = 'theme/rainbowstopwatchnumber.inc';
export const PENALTY_NUMBER_ATLAS = 'theme/rainbowcardpenaltynumber.inc';

/** The keys of the two message boxes that live next to the main box (the legacy `m_pWndRainbowRaceMiniGameEnd`, `m_pWndRRMiniGameLadderFail`). */
export const END_BOX_KEY = 'rr-end';
export const LADDER_FAIL_BOX_KEY = 'rr-ladder-fail';

/** The pictures of the games (paths under `Theme/` unless a directory is given). */
export const ICON_KAWI: readonly string[] = ['kawi', 'bawi', 'bo'];
export const iconKawiBawiBo = (choice: number, side: 1 | 2): string =>
  `icon/icon_${ICON_KAWI[choice] ?? 'kawi'}${String(side)}.dds`;
export const iconDice = (face: number): string => `icon/icon_${String(face + 1)}dice90x90.dds`;
export const cardPicture = (kind: number): string => `RainbowRaceCard${String(kind + 1)}.tga`;
export const CARD_BACK = 'RainbowRaceCardBack.tga';
export const LADDER_PICTURES = {
  bg1: 'WndRainbowRaceLadderBg1.bmp',
  bg2: 'WndRainbowRaceLadderBg2.bmp',
  horizon: 'RainbowRaceLadderHorizon.bmp',
  choice: 'WndRainbowRaceLadderChoice.bmp',
  point: 'WndRainbowRaceLadderPoint.bmp',
  goal: 'WndRainbowRaceLadderGoal.bmp',
  fail: 'WndRainbowRaceLadderFail.bmp',
} as const;

export { FRAME_MS } from '../../../platform/time';
