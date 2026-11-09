// The boot sequence (Plan/40 section 3.1, refines 01 section 6.8; WP-CLIENT-11). `start(root)` is B0 (the context,
// the watchdog binding, the CSS-only boot screen) and runs `runBoot` (B1-B9), which is the part with logic and is
// tested with fakes:
//
//   B1  in parallel: GET /api/bootstrap (retry 1, 2, 4 s, then L4 "server unreachable"), the wasm module
//       (`compileStreaming`), the fallback font (3 s timeout)
//   B2  version guard (`proto_hash`, `build_id`: `reloadOnce`), title, features, bundle manifest + asset manifest
//   B3  wasm main instance, decode workers (`clamp(cores - 2, 2, 4)`), the lazy `render/` chunk
//   B4  bundles: the pre-login subset at Critical; when the server is not gated the rest of the boot set at Near
//   B5  `ClientDataIndex`, UI/audio creation points, the handler modules and `registry.freeze()` BEFORE the socket opens
//   B6  session decision: a token in `sessionStorage` -> `Connecting{reattach: in-world marker}`, else `Login`
//   B7  the login screen is visible
//   B8  after login (or with a token): the binary sim inputs `FLMT`/`FLMB` through the asset layer (`simInputs.ts`)
//   B9  character select waits for `CharList` and the boot bundles (45)
//
// A boot step that fails is `reloadOnce('boot:<step>')` once, then the fatal screen naming the step (Plan/40 section 3.1).

import { createElement } from 'react';
import { createRoot } from 'react-dom/client';
import { fetchBootstrap } from '../auth/api';
import { ClientDataIndex } from '../data';
import { installProbe } from '../debug/probe';
import { PROTO_HASH } from '../gen/proto/proto_hash';
import { log } from '../platform/log';
import { CONNECTED_SCREENS } from '../stores/screen';
import { ensureWasm, type LoadedWasm } from '../wasm/loader';
import { WorkerPool } from '../workers/pool';
import { createBootStatus, type BootStatusStore } from './bootStatus';
import { parseFeatures, readBootstrap, type Bootstrap } from './bootstrap';
import { probeCapabilities, unsupportedReason, workerCount } from './capabilities';
import { createContext, type ClientContext } from './context';
import { createGameAudio, createGameUi } from '../features/session';
import { registerHandlerModules } from './features';
import { loadBootFonts, type FontsResult } from './fonts';
import { Root } from './Root';
import { installSessionHost } from './sessionHost';
import { installInputHost } from './inputHost';
import { installRenderHost } from './renderHost';
import { installGameSim } from './simHost';
import { loadSimInputs } from './simInputs';
import { installedWatchdog } from './watchdog';

/** Retry delays of `GET /api/bootstrap` (Plan/40 section 3.1 B1a). */
export const BOOTSTRAP_RETRY_MS: readonly number[] = [1000, 2000, 4000];

export interface BootResult {
  readonly ctx: ClientContext;
  readonly status: BootStatusStore;
}

/** The things `runBoot` does that touch the browser: replaceable in tests. */
export interface BootEnv {
  /** `GET /api/bootstrap` as parsed JSON (default `auth/api.fetchBootstrap`). */
  readonly fetchBootstrap: () => Promise<unknown>;
  /** The JSON of `bootstrap.asset_manifest_url` (default `fetch`). */
  readonly fetchJson: (url: string) => Promise<unknown>;
  readonly delay: (ms: number) => Promise<void>;
  /** Compiles the wasm module (streaming) and instantiates the main instance. */
  readonly loadWasm: () => Promise<LoadedWasm>;
  /** Starts the decode workers with the compiled module. */
  readonly createPool: (
    size: number,
    module: WebAssembly.Module,
  ) => Promise<{ pool: WorkerPool; wasmVersion: string }>;
  /** B1c. */
  readonly fonts: () => Promise<FontsResult>;
  /** B3: starts loading the renderer chunk (`import('../render')`); never awaited by the boot. */
  readonly preloadRender?: () => void;
  readonly setTitle: (title: string) => void;
  /** B5 plug points of 42 (`createUi(uiHost(ctx))`) and 44 (`createAudio(audioDeps(ctx))`): run before the handlers register. */
  readonly createUi?: (ctx: ClientContext) => void;
  readonly createAudio?: (ctx: ClientContext) => void;
  /** B3: the `SimBridge` over the main wasm instance (`ctx.sim`, default `installGameSim`); absent keeps the null sim. */
  readonly createSim?: (ctx: ClientContext) => void;
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** `true` while the boot may continue (no reload or fatal screen was started). */
function alive(ctx: ClientContext): boolean {
  const s = ctx.machine.screen.s;
  return s === 'Boot';
}

/** A failed boot step: one `reloadOnce('boot:<step>')`, then the fatal screen. */
function stepFailed(
  ctx: ClientContext,
  status: BootStatusStore,
  step: string,
  error: unknown,
): void {
  const text = errorText(error);
  ctx.log.error('boot', `${step}: ${text}`);
  status.setState({ phase: 'failed' });
  ctx.recovery.reloadOnce(`boot:${step}`, `Startup failed at ${step}: ${text}`);
}

async function bootstrapWithRetry(ctx: ClientContext, env: BootEnv): Promise<Bootstrap> {
  let lastError: unknown;
  for (let attempt = 0; attempt <= BOOTSTRAP_RETRY_MS.length; attempt += 1) {
    if (attempt > 0) await env.delay(BOOTSTRAP_RETRY_MS[attempt - 1] ?? 4000);
    try {
      return readBootstrap(await env.fetchBootstrap());
    } catch (error) {
      lastError = error;
      ctx.log.warn('boot', `bootstrap attempt ${String(attempt + 1)} failed: ${errorText(error)}`);
    }
  }
  throw lastError instanceof Error ? lastError : new Error(String(lastError));
}

/** Runs B1-B9. Resolves `true` when the session decision was made (the login screen or a connection is next). */
export async function runBoot(
  ctx: ClientContext,
  status: BootStatusStore,
  env: BootEnv,
): Promise<boolean> {
  const step = (
    name: 'bootstrap' | 'wasm' | 'worker' | 'data' | 'fonts',
    state: 'ok' | 'error',
    detail: string,
  ): void => {
    status.setState({ [name]: { state, detail } });
  };

  // ---- B1: in parallel
  const wasmP = env.loadWasm();
  wasmP.catch(() => undefined); // awaited at B3; a rejection before that is not "unhandled"
  const fontsP = env.fonts().then((r) => {
    step('fonts', r === 'loaded' ? 'ok' : 'error', r);
    if (r !== 'loaded') ctx.log.warn('boot', `fonts: ${r}; the UI uses the fallback chain`);
    return r;
  });

  let bootstrap: Bootstrap;
  try {
    bootstrap = await bootstrapWithRetry(ctx, env);
  } catch (error) {
    step('bootstrap', 'error', errorText(error));
    status.setState({ phase: 'failed' });
    ctx.recovery.l4('The server could not be reached.', errorText(error));
    return false;
  }
  step('bootstrap', 'ok', `${bootstrap.serverName || 'server'} ${bootstrap.serverBuild}`.trim());

  // ---- B2: version guard (Plan/40 section 3.3)
  if (bootstrap.protoHash !== ctx.build.protoHash) {
    ctx.log.warn('boot', `proto_hash ${bootstrap.protoHash} != ${ctx.build.protoHash}`);
    ctx.recovery.reloadOnce('proto');
    return false;
  }
  if (bootstrap.buildId !== ctx.build.id && ctx.build.id !== 'dev') {
    ctx.log.warn('boot', `build_id ${bootstrap.buildId} != ${ctx.build.id}`);
    ctx.recovery.reloadOnce('build');
    return false;
  }
  ctx.bootstrap = bootstrap;
  if (bootstrap.pageTitle !== '') env.setTitle(bootstrap.pageTitle);
  const features = parseFeatures(bootstrap.features);
  ctx.features = features.config;
  for (const w of features.warnings.slice(0, 5)) ctx.log.warn('boot', w);
  if (features.warnings.length > 5) {
    ctx.log.warn('boot', `... and ${String(features.warnings.length - 5)} more feature warnings`);
  }
  ctx.stores.sessionStore.setState({
    server: { name: bootstrap.serverName, build: bootstrap.serverBuild, status: bootstrap.status },
    authRules: bootstrap.auth,
  });
  ctx.assetLayer.prepare.fromBootstrap(bootstrap.prepare);
  try {
    const [, assetManifest] = await Promise.all([
      ctx.bundles.loadManifest(bootstrap.bundleManifestUrl),
      env.fetchJson(bootstrap.assetManifestUrl),
    ]);
    ctx.assetLayer.source.setAssetManifest(
      assetManifest as Parameters<typeof ctx.assetLayer.source.setAssetManifest>[0],
    );
  } catch (error) {
    step('data', 'error', errorText(error));
    stepFailed(ctx, status, 'manifests', error);
    return false;
  }
  if (bootstrap.dataVersion !== '' && bootstrap.dataVersion !== ctx.bundles.dataVersion) {
    ctx.log.warn(
      'boot',
      `data_version ${bootstrap.dataVersion} != manifest ${ctx.bundles.dataVersion}`,
    );
    ctx.recovery.reloadOnce('data');
    return false;
  }

  // ---- B3: wasm main instance, workers, the lazy renderer chunk
  let wasm: LoadedWasm;
  try {
    wasm = await wasmP;
    ctx.wasm = wasm.api;
    step('wasm', 'ok', `v${wasm.api.version()}`);
  } catch (error) {
    step('wasm', 'error', errorText(error));
    stepFailed(ctx, status, 'wasm', error);
    return false;
  }
  try {
    env.createSim?.(ctx);
  } catch (error) {
    stepFailed(ctx, status, 'sim', error);
    return false;
  }
  try {
    const { pool, wasmVersion } = await env.createPool(workerCount(ctx.caps), wasm.module);
    ctx.workers = pool;
    step('worker', 'ok', `${String(pool.stats().size)} workers, v${wasmVersion}`);
  } catch (error) {
    step('worker', 'error', errorText(error));
    status.setState({ phase: 'failed' });
    ctx.recovery.l4('The decoders could not be started.', errorText(error));
    return false;
  }
  env.preloadRender?.();
  if (!alive(ctx)) return false;

  // ---- B4/B5: bundles, data index, handler modules
  try {
    await ctx.bundles.loadPreLogin();
  } catch (error) {
    step('data', 'error', errorText(error));
    stepFailed(ctx, status, 'bundles', error);
    return false;
  }
  ctx.data = new ClientDataIndex(ctx.bundles);
  step('data', 'ok', `${String(ctx.bundles.loaded.size)} bundles, data ${ctx.bundles.dataVersion}`);
  if (!bootstrap.gated) {
    ctx.bundles.loadBootSet().catch((error: unknown) => {
      stepFailed(ctx, status, 'bundles', error);
    });
  }
  try {
    env.createUi?.(ctx);
    env.createAudio?.(ctx);
    registerHandlerModules(ctx); // freezes the registry: before the WebSocket opens
  } catch (error) {
    stepFailed(ctx, status, 'ui', error);
    return false;
  }
  await fontsP;
  if (!alive(ctx)) return false;

  // ---- B6: session decision (Plan/40 section 4.3)
  installSessionHost(ctx);
  const simInputs = (screen: { s: string }): boolean => {
    if (!CONNECTED_SCREENS.has(screen.s as never)) return false;
    void loadSimInputs(ctx);
    return true;
  };
  let simInputsStarted = false;
  const off = ctx.machine.subscribe((screen) => {
    if (!simInputsStarted && simInputs(screen)) {
      simInputsStarted = true;
      off();
    }
  });
  const token = ctx.session.token();
  const marker = ctx.session.inWorld();
  if (token !== null && bootstrap.gated) {
    // a reload while logged in: the asset cookie is still valid, so the boot set can load now
    ctx.bundles.onLoggedIn().catch((error: unknown) => {
      stepFailed(ctx, status, 'bundles', error);
    });
  }
  ctx.machine.dispatch({ type: 'bootReady', hasToken: token !== null, inWorld: marker });
  status.setState({ phase: 'ready' });
  return true;
}

/** The browser implementation of `BootEnv`. */
export function defaultBootEnv(ctx: ClientContext): BootEnv {
  return {
    fetchBootstrap: () => fetchBootstrap(),
    fetchJson: async (url) => {
      const res = await fetch(url);
      if (!res.ok) throw new Error(`GET ${url}: HTTP ${String(res.status)}`);
      return (await res.json()) as unknown;
    },
    delay: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
    loadWasm: () => ensureWasm(),
    createPool: (size, module) =>
      WorkerPool.create(size, module, undefined, {
        s3tc: ctx.caps.s3tc !== false,
        onFatal: (error) => {
          ctx.recovery.l4('The decoders could not be started.', error.message);
        },
      }),
    fonts: () => loadBootFonts(typeof document === 'object' ? document.fonts : undefined),
    preloadRender: () => {
      // the renderer (three.js) loads in the background while the login screen is shown (Plan/40 section 16.1)
      import('../render').catch((error: unknown) => {
        ctx.log.warn('boot', `renderer chunk: ${errorText(error)}`);
      });
    },
    setTitle: (title) => {
      document.title = title;
    },
    // B5: the UI framework (42) mounted over `uiHost(ctx)` with 45's options store, and the audio engine of 44
    createSim: (c) => {
      installGameSim(c);
    },
    createUi: (c) => {
      c.ui = createGameUi(c);
    },
    createAudio: (c) => {
      c.audio = createGameAudio(c);
    },
  };
}

/** Starts the client in `root` (B0) and runs the boot. Resolves when the boot sequence ended. */
export async function start(
  root: HTMLElement,
  overrides: Partial<BootEnv> = {},
): Promise<BootResult> {
  const caps = probeCapabilities();
  const ctx = createContext({
    build: { id: __FLYFF_BUILD_ID__, protoHash: PROTO_HASH, dev: import.meta.env.DEV },
    log,
    caps,
  });
  const status = createBootStatus();
  // the world canvas under the DOM UI and the render port (the renderer itself is built at the first world)
  const renderHost = installRenderHost(ctx, { before: root });
  ctx.render = renderHost.port;
  // the keyboard and pointer of the game (43): the key router on `window`, the pointer handlers on the `#world` canvas
  installInputHost(ctx, { canvas: renderHost.canvas });
  installProbe(ctx.probe);
  installedWatchdog()?.attach({
    recovery: ctx.recovery,
    loop: ctx.loop,
    machine: ctx.machine,
    onConnectingStuck: () => {
      ctx.connection.abandon('connecting took more than 30 s');
    },
  });
  createRoot(root).render(
    createElement(Root, {
      ctx,
      status,
      onReload: () => {
        ctx.recovery.performReload('fatal screen: reload');
      },
      onBackToLogin: () => {
        ctx.session.clear();
        ctx.recovery.performReload('fatal screen: back to login');
      },
    }),
  );
  ctx.log.info('boot', `build ${ctx.build.id}, proto ${ctx.build.protoHash}`);
  ctx.loop.start();
  ctx.background.start();
  ctx.assets.startSweeper(); // idle and over-budget cache entries are evicted every 5 s

  const unsupported = unsupportedReason(caps);
  if (unsupported !== null) {
    status.setState({ phase: 'failed' });
    ctx.recovery.l4(unsupported, 'unsupported browser');
    return { ctx, status };
  }
  try {
    await runBoot(ctx, status, { ...defaultBootEnv(ctx), ...overrides });
  } catch (error) {
    stepFailed(ctx, status, 'boot', error);
  }
  return { ctx, status };
}
