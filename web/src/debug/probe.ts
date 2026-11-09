// `window.__flyffDebug` (Plan/40 section 13.2, WP-CLIENT-02): the read-only debug probe Playwright, the perf
// and the visual-QA suites read instead of scraping the DOM. Everything it returns is a COPY (it never keeps
// large objects alive and the caller cannot mutate client state). Sections of the subsystems that do not
// exist yet return neutral defaults; 41-45 attach their sections with `extend(name, section)`.
//
// The `test` hooks exist only when the page URL has `?e2e=1` or the build is a dev build; the WP that owns
// the behaviour wires its hook with `setTestHook` (until then calling it throws a clear error).
//
// The probe is also installed as `window.__flyff` (an alias of the same object).

export type Vec3Tuple = readonly [number, number, number];

export interface ProbeBuild {
  readonly id: string;
  readonly protoHash: string;
  readonly dataVersion: string;
}

export interface ProbeSession {
  readonly account?: string;
  readonly idPlayer?: number;
  readonly worldId?: number;
  readonly reattached: boolean;
  readonly authority: number;
  readonly characters: readonly { slot: number; idPlayer: number; name: string }[];
}

export interface ProbeSelf {
  readonly objId: number;
  readonly name: string;
  readonly level: number;
  readonly hp: number;
  readonly maxHp: number;
  readonly pos: Vec3Tuple;
  readonly state: number;
}

export type ProbeEntityKind = 'player' | 'npc' | 'item' | 'ctrl';

export interface ProbeEntity {
  readonly objId: number;
  readonly kind: string;
  readonly name: string;
  readonly pos: Vec3Tuple;
  /** The position of the mover in the sim's render view (LH, the logical position of the last step), when the render view exists. */
  readonly renderPos?: Vec3Tuple;
  /**
   * Where the picture draws the mover in the last rendered frame (LH): the render position interpolated with `loop().alpha` (PL-009;
   * `y` includes the heel of a walking player), when the render view exists.
   */
  readonly drawnPos?: Vec3Tuple;
  /** `true` for a monster that attacks on sight (the red name; `SpawnNpc.active_attack`); absent for every other entity. */
  readonly activeAttack?: boolean;
}

export interface ProbeNet {
  readonly connected: boolean;
  readonly rttMs: number;
  readonly offsetFrames: number;
  readonly batches: number;
  readonly sent: Readonly<Record<string, number>>;
  readonly unknown: Readonly<Record<string, number>>;
  readonly unhandled: Readonly<Record<string, number>>;
  readonly handlerErrors: Readonly<Record<string, number>>;
}

/** The plain numbers of `loop()`; `frameTimes` is added by the probe. */
export interface ProbeLoopData {
  readonly simFrame: number;
  readonly clientFrame: number;
  readonly frameSkew: number;
  readonly stepsPerSec: number;
  readonly fps: number;
  readonly droppedSteps: number;
  readonly hidden: boolean;
  readonly lastFrameMs: number;
  /** The interpolation factor of the last RENDERED iteration (`[0, 1)`; `1` with `render.renderInterpolation` off, PL-009). */
  readonly alpha: number;
  readonly wasmCallsMax: number;
  readonly wasmCallsByCategoryMax: Readonly<Record<'F' | 'C' | 'Q' | 'L' | 'R', number>>;
  readonly wasmOverBudget: number;
  readonly wasmUiCallsMax: number;
}

export interface ProbeLoop extends ProbeLoopData {
  /** The last `n` rAF frame durations in ms, oldest first (a copy; `[]` before the first frame). */
  frameTimes(n?: number): number[];
}

export interface ProbeAssets {
  readonly inflight: number;
  readonly queued: number;
  readonly failed: readonly string[];
  readonly cpuBytes: number;
  readonly gpuBytes: number;
  /** Terrain Ready. */
  readonly landsReady: number;
  /** Collision chunks loaded in the sim. */
  readonly chunksReady: number;
}

export interface ProbeRecoveryEntry {
  readonly level: number;
  readonly reason: string;
  readonly at: number;
}

/** Hooks of `?e2e=1` pages (Plan/40 section 13.2). */
export interface ProbeTestHooks {
  simulateHidden(on: boolean): void;
  forceRecovery(level: 2 | 3): void;
  dropSocket(): void;
  setOption(path: string, v: unknown): void;
  login(account: string, password: string, force?: boolean): Promise<void>;
  back(): Promise<void>;
  enterWorld(slot: number): void;
  /** `buttons` are the input bits of Plan/31 section 17.5. */
  forceInput(buttons: number, ms: number): void;
  sentLog(n?: number): { case: string; json: unknown }[];
  /**
   * Pins the game clock the renderer draws the day cycle with (sky, light, fog) to `hour` (1..24) : `min` (the server's
   * `GameTimer` keeps running underneath); `null` releases it. Visual QA of the sky at fixed hours (Plan/41 14).
   */
  setGameClock(hour: number | null, min?: number): void;
  /**
   * Visual QA (Plan/41 26, WP-REND-17): draws the world with a scripted camera. `pos` is the LH point looked at, `yaw`/`pitch` are
   * degrees (view direction `(sin yaw cos pitch, sin pitch, cos yaw cos pitch)`, pitch < 0 looks down), `dist` the distance of the
   * eye behind `pos` (0: the eye is at `pos`); `null` gives the camera back to the input layer. Captures only: picking and input
   * keep their own camera.
   */
  setCamera(pose: { pos: Vec3Tuple; yaw: number; pitch: number; dist: number } | null): void;
  /** A named camera path (`flaris-orbit`: one turn around the own character per 24 s); `null` stops it. An unknown name throws. */
  cameraPath(name: string | null): void;
  /** Pins the hour (1..24) the day cycle is drawn with; `null` releases it. The same pin as `setGameClock(hour)`. */
  setGameHour(hour: number | null): void;
  /**
   * The server weather as `worldStore.env` holds it: `rain` = summer, `snow` = winter (both with the effect on), `none`/`clear` off.
   * The picture also needs the option `render.weatherEffect` (KEEP, default off): `setOption('render.weatherEffect', true)`.
   */
  setWeather(kind: 'none' | 'clear' | 'rain' | 'snow'): void;
  /** Hides the DOM UI and the world overlay canvas (`visibility:hidden`) so that a capture shows the bare world. */
  hideHud(on: boolean): void;
}

export type TestHookName = keyof ProbeTestHooks;

export interface FlyffDebugProbe {
  readonly version: 1;
  build(): ProbeBuild;
  state(): string;
  stateHistory(): { s: string; at: number }[];
  session(): ProbeSession;
  self(): ProbeSelf | null;
  entities(kind?: ProbeEntityKind): ProbeEntity[];
  net(): ProbeNet;
  loop(): ProbeLoop;
  assets(): ProbeAssets;
  recovery(): ProbeRecoveryEntry[];
  logs(n?: number): string[];
  chat(): unknown[];
  /** Attaches a section of 41-45 (`render`, `ui`, `input`, `audio`, `features`) as `probe[name]`. */
  extend(name: string, section: object): void;
  /** The names of the attached sections. */
  sections(): string[];
  /** Present only with `?e2e=1` or in dev builds. */
  readonly test?: ProbeTestHooks;
  /** Wires a test hook (owner WPs call this; absent when test hooks are disabled). */
  setTestHook<K extends TestHookName>(name: K, fn: ProbeTestHooks[K]): void;
}

/** The data providers of the probe; every member is optional and defaults to a neutral value. */
export interface ProbeSources {
  build: () => ProbeBuild;
  state: () => string;
  stateHistory: () => readonly { s: string; at: number }[];
  session: () => ProbeSession;
  self: () => ProbeSelf | null;
  entities: (kind?: ProbeEntityKind) => readonly ProbeEntity[];
  net: () => ProbeNet;
  loop: () => ProbeLoopData & { frameTimes(n?: number): readonly number[] };
  assets: () => ProbeAssets;
  recovery: () => readonly ProbeRecoveryEntry[];
  logs: (n?: number) => readonly string[];
  chat: () => readonly unknown[];
}

export interface ProbeOptions {
  /** Dev build: a duplicate `extend` name throws (default `import.meta.env.DEV`). */
  readonly dev?: boolean;
  /** Create the `test` hooks (default: dev build). */
  readonly testHooks?: boolean;
}

const NEUTRAL_LOOP: ProbeLoopData = {
  simFrame: 0,
  clientFrame: 0,
  frameSkew: 0,
  stepsPerSec: 0,
  fps: 0,
  droppedSteps: 0,
  hidden: false,
  lastFrameMs: 0,
  alpha: 1,
  wasmCallsMax: 0,
  wasmCallsByCategoryMax: { F: 0, C: 0, Q: 0, L: 0, R: 0 },
  wasmOverBudget: 0,
  wasmUiCallsMax: 0,
};

function pickLoopData(src: ProbeLoopData): ProbeLoopData {
  return {
    simFrame: src.simFrame,
    clientFrame: src.clientFrame,
    frameSkew: src.frameSkew,
    stepsPerSec: src.stepsPerSec,
    fps: src.fps,
    droppedSteps: src.droppedSteps,
    hidden: src.hidden,
    lastFrameMs: src.lastFrameMs,
    alpha: src.alpha,
    wasmCallsMax: src.wasmCallsMax,
    wasmCallsByCategoryMax: src.wasmCallsByCategoryMax,
    wasmOverBudget: src.wasmOverBudget,
    wasmUiCallsMax: src.wasmUiCallsMax,
  };
}

const NEUTRAL_NET: ProbeNet = {
  connected: false,
  rttMs: 0,
  offsetFrames: 0,
  batches: 0,
  sent: {},
  unknown: {},
  unhandled: {},
  handlerErrors: {},
};

/** `true` when the URL carries `e2e=1` (Playwright pages): test hooks on, animations frozen (Plan/40 14.4). */
export function e2eEnabled(search: string): boolean {
  return new URLSearchParams(search).get('e2e') === '1';
}

const clone = <T>(v: T): T => structuredClone(v);

const HOOK_OWNERS: Readonly<Record<TestHookName, string>> = {
  simulateHidden: 'WP-CLIENT-25',
  forceRecovery: 'WP-CLIENT-13',
  dropSocket: 'WP-CLIENT-24',
  setOption: 'WP-CFEAT-47',
  login: 'WP-CLIENT-12',
  back: 'WP-CLIENT-12',
  enterWorld: 'WP-CLIENT-12',
  forceInput: 'WP-INPUT-05',
  sentLog: 'WP-CLIENT-02',
  setGameClock: 'WP-REND-28',
  setCamera: 'WP-REND-17',
  cameraPath: 'WP-REND-17',
  setGameHour: 'WP-REND-17',
  setWeather: 'WP-REND-17',
  hideHud: 'WP-REND-17',
};

export function createProbe(
  sources: Partial<ProbeSources> & Pick<ProbeSources, 'build'>,
  options: ProbeOptions = {},
): FlyffDebugProbe {
  const dev = options.dev ?? import.meta.env.DEV;
  const withHooks = options.testHooks ?? dev;
  const extensions = new Map<string, object>();
  const RESERVED = new Set([
    'version',
    'build',
    'state',
    'stateHistory',
    'session',
    'self',
    'entities',
    'net',
    'loop',
    'assets',
    'recovery',
    'logs',
    'chat',
    'extend',
    'sections',
    'test',
    'setTestHook',
  ]);

  let testHooks: ProbeTestHooks | undefined;
  if (withHooks) {
    const unwired = (name: TestHookName) => () => {
      throw new Error(`test hook "${name}" is not wired yet (owner ${HOOK_OWNERS[name]})`);
    };
    const hooks: Record<string, unknown> = {};
    for (const name of Object.keys(HOOK_OWNERS) as TestHookName[]) hooks[name] = unwired(name);
    testHooks = hooks as unknown as ProbeTestHooks;
  }

  const probe: Record<string, unknown> & FlyffDebugProbe = {
    version: 1,
    build: () => clone(sources.build()),
    state: () => sources.state?.() ?? 'Boot',
    stateHistory: () => clone([...(sources.stateHistory?.() ?? [])]),
    session: () =>
      clone(sources.session?.() ?? { reattached: false, authority: 0, characters: [] }),
    self: () => clone(sources.self?.() ?? null),
    entities: (kind) => clone([...(sources.entities?.(kind) ?? [])]),
    net: () => clone(sources.net?.() ?? NEUTRAL_NET),
    loop: () => {
      const src = sources.loop?.();
      if (!src) return { ...clone(NEUTRAL_LOOP), frameTimes: () => [] };
      return { ...clone(pickLoopData(src)), frameTimes: (n) => [...src.frameTimes(n)] };
    },
    assets: () =>
      clone(
        sources.assets?.() ?? {
          inflight: 0,
          queued: 0,
          failed: [],
          cpuBytes: 0,
          gpuBytes: 0,
          landsReady: 0,
          chunksReady: 0,
        },
      ),
    recovery: () => clone([...(sources.recovery?.() ?? [])]),
    logs: (n) => [...(sources.logs?.(n) ?? [])],
    chat: () => clone([...(sources.chat?.() ?? [])]),
    extend(name, section) {
      if (RESERVED.has(name) || extensions.has(name)) {
        const message = `probe section "${name}" already exists`;
        if (dev) throw new Error(message);
        return;
      }
      extensions.set(name, section);
      Object.defineProperty(probe, name, { value: section, enumerable: true, configurable: false });
    },
    sections: () => [...extensions.keys()],
    setTestHook(name, fn) {
      if (!testHooks) return;
      (testHooks as unknown as Record<string, unknown>)[name] = fn;
    },
  };
  if (testHooks) Object.defineProperty(probe, 'test', { value: testHooks, enumerable: true });
  return probe;
}

/** Installs the probe as `window.__flyffDebug` (and the alias `window.__flyff`). */
export function installProbe(probe: FlyffDebugProbe, target: object = globalThis): void {
  for (const name of ['__flyffDebug', '__flyff']) {
    Object.defineProperty(target, name, { value: probe, configurable: true, writable: false });
  }
}
