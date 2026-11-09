// The mini-game windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-63): the five `APP_MINIGAME_*` layouts become `built`. The confirm and the result box of the dice game sit on the
// layouts of `APP_MESSAGEBOX` and `APP_TRADE_GOLD`, the stake box on the latter: those layouts keep their own controllers (the message box service, the trade gold window).
import type { WndCtor } from '../../catalog';
import { MINIGAME_LAZY } from './lazy';

export const MINIGAME_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_MINIGAME_KAWIBAWIBO: () => MINIGAME_LAZY.WndKawiBawiBoGame.load(),
  APP_MINIGAME_KAWIBAWIBO_WIN: () => MINIGAME_LAZY.WndKawiBawiBoGameWin.load(),
  APP_MINIGAME_WORD: () => MINIGAME_LAZY.WndFindWordGame.load(),
  APP_MINIGAME_PUZZLE: () => MINIGAME_LAZY.WndPuzzleGame.load(),
  APP_MINIGAME_DICE: () => MINIGAME_LAZY.WndDiceGame.load(),
};
