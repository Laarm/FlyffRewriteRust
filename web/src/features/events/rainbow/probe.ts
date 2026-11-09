// The probe of the rainbow race (Plan/45 §2.6): `__flyffDebug.features.rainbow()` reads the slice, the lifecycle and the open game window; `rainbowStopwatchPin( ms | null )` exists on test pages only
// (`?e2e=1`, dev builds) and makes a running stopwatch report that elapsed time.

import type { ClientContext } from '../../../app/context';
import {
  APP_MESSAGEBOX,
  APP_RAINBOWRACE_BUTTON,
  APP_RAINBOWRACE_INFO,
  APP_RAINBOWRACE_PRIZE,
  APP_RAINBOWRACE_RULE,
} from '../../../gen/defs/resdata';
import { MessageBoxWnd } from '../../../ui/msgbox/MessageBoxWnd';
import type { RainbowWindowsService } from '../../../ui/windows/rainbow';
import { gameKeyOf } from '../../../ui/windows/rainbow/glow';
import { fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import type { EventFeature } from '../api';
import { EVENT_RAINBOW_RACE } from './menu';
import { racingOf, type RainbowSlice } from './state';

/** The game window facts the probe reports (`app` always, the rest by game). */
export interface ProbeGame {
  app: number;
  game: number;
  target?: number;
  question?: string;
  count?: number;
  wins?: number;
  deck?: string;
  matched?: number[];
  penaltyLeftMs?: number | null;
  running?: boolean;
  elapsed?: number;
  ladder?: { start: number; target: number; result: number };
}

export function registerRainbowProbe(
  ctx: ClientContext,
  events: EventFeature,
  slice: () => RainbowSlice,
  setPin: (ms: number | null) => void,
): void {
  addProbeMember(ctx, 'rainbow', () => {
    const s = slice();
    const ui = fullUi(ctx)?.rainbow;
    return {
      nowGame: s.nowGame,
      completed: s.completed,
      endMs: s.endMs ?? null,
      racing: racingOf(s),
      gameKey: gameKeyOf(s.nowGame),
      flagOn: events.isEventOn(EVENT_RAINBOW_RACE),
      button: ui?.isOpen(APP_RAINBOWRACE_BUTTON) ?? false,
      progressFrozen: ui?.progress()?.frozen ?? false,
      checkEnd: s.checkEnd,
      finishShown: s.finishShown,
      endBox: ui?.endBoxText() ?? null,
      failBox: ui?.ladderFailText() ?? null,
      game: gameFacts(ctx, ui, gameKeyOf(s.nowGame)),
    };
  });
  // the plain texts of the open windows (what Playwright reads instead of pixels)
  addProbeMember(ctx, 'rainbowWindows', () => {
    const ui = fullUi(ctx)?.rainbow;
    const ranking = ui?.ranking();
    const progress = ui?.progress();
    return {
      offer: ui?.offer()?.textCtrl?.getString() ?? null,
      info: ui?.panel(APP_RAINBOWRACE_INFO)?.textCtrl?.getString() ?? null,
      rule: ui?.panel(APP_RAINBOWRACE_RULE)?.textCtrl?.getString() ?? null,
      prize: ui?.panel(APP_RAINBOWRACE_PRIZE)?.textCtrl?.getString() ?? null,
      gameInfo: ui?.info()?.textCtrl?.getString() ?? null,
      // the message boxes: the main one (`OpenMessageBox`) and the upper one (`Text{ kind 2 }`: the answer of the Officer, `TID_GAME_RAINBOWRACE_FINISH`)
      mainBox: boxOf(ctx, 'main'),
      upperBox: boxOf(ctx, 'upper'),
      ranking: ranking ? (ranking.textLayer?.items.map((i) => i.text) ?? []) : null,
      progress: progress
        ? {
            current: progress.current,
            done: [0, 1, 2, 3, 4, 5, 6].filter((g) => progress.completed(g)),
            frozen: progress.frozen,
          }
        : null,
    };
  });
  if (ctx.probe.test !== undefined) {
    addProbeMember(ctx, 'rainbowStopwatchPin', (ms: number | null) => {
      setPin(ms);
    });
  }
}

function boxOf(ctx: ClientContext, key: string): string | null {
  const w = fullUi(ctx)?.wm.get(APP_MESSAGEBOX, key);
  return w instanceof MessageBoxWnd ? w.message : null;
}

function gameFacts(
  ctx: ClientContext,
  ui: RainbowWindowsService | undefined,
  game: number,
): ProbeGame | null {
  if (!ui || game < 0) return null;
  switch (game) {
    case 0: {
      const w = ui.kawiBawiBo();
      return w ? { app: 795, game, wins: w.wins } : null;
    }
    case 1: {
      const w = ui.dice();
      return w ? { app: 796, game, target: w.target, running: w.roll.rolling } : null;
    }
    case 2: {
      const w = ui.arithmetic();
      return w ? { app: 797, game, question: w.question, count: w.correctCount } : null;
    }
    case 3: {
      const w = ui.stopwatch();
      return w
        ? { app: 798, game, target: w.target, running: w.clock.running, elapsed: w.clock.shownMs }
        : null;
    }
    case 4: {
      const w = ui.typing();
      return w
        ? { app: 799, game, question: [0, 1, 2].map((i) => w.questionText(i)).join('|') }
        : null;
    }
    case 5: {
      const w = ui.card();
      if (!w) return null;
      const matched = w.cards.matched.flatMap((m, i) => (m ? [i] : []));
      const at = w.cards.penaltyAt;
      return {
        app: 800,
        game,
        deck: w.cards.kinds.map((k) => String(k)).join(','),
        matched,
        penaltyLeftMs: at === null ? null : Math.max(0, 5000 - (ctx.now() - at)),
      };
    }
    default: {
      const w = ui.ladder();
      return w
        ? { app: 801, game, ladder: { start: w.start, target: w.target, result: w.resultState } }
        : null;
    }
  }
}
