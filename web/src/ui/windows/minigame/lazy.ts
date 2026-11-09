// The mini-game windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically. ONE class per entry (the vitest transform of `lazyWndSync` reads this file).
import { LazyWnd } from '../../wnd/lazy';
import type * as DiceGame from './WndDiceGame';
import type * as DiceGameMessage from './WndDiceGameMessage';
import type * as DiceGameTender from './WndDiceGameTender';
import type * as FindWordGame from './WndFindWordGame';
import type * as KawiBawiBoConfirm from './WndKawiBawiBoConfirm';
import type * as KawiBawiBoGame from './WndKawiBawiBoGame';
import type * as KawiBawiBoGameWin from './WndKawiBawiBoGameWin';
import type * as PuzzleGame from './WndPuzzleGame';

export const MINIGAME_LAZY = {
  WndKawiBawiBoGame: new LazyWnd<typeof KawiBawiBoGame.WndKawiBawiBoGame>(
    'WndKawiBawiBoGame',
    async () => (await import('./WndKawiBawiBoGame')).WndKawiBawiBoGame,
  ),
  WndKawiBawiBoGameWin: new LazyWnd<typeof KawiBawiBoGameWin.WndKawiBawiBoGameWin>(
    'WndKawiBawiBoGameWin',
    async () => (await import('./WndKawiBawiBoGameWin')).WndKawiBawiBoGameWin,
  ),
  WndKawiBawiBoConfirm: new LazyWnd<typeof KawiBawiBoConfirm.WndKawiBawiBoConfirm>(
    'WndKawiBawiBoConfirm',
    async () => (await import('./WndKawiBawiBoConfirm')).WndKawiBawiBoConfirm,
  ),
  WndFindWordGame: new LazyWnd<typeof FindWordGame.WndFindWordGame>(
    'WndFindWordGame',
    async () => (await import('./WndFindWordGame')).WndFindWordGame,
  ),
  WndDiceGame: new LazyWnd<typeof DiceGame.WndDiceGame>(
    'WndDiceGame',
    async () => (await import('./WndDiceGame')).WndDiceGame,
  ),
  WndDiceGameTender: new LazyWnd<typeof DiceGameTender.WndDiceGameTender>(
    'WndDiceGameTender',
    async () => (await import('./WndDiceGameTender')).WndDiceGameTender,
  ),
  WndDiceGameMessage: new LazyWnd<typeof DiceGameMessage.WndDiceGameMessage>(
    'WndDiceGameMessage',
    async () => (await import('./WndDiceGameMessage')).WndDiceGameMessage,
  ),
  WndPuzzleGame: new LazyWnd<typeof PuzzleGame.WndPuzzleGame>(
    'WndPuzzleGame',
    async () => (await import('./WndPuzzleGame')).WndPuzzleGame,
  ),
} as const;
