// The null sim bridge (Plan/40 section 2.3 "a null bridge that accepts and ignores commands"): it steps every requested
// frame, produces no view and answers every query with `undefined`. It is the `ctx.sim` of a context before the wasm
// module exists (login screens, `/dev/ui`), and the base class of `FakeSim` in the tests. Commands written to its
// writer are dropped at the next `frame`.

import type {
  BlinkResult,
  CameraBound,
  RayHit,
  RulesItem,
  SimBridge,
  SimFrameResult,
  SimLoad,
  SimStats,
  SimWorldDesc,
} from './ports';
import type { RenderViewReader } from './renderView';
import { SimCmdWriter, type Vec3 } from './simCmd';
import { createSimEventRouter, type SimEventRouter } from './simEvents';

const NO_STATS: SimStats = {
  callsThisFrame: 0,
  maxCalls: 0,
  byCategory: { F: 0, C: 0, Q: 0, L: 0, R: 0 },
  overBudget: 0,
  uiCalls: 0,
  eventsDropped: 0,
  memBytes: 0,
  chunksLoaded: 0,
  loadsPending: 0,
  commandsApplied: 0,
  commandErrors: 0,
};

export class NullSim implements SimBridge {
  readonly cmd = new SimCmdWriter();
  readonly events: SimEventRouter = createSimEventRouter();
  view: RenderViewReader | null = null;
  worldId = 0;
  modelBoundsLoaded = false;
  frameSkew = 0;
  /** Worlds this sim was told about, oldest first (tests). */
  readonly worlds: SimWorldDesc[] = [];
  /** Loads queued with `enqueueLoad` (tests). */
  readonly loads: SimLoad[] = [];
  readonly rules: RulesItem[] = [];
  /** `forceInput(buttons, durationMs)` calls (the probe's `test.forceInput`, tests). */
  readonly forcedInputs: [buttons: number, durationMs: number][] = [];

  frame(...args: [due: number, record?: unknown, alpha?: number | null]): SimFrameResult {
    this.cmd.reset();
    return { stepped: args[0], viewFresh: false, view: null, events: [] };
  }

  setFrameSkew(skew: number): void {
    this.frameSkew = skew;
  }

  resetRenderSmoothing(): void {
    // nothing to smooth
  }

  forceInput(...args: [buttons: number, durationMs: number]): void {
    this.forcedInputs.push(args);
  }

  setWorld(desc: SimWorldDesc): void {
    this.worldId = desc.worldId;
    this.worlds.push(desc);
    this.cmd.truncateKeepingClock();
  }

  leaveWorld(): void {
    this.worldId = 0;
    this.cmd.truncateKeepingClock();
  }

  loadMotionTable(): void {
    // no sim
  }

  loadModelBounds(): void {
    this.modelBoundsLoaded = true;
  }

  loadCtrlModels(): void {
    // no sim
  }

  enqueueLoad(load: SimLoad): void {
    this.loads.push(load);
  }

  enqueueRules(item: RulesItem): void {
    this.rules.push(item);
  }

  runRules(): boolean {
    const item = this.rules.shift();
    if (!item) return false;
    item.run();
    return true;
  }

  queryBudget(): number {
    return 2;
  }

  localAct(): number | undefined {
    return undefined;
  }

  rayCast(): RayHit | null | undefined {
    return undefined;
  }

  landHeight(): number | undefined {
    return undefined;
  }

  fullHeight(): number | undefined {
    return undefined;
  }

  heightAttr(): number | undefined {
    return undefined;
  }

  blinkPoint(): BlinkResult | undefined {
    return undefined;
  }

  targetCheck(): number | undefined {
    return undefined;
  }

  rangePoint(): boolean | undefined {
    return undefined;
  }

  cameraCheckBound(eye: Vec3): CameraBound {
    return { crash: false, out: eye, len: 0 };
  }

  get stats(): SimStats {
    return NO_STATS;
  }
}
