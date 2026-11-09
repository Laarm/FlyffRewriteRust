// The preview service of the title screens (Plan/41 §21, Plan/42 §8.12, 45 §4.1/§5.1, WP-CFEAT-03/04): turns the
// `PreviewPort` the character select/create shells ask for into the REAL renderer of doc 41. Everything that needs three.js
// is imported lazily (the `render/` chunk), so the login path never pays for it.
//
// What it builds, once, on the first preview request:
//   * a `WorldRenderer` on a hidden canvas (the previews render into render targets of its context and are copied into the
//     DOM canvases of the windows asynchronously, <= 15 fps),
//   * the texture loader over the decode pool and the asset layer (`/res` files),
//   * the model provider (`.o3d`, `.chr`, `.ani` through the pool),
//   * the `VisualRegistry` over the `models`, `items` and `movers` bundles,
//   * a local `SimWorld` (the main wasm instance) with the motion table (`FLMT`) and a `SimPreviewHost` that hosts the
//     PREVIEW movers (`MTI_GETUP` -> `MTI_RUN`, `MTI_SIT` -> `MTI_SITSTAND`, `MTI_STAND`), and
//   * the `PreviewRenderer` plus a rAF loop that steps the previews while any is open.
//
// The game's `SimBridge` (WP-CLIENT-18) has no preview movers yet; when it gains `PreviewMotionSink` the service can use it
// instead of its own `SimWorld` (the seam is `createRuntime`). The renderer is owned by this service and disposed with it
// (`dispose()`, called when the world loads and the world renderer takes over, or when the page unloads).

import type { ClientContext } from '../../app/context';
import { AssetError } from '../../assets/errors';
import { normalizeResourcePath } from '../../assets/paths';
import type { CorePreviewHandle, CorePreviewPort } from '../../ui/windows/char/ports';
import type { PreviewHandleLike, PreviewPort, PreviewRequest } from '../../ui/windows/login/ports';
import { Prio } from '../../workers/priority';
import type { MoverVisualState } from '../../game/entities';
import type { WorldRenderer } from '../../render';
import type { buildSkeletonRender as buildSkeletonRenderFn } from '../../render/movers/skeleton';

/** One row of `__flyffDebug.render.previews` (Plan/41 §21.3): `motions` is the requested chain, `motion` its first entry. */
export interface PreviewProbeRow {
  readonly slot: number;
  readonly preset: string;
  readonly motion: number;
  readonly motions?: readonly number[];
  readonly nonEmptyPixels: number;
}

/** The slice of 41's `PreviewHandle` this module drives. */
interface RealHandle {
  setSelected(selected: boolean): void;
  setSubject(subject: MoverVisualState): void;
  /** The extra model yaw of the paper doll (41's `PreviewHandle.setRotation`). */
  setRotation?(degrees: number): void;
  invalidate(): void;
  readonly nonEmptyPixels: number;
  readonly framesPresented: number;
  readonly ready: Promise<void>;
}

/** Everything the service builds on the first request (and tears down in `dispose`). */
export interface PreviewRuntime {
  open(request: PreviewRequest): RealHandle | null;
  close(handle: RealHandle): void;
  /** Steps the preview simulation and renders the previews that are due. */
  frame(nowMs: number, dtMs: number): void;
  /** `__flyffDebug.render.previews`. */
  probe(): readonly PreviewProbeRow[];
  dispose(): void;
}

export interface PreviewService {
  /** The port the shells open previews through. */
  readonly port: PreviewPort;
  /** The port of the core windows (the inventory paper doll, the status portrait; WP-CFEAT-11): the same service and runtime. */
  readonly corePort: CorePreviewPort;
  /** Resolves when the runtime is built (or failed) and every open preview is resident. */
  ready(): Promise<void>;
  /** The probe rows of the open previews. */
  probe(): readonly PreviewProbeRow[];
  /** `true` once the runtime failed (no WebGL2, no data): the canvases stay empty. */
  readonly failed: boolean;
  dispose(): void;
}

/** A handle the shell holds immediately; the real one attaches when the runtime is built. */
class LazyHandle implements PreviewHandleLike, CorePreviewHandle {
  real: RealHandle | null = null;
  closed = false;
  selected: boolean;
  subject: MoverVisualState;
  rotation = 0;

  constructor(
    readonly request: PreviewRequest,
    private readonly owner: { closeLazy(h: LazyHandle): void },
  ) {
    this.selected = request.selected === true;
    this.subject = request.subject as MoverVisualState;
  }

  setSelected(selected: boolean): void {
    this.selected = selected;
    this.real?.setSelected(selected);
  }
  setSubject(subject: unknown): void {
    this.subject = subject as MoverVisualState;
    this.real?.setSubject(this.subject);
  }
  setRotation(degrees: number): void {
    this.rotation = degrees;
    this.real?.setRotation?.(degrees);
  }
  invalidate(): void {
    this.real?.invalidate();
  }
  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.owner.closeLazy(this);
  }
}

/** Injectable seam of the service (tests replace the runtime, the clock and the frame scheduler). */
export interface PreviewServiceDeps {
  readonly createRuntime: (ctx: ClientContext) => Promise<PreviewRuntime>;
  readonly requestFrame?: (fn: (nowMs: number) => void) => number;
  readonly cancelFrame?: (id: number) => void;
}

export function createPreviewService(
  ctx: ClientContext,
  deps: PreviewServiceDeps = { createRuntime: createRuntime },
): PreviewService {
  const open = new Set<LazyHandle>();
  let runtime: PreviewRuntime | null = null;
  let building: Promise<void> | null = null;
  let failed = false;
  let disposed = false;
  let raf = 0;
  let last = 0;
  const request = deps.requestFrame ?? ((fn) => requestAnimationFrame(fn));
  const cancel =
    deps.cancelFrame ??
    ((id) => {
      cancelAnimationFrame(id);
    });

  const attach = (h: LazyHandle): void => {
    if (!runtime || h.closed || h.real) return;
    h.real = runtime.open({ ...h.request, subject: h.subject, selected: h.selected });
    if (h.rotation !== 0) h.real?.setRotation?.(h.rotation);
  };

  const loop = (now: number): void => {
    raf = 0;
    if (disposed || !runtime || open.size === 0) return;
    const dt = last === 0 ? 0 : now - last;
    last = now;
    try {
      runtime.frame(now, dt);
    } catch (e) {
      ctx.log.error('preview', `frame failed: ${e instanceof Error ? e.message : String(e)}`);
    }
    raf = request(loop);
  };

  const ensureLoop = (): void => {
    if (raf === 0 && runtime && open.size > 0 && !disposed) {
      last = 0;
      raf = request(loop);
    }
  };

  const build = (): Promise<void> => {
    building ??= deps
      .createRuntime(ctx)
      .then((r) => {
        if (disposed) {
          r.dispose();
          return;
        }
        runtime = r;
        for (const h of open) attach(h);
        ensureLoop();
      })
      .catch((e: unknown) => {
        failed = true;
        ctx.log.warn(
          'preview',
          `the 3-D previews are unavailable: ${e instanceof Error ? e.message : String(e)}`,
        );
      });
    return building;
  };

  const openLazy = (req: PreviewRequest): LazyHandle | null => {
    if (disposed) return null;
    const h = new LazyHandle(req, service);
    open.add(h);
    if (runtime) {
      attach(h);
      ensureLoop();
    } else {
      void build();
    }
    return h;
  };

  const service: PreviewService & { closeLazy(h: LazyHandle): void } = {
    port: {
      open: (req) => openLazy(req),
    },
    corePort: {
      open: (req) => openLazy({ canvas: req.canvas, preset: req.preset, subject: req.subject }),
    },
    async ready() {
      await build();
      await Promise.all([...open].map((h) => h.real?.ready ?? Promise.resolve()));
    },
    probe: () => runtime?.probe() ?? [],
    get failed() {
      return failed;
    },
    closeLazy(h) {
      open.delete(h);
      if (h.real && runtime) runtime.close(h.real);
      h.real = null;
      if (open.size === 0 && raf !== 0) {
        cancel(raf);
        raf = 0;
      }
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      if (raf !== 0) cancel(raf);
      raf = 0;
      for (const h of [...open]) {
        h.real = null;
      }
      open.clear();
      runtime?.dispose();
      runtime = null;
    },
  };
  return service;
}

// ---- the real runtime ---------------------------------------------------------------------------------------------------

/** Maps a failed asset request of a texture to the "not found" error the texture cache treats as final. */
async function fetchRaw(
  ctx: ClientContext,
  path: string,
  prio: Prio,
  signal?: AbortSignal,
): Promise<ArrayBuffer> {
  const normal = normalizeResourcePath(path) ?? path;
  try {
    return await ctx.assets.fetchBytes({ ns: 'res', path: normal }, prio, signal ? { signal } : {});
  } catch (e) {
    if (e instanceof AssetError && e.missing) {
      const { TextureNotFoundError } = await import('../../render/gl/textures');
      throw new TextureNotFoundError(path);
    }
    throw e;
  }
}

/** Builds the renderer, the data and the preview host over the live asset layer. Rejects when WebGL2 or the data is missing. */
export async function createRuntime(ctx: ClientContext): Promise<PreviewRuntime> {
  const pool = ctx.workers;
  if (!pool) throw new Error('the decode workers are not started');
  const [render, motionIndex, skeletonModule, simFactory, quality] = await Promise.all([
    import('../../render'),
    import('../../render/movers/motionIndex'),
    import('../../render/movers/skeleton'),
    import('../../wasm/simFactory'),
    import('../../render/quality'),
  ]);
  const [models, items, movers, motions] = await Promise.all([
    ctx.bundles.ensure('models', Prio.Critical),
    ctx.bundles.ensure('items', Prio.Critical),
    ctx.bundles.ensure('movers', Prio.Critical),
    ctx.bundles.ensure('motions', Prio.Critical),
  ]);

  const hidden = document.createElement('canvas');
  hidden.width = 1;
  hidden.height = 1;
  hidden.dataset['testid'] = 'preview-gl';
  hidden.style.cssText =
    'position:fixed;left:0;top:0;width:1px;height:1px;opacity:0;pointer-events:none;';
  document.body.append(hidden);

  const renderer = new render.WorldRenderer({
    onMissingTexture: (path, reason) => {
      ctx.log.debug('preview', `texture ${path}: ${reason}`);
    },
  });
  try {
    await renderer.init(hidden, { ...quality.DEFAULT_RENDER_OPTIONS }, ctx.features);
  } catch (e) {
    hidden.remove();
    throw e;
  }
  renderer.uploads.mode = 'loading';
  renderer.setTextureLoader(
    render.createPoolTextureLoader(pool, {
      fetch: (path, _form, prio, signal) => fetchRaw(ctx, path, prio, signal),
    }),
  );

  const registry = new render.VisualRegistry(models, items, movers);
  const provider = createModelProvider(ctx, renderer, skeletonModule.buildSkeletonRender);
  const sim = simFactory.createSimWorld();
  const flmt = await ctx.assets.fetchBytes({ ns: 'gen', kind: 'motion-table' }, Prio.Critical);
  sim.load_motion_table(new Uint8Array(flmt));
  const host = new render.SimPreviewHost(
    sim,
    simFactory.simMemory,
    provider,
    motionIndex.buildMotionIndex(motions),
    renderer.device.textureHost,
  );
  const previews = new render.PreviewRenderer({
    backend: render.createWebGlPreviewBackend(renderer),
    host: renderer,
    models: provider,
    registry,
    pose: host.pose,
    sink: host,
    runOnCharSelect: ctx.features.features.run_on_charselect,
  });
  renderer.attachPreviews(previews);
  // `__flyffDebug.render.previews` (Plan/41 §21.3) for the e2e pages; a world renderer that installs the section later finds it
  // taken. In the world (the section exists) the previews of the core windows list through `addPreviewProbeRenderer`: this
  // renderer's previews appear in `render.previews` without taking over the stats of the world renderer.
  if (ctx.probe.test) {
    try {
      if (!ctx.probe.sections().includes('render')) render.installRenderProbe(ctx.probe, renderer);
      else render.addPreviewProbeRenderer(ctx.probe, renderer);
    } catch (e) {
      ctx.log.debug('preview', `render probe: ${e instanceof Error ? e.message : String(e)}`);
    }
  }
  let accMs = 0;

  return {
    open(req) {
      return previews.open({
        canvas: req.canvas,
        preset: req.preset,
        subject: req.subject as MoverVisualState,
        animate: req.animate ?? true,
        ...(req.selected === undefined ? {} : { selected: req.selected }),
      });
    },
    close(handle) {
      previews.close(handle as unknown as Parameters<typeof previews.close>[0]);
    },
    frame(nowMs, dtMs) {
      accMs += Math.max(0, dtMs);
      const frames = Math.min(60, Math.floor(accMs / (1000 / 60)));
      accMs -= frames * (1000 / 60);
      if (frames > 0) host.step(frames);
      host.update(1);
      renderer.runUploads();
      previews.render(nowMs);
    },
    probe: () => previews.probe(),
    dispose() {
      previews.dispose();
      renderer.attachPreviews(null);
      host.dispose();
      renderer.dispose();
      hidden.remove();
    },
  };
}

/** Models and skeletons through the decode pool into the renderer's `ModelCache` (a missing file answers `null`: KQ-30). */
function createModelProvider(
  ctx: ClientContext,
  renderer: WorldRenderer,
  buildSkeletonRender: typeof buildSkeletonRenderFn,
) {
  const pool = ctx.workers;
  if (!pool) throw new Error('the decode workers are not started');
  type Geometry = Awaited<ReturnType<typeof renderer.models.add>>;
  const models = new Map<string, Promise<Geometry | null>>();
  const chrs = new Map<
    string,
    Promise<{ render: ReturnType<typeof buildSkeletonRender>; fsk1: Uint8Array } | null>
  >();
  const anis = new Map<string, Promise<Uint8Array | null>>();
  const note = (what: string, e: unknown): null => {
    ctx.log.debug('preview', `${what}: ${e instanceof Error ? e.message : String(e)}`);
    return null;
  };
  const bytes = (path: string): Promise<ArrayBuffer> => fetchRaw(ctx, path, Prio.Visible);

  const chr = (path: string) => {
    let p = chrs.get(path);
    if (!p) {
      p = bytes(path)
        .then(async (buf) => {
          const r = await pool.run({
            kind: 'chr',
            bytes: buf,
            prio: Prio.Visible,
            opts: { space: 'three' },
          });
          if (!r.blob) throw new Error('no blob');
          const blob = new Uint8Array(r.blob);
          return { render: buildSkeletonRender(path, r.meta, blob), fsk1: blob };
        })
        .catch((e: unknown) => note(path, e));
      chrs.set(path, p);
    }
    return p;
  };

  return {
    model(path: string) {
      let p = models.get(path);
      if (!p) {
        p = bytes(path)
          .then(async (buf) => {
            const r = await pool.run({
              kind: 'o3d',
              bytes: buf,
              prio: Prio.Visible,
              opts: {
                requested_name: path.split('/').pop() ?? path,
                reject_name_mismatch: false,
              },
            });
            if (!r.blob) throw new Error('no mesh data');
            return renderer.models.add(path, r.meta, new Uint8Array(r.blob));
          })
          .catch((e: unknown) => note(path, e));
        models.set(path, p);
      }
      return p;
    },
    async skeleton(path: string) {
      return (await chr(path))?.render ?? null;
    },
    async skeletonPack(path: string) {
      return (await chr(path))?.fsk1 ?? null;
    },
    clipPack(path: string) {
      let p = anis.get(path);
      if (!p) {
        p = bytes(path)
          .then(async (buf) => {
            const r = await pool.run({ kind: 'ani', bytes: buf, prio: Prio.Visible });
            return r.blob ? new Uint8Array(r.blob) : null;
          })
          .catch((e: unknown) => note(path, e));
        anis.set(path, p);
      }
      return p;
    },
  };
}
