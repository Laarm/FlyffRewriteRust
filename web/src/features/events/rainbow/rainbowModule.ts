// The `rainbow` feature (Plan/45 §28.7, §28.8; WP-CFEAT-44; server half `event/rainbow/`): the weekly rainbow race with its seven mini-games.
//
//   messages     `RainbowRaceNowState` -> `eventStore.rainbow`; `RainbowRaceMiniGameState` / `...ExtState` -> `reduceMiniGame` effects (open / deliver / close / freezeProgress / endBox / box);
//                `RainbowRaceApplicationOpenWndNtf` -> the offer window, `RainbowRacePrevRankingOpenWndNtf` -> the ranking window + `playerDataOf( ctx ).needAll`; every message reduces whether or not a window is open
//   windows      `ui.rainbow` (`ui/windows/rainbow`): requests with callbacks that name the App. B sends; the panels come from the help bundle (`withPanels`)
//   menu         107-119 through `menuOf( ctx ).registerAction`; hidden by the client while event 25 is off (`features/npc/menu.ts`), refused silently here as well
//   lifecycle    the race button exists while `racing && flag 25 on`; the all-done box once; `{ 0, 0, 0 }` closes the button and the progress list (`reduceLifecycle`)
//   probe        `__flyffDebug.features.rainbow()`, `rainbowStopwatchPin( ms )` (test pages)
//
// The flag gate (lead's note, open question 1): entry points only - the NPC entries, the window-opening messages and the button; the state messages always reduce.

import type { ClientContext } from '../../../app/context';
import {
  APP_RAINBOWRACE_BUTTON,
  APP_RAINBOWRACE_INFO,
  APP_RAINBOWRACE_PRIZE,
  APP_RAINBOWRACE_RULE,
} from '../../../gen/defs/resdata';
import {
  GAME_PANELS,
  PANEL_INFO,
  PANEL_OFFER,
  PANEL_PRIZE,
  PANEL_RULE,
  RMG,
} from '../../../ui/windows/rainbow/constants';
import type { ProgressRequest } from '../../../ui/windows/rainbow/ports';
import { fullUi, type FeatureContext } from '../../common/module';
import { playerDataOf } from '../../common/playerData';
import { menuOf } from '../../npc/menuModule';
import { type EventFeature, eventOf } from '../api';
import { registerEvents } from '../eventsModule';
import type { EventStore } from '../store';
import { EVENT_RAINBOW_RACE, RAINBOW_MENU_IDS, rainbowMenuTable } from './menu';
import { registerRainbowProbe } from './probe';
import {
  endBoxClosed,
  type GameCall,
  type GameInit,
  type LifecycleEffect,
  type MiniGameMsg,
  reduceLifecycle,
  reduceMiniGame,
  reduceNowState,
  type RainbowEffect,
  rankingIds,
} from './reducers';
import { rainbowSenders, type RainbowSenders } from './senders';
import { initialRainbow, racingOf, type RainbowSlice } from './state';
import { gameKeyOf } from '../../../ui/windows/rainbow/glow';

/** The public face of the rainbow feature of a context. */
export interface RainbowFeature {
  readonly store: EventStore;
  readonly senders: RainbowSenders;
  /** The slice now. */
  state(): RainbowSlice;
  /** `m_dwRemainTime != 0`. */
  racing(): boolean;
  /** `CRainbowRace::GetGameKey()`: the lowest set bit of the current game, -1 for none. */
  gameKey(): number;
  /** The finished games (bit per game). */
  completed(): number;
  /** `g_eLocal.GetState( 25 )`. */
  flagOn(): boolean;
  /** The `APP_*` of the rainbow windows open now. */
  windows(): number[];
  /** Runs the lifecycle of the race button now (a store change does it too). */
  evaluate(): void;
}

const features = new WeakMap<ClientContext, RainbowFeature>();

/** The rainbow feature of a context; `undefined` before the module registered. */
export function rainbowOf(ctx: ClientContext): RainbowFeature | undefined {
  return features.get(ctx);
}

/** Registers the rainbow race (after the events feature: `rainbowM7.ts` registers the groups under the one handler module). */
export function registerRainbow(fc: FeatureContext): RainbowFeature {
  const ctx = fc.app;
  const known = rainbowOf(ctx);
  if (known) return known;
  const events: EventFeature = eventOf(ctx) ?? registerEvents(fc);
  const store = events.store;
  const playerData = playerDataOf(ctx);
  const senders = rainbowSenders((msg) => ctx.net.sender.send(msg));
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const flagOn = (): boolean => events.isEventOn(EVENT_RAINBOW_RACE);
  const slice = (): RainbowSlice => store.getState().rainbow;
  const setSlice = (next: RainbowSlice): void => {
    if (next !== store.getState().rainbow) store.setState({ rainbow: next });
  };
  let pin: number | null = null;

  /** The panels of the lazy help bundle, loading it first when needed. */
  const withPanels = (fn: (panels: Readonly<Record<string, string>>) => void): void => {
    const have = ctx.data?.bundle('help');
    if (have) {
      fn(have.panels);
      return;
    }
    ctx.bundles.ensure('help').then(
      () => {
        fn(ctx.data?.bundle('help')?.panels ?? {});
      },
      () => {
        fn({});
      },
    );
  };

  /** `SendMinigamePacket( RMG_MAX, MP_NONE, p1, p2 )`: the STORED game. */
  const play = (p1: number, p2: number): void => {
    senders.miniGameState(slice().nowGame, 0, p1, p2);
  };

  /** The end box of a game; when it goes `m_bCheckEnd` is set (the lifecycle then shows the all-done box). */
  const openEnd = (game: number): void => {
    fullUi(ctx)?.rainbow.openEndBox(game, () => {
      setSlice(endBoxClosed(slice()));
    });
  };

  // ---- the windows ---------------------------------------------------------------------------------------------------------------

  const panelWindow = (
    app: typeof APP_RAINBOWRACE_INFO | typeof APP_RAINBOWRACE_RULE | typeof APP_RAINBOWRACE_PRIZE,
    key: string,
  ): void => {
    withPanels((panels) => {
      fullUi(ctx)?.rainbow.openPanel(app, { text: panels[key] ?? '' });
    });
  };

  const progressRequest = (): ProgressRequest => ({
    state: () => ({ nowGame: slice().nowGame, completed: slice().completed }),
    info: (game) => {
      withPanels((panels) => {
        fullUi(ctx)?.rainbow.openInfo({ game, text: panels[GAME_PANELS[game] ?? ''] ?? '' });
      });
    },
  });

  const toggleProgress = (): void => {
    const ui = fullUi(ctx)?.rainbow;
    if (!ui) return;
    if (ui.isProgressOpen()) ui.closeProgress();
    else ui.openProgress(progressRequest());
  };

  /** Opens the window of a game with what the OPENWND carried (the request applies it while the window is made). */
  const openGame = (game: number, init: GameInit): void => {
    withPanels((panels) => {
      const ui = fullUi(ctx)?.rainbow;
      if (!ui) return;
      const base = {
        text: panels[GAME_PANELS[game] ?? ''] ?? '',
        cue: (e: 'tick' | 'win') => {
          ctx.audio.cues?.miniGame(e);
        },
        now: () => ctx.now(),
      };
      const end = (): void => {
        openEnd(game);
      };
      switch (game) {
        case RMG.KAWIBAWIBO:
          ui.openKawiBawiBo({
            ...base,
            play: () => {
              play(0, 0);
            },
            end,
          });
          break;
        case RMG.DICE:
          ui.openDice({
            ...base,
            target: init.target ?? 0,
            roll: () => {
              play(0, 0);
            },
            end,
          });
          break;
        case RMG.ARITHMETIC:
          ui.openArithmetic({
            ...base,
            question: init.question ?? '',
            count: init.count ?? 0,
            answer: (n) => {
              play(n, 0);
            },
          });
          break;
        case RMG.STOPWATCH:
          ui.openStopwatch({
            ...base,
            target: init.target ?? 0,
            send: (ms) => {
              play(ms, 0);
            },
            pinned: () => pin,
          });
          break;
        case RMG.TYPING:
          ui.openTyping({
            ...base,
            questions: init.questions ?? [],
            submit: (answers) => {
              senders.miniGameExtState(slice().nowGame, 0, 0, 0, answers);
            },
          });
          break;
        case RMG.CARD:
          ui.openCard({
            ...base,
            deck: init.deck ?? '',
            pick: (a, b) => {
              play(a, b);
            },
          });
          break;
        default:
          ui.openLadder({
            ...base,
            roll: () => {
              play(0, 0);
            },
            end,
          });
      }
    });
  };

  /** Calls the open window of a game; nothing happens when it is not open. */
  const deliver = (game: number, call: GameCall): void => {
    const ui = fullUi(ctx)?.rainbow;
    if (!ui) return;
    switch (call.call) {
      case 'result':
        if (game === RMG.KAWIBAWIBO) ui.kawiBawiBo()?.result(call.state);
        else if (game === RMG.CARD) ui.card()?.result(call.state);
        else if (game === RMG.LADDER) ui.ladder()?.result(call.state);
        break;
      case 'diceResult':
        ui.dice()?.result(call.state, call.p1, call.p2);
        break;
      case 'setQuestion':
        ui.arithmetic()?.setQuestion(call.question, call.count);
        break;
      case 'setTarget':
        ui.stopwatch()?.setTarget(call.ms);
        break;
      case 'typingReset':
        ui.typing()?.reset();
        break;
    }
  };

  const run = (effects: readonly RainbowEffect[]): void => {
    const ui = fullUi(ctx);
    for (const e of effects) {
      switch (e.kind) {
        case 'open':
          // the window-opening messages open nothing while the event is off
          if (flagOn()) openGame(e.game, e.init);
          break;
        case 'deliver':
          deliver(e.game, e.call);
          break;
        case 'close':
          ui?.rainbow.closeGame(e.game);
          break;
        case 'freezeProgress':
          ui?.rainbow.freezeProgress();
          break;
        case 'endBox':
          openEnd(e.game);
          break;
        case 'box':
          ui?.msgbox.open(text(e.tid));
          break;
      }
    }
  };

  // ---- the race button -----------------------------------------------------------------------------------------------------------

  const openProgressFromButton = (): void => {
    toggleProgress();
  };
  const runLifecycleEffect = (e: LifecycleEffect): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    switch (e.kind) {
      case 'openButton':
        ui.rainbow.openButton({ onClick: openProgressFromButton });
        break;
      case 'closeButton':
        ui.rainbow.closeButton();
        break;
      case 'closeProgress':
        ui.rainbow.closeProgress();
        break;
      case 'box':
        ui.msgbox.open(text(e.tid));
        break;
    }
  };
  let evaluating = false;
  const evaluate = (): void => {
    if (evaluating) return;
    evaluating = true;
    try {
      // the slice may change while the effects run (a box closing): repeat until it is stable
      for (let i = 0; i < 8; i += 1) {
        const ui = fullUi(ctx);
        const cur = slice();
        const r = reduceLifecycle(cur, {
          flagOn: flagOn(),
          buttonOpen: ui?.rainbow.isOpen(APP_RAINBOWRACE_BUTTON) ?? false,
        });
        if (r.slice !== cur) store.setState({ rainbow: r.slice });
        for (const e of r.effects) runLifecycleEffect(e);
        if (r.slice === cur) break;
      }
    } finally {
      evaluating = false;
    }
  };
  store.subscribe((s, prev) => {
    if (s.rainbow !== prev.rainbow || s.flags !== prev.flags) evaluate();
  });

  // ---- the messages ----------------------------------------------------------------------------------------------------------------

  fc.net.on('rainbowRaceNowState', (m) => {
    const race = m.race;
    setSlice(
      reduceNowState(
        slice(),
        {
          nowGame: race?.nowGame ?? 0,
          completed: race?.completedMask ?? 0,
          remainMs: race?.remainMs ?? 0,
        },
        ctx.now(),
      ),
    );
  });
  const miniGame = (m: MiniGameMsg): void => {
    run(reduceMiniGame(slice(), m));
  };
  fc.net.on('rainbowRaceMiniGameState', (m) => {
    const p = m.packet;
    if (!p) return;
    miniGame({ nowGame: p.nowGame, state: p.state, p1: p.param1, p2: p.param2, data: [] });
  });
  fc.net.on('rainbowRaceMiniGameExtState', (m) => {
    const p = m.packet?.packet;
    if (!p) return;
    miniGame({
      nowGame: p.nowGame,
      state: p.state,
      p1: p.param1,
      p2: p.param2,
      data: m.packet?.data ?? [],
    });
  });
  fc.net.on('rainbowRaceApplicationOpenWndNtf', (m) => {
    if (!flagOn()) return;
    withPanels((panels) => {
      fullUi(ctx)?.rainbow.openOffer({
        count: m.applicants,
        text: panels[PANEL_OFFER] ?? '',
        apply: () => {
          senders.application();
        },
      });
    });
  });
  fc.net.on('rainbowRacePrevRankingOpenWndNtf', (m) => {
    if (!flagOn()) return;
    const ids = rankingIds(m.idPlayers);
    playerData.needAll(ids.filter((id) => id !== 0));
    fullUi(ctx)?.rainbow.openRanking({
      ids,
      player: (id) => {
        const e = playerData.get(id);
        if (!e) return null;
        return {
          name: e.name,
          jobType: ctx.data?.job(e.job)?.type ?? 0,
          job: e.job,
          level: e.level,
        };
      },
    });
  });

  // the racer leaves the world: nothing is left of the race (`WndManager.cpp:2164`); a world change keeps it
  ctx.machine.subscribe((screen, previous) => {
    if (previous.s !== 'InGame') return;
    if (screen.s === 'InGame' || screen.s === 'Reconnecting') return;
    if (screen.s === 'Loading' || screen.s === 'Entering') return;
    store.setState({ rainbow: initialRainbow() });
    fullUi(ctx)?.rainbow.closeAll();
  });

  // ---- the NPC menu ----------------------------------------------------------------------------------------------------------------

  const table = rainbowMenuTable({
    applicationOpenWnd: () => {
      senders.applicationOpenWnd();
    },
    openInfo: () => {
      panelWindow(APP_RAINBOWRACE_INFO, PANEL_INFO);
    },
    openRule: () => {
      panelWindow(APP_RAINBOWRACE_RULE, PANEL_RULE);
    },
    prevRankingOpenWnd: () => {
      senders.prevRankingOpenWnd();
    },
    openPrize: () => {
      panelWindow(APP_RAINBOWRACE_PRIZE, PANEL_PRIZE);
    },
    reqFinish: () => {
      senders.reqFinish();
    },
    openGame: (game, state) => {
      senders.miniGameState(1 << game, state, 0, 0);
    },
  });
  for (const id of RAINBOW_MENU_IDS) {
    const fn = table.get(id);
    if (!fn) continue;
    menuOf(ctx)?.registerAction(id, () => {
      if (!flagOn()) {
        ctx.log.debug(
          'events',
          `rainbow menu ${String(id)} refused: event ${String(EVENT_RAINBOW_RACE)} is off`,
        );
        return;
      }
      fn();
    });
  }

  const feature: RainbowFeature = {
    store,
    senders,
    state: slice,
    racing: () => racingOf(slice()),
    gameKey: () => gameKeyOf(slice().nowGame),
    completed: () => slice().completed,
    flagOn,
    windows: () => fullUi(ctx)?.rainbow.openApps() ?? [],
    evaluate,
  };
  features.set(ctx, feature);
  registerRainbowProbe(ctx, events, slice, (ms) => {
    pin = ms;
  });
  return feature;
}
