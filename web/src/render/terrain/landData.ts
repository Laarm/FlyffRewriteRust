// CPU-side land data: the typed views of a decoded `.lnd` pack (Plan/41 section 9.1, Plan/30 section 13.5,
// ref/19 section 6) plus the quantities the renderer derives once per land (patch bounds, the patch-major layer masks
// of the splat shader, the water/cloud patch lists). Everything here is plain arrays: no GL, no three.js.
//
// Layout facts (VERIFIED on all 1357 files, ref/19):
//   * 129 x 129 vertices per land (128 cells), row-major `h[vz * 129 + vx]`, the client heights have the attribute
//     offsets stripped (`strip_height_client`); land `(lx, lz)` covers `x in [lx * 128 * mpu, (lx + 1) * 128 * mpu)`;
//   * 16 x 16 patches of 8 x 8 cells, patch index `px + pz * 16`;
//   * water/cloud per patch: `{height, tex}` (`tex & 3`: 0 none, 1 cloud, 2 water; `tex >> 2`: the water set);
//   * layers: `texture_id`, a 256-bit patch mask (bit `i % 32` of word `i / 32` = patch `i`), the 128 x 128 alpha map
//     (`z * 128 + x`) and the light map RGB of layer 0 (identical in all layers of the corpus).

import type { LndMeta } from '../../gen/formats';
import { view } from '../../wasm/decoders';

export const MAP_SIZE = 128;
/** Vertices per land side. */
export const VERTS = 129;
export const VERTEX_COUNT = VERTS * VERTS;
export const PATCHES_PER_SIDE = 16;
export const PATCH_COUNT = PATCHES_PER_SIDE * PATCHES_PER_SIDE;
/** Cells per patch side. */
export const PATCH_SIZE = 8;
/** Side of the light/alpha maps in texels (113 of them are used, 15 are filler). */
export const LIGHTMAP_SIZE = 128;
export const LIGHTMAP_TEXELS = LIGHTMAP_SIZE * LIGHTMAP_SIZE;
/** Layers the splat mask can address (two 32-bit words per patch). */
export const MAX_MASK_LAYERS = 64;

/** `WTYPE_*` of the water texture byte (`tex & 3`). */
export const WTYPE = { none: 0, cloud: 1, water: 2 } as const;

/** The pack does not describe a land (a corrupt meta or blob). */
export class LandBuildError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'LandBuildError';
  }
}

export interface LandLayer {
  /** `Terrain.inc` texture id. */
  readonly textureId: number;
  /** The 256-bit patch mask as the pack delivers it. */
  readonly patchMask: readonly [number, number, number, number, number, number, number, number];
}

/** Placements of one kind as typed views (absolute LH positions, Plan/30 7.3). */
export interface PlacementData {
  readonly count: number;
  /** `u32 x 4` per record: object type, index, motion, ai interface. */
  readonly ids: Uint32Array;
  /** `f32 x 10` per record: pos xyz, angle, angX, angZ, scale xyz, 0. */
  readonly xf: Float32Array;
}

export interface LandData {
  readonly x: number;
  readonly z: number;
  readonly mpu: number;
  /** `f32 x 16641` real heights. */
  readonly heights: Float32Array;
  readonly attrs: Uint8Array;
  /** `u8 x 2 x 256`: `{height, tex}` per patch. */
  readonly water: Uint8Array;
  readonly layers: readonly LandLayer[];
  /** `u8 x layers * 16384`, layer-major. */
  readonly alpha: Uint8Array;
  /** `u8 x 3 x 16384`. */
  readonly lightRgb: Uint8Array;
  readonly objects: PlacementData;
  readonly sfx: PlacementData;
  /** `[minY, maxY]` per patch (`2 * 256` floats), the AABB the LOD distance and the culling use. */
  readonly patchBounds: Float32Array;
  /** `[minY, maxY]` of the whole land. */
  readonly bounds: readonly [number, number];
  /** The decoded blob stays referenced by the views above. */
  readonly blob: Uint8Array;
  readonly warnings: readonly string[];
}

function placements(p: LndMeta['objects'], blob: Uint8Array, what: string): PlacementData {
  const ids = view(blob, { ...p.ids, ty: 'u32' });
  const xf = view(blob, { ...p.xf, ty: 'f32' });
  if (ids.length !== p.count * 4 || xf.length !== p.count * 10) {
    throw new LandBuildError(`${what} placements do not match their count ${String(p.count)}`);
  }
  return { count: p.count, ids, xf };
}

/** `[minY, maxY]` of every patch over its 9 x 9 vertices (neighbouring patches share their border vertices). */
export function computePatchBounds(heights: Float32Array): Float32Array {
  const out = new Float32Array(PATCH_COUNT * 2);
  for (let pz = 0; pz < PATCHES_PER_SIDE; pz++) {
    for (let px = 0; px < PATCHES_PER_SIDE; px++) {
      let lo = Infinity;
      let hi = -Infinity;
      for (let j = 0; j <= PATCH_SIZE; j++) {
        const row = (pz * PATCH_SIZE + j) * VERTS + px * PATCH_SIZE;
        for (let i = 0; i <= PATCH_SIZE; i++) {
          const h = heights[row + i] ?? 0;
          if (h < lo) lo = h;
          if (h > hi) hi = h;
        }
      }
      const at = (px + pz * PATCHES_PER_SIDE) * 2;
      out[at] = lo;
      out[at + 1] = hi;
    }
  }
  return out;
}

/** Builds the typed views of a `.lnd` pack. Throws `LandBuildError` when the sections do not fit the blob. */
export function landFromPack(meta: LndMeta, blob: Uint8Array): LandData {
  let heights: Float32Array;
  let attrs: Uint8Array;
  let water: Uint8Array;
  let alpha: Uint8Array;
  let lightRgb: Uint8Array;
  let objects: PlacementData;
  let sfx: PlacementData;
  try {
    heights = view(blob, { ...meta.heights, ty: 'f32' });
    attrs = view(blob, { ...meta.attrs, ty: 'u8' });
    water = view(blob, { ...meta.water, ty: 'u8' });
    alpha = view(blob, { ...meta.alpha, ty: 'u8' });
    lightRgb = view(blob, { ...meta.light_rgb, ty: 'u8' });
    objects = placements(meta.objects, blob, 'object');
    sfx = placements(meta.sfx, blob, 'sfx');
  } catch (e) {
    if (e instanceof LandBuildError) throw e;
    throw new LandBuildError(e instanceof Error ? e.message : String(e));
  }
  if (heights.length !== VERTEX_COUNT) {
    throw new LandBuildError(`height section has ${String(heights.length)} values`);
  }
  if (water.length !== PATCH_COUNT * 2) {
    throw new LandBuildError(`water section has ${String(water.length)} bytes`);
  }
  if (alpha.length !== meta.layers.length * LIGHTMAP_TEXELS) {
    throw new LandBuildError(
      `alpha section has ${String(alpha.length)} bytes for ${String(meta.layers.length)} layers`,
    );
  }
  if (meta.layers.length > 0 && lightRgb.length !== LIGHTMAP_TEXELS * 3) {
    throw new LandBuildError(`light section has ${String(lightRgb.length)} bytes`);
  }
  const patchBounds = computePatchBounds(heights);
  let lo = Infinity;
  let hi = -Infinity;
  for (let i = 0; i < PATCH_COUNT; i++) {
    lo = Math.min(lo, patchBounds[i * 2] ?? 0);
    hi = Math.max(hi, patchBounds[i * 2 + 1] ?? 0);
  }
  return {
    x: meta.land_x,
    z: meta.land_z,
    mpu: meta.mpu,
    heights,
    attrs,
    water,
    layers: meta.layers.map((l) => ({ textureId: l.texture_id, patchMask: l.patch_mask })),
    alpha,
    lightRgb,
    objects,
    sfx,
    patchBounds,
    bounds: [lo, hi],
    blob,
    warnings: meta.warnings,
  };
}

/** `true` when layer `layer` covers patch `patch` (`px + pz * 16`). */
export function layerCoversPatch(layer: LandLayer, patch: number): boolean {
  const word = layer.patchMask[patch >> 5] ?? 0;
  return ((word >>> (patch & 31)) & 1) === 1;
}

/** The result of `buildPatchMasks`: what `uMask` holds, patch-major. */
export interface PatchMasks {
  /** `uint32 x 4` per patch (256 patches, row-major `px + pz * 16`): `[layers 0-31, layers 32-63, first, 0]`. */
  readonly data: Uint32Array;
  /** The kept layers' indices in the original order (the alpha map and texture slots follow this order). */
  readonly kept: readonly number[];
  /** Layers beyond `MAX_MASK_LAYERS` that had to be dropped. */
  readonly dropped: number;
  /** Patches without any enabled layer: holes (KEEP-QUIRK, Plan/41 9.5). */
  readonly holes: number;
}

/** `first` value of a patch without a layer (the shader discards it). */
export const NO_LAYER = 255;

/**
 * Transposes the layer-major patch masks to the patch-major `uMask` texels (Plan/41 9.4): layer `k` of the KEPT list
 * (layers whose texture failed to load are removed first, so "the next enabled layer becomes first") sets bit `k` of
 * the patch's word. `R` = layers 0-31, `G` = layers 32-63, `B` = the first enabled layer (`NO_LAYER` = none).
 */
export function buildPatchMasks(
  layers: readonly LandLayer[],
  keep: (layerIndex: number) => boolean = () => true,
): PatchMasks {
  const kept: number[] = [];
  layers.forEach((_l, i) => {
    if (keep(i)) kept.push(i);
  });
  const dropped = Math.max(0, kept.length - MAX_MASK_LAYERS);
  if (dropped > 0) kept.length = MAX_MASK_LAYERS;
  const data = new Uint32Array(PATCH_COUNT * 4);
  for (let p = 0; p < PATCH_COUNT; p++) {
    let w0 = 0;
    let w1 = 0;
    let first = NO_LAYER;
    kept.forEach((srcIndex, k) => {
      const layer = layers[srcIndex];
      if (!layer || !layerCoversPatch(layer, p)) return;
      if (k < 32) w0 = (w0 | (1 << k)) >>> 0;
      else w1 = (w1 | (1 << (k - 32))) >>> 0;
      if (first === NO_LAYER) first = k;
    });
    data[p * 4] = w0;
    data[p * 4 + 1] = w1;
    data[p * 4 + 2] = first;
  }
  let holes = 0;
  for (let p = 0; p < PATCH_COUNT; p++) if (data[p * 4 + 2] === NO_LAYER) holes++;
  return { data, kept, dropped, holes };
}

/** One water or cloud patch quad. */
export interface WaterPatch {
  readonly px: number;
  readonly pz: number;
  /** `byWaterHeight` (integer world units). */
  readonly height: number;
  /** `tex >> 2`. */
  readonly set: number;
}

/**
 * The patches of a land by type: water (`tex & 3 == WTYPE_WATER`, grouped by `tex >> 2` later) and cloud
 * (`tex == WTYPE_CLOUD` UNMASKED: KEEP-QUIRK, a cloud patch with a non-zero frame set is not a cloud, Plan/41 10.2).
 */
export function waterPatches(water: Uint8Array): { water: WaterPatch[]; cloud: WaterPatch[] } {
  const w: WaterPatch[] = [];
  const c: WaterPatch[] = [];
  for (let p = 0; p < PATCH_COUNT; p++) {
    const height = water[p * 2] ?? 0;
    const tex = water[p * 2 + 1] ?? 0;
    const patch: WaterPatch = {
      px: p % PATCHES_PER_SIDE,
      pz: Math.floor(p / PATCHES_PER_SIDE),
      height,
      set: tex >> 2,
    };
    if ((tex & 3) === WTYPE.water) w.push(patch);
    else if (tex === WTYPE.cloud) c.push(patch);
  }
  return { water: w, cloud: c };
}
