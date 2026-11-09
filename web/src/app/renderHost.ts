// The world renderer of the game (Plan/40 sections 2.1, 9.3, 9.8, 10.2 step 10; Plan/41 section 2.2; WP-CLIENT-21/22/23 seams):
// the `#world` canvas under the DOM UI, the lazily created `WorldRenderer`, and the `RenderApi` port the loop and the world
// handlers talk to (`ctx.render`).
//
// Nothing of three.js is loaded or created before the first world: `installRenderHost` only prepares the canvas and the port.
// `setWorld(desc)` (WorldReadInfo / Replace, through the world runtime) builds the renderer once - the `render/` chunk, the
// device on the canvas, the texture loader over the decode pool and the asset layer, the land / model / registry ports over the
// asset manager - and hands the world over; until it is up the render side of the loading gate reports "not open", after a
// failed start (no WebGL2, no data) it reports nothing so the gate opens on the collision window alone. The loop's upload step
// (`ctx.uploads`) runs the renderer's queue with the budget of the mode (game 4 MB / loading 16 MB / hidden none), the WebGL
// context loss is coordinated with the asset manager (handles marked for re-upload, VRAM accounting reset).
//
// A camera is the business of 43 (`ctx.input.camera`: the `CBackCamera` port); `RenderFrame.camera` is its sample of the frame and
// the only camera the world is drawn with once there is one. The plain follow camera behind the active player is the
// stand-in of the frames BEFORE the first sample (no camera installed, or the own mover is not in the sim yet while the world
// loads: the terrain streams around the player and the loading gate needs the draws); `__flyffDebug.renderHost.camera` counts
// the frames of each source.
//
// The in-world overlay (names, HP bars, balloons, emoticons, damage numbers: `overlayHost.ts`) is created after the renderer
// is initialised, composed into `renderer.fx` (`ctx.render.fx`) and drawn after every world frame.

import { withAudioFx } from '../audio/fxAudio';
import { coordinateContextLoss, createUploads } from '../assets/uploads';
import { AssetError } from '../assets/errors';
import { normalizeResourcePath } from '../assets/paths';
import type { RenderApi, RenderFrame, RenderGateState, UploadsApi } from '../game/ports';
import type { RenderViewReader } from '../game/renderView';
import { peekSkillsRuntime } from '../game/combat/skills';
import { fxRuntime } from '../game/fx';
import { createMotionLookup, type MotionLookup } from '../game/motionLookup';
import { defineOptionsStore } from '../features/options';
import { worldRuntime } from '../game/world/runtime';
import type {
  createManagerModelProvider,
  createManagerSfxSource,
  createPoolTextureLoader,
  installRenderProbe,
  resolveVisualPlan,
  VisualRegistry,
  WorldRenderer,
} from '../render';
import type { TextureNotFoundError } from '../render/gl/textures';
import type { tierPreset } from '../render/quality';
import type { CameraState, GaugeState, Vec3LH, WorldRenderDesc } from '../render/types';
import type { createManagerPorts } from '../render/world/managerPorts';
import type { DecorSink } from '../render/objects/decor';
import type { RideAssets } from '../render/movers/rides';
import { FUEL_GAUGE_MAX, flightOf } from '../game/flight';
import { OBJSTAF as STAF } from '../game/world/constants';
import type { createWorldRegistry } from '../render/world/ports';
import { Prio } from '../workers/priority';
import { startTier } from './capabilities';
import type { ClientContext } from './context';
import { qaCamera } from './qaHooks';
import { createOverlayHost, type OverlayChunk, type OverlayHost } from './overlayHost';
import { createSkillFxHost } from './skillFxHost';

/** What the host needs of the lazily imported render modules (a test seam: the default is the real chunk). */
export interface RenderChunk extends OverlayChunk {
  readonly WorldRenderer: typeof WorldRenderer;
  readonly createPoolTextureLoader: typeof createPoolTextureLoader;
  readonly installRenderProbe: typeof installRenderProbe;
  readonly tierPreset: typeof tierPreset;
  readonly createManagerPorts: typeof createManagerPorts;
  readonly createWorldRegistry: typeof createWorldRegistry;
  readonly TextureNotFoundError: typeof TextureNotFoundError;
  /** The mover visuals of the world (WP-REND-12): the registry over the client bundles and the asset-manager provider. */
  readonly VisualRegistry: typeof VisualRegistry;
  readonly createManagerModelProvider: typeof createManagerModelProvider;
  readonly resolveVisualPlan: typeof resolveVisualPlan;
  /** The effect runtime (WP-REND-18): the `.sfx` source over the asset manager. Absent in test chunks (no effects). */
  readonly createManagerSfxSource?: typeof createManagerSfxSource;
}

export interface RenderHostOptions {
  /** The element the canvas goes in front of (the React root); `null` appends the canvas to `document.body`. */
  readonly before?: Element | null;
  /** Test seam: the canvas (default: a new full-window `#world` canvas). */
  readonly canvas?: HTMLCanvasElement;
  /** Test seam: the render chunk (default: dynamic imports of `render/`). */
  readonly loadChunk?: () => Promise<RenderChunk>;
  /** Test seam: the page query the start tier is read from (default: `location.search`, else empty). */
  readonly search?: string;
}

export interface RenderHost {
  /** The `RenderApi` the loop and the world handlers use (`ctx.render`). */
  readonly port: RenderApi;
  readonly canvas: HTMLCanvasElement;
  /** The renderer once it is built, else `null`. */
  renderer(): WorldRenderer | null;
  /** The in-world overlay (names, balloons, damage numbers) once the renderer is built, else `null`. */
  overlay(): OverlayHost | null;
  /** Resolves when the renderer is built (or failed); never rejects. */
  ready(): Promise<void>;
  /** The start failed (no WebGL2, the data could not be loaded). */
  readonly failed: boolean;
  dispose(): void;
}

async function loadRenderChunk(): Promise<RenderChunk> {
  const [render, quality, managerPorts, ports, textures] = await Promise.all([
    import('../render'),
    import('../render/quality'),
    import('../render/world/managerPorts'),
    import('../render/world/ports'),
    import('../render/gl/textures'),
  ]);
  return {
    WorldRenderer: render.WorldRenderer,
    createPoolTextureLoader: render.createPoolTextureLoader,
    installRenderProbe: render.installRenderProbe,
    tierPreset: quality.tierPreset,
    createManagerPorts: managerPorts.createManagerPorts,
    createWorldRegistry: ports.createWorldRegistry,
    TextureNotFoundError: textures.TextureNotFoundError,
    VisualRegistry: render.VisualRegistry,
    createManagerModelProvider: render.createManagerModelProvider,
    resolveVisualPlan: render.resolveVisualPlan,
    createManagerSfxSource: render.createManagerSfxSource,
    OverlayLayer: render.OverlayLayer,
    createOverlayAssets: render.createOverlayAssets,
    composeFx: render.composeFx,
    subjectOf: render.subjectOf,
    projectorOfCamera: render.projectorOfCamera,
    parseArgb: render.parseArgb,
    buildRampLut: render.buildRampLut,
  };
}

function createCanvas(before: Element | null | undefined): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.id = 'world';
  canvas.dataset['testid'] = 'world-canvas';
  canvas.style.cssText =
    'position:fixed;left:0;top:0;width:100%;height:100%;display:none;z-index:0;outline:none;touch-action:none;';
  canvas.tabIndex = -1;
  const parent = before?.parentNode ?? document.body;
  parent.insertBefore(canvas, before ?? parent.firstChild);
  if (before instanceof HTMLElement) {
    // the DOM UI paints above the canvas
    before.style.position ||= 'relative';
    before.style.zIndex ||= '1';
  }
  return canvas;
}

/** The follow camera of a context without 43 (or for a frame without a camera sample): behind and above `pos`, looking at it. */
export function fallbackCamera(pos: Vec3LH): CameraState {
  return {
    pos: [pos[0], pos[1] + 9, pos[2] - 13],
    lookAt: [pos[0], pos[1] + 1.3, pos[2]],
    up: [0, 1, 0],
    fovDiv: 4,
    telescope: 0,
    yawDeg: 0,
    mode: 'normal',
  };
}

/** The gate state of a renderer that is still starting: terrain and statics are wanted and not there. */
const STARTING_GATE: RenderGateState = {
  terrain: { ready: 0, wanted: 1 },
  statics: { ready: 0, wanted: 1 },
  open: false,
};

/** The `motions` bundle as a lookup (rebuilt when the bundle changes). */
function motionsOf(ctx: ClientContext): () => MotionLookup | null {
  let lookup: MotionLookup | null = null;
  let lookupFor: unknown = null;
  return () => {
    const bundle = ctx.data?.bundle('motions');
    if (!bundle) return null;
    if (lookupFor !== bundle) {
      lookup = createMotionLookup(bundle);
      lookupFor = bundle;
    }
    return lookup;
  };
}

/** What the renderer tells the sim about ctrls and animated statics (`SimBridge.cmd`, the clips through the load queue). */
function decorSink(ctx: ClientContext, motions: () => MotionLookup | null): DecorSink {
  return {
    add: (c) => {
      ctx.sim.cmd.decorAdd(c);
    },
    remove: (id) => {
      ctx.sim.cmd.decorRemove(id);
    },
    setVisible: (id, visible) => {
      ctx.sim.cmd.decorVisible(id, visible);
    },
    setCtrlVisible: (id, visible) => {
      ctx.sim.cmd.setVisible(id, visible, false);
    },
    loadClip: (clip, bytes) => {
      ctx.sim.enqueueLoad({ kind: 'clip', id: clip, bytes, prio: Prio.Visible });
    },
    builtinClip: (ot, index) => motions()?.builtinClip(ot, index) ?? null,
  };
}

/** The sim side of rides and wings (WP-REND-25): the decor sink, the wing's skeleton and clips, the motion table. */
function rideAssets(
  ctx: ClientContext,
  decor: DecorSink,
  motions: () => MotionLookup | null,
): RideAssets {
  return {
    decor,
    skeletonId: (path) => ctx.poseFeeder?.skeletonId?.(path) ?? 0,
    requestClip: (clip) => {
      ctx.poseFeeder?.requestClip?.(clip);
    },
    resolveClip: (ot, index, mti) => motions()?.resolveClip(ot, index, mti) ?? null,
  };
}

export function installRenderHost(ctx: ClientContext, options: RenderHostOptions = {}): RenderHost {
  const loadChunk = options.loadChunk ?? loadRenderChunk;
  const canvas = options.canvas ?? createCanvas(options.before);
  let chunk: RenderChunk | null = null;
  let renderer: WorldRenderer | null = null;
  let overlay: OverlayHost | null = null;
  /** Frames drawn with each camera source (`__flyffDebug.renderHost.camera`). */
  const cameraFrames = { input: 0, fallback: 0, none: 0, qa: 0 };
  let lastCameraSource: 'input' | 'fallback' | 'none' | 'qa' | null = null;
  let building: Promise<void> | null = null;
  let failed = false;
  let desc: WorldRenderDesc | null = null;
  const life = { disposed: false };
  const isDisposed = (): boolean => life.disposed;
  let removeResize: (() => void) | null = null;
  const outPos: [number, number, number] = [0, 0, 0];

  const fetchTexture = async (
    path: string,
    prio: Prio,
    signal: AbortSignal | undefined,
  ): Promise<ArrayBuffer> => {
    const normal = normalizeResourcePath(path) ?? path;
    try {
      return await ctx.assets.fetchBytes(
        { ns: 'res', path: normal },
        prio,
        signal ? { signal } : {},
      );
    } catch (e) {
      if (e instanceof AssetError && e.missing && chunk) throw new chunk.TextureNotFoundError(path);
      throw e;
    }
  };

  const attachMovers = async (c: RenderChunk, r: WorldRenderer): Promise<void> => {
    try {
      const [models, items, movers] = await Promise.all([
        ctx.bundles.ensure('models', Prio.Critical),
        ctx.bundles.ensure('items', Prio.Critical),
        ctx.bundles.ensure('movers', Prio.Critical),
      ]);
      if (isDisposed()) return;
      const registry = new c.VisualRegistry(models, items, movers);
      const provider = c.createManagerModelProvider({
        manager: ctx.assets,
        models: r.models,
        log: (message) => {
          ctx.log.debug('render', message);
        },
      });
      const motions = motionsOf(ctx);
      const decor = decorSink(ctx, motions);
      r.attachMovers({
        registry,
        models: provider,
        sim: {
          setVisible: (id, visible, wantAttach) => {
            ctx.sim.cmd.setVisible(id, visible, wantAttach);
          },
        },
        // the ground height of the items that drop (the sim's `item_height`, one query per drop inside the bridge's Q budget)
        rayCast: (o, d, t, m) => ctx.sim.rayCast(o, d, t, m),
        // the decor entries of the sim (WP-REND-23): ctrls and animated statics are visible-toggled and posed by the sim
        decor,
        // the rides and wings of flying movers (WP-REND-25): an attached decor entry per ride, a wing's skeleton and clips
        rides: rideAssets(ctx, decor, motions),
        ctrlSfx: (id) => ctx.data?.ctrl(id)?.sfx_ctrl,
        onAppearanceChanged: (id) => {
          worldRuntime(ctx).params.mark(id);
        },
        dispose: () => {
          provider.release();
        },
      });
      // the bounds of a player are the union of the parts the renderer loads for the look (Plan/24 10.6): the sim computes the
      // box from the `FLMB` entries of exactly these files
      worldRuntime(ctx).parts = {
        filesOf: (visual) =>
          c
            .resolveVisualPlan(visual, registry, { activeMover: false })
            .elements.flatMap((el) => (el.file === null ? [] : [el.file])),
      };
    } catch (error) {
      ctx.log.error(
        'render',
        `the mover visuals could not start: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  };

  /** The game side of the skill effect classes (WP-REND-24): the hooks of the classes and the duel flags. */
  const skillFx = createSkillFxHost(ctx);

  /** The effect runtime (WP-REND-18): `.sfx` files through the asset manager, the `sfx-table` rows of the client bundles. */
  const attachSfx = async (c: RenderChunk, r: WorldRenderer): Promise<void> => {
    skillFx.reset();
    // the option "hide pets" also silences the spawn effect of a pet (`g_Option.m_OffPets`)
    const options = defineOptionsStore(ctx.stores.registry);
    fxRuntime(ctx).hooks.hidePets = () => options.getState().render.hidePets;
    if (!c.createManagerSfxSource || typeof r.attachSfx !== 'function') return;
    r.setSkillHooks(skillFx.hooks);
    try {
      await ctx.bundles.ensure('sfx-table', Prio.Critical);
      if (isDisposed()) return;
      const source = c.createManagerSfxSource(ctx.assets);
      // the MESH parts of the effects load their models like any other model
      const models = c.createManagerModelProvider({
        manager: ctx.assets,
        models: r.models,
        log: (message) => {
          ctx.log.debug('render', message);
        },
      });
      r.attachSfx({
        source,
        models,
        catalog: {
          row: (xi) => {
            const row = ctx.data?.sfx(xi);
            return row
              ? {
                  file: row.file,
                  scale: row.scale,
                  distant: row.distant,
                  renderFlag: row.render_flag,
                }
              : undefined;
          },
        },
        dispose: () => {
          source.release();
          models.release();
        },
      });
    } catch (error) {
      ctx.log.error(
        'render',
        `the effects could not start: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  };

  /** The advisory of the probe, once per renderer build: the device could manage `low` but the renderer starts in `tier`. */
  const advise = (tier: string): void => {
    const caps = ctx.caps;
    if (caps.recommendedTier !== 'low' || tier === 'low') return;
    const gpu = caps.renderer ?? 'unknown renderer';
    const why = caps.softwareGl
      ? `this browser renders without a GPU (${gpu})`
      : `this device looks small (${String(caps.hardwareConcurrency ?? '?')} cores, ${String(caps.deviceMemory ?? '?')} GB memory; ${gpu})`;
    ctx.log.info('render', `${why}: ?tier=low may run smoother`);
  };

  const build = async (): Promise<void> => {
    try {
      const c = await loadChunk();
      chunk = c;
      const pool = ctx.workers;
      if (!pool) throw new Error('the decode workers are not started');
      const [terrain, models] = await Promise.all([
        ctx.bundles.ensure('terrain', Prio.Critical),
        ctx.bundles.ensure('models', Prio.Critical),
      ]);
      if (isDisposed()) return;
      // the overlay half of `renderer.fx` (`ctx.render.fx`): requests before the layer exists are dropped (cosmetic)
      const ov = createOverlayHost(ctx, c, canvas);
      const r = new c.WorldRenderer({
        // every effect the factory creates also plays the sound of its `sfx-table` row (Plan/44 T41/T42)
        fxWrap: (base) =>
          withAudioFx(
            ov.compose(base),
            () => ctx.audio.cues,
            (id) => {
              const out: [number, number, number] = [0, 0, 0];
              const view = ctx.sim.view;
              return view?.valid === true && view.posOf(id, out) ? out : undefined;
            },
          ),
        textureLoader: c.createPoolTextureLoader(pool, {
          fetch: (path, _form, prio, signal) => fetchTexture(path, prio, signal),
        }),
        onMissingTexture: (path, reason) => {
          ctx.log.debug('render', `texture ${path}: ${reason}`);
        },
        onWorldLog: (message) => {
          ctx.log.debug('render', message);
        },
      });
      const dpr = typeof window === 'object' ? window.devicePixelRatio : 1;
      // Medium (the original options) on every device; `?tier=low|medium|high` is the player's choice (owner 2026-10-29)
      const tier = startTier(
        options.search ?? (typeof location === 'object' ? location.search : ''),
      );
      ctx.log.info(
        'render',
        `the renderer starts in the ${tier} tier (device pixel ratio ${String(dpr)})`,
      );
      advise(tier);
      await r.init(canvas, c.tierPreset(tier, dpr), ctx.features);
      if (isDisposed()) {
        r.dispose();
        return;
      }
      // the 2-D layer above the world canvas, after the device exists (its size and stacking follow the canvas); names and
      // balloons are cosmetic: a failure leaves the world running
      try {
        ov.attach(r);
        overlay = ov;
        worldVisible();
      } catch (error) {
        ov.dispose();
        ctx.log.error(
          'render',
          `the overlay could not start: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
      const mp = c.createManagerPorts(ctx.assets);
      r.setWorldAssets({
        lands: mp.lands,
        models: mp.models,
        registry: c.createWorldRegistry(terrain, models),
      });
      // the world's movers (WP-REND-12): visuals from the client bundles, models and skeletons through the asset manager,
      // `SET_VISIBLE` to the sim; a failure here leaves terrain and statics working
      await attachMovers(c, r);
      // the effects (SFX): a failure here leaves the world without effects
      await attachSfx(c, r);
      // the loop's upload step runs the renderer's queue; the context loss is coordinated with the asset manager
      const uploads = createUploads({
        queue: r.uploads,
        loading: () => ctx.machine.screen.s === 'Loading',
        hidden: () => ctx.background.hidden,
      });
      ctx.uploads = uploads satisfies UploadsApi;
      const loss = coordinateContextLoss(ctx.assets, r.accounting);
      r.device.onContextLost(() => {
        const marked = loss.lost();
        ctx.log.warn('render', `WebGL context lost: ${String(marked)} assets marked for re-upload`);
      });
      r.device.onContextRestored(() => {
        const queued = loss.restored();
        ctx.log.info('render', `WebGL context restored: ${String(queued)} uploads queued`);
      });
      c.installRenderProbe(ctx.probe, r);
      if (typeof window === 'object') {
        const onResize = (): void => {
          r.resize(
            canvas.clientWidth || window.innerWidth,
            canvas.clientHeight || window.innerHeight,
          );
        };
        window.addEventListener('resize', onResize);
        removeResize = () => {
          window.removeEventListener('resize', onResize);
        };
      }
      renderer = r;
      if (desc) r.setWorld(desc);
    } catch (error) {
      failed = true;
      ctx.log.error(
        'render',
        `the world renderer could not start: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  };

  const ensure = (): Promise<void> => (building ??= build());

  /** The overlay layer follows the visibility of the world canvas (no world, no names). */
  const worldVisible = (): void => {
    overlay?.worldChanged(desc !== null);
  };

  const port: RenderApi = {
    tick(due) {
      renderer?.tick(due);
      overlay?.tick(due);
    },
    // `renderer.fx` with the overlay composed in (`WorldRendererOptions.fxWrap`); absent until the renderer is built
    get fx() {
      return renderer?.fx;
    },
    // ---- housing / guild house (ui-house-quiz): the placement ghost of `features/events/deploy`, read at the moment of use
    get deploy() {
      return renderer?.deploy ?? undefined;
    },
    draw(frame: RenderFrame) {
      const r = renderer;
      if (!r || desc === null) return;
      const view = frame.view as RenderViewReader | null;
      const rt = worldRuntime(ctx);
      const self = ctx.entities.self();
      let active: Vec3LH | null = null;
      if (view?.valid === true && self && view.posOf(self.objId, outPos)) {
        active = [outPos[0], outPos[1], outPos[2]];
      } else {
        const p = rt.activePos();
        if (p) active = [p.x, p.y, p.z];
      }
      // the target brackets and the charge gauge of the targeting (WP-REND-20; 43 section 7): the overlay draws them
      const tv = ctx.input.targetView?.();
      if (tv) {
        const focus = tv.focus ?? 0;
        // red: a hostile focus (43 sets `renderTarget` for a non-peaceful mover or a player); the queued next target is white
        // the fly targets of the frame come with it: the overlay boxes them (43 section 9.4, WP-REND-25)
        r.setTargets({
          focus,
          next: tv.autoAttack ? (tv.nextTarget ?? 0) : 0,
          red: focus !== 0 && tv.renderTarget === focus,
          flyTargets: tv.flyTargets ?? [],
        });
        const g = tv.gauge ?? 0;
        const gauges: GaugeState = {
          charge: g >= 4 ? 4 : g === 3 ? 3 : g === 2 ? 2 : g === 1 ? 1 : 0,
        };
        // the casting bar of the own skill (`UseSkillNtf` cast time; WP-SKILL-12)
        const cast = peekSkillsRuntime(ctx)?.castGauge(frame.nowMs);
        if (cast) gauges.casting = cast;
        // the turbo bar and the speed of a flying player (WP-REND-25; the fuel is the mirror of `SetFuel` + the burn of the turbo)
        const flightFlags = self && view?.valid === true ? view.stateFlagsOf(self.objId) : 0;
        if ((flightFlags & STAF.FLY) !== 0) {
          const fl = flightOf(ctx);
          gauges.fuel = [fl.accFuel, FUEL_GAUGE_MAX];
          gauges.turbo = (flightFlags & STAF.TURBO) !== 0;
          gauges.speed = fl.speed;
        }
        r.setGauges(gauges);
      }
      skillFx.frame();
      // a visual-QA camera (`test.setCamera` / `test.cameraPath`, WP-REND-17) wins over the input camera: captures only
      const qa = qaCamera(ctx, frame.nowMs, active);
      const sample = qa ?? (frame.camera as CameraState | null);
      const camera = sample ?? (active ? fallbackCamera(active) : null);
      lastCameraSource = qa ? 'qa' : sample ? 'input' : camera ? 'fallback' : 'none';
      cameraFrames[lastCameraSource] += 1;
      if (!camera) return;
      // `worldStore.env`: the season of `EnvironmentAll` and the effect toggle of `Environment` (the sky's weather, Plan/41 15)
      const env = ctx.stores.worldStore.getState().env;
      r.draw({
        view,
        camera,
        gameClock: rt.environment.sample(),
        nowMs: frame.nowMs,
        entities: ctx.entities,
        weather: { season: env.season, effect: env.weather !== 0 },
        ...(active ? { activePos: active } : {}),
      });
      // names, balloons and damage numbers of the frame just drawn (same camera, same drawn movers)
      overlay?.draw(r, camera, frame.nowMs);
    },
    setWorld(world) {
      desc = world;
      canvas.style.display = world ? 'block' : 'none';
      // a new world or none: balloons, emoticons and numbers of the old one are gone (`ClearAllMessage`)
      worldVisible();
      if (world) {
        void ensure().then(() => {
          // the world may have changed again while the renderer was starting
          if (renderer && desc === world) renderer.setWorld(world);
        });
      } else {
        renderer?.setWorld(null);
      }
    },
    worldGate() {
      if (desc === null) return null;
      if (failed) return null;
      if (!renderer) return STARTING_GATE;
      return renderer.worldGate();
    },
    // the own player's visual is part of the gate (WP-REND-12): complete and showed a pose; nothing to wait for after a failed start
    ownModelReady(objId) {
      if (failed) return true;
      return renderer ? renderer.ownModelReady(objId) : false;
    },
    // picking support for 43 (WP-REND-15): empty / null until the renderer is up
    visibleObjects: () => renderer?.visibleObjects() ?? [],
    raycastPrecise: (id, ray) => renderer?.raycastPrecise(id, ray) ?? null,
    projectBounds: (id) => renderer?.projectBounds(id) ?? null,
    setHighlight: (id) => {
      renderer?.setHighlight(id);
    },
    // the click-to-move mark of 43 (the base renderer counts the request, WP-REND-18 draws it)
    moveMark: (pos, normal) => {
      renderer?.fx.moveMark(pos, normal);
    },
    // the PvP countdown sprite of the overlay (WP-REND-25)
    pvpCountdown: (n) => {
      overlay?.countdown(n);
    },
    pvpCountdownDrawn: () => overlay?.countdownDrawn() ?? false,
  };

  // the camera of each drawn frame: the sample of 43 once there is one, the stand-in before (probe `renderHost`)
  if (!ctx.probe.sections().includes('renderHost')) {
    ctx.probe.extend('renderHost', {
      camera: () => ({ last: lastCameraSource, frames: { ...cameraFrames } }),
    });
  }

  return {
    port,
    canvas,
    renderer: () => renderer,
    overlay: () => overlay,
    ready: () => (building ?? Promise.resolve()).then(() => undefined),
    get failed() {
      return failed;
    },
    dispose() {
      life.disposed = true;
      removeResize?.();
      removeResize = null;
      overlay?.dispose();
      overlay = null;
      renderer?.dispose();
      renderer = null;
      canvas.remove();
    },
  };
}
