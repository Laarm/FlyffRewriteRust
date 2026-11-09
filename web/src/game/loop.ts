// GameLoop (Plan/40 section 10, WP-CLIENT-10): rAF + a fixed 60 Hz logic step derived from the wall clock.
//
// The loop does not accumulate `dt`. It reads the wall clock, converts it to the 60 Hz client frame counter
// (`clientFrameAt`, CA9) and steps the sim by the difference `due` to its own frame counter: a 144 Hz display
// renders two or three times per step (the interpolation factor `alpha` smooths movers and camera), a 30 Hz
// display steps twice per render. At most `MAX_STEPS_PER_ITERATION` (one second) steps run per iteration; any
// backlog beyond that is dropped and added to `frameSkew` (the dropped frames never happen in the sim; the sim
// bridge re-sends `SET_CLOCK`). The iteration order is the one of section 10.2; all calls into 31/41-44 go
// through the ports of `game/ports.ts`.

import type { Logger } from '../platform/log';
import { clientFrameAt, FRAME_MS, now as platformNow } from '../platform/time';
import type { Unsubscribe } from '../platform/visibility';
import { FrameTimeRing, SectionTimes, SlidingSum } from './frameStats';
import type { LoopPorts, SimFrameResult } from './ports';
import type { TickSystems } from './tickSystems';

export { FRAME_MS };

/** One second of backlog; beyond it frames are dropped (ref/21 section 5.2, 31 `MAX_STEPS_PER_CALL`). */
export const MAX_STEPS_PER_ITERATION = 60;

/** Frame budget of one iteration on the main thread (section 15). */
export const DEFAULT_BUDGET_MS = 8;

/** Iterations above this, three in a row while the backlog cap is hit, mean the device is too slow (10.4). */
export const OVERLOAD_ITERATION_MS = 50;

/** `requestAnimationFrame` seam (tests drive iterations by hand). */
export interface FrameScheduler {
  request(cb: (nowMs: number) => void): number;
  cancel(handle: number): void;
}

/** The browser scheduler (resolved lazily so importing the module needs no DOM). */
export const rafScheduler: FrameScheduler = {
  request: (cb) => requestAnimationFrame(cb),
  cancel: (handle) => {
    cancelAnimationFrame(handle);
  },
};

export type LoopSection =
  | 'net'
  | 'input'
  | 'sim'
  | 'events'
  | 'systems'
  | 'report'
  | 'pose'
  | 'send'
  | 'render'
  | 'publish';

const SECTIONS: readonly LoopSection[] = [
  'net',
  'input',
  'sim',
  'events',
  'systems',
  'report',
  'pose',
  'send',
  'render',
  'publish',
];

export interface LoopDeps {
  /** Step 1 and 9: drains the inbound queue (handlers run here) and flushes the sender. */
  readonly net: { drain(): number; readonly sender: { flush(): void } };
  /** Step 11 (rendered iterations only). */
  readonly publisher: { tick(nowMs: number): void };
  readonly tickSystems: TickSystems;
  /** Resolved once per iteration so that a renderer or UI installed later is picked up. */
  readonly ports: () => LoopPorts;
  readonly now?: () => number;
  readonly scheduler?: FrameScheduler;
  readonly log?: Pick<Logger, 'warn' | 'error'>;
  /** Render interpolation option (45's `optionsStore`, default on); off = `alpha = 1` (PL-009). */
  readonly interpolation?: () => boolean;
  /** Per-iteration CPU budget in ms for `stats.overBudgetFrames` (default 8). */
  readonly budgetMs?: number;
  /** Called once when the device is detected as too slow (section 10.4). */
  readonly onOverload?: () => void;
}

/** What the last iteration did (a reused object: read it right after `iterate`). */
export interface IterationInfo {
  readonly nowMs: number;
  /** Frames asked from the sim (after the backlog cap). */
  readonly due: number;
  /** Frames the sim actually stepped. */
  readonly stepped: number;
  readonly rendered: boolean;
  /** Interpolation factor used by this iteration (`1` for non-rendered stepping iterations). */
  readonly alpha: number;
  /** Frames dropped by the backlog cap in this iteration. */
  readonly dropped: number;
}

export interface LoopStats {
  /** Frames the sim has stepped since the loop started (plus its initial frame). */
  readonly simFrame: number;
  /** The wall-clock 60 Hz counter at the last iteration. */
  readonly clientFrame: number;
  /** `clientFrame - simFrame`: grows only when backlog was dropped (CA9). */
  readonly frameSkew: number;
  /** Steps per second over the last second. */
  readonly stepsPerSec: number;
  /** Rendered frames per second over the last second. */
  readonly fps: number;
  readonly droppedSteps: number;
  readonly hidden: boolean;
  /** CPU time of the last iteration in ms. */
  readonly lastFrameMs: number;
  /**
   * The interpolation factor of the last RENDERED iteration (`[0, 1)`; `1` while the interpolation option is off, PL-009,
   * and before the first rendered frame). Background-pump iterations do not change it.
   */
  readonly alpha: number;
  readonly iterations: number;
  readonly renderedFrames: number;
  /** Exceptions caught per step name (rule L3). */
  readonly stepErrors: Readonly<Record<string, number>>;
  /** Total caught exceptions (the watchdog counts them, section 12.1). */
  readonly errors: number;
  readonly overloaded: boolean;
  /** Iterations above the CPU budget. */
  readonly overBudgetFrames: number;
  /** Last and maximum time of each section of the iteration (frame budget probes). */
  readonly sections: Readonly<Record<LoopSection, { last: number; max: number }>>;
}

export interface GameLoop {
  /** Starts the rAF loop (idempotent). */
  start(): void;
  /** Stops it; `start()` resumes (the sim frame counter keeps its value, the next iteration catches up or drops). */
  stop(): void;
  /** `stop()` + `start()` (watchdog L1: the loop died, section 12.1). */
  restart(): void;
  readonly running: boolean;
  /**
   * One iteration (the rAF callback body). The background pump calls it with `render = false`: steps 0-9 of
   * section 10.2 run, `render.tick`, the camera, the upload queue, the draw, the UI and the publisher do not.
   */
  iterate(nowMs: number, render: boolean): IterationInfo;
  /** Receives the sim events of every iteration (step 5: 41, 43, 44, 17 and the reporters subscribe here). */
  onSimEvents(listener: (events: readonly unknown[]) => void): Unsubscribe;
  /** Marks the tab as hidden/visible (informational: the pump decides whether to render). */
  setHidden(hidden: boolean): void;
  /** `nowMs` of the last iteration start (watchdog heartbeat, section 12.1). */
  readonly heartbeat: number;
  /** Durations of the last rAF frames (`__flyffDebug.loop().frameTimes`). */
  readonly frameTimes: FrameTimeRing;
  readonly stats: LoopStats;
}

const ORIGIN: readonly [number, number, number] = [0, 0, 0];
const EMPTY_RESULT: SimFrameResult = { stepped: 0, viewFresh: false, view: null, events: [] };

export function createGameLoop(deps: LoopDeps): GameLoop {
  const time = deps.now ?? platformNow;
  const scheduler = deps.scheduler ?? rafScheduler;
  const budgetMs = deps.budgetMs ?? DEFAULT_BUDGET_MS;
  const interpolation = deps.interpolation ?? (() => true);

  const frameTimes = new FrameTimeRing(600);
  const stepsWindow = new SlidingSum(1000, 1024);
  const fpsWindow = new SlidingSum(1000, 1024);
  const sections = new SectionTimes<LoopSection>(SECTIONS);
  const eventListeners = new Set<(events: readonly unknown[]) => void>();
  const stepErrors: Record<string, number> = {};

  let running = false;
  let handle = 0;
  let initialised = false;
  let simFrame = 0;
  let frameSkew = 0;
  let droppedSteps = 0;
  let iterations = 0;
  let renderedFrames = 0;
  let errors = 0;
  let hidden = false;
  let heartbeat = 0;
  let lastNow = 0;
  let lastFrameMs = 0;
  let renderedAlpha = 1;
  let lastRenderAt: number | null = null;
  let overBudgetFrames = 0;
  let slowStreak = 0;
  let overloaded = false;

  const info = { nowMs: 0, due: 0, stepped: 0, rendered: false, alpha: 1, dropped: 0 };

  const fail = (step: string, error: unknown): void => {
    errors += 1;
    const n = (stepErrors[step] ?? 0) + 1;
    stepErrors[step] = n;
    // the first failures are logged, then one line per 600 (a step that throws every frame must not flood)
    if (deps.log && (n <= 3 || n % 600 === 0)) {
      deps.log.error(
        'loop',
        `step ${step} failed (${String(n)}x): ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  };

  let lapStart = 0;
  const lap = (section: LoopSection): void => {
    const t = time();
    sections.record(section, t - lapStart);
    lapStart = t;
  };
  const onSystemError = (name: string, error: unknown): void => {
    fail(`system:${name}`, error);
  };

  const iterate = (nowMs: number, render: boolean): IterationInfo => {
    const started = time();
    heartbeat = nowMs;
    lastNow = nowMs;
    lapStart = started;
    const ports = deps.ports();

    // 0. due and alpha
    const target = clientFrameAt(nowMs);
    if (!initialised) {
      initialised = true;
      simFrame = target;
    }
    let due = target - (simFrame + frameSkew);
    let dropped = 0;
    if (due < 0) due = 0;
    if (due > MAX_STEPS_PER_ITERATION) {
      dropped = due - MAX_STEPS_PER_ITERATION;
      due = MAX_STEPS_PER_ITERATION;
      frameSkew += dropped;
      droppedSteps += dropped;
      try {
        ports.sim.setFrameSkew(frameSkew);
      } catch (error) {
        fail('skew', error);
      }
    }
    const frames = (nowMs * 60) / 1000;
    const fraction = frames - Math.floor(frames);
    const alpha = render ? (interpolation() ? fraction : 1) : 1;

    // 1. net drain: handlers, entity sweep, drain-end hooks
    try {
      deps.net.drain();
    } catch (error) {
      fail('net', error);
    }
    lap('net');

    // 2. input
    let record: unknown = null;
    try {
      ports.input.flushImmediate();
      if (due >= 1) record = ports.input.worldTick?.(nowMs) ?? null;
    } catch (error) {
      fail('input', error);
    }
    lap('input');

    // 4. sim frame (3, the sim loads, belongs to the sim bridge)
    let result = EMPTY_RESULT;
    try {
      result = ports.sim.frame(due, record, render ? alpha : due >= 1 ? 1.0 : null);
    } catch (error) {
      fail('sim', error);
    }
    const stepped = Math.max(0, Math.min(result.stepped, due));
    simFrame += stepped;
    lap('sim');

    // 5. sim events
    if (result.events.length > 0) {
      for (const listener of eventListeners) {
        try {
          listener(result.events);
        } catch (error) {
          fail('events', error);
        }
      }
    }
    lap('events');

    // 6. per-step systems: advance `due` steps, never per rendered frame
    if (due >= 1) {
      if (render) {
        try {
          ports.render?.tick(due);
        } catch (error) {
          fail('render.tick', error);
        }
      }
      try {
        for (let i = 0; i < due; i += 1) ports.audio.tick();
      } catch (error) {
        fail('audio.tick', error);
      }
      deps.tickSystems.run(due, onSystemError);
    }
    lap('systems');

    // 7. move reporter
    if (stepped >= 1) {
      try {
        ports.moveReporter?.afterStep(result.view);
      } catch (error) {
        fail('moveReporter', error);
      }
    }
    lap('report');

    // 8. pose feeder
    if (result.viewFresh) {
      try {
        ports.poseFeeder?.scan(result.view);
      } catch (error) {
        fail('poseFeeder', error);
      }
    }
    lap('pose');

    // 9. flush the sender
    try {
      deps.net.sender.flush();
    } catch (error) {
      fail('send', error);
    }
    lap('send');

    // 10. render
    if (render) {
      let camera: ReturnType<NonNullable<typeof ports.input.camera>> = null;
      try {
        camera = ports.input.camera?.(due, alpha, result.view) ?? null;
        ports.input.hover?.();
      } catch (error) {
        fail('camera', error);
      }
      try {
        ports.audio.setListener(camera?.listener ?? ORIGIN, camera?.yaw ?? 0);
      } catch (error) {
        fail('audio.setListener', error);
      }
      try {
        ports.uploads?.runFrame();
      } catch (error) {
        fail('uploads', error);
      }
      try {
        ports.render?.draw({ view: result.view, camera: camera?.camera ?? null, nowMs, alpha });
      } catch (error) {
        fail('render.draw', error);
      }
      try {
        const dt = lastRenderAt === null ? 0 : Math.min(100, Math.max(0, nowMs - lastRenderAt));
        ports.ui?.process(dt);
      } catch (error) {
        fail('ui', error);
      }
      lap('render');

      // 11. publish continuous values to the stores
      try {
        deps.publisher.tick(nowMs);
      } catch (error) {
        fail('publish', error);
      }
      lap('publish');

      if (lastRenderAt !== null) frameTimes.record(nowMs - lastRenderAt);
      lastRenderAt = nowMs;
      renderedFrames += 1;
      renderedAlpha = alpha;
      fpsWindow.add(nowMs, 1);
    }

    // 12. the boundary-call budget of this iteration is complete (Plan/40 section 7.3), then the statistics
    try {
      ports.sim.endFrame?.();
    } catch (error) {
      fail('sim.endFrame', error);
    }
    iterations += 1;
    stepsWindow.add(nowMs, stepped);
    const elapsed = time() - started;
    lastFrameMs = elapsed;
    if (elapsed > budgetMs) overBudgetFrames += 1;
    if (elapsed > OVERLOAD_ITERATION_MS && due >= MAX_STEPS_PER_ITERATION) {
      slowStreak += 1;
      if (slowStreak >= 3 && !overloaded) {
        overloaded = true;
        deps.onOverload?.();
      }
    } else {
      slowStreak = 0;
    }

    info.nowMs = nowMs;
    info.due = due;
    info.stepped = stepped;
    info.rendered = render;
    info.alpha = alpha;
    info.dropped = dropped;
    return info;
  };

  const onFrame = (nowMs: number): void => {
    try {
      // a hidden tab never renders (section 10.5): a real hidden tab has no rAF at all, `test.simulateHidden` keeps it firing
      iterate(nowMs, !hidden);
    } finally {
      if (running) handle = scheduler.request(onFrame);
    }
  };

  const start = (): void => {
    if (running) return;
    running = true;
    handle = scheduler.request(onFrame);
  };
  const stop = (): void => {
    if (!running) return;
    running = false;
    scheduler.cancel(handle);
  };

  return {
    start,
    stop,
    restart() {
      stop();
      start();
    },
    get running() {
      return running;
    },
    iterate,
    onSimEvents(listener) {
      eventListeners.add(listener);
      return () => {
        eventListeners.delete(listener);
      };
    },
    setHidden(value) {
      hidden = value;
    },
    get heartbeat() {
      return heartbeat;
    },
    frameTimes,
    get stats(): LoopStats {
      return {
        simFrame,
        clientFrame: clientFrameAt(lastNow),
        frameSkew,
        stepsPerSec: stepsWindow.sum(lastNow),
        fps: fpsWindow.sum(lastNow),
        droppedSteps,
        hidden,
        lastFrameMs,
        alpha: renderedAlpha,
        iterations,
        renderedFrames,
        stepErrors: { ...stepErrors },
        errors,
        overloaded,
        overBudgetFrames,
        sections: sections.snapshot(),
      };
    },
  };
}
