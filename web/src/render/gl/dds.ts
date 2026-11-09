// DDS / bitmap texture upload paths (Plan/41 section 6.1, WP-REND-02; Plan/30 sections 11.3 and 13.6).
//
// The pack of `decode_dds` (S3TC present) keeps BC blocks; `decode_dds_bc` (no `WEBGL_compressed_texture_s3tc`)
// returns RGBA8; uncompressed files come as `to_gl_upload` words (RGB565, RGBA5551) or RGBA8. Everything is
// uploaded with `texStorage2D` + `(compressed)TexSubImage2D` (immutable storage, exactly the levels we planned,
// `TEXTURE_MAX_LEVEL` set, UNPACK_ALIGNMENT 1 because rows are tightly packed) into a texture object that the
// renderer wraps in a three.js `ExternalTexture`: three.js has no RGB565 type and no max-level API.
//
// `planDdsUpload` is pure (no GL), `uploadPlan` performs the GL calls; both are unit-tested.

import { view } from '../../wasm/decoders';
import type { DdsMeta, GlUpload, LevelMeta } from '../../gen/formats';
import { blockBytes, GL } from './glConsts';
import { fullChainLength, type SamplerSpec, usesMips, withoutMips } from './samplers';

/** The file cannot be uploaded (corrupt meta, dimensions, missing S3TC for a BC pack). */
export class TextureUploadError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'TextureUploadError';
  }
}

export interface PlanOptions {
  /** `WEBGL_compressed_texture_s3tc` is available (BC packs can be uploaded as they are). */
  readonly s3tc: boolean;
  /** Upload the whole chain instead of the preset's `TEXTURE_MAX_LEVEL` (RenderOptions.fullMips). */
  readonly fullMips: boolean;
  /** Requested anisotropy (1 = off) and the device maximum. */
  readonly anisotropy: number;
  readonly maxAnisotropy: number;
}

export const DEFAULT_PLAN_OPTIONS: PlanOptions = {
  s3tc: true,
  fullMips: false,
  anisotropy: 1,
  maxAnisotropy: 1,
};

/** One level of a plan. */
export interface PlannedLevel {
  readonly level: number;
  readonly width: number;
  readonly height: number;
  /** The section of the blob; absent for bitmap and generated levels. */
  readonly view: LevelMeta['view'] | null;
  readonly bytes: number;
}

/** Everything `uploadPlan` needs, decided without a GL context. */
export interface UploadPlan {
  readonly path: 'compressed' | 'raw' | 'bitmap';
  readonly pixel: GlUpload | 'bitmap';
  /** Sized internal format (`texStorage2D`). */
  readonly internalFormat: number;
  /** Upload format and type (raw and bitmap paths), 0 for compressed. */
  readonly format: number;
  readonly type: number;
  readonly width: number;
  readonly height: number;
  readonly levels: readonly PlannedLevel[];
  /** `TEXTURE_MAX_LEVEL`. */
  readonly maxLevel: number;
  readonly minFilter: number;
  readonly magFilter: number;
  readonly wrap: number;
  readonly anisotropy: number;
  /** The bitmap path generates its levels with `generateMipmap`. */
  readonly generateMips: boolean;
  /** Levels of the source file (for diagnostics). */
  readonly fileLevels: number;
  /** GPU bytes of the uploaded levels. */
  readonly gpuBytes: number;
}

function levelBytes(pixel: GlUpload, w: number, h: number): number {
  switch (pixel) {
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
}

/** How many of the file's `available` levels a preset uploads. */
export function levelsToUpload(spec: SamplerSpec, available: number, fullMips: boolean): number {
  if (available < 1) return 0;
  if (spec.maxLevel === 'full' || (fullMips && spec.maxLevel !== 0)) return available;
  return Math.min(available, spec.maxLevel + 1);
}

function formatsOf(pixel: GlUpload): {
  path: UploadPlan['path'];
  internalFormat: number;
  format: number;
  type: number;
} {
  switch (pixel) {
    // BC1 always uses the RGBA variant so the 1-bit alpha of alpha-tested foliage and hair works (Plan/41 6.1)
    case 'bc1':
      return {
        path: 'compressed',
        internalFormat: GL.COMPRESSED_RGBA_S3TC_DXT1_EXT,
        format: 0,
        type: 0,
      };
    case 'bc2':
      return {
        path: 'compressed',
        internalFormat: GL.COMPRESSED_RGBA_S3TC_DXT3_EXT,
        format: 0,
        type: 0,
      };
    case 'bc3':
      return {
        path: 'compressed',
        internalFormat: GL.COMPRESSED_RGBA_S3TC_DXT5_EXT,
        format: 0,
        type: 0,
      };
    case 'rgb565':
      return {
        path: 'raw',
        internalFormat: GL.RGB565,
        format: GL.RGB,
        type: GL.UNSIGNED_SHORT_5_6_5,
      };
    case 'rgba5551':
      return {
        path: 'raw',
        internalFormat: GL.RGB5_A1,
        format: GL.RGBA,
        type: GL.UNSIGNED_SHORT_5_5_5_1,
      };
    case 'rgba8':
      return { path: 'raw', internalFormat: GL.RGBA8, format: GL.RGBA, type: GL.UNSIGNED_BYTE };
  }
}

/** Filters of a plan: a texture with a single uploaded level never uses a mip filter. */
function filtersOf(spec: SamplerSpec, levelCount: number): { min: number; mag: number } {
  const min = levelCount > 1 || !usesMips(spec.min) ? spec.min : withoutMips(spec.min);
  return { min, mag: spec.mag };
}

/**
 * Decides how a `DdsMeta` is uploaded (Plan/41 6.1 table): GL formats, the levels, `TEXTURE_MAX_LEVEL`, filters,
 * GPU bytes. Throws `TextureUploadError` for a BC pack without S3TC (the caller must have asked for `dds_bc`),
 * for a BC texture whose base size is not a multiple of 4 and for an empty level list.
 */
export function planDdsUpload(
  meta: DdsMeta,
  spec: SamplerSpec,
  options: PlanOptions = DEFAULT_PLAN_OPTIONS,
): UploadPlan {
  const faceLevels = meta.levels.filter((l) => l.face === 0);
  if (faceLevels.length === 0) throw new TextureUploadError('dds pack without levels');
  const f = formatsOf(meta.pixel);
  if (f.path === 'compressed') {
    if (!options.s3tc) {
      throw new TextureUploadError(
        `${meta.pixel} pack needs WEBGL_compressed_texture_s3tc (decode with dds_bc instead)`,
      );
    }
    if (meta.width % 4 !== 0 || meta.height % 4 !== 0) {
      throw new TextureUploadError(
        `BC texture ${String(meta.width)}x${String(meta.height)} is not a multiple of 4`,
      );
    }
  }
  const count = levelsToUpload(spec, faceLevels.length, options.fullMips);
  const levels: PlannedLevel[] = faceLevels.slice(0, count).map((l, i) => ({
    level: i,
    width: l.w,
    height: l.h,
    view: l.view,
    bytes: levelBytes(meta.pixel, l.w, l.h),
  }));
  const filters = filtersOf(spec, levels.length);
  const aniso = Math.max(1, Math.min(options.anisotropy, options.maxAnisotropy));
  return {
    path: f.path,
    pixel: meta.pixel,
    internalFormat: f.internalFormat,
    format: f.format,
    type: f.type,
    width: meta.width,
    height: meta.height,
    levels,
    maxLevel: levels.length - 1,
    minFilter: filters.min,
    magFilter: filters.mag,
    wrap: spec.wrap,
    // anisotropy only matters for mipmapped minification
    anisotropy: usesMips(filters.min) ? aniso : 1,
    generateMips: false,
    fileLevels: faceLevels.length,
    gpuBytes: levels.reduce((n, l) => n + l.bytes, 0),
  };
}

/**
 * The plan of an SFX texture (Plan/41 16.1, R9): the RGBA8 levels of `decode_dds_bc`, quantised to RGBA5551 at
 * upload and uploaded with the full chain.
 */
export function planSfxUpload(
  meta: DdsMeta,
  spec: SamplerSpec,
  options: PlanOptions = DEFAULT_PLAN_OPTIONS,
): UploadPlan {
  if (meta.pixel !== 'rgba8') {
    throw new TextureUploadError(`sfx textures are decoded to RGBA8 first (got ${meta.pixel})`);
  }
  const faceLevels = meta.levels.filter((l) => l.face === 0);
  if (faceLevels.length === 0) throw new TextureUploadError('dds pack without levels');
  const count = levelsToUpload(spec, faceLevels.length, options.fullMips);
  const levels: PlannedLevel[] = faceLevels.slice(0, count).map((l, i) => ({
    level: i,
    width: l.w,
    height: l.h,
    view: l.view,
    bytes: l.w * l.h * 2,
  }));
  const filters = filtersOf(spec, levels.length);
  return {
    path: 'raw',
    pixel: 'rgba5551',
    internalFormat: GL.RGB5_A1,
    format: GL.RGBA,
    type: GL.UNSIGNED_SHORT_5_5_5_1,
    width: meta.width,
    height: meta.height,
    levels,
    maxLevel: levels.length - 1,
    minFilter: filters.min,
    magFilter: filters.mag,
    wrap: spec.wrap,
    anisotropy: 1,
    generateMips: false,
    fileLevels: faceLevels.length,
    gpuBytes: levels.reduce((n, l) => n + l.bytes, 0),
  };
}

/** The plan of a PNG (`ImageBitmap`) texture: RGBA8, levels generated with `generateMipmap`. */
export function planBitmapUpload(
  width: number,
  height: number,
  spec: SamplerSpec,
  options: PlanOptions = DEFAULT_PLAN_OPTIONS,
): UploadPlan {
  if (!(width > 0 && height > 0)) throw new TextureUploadError('bitmap without size');
  const chain = fullChainLength(width, height);
  const count = levelsToUpload(spec, chain, options.fullMips);
  const levels: PlannedLevel[] = [];
  for (let i = 0; i < count; i++) {
    const w = Math.max(1, width >> i);
    const h = Math.max(1, height >> i);
    levels.push({ level: i, width: w, height: h, view: null, bytes: w * h * 4 });
  }
  const filters = filtersOf(spec, levels.length);
  const aniso = Math.max(1, Math.min(options.anisotropy, options.maxAnisotropy));
  return {
    path: 'bitmap',
    pixel: 'bitmap',
    internalFormat: GL.RGBA8,
    format: GL.RGBA,
    type: GL.UNSIGNED_BYTE,
    width,
    height,
    levels,
    maxLevel: levels.length - 1,
    minFilter: filters.min,
    magFilter: filters.mag,
    wrap: spec.wrap,
    anisotropy: usesMips(filters.min) ? aniso : 1,
    generateMips: levels.length > 1,
    fileLevels: 1,
    gpuBytes: levels.reduce((n, l) => n + l.bytes, 0),
  };
}

/** The part of three.js `WebGLState` the uploads use, so that three's texture-binding cache stays correct. */
export interface TextureStateLike {
  bindTexture(webglType: number, webglTexture: WebGLTexture | null): void;
  unbindTexture(): void;
}

/** Source of the pixel data of one level of a plan (`null` for generated levels). */
export type LevelData = (level: PlannedLevel) => ArrayBufferView;

/** Level data of a DDS pack: a typed view of the blob section. */
export function blobLevelData(blob: Uint8Array): LevelData {
  return (l) => {
    if (!l.view) throw new TextureUploadError(`level ${String(l.level)} has no data`);
    return view(blob, l.view);
  };
}

function setSamplerState(
  gl: WebGL2RenderingContext,
  plan: UploadPlan,
  anisoParam: number | null,
): void {
  gl.texParameteri(GL.TEXTURE_2D, GL.TEXTURE_MIN_FILTER, plan.minFilter);
  gl.texParameteri(GL.TEXTURE_2D, GL.TEXTURE_MAG_FILTER, plan.magFilter);
  gl.texParameteri(GL.TEXTURE_2D, GL.TEXTURE_WRAP_S, plan.wrap);
  gl.texParameteri(GL.TEXTURE_2D, GL.TEXTURE_WRAP_T, plan.wrap);
  gl.texParameteri(GL.TEXTURE_2D, GL.TEXTURE_BASE_LEVEL, 0);
  gl.texParameteri(GL.TEXTURE_2D, GL.TEXTURE_MAX_LEVEL, plan.maxLevel);
  if (anisoParam !== null && plan.anisotropy > 1) {
    gl.texParameterf(GL.TEXTURE_2D, anisoParam, plan.anisotropy);
  }
}

function resetPixelStore(gl: WebGL2RenderingContext): void {
  gl.pixelStorei(GL.UNPACK_ALIGNMENT, 4);
  gl.pixelStorei(GL.UNPACK_FLIP_Y_WEBGL, 0);
  gl.pixelStorei(GL.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 0);
  gl.pixelStorei(GL.UNPACK_COLORSPACE_CONVERSION_WEBGL, GL.BROWSER_DEFAULT_WEBGL);
}

/**
 * Creates the texture object and uploads `plan` (`texStorage2D` with exactly `plan.levels.length` levels, then
 * one sub-image call per level). `data` supplies the pixels of the blob levels; the bitmap path takes the bitmap
 * instead. Throws `TextureUploadError` when `checkErrors` is set and GL reports an error. The texture binding
 * cache of three.js is kept consistent through `state`.
 */
export function uploadPlan(
  gl: WebGL2RenderingContext,
  state: TextureStateLike,
  plan: UploadPlan,
  source: { readonly data: LevelData } | { readonly bitmap: ImageBitmap },
  options: { readonly anisotropyParam?: number | null; readonly checkErrors?: boolean } = {},
): WebGLTexture {
  const texture = gl.createTexture();
  state.bindTexture(GL.TEXTURE_2D, texture);
  try {
    gl.pixelStorei(GL.UNPACK_ALIGNMENT, 1);
    gl.pixelStorei(GL.UNPACK_FLIP_Y_WEBGL, 0);
    gl.pixelStorei(GL.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 0);
    gl.pixelStorei(GL.UNPACK_COLORSPACE_CONVERSION_WEBGL, GL.NONE);
    gl.texStorage2D(
      GL.TEXTURE_2D,
      plan.levels.length,
      plan.internalFormat,
      plan.width,
      plan.height,
    );
    if ('bitmap' in source) {
      gl.texSubImage2D(
        GL.TEXTURE_2D,
        0,
        0,
        0,
        plan.width,
        plan.height,
        plan.format,
        plan.type,
        source.bitmap,
      );
      if (plan.generateMips) gl.generateMipmap(GL.TEXTURE_2D);
    } else {
      for (const l of plan.levels) {
        const data = source.data(l);
        if (plan.path === 'compressed') {
          gl.compressedTexSubImage2D(
            GL.TEXTURE_2D,
            l.level,
            0,
            0,
            l.width,
            l.height,
            plan.internalFormat,
            data,
          );
        } else {
          gl.texSubImage2D(
            GL.TEXTURE_2D,
            l.level,
            0,
            0,
            l.width,
            l.height,
            plan.format,
            plan.type,
            data,
          );
        }
      }
    }
    setSamplerState(gl, plan, options.anisotropyParam ?? null);
    if (options.checkErrors === true) {
      const err = gl.getError();
      if (err !== GL.NO_ERROR) {
        throw new TextureUploadError(`GL error 0x${err.toString(16)} while uploading a texture`);
      }
    }
  } catch (e) {
    gl.deleteTexture(texture);
    throw e;
  } finally {
    resetPixelStore(gl);
    state.unbindTexture();
  }
  return texture;
}

/** A 1x1 RGBA8 texture (the white of untextured models, neutral grey placeholders). */
export function uploadSolidTexture(
  gl: WebGL2RenderingContext,
  state: TextureStateLike,
  rgba: readonly [number, number, number, number],
): WebGLTexture {
  const plan = planSolid();
  return uploadPlan(gl, state, plan, { data: () => Uint8Array.from(rgba) });
}

function planSolid(): UploadPlan {
  return {
    path: 'raw',
    pixel: 'rgba8',
    internalFormat: GL.RGBA8,
    format: GL.RGBA,
    type: GL.UNSIGNED_BYTE,
    width: 1,
    height: 1,
    levels: [{ level: 0, width: 1, height: 1, view: null, bytes: 4 }],
    maxLevel: 0,
    minFilter: GL.NEAREST,
    magFilter: GL.NEAREST,
    wrap: GL.CLAMP_TO_EDGE,
    anisotropy: 1,
    generateMips: false,
    fileLevels: 1,
    gpuBytes: 4,
  };
}

/** `blockBytes` re-exported for probes. */
export { blockBytes };
