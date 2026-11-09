// The rainbow-race windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-60..62): the fifteen layouts become `built`, all lazy. `APP_RAINBOWRACE_END` (776) stays DROP: the original never opens it
// (the end of a game is a message box, `boxes.ts`).
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
import type { WndCtor } from '../../catalog';
import { RAINBOW_LAZY } from './lazy';

/** Keyed by the `APP_*` NAME, like every controller table of the catalog (the `APP_*` ids are imported only to keep the list honest). */
export const RAINBOW_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_RAINBOWRACE_OFFER: () => RAINBOW_LAZY.WndRainbowRaceOffer.load(),
  APP_RAINBOWRACE_INFO: () => RAINBOW_LAZY.WndRainbowRacePanel.load(),
  APP_RAINBOWRACE_RULE: () => RAINBOW_LAZY.WndRainbowRacePanel.load(),
  APP_RAINBOWRACE_PRIZE: () => RAINBOW_LAZY.WndRainbowRacePanel.load(),
  APP_RAINBOWRACE_MINIGAME_INFO: () => RAINBOW_LAZY.WndRainbowRacePanel.load(),
  APP_RAINBOWRACE_RANKING: () => RAINBOW_LAZY.WndRainbowRaceRanking.load(),
  APP_RAINBOWRACE_BUTTON: () => RAINBOW_LAZY.WndRainbowRaceButton.load(),
  APP_RAINBOWRACE_MINIGAME: () => RAINBOW_LAZY.WndRainbowRaceMiniGame.load(),
  APP_RR_MINIGAME_KAWIBAWIBO: () => RAINBOW_LAZY.WndRRMiniGameKawiBawiBo.load(),
  APP_RR_MINIGAME_DICE: () => RAINBOW_LAZY.WndRRMiniGameDice.load(),
  APP_RR_MINIGAME_ARITHMETIC: () => RAINBOW_LAZY.WndRRMiniGameArithmetic.load(),
  APP_RR_MINIGAME_STOPWATCH: () => RAINBOW_LAZY.WndRRMiniGameStopWatch.load(),
  APP_RR_MINIGAME_TYPING: () => RAINBOW_LAZY.WndRRMiniGameTyping.load(),
  APP_RR_MINIGAME_CARD: () => RAINBOW_LAZY.WndRRMiniGameCard.load(),
  APP_RR_MINIGAME_LADDER: () => RAINBOW_LAZY.WndRRMiniGameLadder.load(),
};

/** The ids of the fifteen windows (tests). */
export const RAINBOW_BUILT_APPS: readonly number[] = [
  APP_RAINBOWRACE_OFFER,
  APP_RAINBOWRACE_INFO,
  APP_RAINBOWRACE_RULE,
  APP_RAINBOWRACE_RANKING,
  APP_RAINBOWRACE_PRIZE,
  APP_RAINBOWRACE_MINIGAME,
  APP_RAINBOWRACE_MINIGAME_INFO,
  APP_RAINBOWRACE_BUTTON,
  APP_RR_MINIGAME_KAWIBAWIBO,
  APP_RR_MINIGAME_DICE,
  APP_RR_MINIGAME_ARITHMETIC,
  APP_RR_MINIGAME_STOPWATCH,
  APP_RR_MINIGAME_TYPING,
  APP_RR_MINIGAME_CARD,
  APP_RR_MINIGAME_LADDER,
];
