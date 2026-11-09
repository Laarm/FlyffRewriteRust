// ClientContext: the dependency container created once at boot and passed to every subsystem
// (Plan/40 CA3, section 2.3). No module-level mutable singletons except `platform/*` and the debug probe.
// The APIs of docs 41-44 (render, ui, input, audio) are owned by those documents; until their work packages
// land the context carries NULL implementations so the app compiles and every consumer can call them
// unconditionally. `createContext(env)` selects real or fake implementations (tests, /dev surfaces).

import {
  createAssetLayer,
  type AssetLayer,
  type AssetLayerOptions,
  type AssetManager,
  type DecodePool,
} from '../assets';
import { AssetError } from '../assets/errors';
import {
  BundleLoader,
  manifestFetcher,
  mainThreadBundleParser,
  workerBundleParser,
  type ClientDataIndex,
} from '../data';
import { createEntityTable, type EntityTable } from '../game/entities';
import { createBackgroundPump, type BackgroundPump } from '../game/background';
import { createGameLoop, rafScheduler, type FrameScheduler, type GameLoop } from '../game/loop';
import { NullSim } from '../game/nullSim';
import {
  NULL_AUDIO,
  NULL_INPUT,
  NULL_SIM,
  type AudioApi,
  type InputApi,
  type LoopPorts,
  type MoveReporterApi,
  type PoseFeederApi,
  type RenderApi,
  type SimBridge,
  type UiApi,
  type UploadsApi,
} from '../game/ports';
import { createTickSystems, type TickSystems } from '../game/tickSystems';
import type { ClientFeatureConfig } from '../gen/config/clientFeatures';
import { DEFAULT_CLIENT_FEATURES } from '../gen/config/clientFeatures';
import { createNetClient, type NetClient } from '../net/client';
import { createConnection, type Connection, type ConnectionDeps } from '../net/connection';
import { createInspector, netDebugEnabled } from '../net/inspector';
import { createLogger, type Logger } from '../platform/log';
import { session as defaultSession, type SessionKeys } from '../platform/session';
import { safeStorage } from '../platform/storage';
import { now as platformNow, wallNow } from '../platform/time';
import { isHidden } from '../platform/visibility';
import { createCoreStores, type CoreStores } from '../stores/core';
import { createPublisher, type Publisher } from '../stores/publisher';
import type { StoreRegistry, StoreRegistryOptions } from '../stores/registry';
import type { ScreenName } from '../stores/screen';
import { e2eEnabled, type FlyffDebugProbe } from '../debug/probe';
import type { WorkerPool } from '../workers/pool';
import type { WasmApi } from '../wasm/loader';
import type { Bootstrap } from './bootstrap';
import { detectCapabilities, type Capabilities } from './capabilities';
import { createContextProbe } from './probeWiring';
import { createRecovery, type Recovery, type RecoveryDeps } from './recovery';
import { createScreenRegistry, type ScreenRegistry } from './screens';
import { createStateMachine, type SessionHost, type StateMachine } from './stateMachine';

export type {
  AudioApi,
  InputApi,
  MoveReporterApi,
  PoseFeederApi,
  RenderApi,
  SimBridge,
  UiApi,
  UploadsApi,
};
export { NULL_AUDIO, NULL_INPUT, NULL_SIM };

export interface BuildInfo {
  /** `__FLYFF_BUILD_ID__` (env `FLYFF_BUILD_ID` at build time, `"dev"` otherwise). */
  readonly id: string;
  /** `PROTO_HASH` of the generated protocol code. */
  readonly protoHash: string;
  /** `true` for dev builds (Vite dev server). */
  readonly dev: boolean;
}

export type { Capabilities };
export { detectCapabilities };

export interface ClientContext {
  readonly build: BuildInfo;
  /** Data bundle version (`manifest.data_version`, set by the bundle loader; `''` before). */
  dataVersion: string;
  readonly caps: Capabilities;
  readonly log: Logger;
  /** `GET /api/bootstrap` (section 3.3); `null` until fetched. */
  bootstrap: Bootstrap | null;
  /** The typed client features; the defaults of this build until the bootstrap is in. */
  features: ClientFeatureConfig;
  /** Typed lookups over the loaded bundles; `null` until the pre-login bundles are in (section 3.4). */
  data: ClientDataIndex | null;
  /** The bundle loader (manifest, boot/lazy/gated sets, raw bytes for wasm). */
  readonly bundles: BundleLoader;
  /** Asset layer: fetch scheduler, HTTP source, manager and the preparing-assets tracker (section 9). */
  readonly assetLayer: AssetLayer;
  /** `assetLayer.manager`: ref-counted asset handles (section 9.3). */
  readonly assets: AssetManager;
  /** The session keys in `sessionStorage` (token, account, in-world marker). */
  readonly session: SessionKeys;
  /** The WebSocket lifecycle (section 5.1); `connection.install(ctx.sessionHost)` makes it the machine's socket. */
  readonly connection: Connection;
  /** The recovery ladder L1-L4 and `reloadOnce` (section 12). */
  readonly recovery: Recovery;
  /** `fetch` for the lobby calls (`/api/login`, `/api/register`) and the boot; tests point it at a mock server. */
  readonly fetch: typeof fetch;
  /** The monotonic clock of this context (`performance.now()` ms, or the test clock). */
  now(): number;
  /** The core stores and the registry for feature stores (Plan/40 section 6). */
  readonly stores: CoreStores;
  /** ObjId -> EntityRecord, outside React (section 6.3). */
  readonly entities: EntityTable;
  /** Inbound queue, handler registry, sender and clock (section 5). */
  readonly net: NetClient;
  /** <= 10 Hz store publication (section 6.4). */
  readonly publisher: Publisher;
  /** Per-60-Hz-step systems of 42/45 (section 10.2 step 6). */
  readonly tickSystems: TickSystems;
  /** The fixed-step game loop (section 10); `boot` starts it. */
  readonly loop: GameLoop;
  /** The hidden-tab pump (section 10.5); `boot` starts it. */
  readonly background: BackgroundPump;
  /** The screen state machine (section 4); the only writer of `sessionStore.screen`. */
  readonly machine: StateMachine;
  /**
   * The effect port of the state machine: `net/connection.ts` (WP-CLIENT-12/-24), the recovery ladder
   * (WP-CLIENT-13) and `boot` assign their methods here (`ctx.sessionHost.openSocket = ...`).
   */
  readonly sessionHost: Partial<SessionHost>;
  /** Screen bodies of doc 45 (section 4.1). */
  readonly screens: ScreenRegistry;
  /** `window.__flyffDebug` (section 13.2); `installProbe(ctx.probe)` publishes it. */
  readonly probe: FlyffDebugProbe;
  /** `null` until the wasm module is loaded (boot step B3). */
  wasm: WasmApi | null;
  /** `null` until the decode workers are started. */
  workers: WorkerPool | null;
  /**
   * `RulesContext` (wasm `rules`, 16 section 18.4): the `RulesFacade` of `rules/`, built by `game/rules` as soon as its four
   * bundles are in; `null` before. (Typed `object` for the host adapters and the UI tests that fake it.)
   */
  rules: object | null;
  /** The sim bridge (`game/simBridge.ts`, installed at boot B3); a null bridge until the wasm module exists. */
  sim: SimBridge;
  input: InputApi;
  audio: AudioApi;
  render: RenderApi | null;
  ui: UiApi | null;
  /** GPU upload queue (WP-CLIENT-22). */
  uploads: UploadsApi | null;
  /** Move reporter (WP-CLIENT-20). */
  moveReporter: MoveReporterApi | null;
  /** Pose feeder (WP-CLIENT-18). */
  poseFeeder: PoseFeederApi | null;
}

export interface ContextEnv {
  readonly build: BuildInfo;
  /** Test seam: defaults to a logger that mirrors to the console. */
  readonly log?: Logger;
  /** Test seam: defaults to feature detection in the current environment. */
  readonly caps?: Capabilities;
  /** Test seam: monotonic clock for the loop, the sender, the registry and the machine (default `platform/time`). */
  readonly now?: () => number;
  /** Test seam: the rAF scheduler (default `requestAnimationFrame`). */
  readonly scheduler?: FrameScheduler;
  /** Store registry options (`assertObservers`). */
  readonly stores?: StoreRegistryOptions;
  /** `?debug=net`: log every message (default: read from `location.search`). */
  readonly netDebug?: boolean;
  /** The probe's test hooks (default: dev builds and pages with `?e2e=1`). */
  readonly e2e?: boolean;
  /** `send` throws for an envelope above the size cap (default: dev builds, Plan/40 S1). */
  readonly throwOnTooBig?: boolean;
  /** Test seam: replaces the read of the `render.renderInterpolation` option of the `options` store (default on, see {@link renderInterpolationOf}). */
  readonly interpolation?: () => boolean;
  /** Session keys (default: `sessionStorage` through `safeStorage`). */
  readonly session?: SessionKeys;
  /** Test seams of the connection: `openTransport`, `url`, `timers`, `graceMs`, `isHidden`, `subscribeVisibility`, `dataVersion`. */
  readonly connection?: Partial<
    Pick<
      ConnectionDeps,
      | 'openTransport'
      | 'url'
      | 'timers'
      | 'graceMs'
      | 'isHidden'
      | 'subscribeVisibility'
      | 'dataVersion'
    >
  >;
  /** Test seams of the recovery ladder: storage, reload and replace functions, location, wall clock. */
  readonly recovery?: Partial<
    Pick<RecoveryDeps, 'storage' | 'reload' | 'replaceUrl' | 'location' | 'wall'>
  >;
  /** Test seams of the asset layer (`fetch`, `concurrency`, `timers`, budgets). */
  readonly assets?: Pick<AssetLayerOptions, 'scheduler' | 'manager' | 'fetchProgress'>;
  /** `fetch` of the asset scheduler, the bundle manifest and `/api/prepare` (default: the global one). */
  readonly fetch?: typeof fetch;
}

/** The client messages of the gateway's Hello, Lobby and ServerTime classes (`gateway/pipeline.rs` `class_of`): every other case is a game message. */
const LOBBY_CASES: ReadonlySet<string> = new Set([
  'hello',
  'createPlayer',
  'delPlayer',
  'enterWorld',
  'serverTimeReq',
]);
/** The states in which the gateway kicks (CharSelect, ...) or drops (Leaving) a game message: nothing is lost by not sending it. */
const GAME_REFUSED_SCREENS: ReadonlySet<ScreenName> = new Set<ScreenName>([
  'Connecting',
  'CharSelect',
  'CharCreate',
  'Leaving',
  'Login',
]);

/**
 * The send gate of the net sender (BFIX1 #021): may a message of case `msgCase` leave while the machine shows `screen`? A game
 * message is refused on the lobby and title screens and while leaving (a window closing at the logout must not turn it into a
 * kick: the gateway answers a game message in CharSelect with `Kick{INVALID_STATE}`); every other state keeps its behaviour
 * (the test rigs send game messages from Boot and Entering). Evaluated in `send` only, so a message accepted while in game
 * (the `returnToCharSelect` of the logout) is still flushed.
 */
export function sendAllowed(msgCase: string | undefined, screen: ScreenName | undefined): boolean {
  if (screen === undefined || !GAME_REFUSED_SCREENS.has(screen)) return true;
  return msgCase !== undefined && LOBBY_CASES.has(msgCase);
}

function searchString(): string {
  return typeof location === 'undefined' ? '' : location.search;
}

/**
 * The render interpolation option (PL-009, Plan/40 section 10.3): `render.renderInterpolation` of the `options` store (45), default on.
 * Read through a structural cast because `app/` does not import `features/`; a registry that defines no `options` store (dev surfaces,
 * most tests) keeps the interpolation on. The loop asks once per rendered iteration.
 */
export function renderInterpolationOf(registry: Pick<StoreRegistry, 'get'>): boolean {
  const state = registry.get('options')?.getState() as
    { render?: { renderInterpolation?: unknown } } | undefined;
  return state?.render?.renderInterpolation !== false;
}

export function createContext(env: ContextEnv): ClientContext {
  const log = env.log ?? createLogger();
  const now = env.now ?? platformNow;
  const stores = createCoreStores(env.stores);
  const entities = createEntityTable();
  const publisher = createPublisher({
    onError: (name, error) => {
      log.error(
        'publisher',
        `${name} failed: ${error instanceof Error ? error.message : String(error)}`,
      );
    },
  });
  const tickSystems = createTickSystems();
  const sessionHost: Partial<SessionHost> = {};
  const inspector = (env.netDebug ?? netDebugEnabled(searchString())) ? createInspector(log) : null;

  // `ctx` is filled in below; the closures only run after construction
  const ctx = {
    build: env.build,
    now,
    fetch: env.fetch ?? ((input, init) => fetch(input, init)),
    dataVersion: '',
    caps: env.caps ?? detectCapabilities(),
    log,
    stores,
    entities,
    publisher,
    tickSystems,
    sessionHost,
    wasm: null,
    workers: null,
    rules: null,
    sim: new NullSim(),
    input: NULL_INPUT,
    audio: NULL_AUDIO,
    render: null,
    ui: null,
    uploads: null,
    moveReporter: null,
    poseFeeder: null,
  } as unknown as ClientContext;
  const mutable = ctx as { -readonly [K in keyof ClientContext]: ClientContext[K] };

  // the recovery ladder (Plan/40 section 12.2): every resync and reload request goes through it
  const requestResync = (reason: string): void => {
    mutable.recovery.l2(reason);
  };
  const requestReload = (reason: string): void => {
    mutable.recovery.l3(reason);
  };

  mutable.net = createNetClient({
    now,
    log,
    inbound: {
      onOverflow: (reason) => {
        requestResync(`inbound overflow (${reason})`);
      },
    },
    registry: {
      entities,
      guard: stores.registry,
      requestResync,
      requestReload,
      ...(inspector ? { inspector } : {}),
    },
    sender: {
      // a dev build throws on a message above the size cap (Plan/40 S1); production returns `too_big`
      throwOnTooBig: env.throwOnTooBig ?? env.build.dev,
      // no game message leaves while the gateway would kick or drop it (the machine is created after the net client: read it lazily)
      gate: (msg) =>
        sendAllowed(msg.case, (mutable.machine as ClientContext['machine'] | undefined)?.screen.s),
      ...(inspector ? { inspector } : {}),
    },
    afterDispatch: () => {
      entities.sweep();
    },
  });

  mutable.machine = createStateMachine({
    stores,
    entities,
    sender: mutable.net.sender,
    host: sessionHost,
    log,
    now,
  });

  // session keys, recovery ladder, asset layer, bundle loader, connection (nothing here touches the network)
  const session = env.session ?? defaultSession;
  mutable.session = session;
  mutable.bootstrap = null;
  mutable.features = DEFAULT_CLIENT_FEATURES;
  mutable.data = null;
  mutable.recovery = createRecovery({
    dispatch: (event) => mutable.machine.dispatch(event),
    log,
    now,
    wall: wallNow,
    storage: safeStorage.session,
    reload: () => {
      if (typeof location !== 'undefined') location.reload();
    },
    replaceUrl: (url) => {
      if (typeof location !== 'undefined') location.replace(url);
    },
    location: () =>
      typeof location === 'undefined'
        ? { pathname: '/', search: '' }
        : { pathname: location.pathname, search: location.search },
    text: (tid, fallback) => {
      const d = ctx.data;
      return d?.hasText(tid) === true ? d.text(tid) : fallback;
    },
    resync: (reason) => {
      sessionHost.resync?.(reason);
    },
    ...env.recovery,
  });
  sessionHost.reloadOnce = (reason) => {
    mutable.recovery.reloadOnce(reason);
  };
  sessionHost.reloadPage = (reason) => {
    mutable.recovery.performReload(reason);
  };
  // decode jobs go to the worker pool once boot step B3 created it
  const decodePool: DecodePool = {
    submit: (spec) => {
      const pool = ctx.workers;
      if (!pool)
        throw new AssetError('server', spec.kind, 'the decode workers are not started yet');
      return pool.submit(spec);
    },
    setGeneration: (generation) => {
      ctx.workers?.setGeneration(generation);
    },
  };
  mutable.assetLayer = createAssetLayer({
    sessionStore: stores.sessionStore,
    pool: decodePool,
    caps: { s3tc: ctx.caps.s3tc !== false },
    log,
    now,
    ...env.assets,
    scheduler: { ...(env.fetch ? { fetch: env.fetch } : {}), ...env.assets?.scheduler },
  });
  mutable.assets = mutable.assetLayer.manager;
  // the manager never evicts what was uploaded in the current frame: it counts the 60 Hz steps of the loop
  let assetFrame = 0;
  tickSystems.register('assets.frame', (due) => {
    assetFrame += due;
    mutable.assets.setFrame(assetFrame);
  });
  mutable.bundles = new BundleLoader({
    source: mutable.assetLayer.source,
    ...(env.fetch ? { fetchManifest: manifestFetcher(env.fetch) } : {}),
    gated: () => ctx.bootstrap?.gated === true,
    parse: (bytes, prio) =>
      ctx.workers
        ? workerBundleParser(ctx.workers)(bytes, prio)
        : mainThreadBundleParser(bytes, prio),
    onSchemaMismatch: (error) => {
      log.warn('data', error.message);
      mutable.recovery.reloadOnce('data');
    },
    onManifest: (manifest) => {
      mutable.assetLayer.source.setBundleManifest(manifest);
      mutable.dataVersion = manifest.data_version;
    },
    now,
    log,
  });
  mutable.connection = createConnection({
    net: mutable.net,
    machine: mutable.machine,
    sessionStore: stores.sessionStore,
    publisher,
    session,
    build: env.build,
    log,
    now,
    graceMs: () => (ctx.bootstrap?.auth.token_grace_s ?? 60) * 1000,
    dataVersion: () => ctx.bundles.dataVersion,
    requestReload: (reason) => {
      mutable.recovery.reloadOnce(reason);
    },
    isHidden: () => ctx.background.hidden,
    ...env.connection,
  });

  // the loop reads the replaceable ports through getters: a renderer or UI installed later is picked up
  const ports: LoopPorts = {
    get sim() {
      return ctx.sim;
    },
    get input() {
      return ctx.input;
    },
    get audio() {
      return ctx.audio;
    },
    get render() {
      return ctx.render;
    },
    get ui() {
      return ctx.ui;
    },
    get uploads() {
      return ctx.uploads;
    },
    get moveReporter() {
      return ctx.moveReporter;
    },
    get poseFeeder() {
      return ctx.poseFeeder;
    },
  };
  mutable.loop = createGameLoop({
    net: mutable.net,
    publisher,
    tickSystems,
    ports: () => ports,
    now,
    scheduler: env.scheduler ?? rafScheduler,
    log,
    interpolation: env.interpolation ?? (() => renderInterpolationOf(stores.registry)),
  });
  mutable.background = createBackgroundPump({
    loop: mutable.loop,
    inbound: mutable.net.inbound,
    onReturn: () => {
      ctx.connection.onVisible();
    },
    onHiddenChange: (hidden) => {
      ctx.connection.onHiddenChange(hidden);
    },
    releaseInput: () => {
      ctx.input.release();
    },
    resetRenderSmoothing: () => {
      ctx.sim.resetRenderSmoothing();
    },
    flushPublishers: () => {
      publisher.flushAll();
    },
    isHidden,
    now,
  });
  mutable.screens = createScreenRegistry();
  mutable.probe = createContextProbe(ctx, {
    e2e: env.e2e ?? (env.build.dev || e2eEnabled(searchString())),
  });
  return ctx;
}
