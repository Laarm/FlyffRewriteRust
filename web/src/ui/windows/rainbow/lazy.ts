// The rainbow-race windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports them dynamically. One class per line (the vitest transform `lazyWndSync` reads this file line by line).
import { LazyWnd } from '../../wnd/lazy';

import type * as Offer from './WndRainbowRaceOffer';
import type * as Panel from './WndRainbowRacePanel';
import type * as Ranking from './WndRainbowRaceRanking';
import type * as RaceButton from './WndRainbowRaceButton';
import type * as Progress from './WndRainbowRaceMiniGame';
import type * as KawiBawiBo from './WndRRMiniGameKawiBawiBo';
import type * as Dice from './WndRRMiniGameDice';
import type * as Arithmetic from './WndRRMiniGameArithmetic';
import type * as StopWatch from './WndRRMiniGameStopWatch';
import type * as Typing from './WndRRMiniGameTyping';
import type * as Card from './WndRRMiniGameCard';
import type * as Ladder from './WndRRMiniGameLadder';

export const RAINBOW_LAZY = {
  WndRainbowRaceOffer: new LazyWnd<typeof Offer.WndRainbowRaceOffer>(
    'WndRainbowRaceOffer',
    async () => (await import('./WndRainbowRaceOffer')).WndRainbowRaceOffer,
  ),
  WndRainbowRacePanel: new LazyWnd<typeof Panel.WndRainbowRacePanel>(
    'WndRainbowRacePanel',
    async () => (await import('./WndRainbowRacePanel')).WndRainbowRacePanel,
  ),
  WndRainbowRaceRanking: new LazyWnd<typeof Ranking.WndRainbowRaceRanking>(
    'WndRainbowRaceRanking',
    async () => (await import('./WndRainbowRaceRanking')).WndRainbowRaceRanking,
  ),
  WndRainbowRaceButton: new LazyWnd<typeof RaceButton.WndRainbowRaceButton>(
    'WndRainbowRaceButton',
    async () => (await import('./WndRainbowRaceButton')).WndRainbowRaceButton,
  ),
  WndRainbowRaceMiniGame: new LazyWnd<typeof Progress.WndRainbowRaceMiniGame>(
    'WndRainbowRaceMiniGame',
    async () => (await import('./WndRainbowRaceMiniGame')).WndRainbowRaceMiniGame,
  ),
  WndRRMiniGameKawiBawiBo: new LazyWnd<typeof KawiBawiBo.WndRRMiniGameKawiBawiBo>(
    'WndRRMiniGameKawiBawiBo',
    async () => (await import('./WndRRMiniGameKawiBawiBo')).WndRRMiniGameKawiBawiBo,
  ),
  WndRRMiniGameDice: new LazyWnd<typeof Dice.WndRRMiniGameDice>(
    'WndRRMiniGameDice',
    async () => (await import('./WndRRMiniGameDice')).WndRRMiniGameDice,
  ),
  WndRRMiniGameArithmetic: new LazyWnd<typeof Arithmetic.WndRRMiniGameArithmetic>(
    'WndRRMiniGameArithmetic',
    async () => (await import('./WndRRMiniGameArithmetic')).WndRRMiniGameArithmetic,
  ),
  WndRRMiniGameStopWatch: new LazyWnd<typeof StopWatch.WndRRMiniGameStopWatch>(
    'WndRRMiniGameStopWatch',
    async () => (await import('./WndRRMiniGameStopWatch')).WndRRMiniGameStopWatch,
  ),
  WndRRMiniGameTyping: new LazyWnd<typeof Typing.WndRRMiniGameTyping>(
    'WndRRMiniGameTyping',
    async () => (await import('./WndRRMiniGameTyping')).WndRRMiniGameTyping,
  ),
  WndRRMiniGameCard: new LazyWnd<typeof Card.WndRRMiniGameCard>(
    'WndRRMiniGameCard',
    async () => (await import('./WndRRMiniGameCard')).WndRRMiniGameCard,
  ),
  WndRRMiniGameLadder: new LazyWnd<typeof Ladder.WndRRMiniGameLadder>(
    'WndRRMiniGameLadder',
    async () => (await import('./WndRRMiniGameLadder')).WndRRMiniGameLadder,
  ),
} as const;
