// The texture cache (Plan/41 section 6.1-6.2, WP-REND-02): canonical path -> GPU texture, ref-counted, with the
// sampler presets, the S3TC passthrough / RGBA8 fallback / RGB565 / RGBA5551 / PNG upload paths of `dds.ts`, the
// missing-texture policy and GPU byte accounting. The cache does not fetch or decode: a `TextureLoader` (the
// asset manager + the worker pool in production, `loader.ts`) delivers decoded packs, the uploads run through the
// `UploadQueue` under the per-frame budget.
//
// Texture objects are three.js `ExternalTexture`s wrapping a WebGL texture that `dds.ts` created and filled
// (immutable storage, exact level count, `TEXTURE_MAX_LEVEL`), because three.js has no RGB565 type and no
// max-level API. A texture that is not resident (loading, failed, lost) has `sourceTexture === null` and
// samples black: consumers hide the draw until `state` is `ready` or `failed` (Plan/41 section 22.4) and use the
// shared white texture for failed ones ("untextured white", ref/18 section 8.1, KEEP-QUIRK).

import { ExternalTexture, type Texture } from 'three';
import type { DdsMeta } from '../../gen/formats';
import { Prio } from '../../workers/priority';
import type { DeviceCaps } from './caps';
import {
  blobLevelData,
  planBitmapUpload,
  planDdsUpload,
  planSfxUpload,
  type LevelData,
  type PlanOptions,
  type PlannedLevel,
  type TextureStateLike,
  type UploadPlan,
  uploadPlan,
  uploadSolidTexture,
} from './dds';
import { quantizeRgba5551 } from './sfxQuantize';
import { SAMPLER_PRESETS, type SamplerPresetName } from './samplers';
import { view } from '../../wasm/decoders';
import { type GpuAccounting, type UploadQueue } from './uploadQueue';

// ---- loader contract --------------------------------------------------------------------------------------

/** A decoded texture as the worker pool delivers it. */
export type DecodedTexture =
  | { readonly kind: 'dds'; readonly meta: DdsMeta; readonly blob: Uint8Array }
  | { readonly kind: 'bitmap'; readonly bitmap: ImageBitmap };

export interface TextureLoadRequest {
  /** Canonical VFS path (lower-case, `/`). */
  readonly path: string;
  /** SFX texture: decode to RGBA8 (`dds_bc`) whatever the device supports, the cache quantises to RGBA5551. */
  readonly sfx: boolean;
  readonly prio: Prio;
  readonly distance: number;
}

/** The file does not exist on the server (404) or is not client-visible: the texture fails without retry. */
export class TextureNotFoundError extends Error {
  constructor(path: string) {
    super(`texture ${path} not found`);
    this.name = 'TextureNotFoundError';
  }
}

export interface TextureLoader {
  load(req: TextureLoadRequest): Promise<DecodedTexture>;
}

// ---- handles ----------------------------------------------------------------------------------------------

export type TextureState = 'loading' | 'ready' | 'failed' | 'lost';

/** A reference to a cached texture; `release()` is idempotent. */
export interface TextureHandle {
  readonly key: string;
  readonly path: string;
  readonly state: TextureState;
  /** The texture to sample: the real one when `ready`, the shared white when `failed`, else the (unusable) own object. */
  readonly texture: Texture;
  /** Uploaded GPU bytes (0 until `ready`). */
  readonly gpuBytes: number;
  /** Settles when the state leaves `loading` (`ready` or `failed`; never rejects). */
  readonly ready: Promise<TextureHandle>;
  /** The plan of the upload, for probes and tests (`null` until `ready`). */
  readonly plan: UploadPlan | null;
  onChange(listener: (handle: TextureHandle) => void): () => void;
  release(): void;
}

export interface AcquireOptions {
  readonly prio?: Prio;
  readonly distance?: number;
  /** SFX texture (RGBA5551 conversion, `sfx` sampler unless `preset` is given). */
  readonly sfx?: boolean;
}

/** The renderer part the cache needs (a `WebGLRenderer` or a test double). */
export interface TextureRenderer {
  getContext(): WebGL2RenderingContext;
  readonly state: TextureStateLike;
}

export interface TextureCacheDeps {
  readonly renderer: TextureRenderer;
  readonly caps: DeviceCaps;
  readonly loader: TextureLoader;
  readonly uploads: UploadQueue;
  readonly accounting: GpuAccounting;
  /** Read at plan time (RenderOptions.fullMips / anisotropy). */
  readonly options?: () => { fullMips: boolean; anisotropy: number };
  /** Called ONCE per path that cannot be shown (missing file, decode failure, upload failure). */
  readonly onMissing?: (path: string, reason: string) => void;
  /** `gl.getError()` after every upload (tests). */
  readonly checkErrors?: boolean;
}

interface Entry {
  readonly key: string;
  readonly path: string;
  readonly preset: SamplerPresetName;
  readonly sfx: boolean;
  readonly prio: Prio;
  readonly distance: number;
  refs: number;
  state: TextureState;
  readonly external: ExternalTexture;
  gl: WebGLTexture | null;
  gpuBytes: number;
  plan: UploadPlan | null;
  lastUsed: number;
  generation: number;
  readonly listeners: Set<(h: TextureHandle) => void>;
  promise: Promise<TextureHandle>;
  resolve: (h: TextureHandle) => void;
}

export interface TextureCacheStats {
  readonly entries: number;
  readonly ready: number;
  readonly loading: number;
  readonly failed: number;
  readonly missing: number;
  readonly gpuBytes: number;
}

/** The cache key of `path` for a preset and the options (`acquire` and `peekReady` agree on it). */
function keyOf(
  path: string,
  preset: SamplerPresetName,
  options: AcquireOptions,
): { key: string; effective: SamplerPresetName; sfx: boolean } {
  const sfx = options.sfx === true;
  const effective: SamplerPresetName = sfx && preset === 'world' ? 'sfx' : preset;
  return { key: `${effective}${sfx ? ':sfx' : ''}:${path}`, effective, sfx };
}

/** The ref-counted texture cache. */
export class TextureCache {
  private readonly entries = new Map<string, Entry>();
  private readonly reported = new Set<string>();
  private white: ExternalTexture | null = null;
  private whiteGl: WebGLTexture | null = null;
  private readonly solids = new Map<
    string,
    {
      external: ExternalTexture;
      gl: WebGLTexture | null;
      rgba: readonly [number, number, number, number];
    }
  >();
  private lost = false;
  private generation = 0;
  private disposed = false;
  /** Paths that failed (for the `missingTextures` counter). */
  missing = 0;

  constructor(private readonly deps: TextureCacheDeps) {}

  private planOptions(): PlanOptions {
    const o = this.deps.options?.() ?? { fullMips: false, anisotropy: 1 };
    return {
      s3tc: this.deps.caps.s3tc,
      fullMips: o.fullMips,
      anisotropy: o.anisotropy,
      maxAnisotropy: this.deps.caps.maxAnisotropy,
    };
  }

  /** The shared white texture of untextured models and failed textures. */
  whiteTexture(): Texture {
    this.white ??= new ExternalTexture(null);
    if (this.whiteGl === null && !this.lost) {
      const gl = this.deps.renderer.getContext();
      this.whiteGl = uploadSolidTexture(gl, this.deps.renderer.state, [255, 255, 255, 255]);
      this.white.sourceTexture = this.whiteGl;
    }
    return this.white;
  }

  /** A cached 1x1 texture of one colour (neutral placeholders such as the sphere map before `etc_reflect` loaded). */
  solidTexture(rgba: readonly [number, number, number, number]): Texture {
    const key = rgba.join(',');
    let s = this.solids.get(key);
    if (!s) {
      s = { external: new ExternalTexture(null), gl: null, rgba };
      this.solids.set(key, s);
    }
    if (s.gl === null && !this.lost) {
      const gl = this.deps.renderer.getContext();
      s.gl = uploadSolidTexture(gl, this.deps.renderer.state, rgba);
      s.external.sourceTexture = s.gl;
    }
    return s.external;
  }

  /**
   * Acquires `path` with a sampler preset. A resident or loading entry of the same path and preset is shared.
   * SFX textures default to the `sfx` preset and the RGBA5551 conversion.
   */
  acquire(
    path: string,
    preset: SamplerPresetName = 'world',
    options: AcquireOptions = {},
  ): TextureHandle {
    const { key, effective, sfx } = keyOf(path, preset, options);
    let entry = this.entries.get(key);
    if (!entry) {
      entry = this.createEntry(key, path, effective, sfx, options);
      this.entries.set(key, entry);
      this.start(entry);
    }
    return this.take(entry);
  }

  /** One more reference on `entry` and the owning handle of it (the bookkeeping of `acquire`). */
  private take(entry: Entry): TextureHandle {
    entry.refs++;
    entry.lastUsed = performance.now();
    return this.handleOf(entry, true);
  }

  private createEntry(
    key: string,
    path: string,
    preset: SamplerPresetName,
    sfx: boolean,
    options: AcquireOptions,
  ): Entry {
    let resolve: (h: TextureHandle) => void = () => undefined;
    const promise = new Promise<TextureHandle>((r) => {
      resolve = r;
    });
    return {
      key,
      path,
      preset,
      sfx,
      prio: options.prio ?? Prio.Visible,
      distance: options.distance ?? 0,
      refs: 0,
      state: 'loading',
      external: new ExternalTexture(null),
      gl: null,
      gpuBytes: 0,
      plan: null,
      lastUsed: 0,
      generation: this.generation,
      listeners: new Set(),
      promise,
      resolve,
    };
  }

  /**
   * A handle view of `entry`. `owning` handles (returned by `acquire`) drop one reference on `release()`;
   * views passed to listeners and resolved from `ready` do not own a reference.
   */
  private handleOf(entry: Entry, owning = false): TextureHandle {
    let released = false;
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const cache = this;
    return {
      key: entry.key,
      path: entry.path,
      get state() {
        return entry.state;
      },
      get texture() {
        return entry.state === 'failed' ? cache.whiteTexture() : entry.external;
      },
      get gpuBytes() {
        return entry.gpuBytes;
      },
      get plan() {
        return entry.plan;
      },
      get ready() {
        return entry.promise;
      },
      onChange(listener) {
        entry.listeners.add(listener);
        return () => entry.listeners.delete(listener);
      },
      release() {
        if (!owning || released) return;
        released = true;
        entry.refs = Math.max(0, entry.refs - 1);
        entry.lastUsed = performance.now();
      },
    };
  }

  private notify(entry: Entry): void {
    const handle = this.handleOf(entry);
    for (const l of [...entry.listeners]) l(handle);
  }

  private start(entry: Entry): void {
    const generation = entry.generation;
    entry.state = 'loading';
    this.deps.loader
      .load({
        path: entry.path,
        sfx: entry.sfx,
        prio: entry.prio,
        distance: entry.distance,
      })
      .then(
        (decoded) => {
          if (this.disposed || entry.generation !== generation) {
            if (decoded.kind === 'bitmap') decoded.bitmap.close();
            return;
          }
          this.enqueueUpload(entry, decoded);
        },
        (error: unknown) => {
          if (this.disposed || entry.generation !== generation) return;
          this.fail(entry, error instanceof Error ? error.message : String(error));
        },
      );
  }

  private enqueueUpload(entry: Entry, decoded: DecodedTexture): void {
    let plan: UploadPlan;
    let data: LevelData | null = null;
    try {
      const spec = SAMPLER_PRESETS[entry.preset];
      const opts = this.planOptions();
      if (decoded.kind === 'bitmap') {
        plan = planBitmapUpload(decoded.bitmap.width, decoded.bitmap.height, spec, opts);
      } else if (entry.sfx) {
        plan = planSfxUpload(decoded.meta, spec, opts);
        const blob = decoded.blob;
        data = (l: PlannedLevel) => {
          if (!l.view) throw new Error('sfx level without data');
          return quantizeRgba5551(view(blob, { ...l.view, ty: 'u8' }));
        };
      } else {
        plan = planDdsUpload(decoded.meta, spec, opts);
        data = blobLevelData(decoded.blob);
      }
    } catch (e) {
      if (decoded.kind === 'bitmap') decoded.bitmap.close();
      this.fail(entry, e instanceof Error ? e.message : String(e));
      return;
    }
    const generation = entry.generation;
    this.deps.uploads.enqueue({
      key: entry.key,
      prio: entry.prio,
      distance: entry.distance,
      bytes: plan.gpuBytes,
      run: () => {
        try {
          if (this.disposed || this.lost || entry.generation !== generation) return;
          this.finishUpload(entry, plan, decoded, data);
        } finally {
          if (decoded.kind === 'bitmap') decoded.bitmap.close();
        }
      },
    });
  }

  private finishUpload(
    entry: Entry,
    plan: UploadPlan,
    decoded: DecodedTexture,
    data: LevelData | null,
  ): void {
    try {
      const gl = this.deps.renderer.getContext();
      const source =
        decoded.kind === 'bitmap'
          ? { bitmap: decoded.bitmap }
          : { data: data ?? (() => new Uint8Array(0)) };
      const glTexture = uploadPlan(gl, this.deps.renderer.state, plan, source, {
        anisotropyParam: this.deps.caps.anisotropyParam,
        checkErrors: this.deps.checkErrors ?? false,
      });
      entry.gl = glTexture;
      entry.external.sourceTexture = glTexture;
      entry.plan = plan;
      entry.gpuBytes = plan.gpuBytes;
      entry.state = 'ready';
      this.deps.accounting.set(entry.key, plan.gpuBytes);
      entry.resolve(this.handleOf(entry));
      this.notify(entry);
    } catch (e) {
      this.fail(entry, e instanceof Error ? e.message : String(e));
    }
  }

  private fail(entry: Entry, reason: string): void {
    entry.state = 'failed';
    entry.gpuBytes = 0;
    if (!this.reported.has(entry.path)) {
      this.reported.add(entry.path);
      this.missing++;
      this.deps.onMissing?.(entry.path, reason);
    }
    entry.resolve(this.handleOf(entry));
    this.notify(entry);
  }

  /** Resolves the first of `paths` that exists (the `-etNN` variant, then the base texture, Plan/41 6.3). */
  async acquireFirst(
    paths: readonly string[],
    preset: SamplerPresetName = 'world',
    options: AcquireOptions = {},
  ): Promise<TextureHandle> {
    let last: TextureHandle | null = null;
    for (const p of paths) {
      const h = this.acquire(p, preset, options);
      await h.ready;
      if (h.state === 'ready') {
        last?.release();
        return h;
      }
      last?.release();
      last = h;
    }
    if (last) return last;
    throw new Error('acquireFirst needs at least one path');
  }

  /**
   * The synchronous form of `acquireFirst` (BFIX1 B8): the same decision when it can be made NOW, so a resident texture costs no
   * frame without it. Walks `paths` in order with the key `acquire` would use: an entry that does not exist, is `loading` or
   * `lost` returns `null` (no reference taken, no load started: the caller takes the async path); a `failed` entry is skipped;
   * a `ready` one is acquired exactly like `acquire` (reference, `lastUsed`) and returned as an owning handle. When every path
   * exists and failed, an owning handle of the last one is returned (its `texture` is the shared white), like `acquireFirst`.
   * `null` for an empty list.
   */
  peekReady(
    paths: readonly string[],
    preset: SamplerPresetName = 'world',
    options: AcquireOptions = {},
  ): TextureHandle | null {
    let last: Entry | null = null;
    for (const path of paths) {
      const entry = this.entries.get(keyOf(path, preset, options).key);
      if (!entry || entry.state === 'loading' || entry.state === 'lost') return null;
      if (entry.state === 'ready') return this.take(entry);
      last = entry;
    }
    return last ? this.take(last) : null;
  }

  // ---- residency ----------------------------------------------------------------------------------------

  private dispose1(entry: Entry): void {
    if (entry.gl) this.deps.renderer.getContext().deleteTexture(entry.gl);
    entry.gl = null;
    entry.external.sourceTexture = null;
    entry.external.dispose();
    this.deps.accounting.remove(entry.key);
    this.deps.uploads.cancel(entry.key);
    entry.generation++;
    this.entries.delete(entry.key);
  }

  /**
   * Evicts zero-ref entries, least recently used first, until the resident bytes are at most `budgetBytes`; with
   * `maxIdleMs` also every zero-ref entry idle for longer. Returns the number of evicted entries.
   */
  trim(budgetBytes: number, maxIdleMs = Infinity, now = performance.now()): number {
    const idle = [...this.entries.values()]
      .filter((e) => e.refs === 0 && e.state !== 'loading')
      .sort((a, b) => a.lastUsed - b.lastUsed);
    let n = 0;
    for (const e of idle) {
      if (this.deps.accounting.totalBytes > budgetBytes || now - e.lastUsed > maxIdleMs) {
        this.dispose1(e);
        n++;
      }
    }
    return n;
  }

  stats(): TextureCacheStats {
    let ready = 0;
    let loading = 0;
    let failed = 0;
    for (const e of this.entries.values()) {
      if (e.state === 'ready') ready++;
      else if (e.state === 'loading') loading++;
      else if (e.state === 'failed') failed++;
    }
    return {
      entries: this.entries.size,
      ready,
      loading,
      failed,
      missing: this.missing,
      gpuBytes: this.deps.accounting.totalBytes,
    };
  }

  // ---- context loss (Plan/41 section 4.2, Plan/40 section 12.4) -------------------------------------------

  /** Every GL texture is gone: mark the entries for re-upload and reset the accounting. */
  onContextLost(): void {
    this.lost = true;
    this.deps.accounting.reset();
    this.whiteGl = null;
    if (this.white) this.white.sourceTexture = null;
    for (const s of this.solids.values()) {
      s.gl = null;
      s.external.sourceTexture = null;
    }
    for (const e of this.entries.values()) {
      e.gl = null;
      e.external.sourceTexture = null;
      e.generation++;
      e.gpuBytes = 0;
      e.plan = null;
      if (e.state === 'ready' || e.state === 'loading') e.state = 'lost';
    }
  }

  /** The context is back: re-fetch and re-upload every referenced texture, drop the unreferenced ones. */
  onContextRestored(): void {
    this.lost = false;
    if (this.white) this.whiteTexture();
    for (const s of this.solids.values()) this.solidTexture(s.rgba);
    for (const e of [...this.entries.values()]) {
      if (e.refs === 0) {
        this.entries.delete(e.key);
        continue;
      }
      if (e.state === 'lost') {
        // a fresh `ready` promise for the new upload
        e.promise = new Promise<TextureHandle>((r) => {
          e.resolve = r;
        });
        this.start(e);
        this.notify(e);
      }
    }
  }

  dispose(): void {
    this.disposed = true;
    for (const e of [...this.entries.values()]) this.dispose1(e);
    if (this.whiteGl) this.deps.renderer.getContext().deleteTexture(this.whiteGl);
    this.whiteGl = null;
    this.white?.dispose();
    this.white = null;
    for (const s of this.solids.values()) {
      if (s.gl) this.deps.renderer.getContext().deleteTexture(s.gl);
      s.external.dispose();
    }
    this.solids.clear();
  }
}
