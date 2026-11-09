// The terrain texture pool (Plan/41 section 9.4, WP-REND-09): the diffuse textures of the land layers live in up to
// SIX `TEXTURE_2D_ARRAY` buckets keyed by (GL format, size, levels), e.g. DXT1-256, DXT1-128, DXT1-512, RGB565-512,
// DXT3-256 and the RGBA8-256 fallback. A layer is a `(bucket, slice)` pair that the splat shader receives as uniforms,
// so one draw per land (pass of <= 16 layers) can sample any layer.
//
//   * `world` sampler: `LINEAR_MIPMAP_NEAREST` / `LINEAR`, REPEAT, `TEXTURE_MAX_LEVEL = 3` (K1, the original created 4
//     levels with a POINT mip filter);
//   * uploads are raw WebGL (`texStorage3D` + `(compressed)TexSubImage3D`) wrapped in `ExternalTexture`s; they run as
//     jobs of the `UploadQueue` (4 MB / 3 ms per frame), growth of a bucket (power of two, 8, 16, 32, ...) included;
//   * the level data of every slice is RETAINED (it is the compressed data, about the GPU size of the texture): a bucket
//     that grows is re-filled from it, and so is a bucket after a context loss (no re-fetch);
//   * a texture that cannot take its natural bucket (fewer than 4 levels, e.g. the 5 mip-less DXT1 files, or no bucket
//     left because six combinations exist) is decoded to RGBA8 (`dds_bc`), resampled to 256 x 256 and given a generated
//     4-level chain: it goes to the RGBA8-256 bucket, which keeps its place among the six (reserved);
//   * a texture that fails to load is `failed`: the land removes the layer ("the next enabled layer becomes first",
//     lod.cpp:1326-1340).

import { ExternalTexture } from 'three';
import type { DdsMeta, GlUpload } from '../../gen/formats';
import { view } from '../../wasm/decoders';
import { Prio } from '../../workers/priority';
import type { DeviceCaps } from '../gl/caps';
import {
  blobLevelData,
  planDdsUpload,
  type PlanOptions,
  type PlannedLevel,
  type TextureStateLike,
  type UploadPlan,
} from '../gl/dds';
import { GL } from '../gl/glConsts';
import { SAMPLER_PRESETS } from '../gl/samplers';
import type { TextureLoader, TextureRenderer } from '../gl/textures';
import type { GpuAccounting, UploadQueue } from '../gl/uploadQueue';
import { buildChain, resampleRgba8 } from './resample';

export const MAX_BUCKETS = 6;
export const INITIAL_CAPACITY = 8;
/** Levels every bucket holds (`TEXTURE_MAX_LEVEL + 1`). */
export const BUCKET_LEVELS = 4;
/** Size of the RGBA8 fallback bucket. */
export const FALLBACK_SIZE = 256;

export type TerrainTexState = 'loading' | 'ready' | 'failed';

/** A reference to the diffuse texture of a terrain id; `release()` is idempotent. */
export interface TerrainTexHandle {
  readonly id: number;
  readonly path: string;
  readonly state: TerrainTexState;
  /** Shader sampler index of the bucket (valid when `ready`). */
  readonly bucket: number;
  /** Array layer inside the bucket (valid when `ready`). */
  readonly slice: number;
  /** Settles when the state leaves `loading` (never rejects). */
  readonly ready: Promise<TerrainTexHandle>;
  release(): void;
}

interface SliceLevel {
  readonly w: number;
  readonly h: number;
  readonly data: ArrayBufferView;
}

interface Bucket {
  readonly index: number;
  readonly key: string;
  readonly pixel: GlUpload;
  readonly w: number;
  readonly h: number;
  readonly levels: number;
  readonly internalFormat: number;
  readonly format: number;
  readonly type: number;
  readonly compressed: boolean;
  capacity: number;
  /** Slice -> its retained level data (`null` = free). */
  readonly slices: (readonly SliceLevel[] | null)[];
  tex: WebGLTexture | null;
  readonly external: ExternalTexture;
  gpuBytes: number;
  /** Creations of the storage (growth and context restore). */
  allocations: number;
}

interface Entry {
  readonly id: number;
  readonly path: string;
  refs: number;
  state: TerrainTexState;
  bucket: number;
  slice: number;
  generation: number;
  promise: Promise<TerrainTexHandle>;
  resolve: (h: TerrainTexHandle) => void;
  prio: Prio;
  distance: number;
  /** Was decoded through the RGBA8 fallback. */
  fallback: boolean;
}

export interface TerrainPoolDeps {
  readonly renderer: TextureRenderer;
  readonly caps: DeviceCaps;
  readonly loader: TextureLoader;
  readonly uploads: UploadQueue;
  readonly accounting: GpuAccounting;
  /** Called ONCE per texture that cannot be shown. */
  readonly onMissing?: (path: string, reason: string) => void;
  /** `gl.getError()` after every upload (tests). */
  readonly checkErrors?: boolean;
}

export interface BucketInfo {
  readonly index: number;
  readonly key: string;
  readonly capacity: number;
  readonly used: number;
  readonly gpuBytes: number;
  readonly allocations: number;
}

function bucketKeyOf(pixel: GlUpload, w: number, h: number, levels: number): string {
  return `${pixel}:${String(w)}x${String(h)}:${String(levels)}`;
}

function layout(pixel: GlUpload): {
  internalFormat: number;
  format: number;
  type: number;
  compressed: boolean;
} {
  switch (pixel) {
    case 'bc1':
      return {
        internalFormat: GL.COMPRESSED_RGBA_S3TC_DXT1_EXT,
        format: 0,
        type: 0,
        compressed: true,
      };
    case 'bc2':
      return {
        internalFormat: GL.COMPRESSED_RGBA_S3TC_DXT3_EXT,
        format: 0,
        type: 0,
        compressed: true,
      };
    case 'bc3':
      return {
        internalFormat: GL.COMPRESSED_RGBA_S3TC_DXT5_EXT,
        format: 0,
        type: 0,
        compressed: true,
      };
    case 'rgb565':
      return {
        internalFormat: GL.RGB565,
        format: GL.RGB,
        type: GL.UNSIGNED_SHORT_5_6_5,
        compressed: false,
      };
    case 'rgba5551':
      return {
        internalFormat: GL.RGB5_A1,
        format: GL.RGBA,
        type: GL.UNSIGNED_SHORT_5_5_5_1,
        compressed: false,
      };
    case 'rgba8':
      return {
        internalFormat: GL.RGBA8,
        format: GL.RGBA,
        type: GL.UNSIGNED_BYTE,
        compressed: false,
      };
  }
}

const bytesOfLevel = (b: Bucket, w: number, h: number): number => {
  switch (b.pixel) {
    case 'bc1':
      return Math.max(1, (w + 3) >> 2) * Math.max(1, (h + 3) >> 2) * 8;
    case 'bc2':
    case 'bc3':
      return Math.max(1, (w + 3) >> 2) * Math.max(1, (h + 3) >> 2) * 16;
    case 'rgb565':
    case 'rgba5551':
      return w * h * 2;
    case 'rgba8':
      return w * h * 4;
  }
};

/**
 * Where a decoded terrain texture goes (pure; the pool applies the same rule): a texture with at least 4 levels takes the
 * bucket of its (format, size) while the table has room; everything else (mip-less files, the sixth combination onwards)
 * is decoded to RGBA8 and resampled into the RGBA8-256 fallback bucket.
 */
export function classifyTerrainTexture(
  meta: DdsMeta,
  options: PlanOptions,
): { readonly kind: 'bucket' | 'fallback'; readonly key: string } {
  const plan = planDdsUpload(meta, SAMPLER_PRESETS.world, options);
  if (plan.levels.length >= BUCKET_LEVELS) {
    return { kind: 'bucket', key: bucketKeyOf(meta.pixel, plan.width, plan.height, BUCKET_LEVELS) };
  }
  return {
    kind: 'fallback',
    key: bucketKeyOf('rgba8', FALLBACK_SIZE, FALLBACK_SIZE, BUCKET_LEVELS),
  };
}

export class TerrainTexturePool {
  private readonly buckets: Bucket[] = [];
  private readonly entries = new Map<number, Entry>();
  private readonly reported = new Set<string>();
  private lost = false;
  private disposed = false;
  private generation = 0;
  /** Textures that fell back to RGBA8-256 (probe). */
  fallbackCount = 0;
  /** Paths that could not be shown. */
  missing = 0;

  constructor(private readonly deps: TerrainPoolDeps) {
    // the six sampler slots always exist so that the shader's uniforms are always bound to an object
    for (let i = 0; i < MAX_BUCKETS; i++) this.slotExternals.push(new ExternalTexture(null));
  }

  private readonly slotExternals: ExternalTexture[] = [];

  /** The texture object of shader sampler `i` (0..5); unused slots hold no GL texture (sampling gives black). */
  samplerTexture(i: number): ExternalTexture {
    const t = this.slotExternals[i];
    if (!t) throw new RangeError(`bucket ${String(i)}`);
    return t;
  }

  info(): BucketInfo[] {
    return this.buckets.map((b) => ({
      index: b.index,
      key: b.key,
      capacity: b.capacity,
      used: b.slices.filter((s) => s !== null).length,
      gpuBytes: b.gpuBytes,
      allocations: b.allocations,
    }));
  }

  get bucketCount(): number {
    return this.buckets.length;
  }

  /** GPU bytes of all buckets. */
  get gpuBytes(): number {
    return this.buckets.reduce((n, b) => n + b.gpuBytes, 0);
  }

  private planOptions(): PlanOptions {
    return {
      s3tc: this.deps.caps.s3tc,
      fullMips: false,
      anisotropy: 1,
      maxAnisotropy: this.deps.caps.maxAnisotropy,
    };
  }

  /** A handle of terrain texture `id` (`path` is the VFS path `world/texture/<file>`); loads it on first use. */
  acquire(
    id: number,
    path: string,
    options: { readonly prio?: Prio; readonly distance?: number } = {},
  ): TerrainTexHandle {
    let e = this.entries.get(id);
    if (!e) {
      let resolve: (h: TerrainTexHandle) => void = () => undefined;
      const promise = new Promise<TerrainTexHandle>((r) => {
        resolve = r;
      });
      e = {
        id,
        path,
        refs: 0,
        state: 'loading',
        bucket: -1,
        slice: -1,
        generation: this.generation,
        promise,
        resolve,
        prio: options.prio ?? Prio.Critical,
        distance: options.distance ?? 0,
        fallback: false,
      };
      this.entries.set(id, e);
      this.load(e, false);
    }
    e.refs++;
    return this.handleOf(e, true);
  }

  private handleOf(e: Entry, owning: boolean): TerrainTexHandle {
    let released = false;
    return {
      id: e.id,
      path: e.path,
      get state() {
        return e.state;
      },
      get bucket() {
        return e.bucket;
      },
      get slice() {
        return e.slice;
      },
      get ready() {
        return e.promise;
      },
      release: () => {
        if (!owning || released) return;
        released = true;
        e.refs = Math.max(0, e.refs - 1);
      },
    };
  }

  private fail(e: Entry, reason: string): void {
    e.state = 'failed';
    if (!this.reported.has(e.path)) {
      this.reported.add(e.path);
      this.missing++;
      this.deps.onMissing?.(e.path, reason);
    }
    e.resolve(this.handleOf(e, false));
  }

  private load(e: Entry, rgba: boolean): void {
    const generation = e.generation;
    this.deps.loader.load({ path: e.path, sfx: rgba, prio: e.prio, distance: e.distance }).then(
      (decoded) => {
        if (this.disposed || e.generation !== generation) {
          if (decoded.kind === 'bitmap') decoded.bitmap.close();
          return;
        }
        if (decoded.kind !== 'dds') {
          decoded.bitmap.close();
          this.fail(e, 'terrain textures are DDS files');
          return;
        }
        this.place(e, decoded.meta, decoded.blob, rgba);
      },
      (error: unknown) => {
        if (this.disposed || e.generation !== generation) return;
        this.fail(e, error instanceof Error ? error.message : String(error));
      },
    );
  }

  /** Chooses the bucket of a decoded texture and enqueues the upload job. */
  private place(e: Entry, meta: DdsMeta, blob: Uint8Array, rgbaRequested: boolean): void {
    let plan: UploadPlan;
    try {
      plan = planDdsUpload(meta, SAMPLER_PRESETS.world, this.planOptions());
    } catch (err) {
      this.fail(e, err instanceof Error ? err.message : String(err));
      return;
    }
    const regular = plan.levels.length >= BUCKET_LEVELS;
    const key = bucketKeyOf(meta.pixel, plan.width, plan.height, BUCKET_LEVELS);
    let bucket = regular ? this.findBucket(key) : undefined;
    if (regular && !bucket) bucket = this.createBucket(meta.pixel, plan.width, plan.height, false);
    if (bucket && regular) {
      const data = blobLevelData(blob);
      const levels: SliceLevel[] = plan.levels
        .slice(0, BUCKET_LEVELS)
        .map((l: PlannedLevel) => ({ w: l.width, h: l.height, data: data(l) }));
      this.enqueue(e, bucket, levels);
      return;
    }
    // irregular: RGBA8-256 with a generated chain
    if (meta.pixel !== 'rgba8') {
      if (rgbaRequested) {
        this.fail(e, `${meta.pixel} pack returned for an RGBA8 request`);
        return;
      }
      e.fallback = true;
      this.load(e, true); // decode as RGBA8 (dds_bc) and come back
      return;
    }
    const level0 = view(
      blob,
      meta.levels[0]?.view ?? { off: 0, len: 0, ty: 'u8', comps: 1, count: 0 },
    );
    const src = new Uint8Array(level0.buffer, level0.byteOffset, level0.byteLength);
    const base = resampleRgba8(src, meta.width, meta.height, FALLBACK_SIZE, FALLBACK_SIZE);
    const chain = buildChain(base, FALLBACK_SIZE, FALLBACK_SIZE, BUCKET_LEVELS);
    const fb =
      this.findBucket(bucketKeyOf('rgba8', FALLBACK_SIZE, FALLBACK_SIZE, BUCKET_LEVELS)) ??
      this.createBucket('rgba8', FALLBACK_SIZE, FALLBACK_SIZE, true);
    if (!fb) {
      this.fail(e, 'no terrain texture bucket available');
      return;
    }
    this.fallbackCount++;
    this.enqueue(e, fb, chain);
  }

  private findBucket(key: string): Bucket | undefined {
    return this.buckets.find((b) => b.key === key);
  }

  /**
   * Creates a bucket. Natural buckets leave one of the six slots for the RGBA8 fallback; the fallback itself may take
   * the last one. Returns `undefined` when the table is full (the caller falls back to RGBA8-256).
   */
  private createBucket(
    pixel: GlUpload,
    w: number,
    h: number,
    fallback: boolean,
  ): Bucket | undefined {
    const fbKey = bucketKeyOf('rgba8', FALLBACK_SIZE, FALLBACK_SIZE, BUCKET_LEVELS);
    const haveFallback = this.buckets.some((b) => b.key === fbKey);
    const limit = fallback || haveFallback ? MAX_BUCKETS : MAX_BUCKETS - 1;
    if (this.buckets.length >= limit) return undefined;
    const lay = layout(pixel);
    const index = this.buckets.length;
    const external = this.samplerTexture(index);
    const b: Bucket = {
      index,
      key: bucketKeyOf(pixel, w, h, BUCKET_LEVELS),
      pixel,
      w,
      h,
      levels: BUCKET_LEVELS,
      internalFormat: lay.internalFormat,
      format: lay.format,
      type: lay.type,
      compressed: lay.compressed,
      capacity: 0,
      slices: [],
      tex: null,
      external,
      gpuBytes: 0,
      allocations: 0,
    };
    this.buckets.push(b);
    return b;
  }

  private enqueue(e: Entry, bucket: Bucket, levels: readonly SliceLevel[]): void {
    // the slice is reserved NOW so that two textures never take the same one
    const slice = this.reserveSlice(bucket, levels);
    const generation = e.generation;
    const sliceBytes = levels.reduce((n, l) => n + bytesOfLevel(bucket, l.w, l.h), 0);
    this.deps.uploads.enqueue({
      key: `terrain-tex:${String(e.id)}`,
      prio: e.prio,
      distance: e.distance,
      bytes: sliceBytes,
      run: () => {
        if (this.disposed || this.lost || e.generation !== generation) return;
        try {
          this.ensureStorage(bucket);
          this.uploadSlice(bucket, slice, levels);
          e.bucket = bucket.index;
          e.slice = slice;
          e.state = 'ready';
          e.resolve(this.handleOf(e, false));
        } catch (err) {
          bucket.slices[slice] = null;
          this.fail(e, err instanceof Error ? err.message : String(err));
        }
      },
    });
    // the entry knows its place before the upload (only `state` gates the shader)
    e.bucket = bucket.index;
    e.slice = slice;
  }

  private reserveSlice(bucket: Bucket, levels: readonly SliceLevel[]): number {
    let slice = bucket.slices.indexOf(null);
    if (slice < 0) {
      slice = bucket.slices.length;
      bucket.slices.push(null);
    }
    bucket.slices[slice] = levels;
    return slice;
  }

  /** Makes sure the bucket's storage exists and holds all reserved slices (growth: power of two, re-upload). */
  private ensureStorage(b: Bucket): void {
    const need = b.slices.length;
    if (b.tex !== null && need <= b.capacity) return;
    const gl = this.deps.renderer.getContext();
    const state: TextureStateLike = this.deps.renderer.state;
    let capacity = Math.max(INITIAL_CAPACITY, b.capacity);
    while (capacity < need) capacity *= 2;
    const old = b.tex;
    const tex = gl.createTexture();
    state.bindTexture(GL.TEXTURE_2D_ARRAY, tex);
    try {
      gl.pixelStorei(GL.UNPACK_ALIGNMENT, 1);
      gl.texStorage3D(GL.TEXTURE_2D_ARRAY, b.levels, b.internalFormat, b.w, b.h, capacity);
      gl.texParameteri(GL.TEXTURE_2D_ARRAY, GL.TEXTURE_MIN_FILTER, GL.LINEAR_MIPMAP_NEAREST);
      gl.texParameteri(GL.TEXTURE_2D_ARRAY, GL.TEXTURE_MAG_FILTER, GL.LINEAR);
      gl.texParameteri(GL.TEXTURE_2D_ARRAY, GL.TEXTURE_WRAP_S, GL.REPEAT);
      gl.texParameteri(GL.TEXTURE_2D_ARRAY, GL.TEXTURE_WRAP_T, GL.REPEAT);
      gl.texParameteri(GL.TEXTURE_2D_ARRAY, GL.TEXTURE_BASE_LEVEL, 0);
      gl.texParameteri(GL.TEXTURE_2D_ARRAY, GL.TEXTURE_MAX_LEVEL, b.levels - 1);
      b.tex = tex;
      b.capacity = capacity;
      b.allocations++;
      // re-fill from the retained data (growth and context restore); slices not uploaded yet are filled by their jobs
      for (let s = 0; s < b.slices.length; s++) {
        const lv = b.slices[s];
        if (lv && this.uploaded.has(this.sliceKey(b, s))) this.putSlice(gl, b, s, lv);
      }
      if (this.deps.checkErrors === true) {
        const err = gl.getError();
        if (err !== GL.NO_ERROR)
          throw new Error(`GL error 0x${err.toString(16)} allocating a bucket`);
      }
    } catch (e) {
      gl.deleteTexture(tex);
      b.tex = old;
      throw e;
    } finally {
      state.unbindTexture();
    }
    if (old) gl.deleteTexture(old);
    b.external.sourceTexture = tex;
    let total = 0;
    for (let l = 0; l < b.levels; l++) {
      total += bytesOfLevel(b, Math.max(1, b.w >> l), Math.max(1, b.h >> l)) * capacity;
    }
    b.gpuBytes = total;
    this.deps.accounting.set(`terrain-bucket:${b.key}`, total);
  }

  private readonly uploaded = new Set<string>();
  private sliceKey(b: Bucket, s: number): string {
    return `${String(b.index)}:${String(s)}`;
  }

  private uploadSlice(b: Bucket, slice: number, levels: readonly SliceLevel[]): void {
    const gl = this.deps.renderer.getContext();
    const state: TextureStateLike = this.deps.renderer.state;
    state.bindTexture(GL.TEXTURE_2D_ARRAY, b.tex);
    try {
      this.putSlice(gl, b, slice, levels);
      this.uploaded.add(this.sliceKey(b, slice));
      if (this.deps.checkErrors === true) {
        const err = gl.getError();
        if (err !== GL.NO_ERROR)
          throw new Error(`GL error 0x${err.toString(16)} uploading a slice`);
      }
    } finally {
      state.unbindTexture();
    }
  }

  private putSlice(
    gl: WebGL2RenderingContext,
    b: Bucket,
    slice: number,
    levels: readonly SliceLevel[],
  ): void {
    gl.pixelStorei(GL.UNPACK_ALIGNMENT, 1);
    levels.forEach((l, i) => {
      if (b.compressed) {
        gl.compressedTexSubImage3D(
          GL.TEXTURE_2D_ARRAY,
          i,
          0,
          0,
          slice,
          l.w,
          l.h,
          1,
          b.internalFormat,
          l.data,
        );
      } else {
        gl.texSubImage3D(
          GL.TEXTURE_2D_ARRAY,
          i,
          0,
          0,
          slice,
          l.w,
          l.h,
          1,
          b.format,
          b.type,
          l.data,
        );
      }
    });
    gl.pixelStorei(GL.UNPACK_ALIGNMENT, 4);
  }

  // ---- context loss -------------------------------------------------------------------------------------

  /** Every GL texture is gone: the buckets keep their retained data and are rebuilt on restore. */
  onContextLost(): void {
    this.lost = true;
    for (const b of this.buckets) {
      b.tex = null;
      b.capacity = 0;
      b.external.sourceTexture = null;
      this.deps.accounting.remove(`terrain-bucket:${b.key}`);
      b.gpuBytes = 0;
    }
    this.uploaded.clear();
  }

  /** The context is back: re-create every bucket with all retained slices (no fetch). */
  onContextRestored(): void {
    this.lost = false;
    for (const b of this.buckets) {
      const have = [...b.slices.keys()].filter((s) => b.slices[s]);
      if (have.length === 0) continue;
      // every retained slice is re-uploaded; mark them so that `ensureStorage` fills them
      for (const s of have) this.uploaded.add(this.sliceKey(b, s));
      this.ensureStorage(b);
    }
    // upload jobs that were skipped while the context was lost: their slices were just re-filled from the retained data
    for (const e of this.entries.values()) {
      if (e.state === 'loading' && e.bucket >= 0 && e.slice >= 0) {
        e.state = 'ready';
        e.resolve(this.handleOf(e, false));
      }
    }
  }

  stats(): { buckets: number; slices: number; ready: number; failed: number; loading: number } {
    let ready = 0;
    let failed = 0;
    let loading = 0;
    for (const e of this.entries.values()) {
      if (e.state === 'ready') ready++;
      else if (e.state === 'failed') failed++;
      else loading++;
    }
    return {
      buckets: this.buckets.length,
      slices: this.buckets.reduce((n, b) => n + b.slices.filter((s) => s !== null).length, 0),
      ready,
      failed,
      loading,
    };
  }

  /** The handle state of a texture id (tests, probes). */
  stateOf(id: number): TerrainTexState | undefined {
    return this.entries.get(id)?.state;
  }

  dispose(): void {
    this.disposed = true;
    const gl = this.deps.renderer.getContext();
    for (const b of this.buckets) {
      if (b.tex) gl.deleteTexture(b.tex);
      b.tex = null;
      b.external.sourceTexture = null;
      this.deps.accounting.remove(`terrain-bucket:${b.key}`);
    }
    for (const t of this.slotExternals) t.dispose();
    this.buckets.length = 0;
    this.entries.clear();
    this.uploaded.clear();
    this.generation++;
  }
}
