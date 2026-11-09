// The renderer's asset types for doc 40's asset manager (Plan/41 section 22.1, Plan/40 section 9.3-9.4). The manager
// (WP-CLIENT-09, `assets/`) owns fetching, ref-counting and eviction; an asset type tells it which worker job decodes
// a key, whether the decoded CPU data must be kept, and which other keys the decoded asset depends on. The upload
// step ("run() performs the three.js upload") lives in `gl/textures.ts` (`TextureCache`, textures) and
// `objects/ModelCache.ts` (`ModelCache`, geometry) and runs through the `UploadQueue`.
//
// The interface is declared here structurally (the manager's `AssetType<T>` of 40 section 9.3 is a superset); WP-REND-16
// adapts it when the manager lands.

import type {
  ChrMeta,
  LndMeta,
  LndPackOptions,
  O3dMeta,
  O3dPackOptions,
  SfxMeta,
} from '../gen/formats';
import type { JobKind } from '../workers/protocol';
import type { DeviceCaps } from './gl/caps';
import { textureFormOf } from './gl/textureLoader';
import { modelTexturePath, textureCandidates, type TextureNameRules } from './gl/textureNames';

/** What an asset type needs to know about the device. */
export interface AssetTypeContext {
  readonly caps: DeviceCaps;
}

/** The decode job of a key. */
export interface DecodeSpec {
  readonly kind: JobKind;
  readonly opts?: object;
}

/** A renderer asset type (structure of Plan/40 9.3 `AssetType<T>`). */
export interface RenderAssetType<TDecoded> {
  readonly name: string;
  /** Keep the decoded CPU data after the upload (precise pick meshes, land heights). */
  readonly cpuKeep: boolean;
  /** The worker job that decodes `path`, `null` when the asset needs no decode. */
  decode(path: string, ctx: AssetTypeContext): DecodeSpec | null;
  /** Other keys of the decoded asset. */
  dependencies?(decoded: TDecoded): string[];
}

/** `TextureType`: model, world and weather textures (Plan/41 6.1): `dds` (BC passthrough or the RGBA8 fallback) or `bitmap`. */
export const TextureType: RenderAssetType<never> = {
  name: 'texture',
  cpuKeep: false,
  decode(path, ctx) {
    const form = textureFormOf(path);
    if (form === 'png') return { kind: 'bitmap' };
    if (form === 'dds') return { kind: ctx.caps.s3tc ? 'dds' : 'dds_bc' };
    return null;
  },
};

/** `SfxTextureType`: always RGBA8 from `dds_bc`, quantised to RGBA5551 at upload (Plan/41 16.1). */
export const SfxTextureType: RenderAssetType<never> = {
  name: 'sfx-texture',
  cpuKeep: false,
  decode(path) {
    const form = textureFormOf(path);
    if (form === 'png') return { kind: 'bitmap' };
    if (form === 'dds') return { kind: 'dds_bc' };
    return null;
  },
};

/** The texture keys of a decoded mesh after the per-instance name rules (variants first, the base texture last). */
export function meshTextureKeys(meta: O3dMeta, rules: TextureNameRules = {}, lod = 0): string[] {
  const keys = new Set<string>();
  const group = meta.lods[Math.min(lod, meta.lods.length - 1)];
  for (const o of group?.objects ?? []) {
    for (const s of o.submeshes) {
      if (!s.texture) continue;
      for (const n of textureCandidates(s.texture, rules)) keys.add(modelTexturePath(n));
    }
  }
  return [...keys];
}

/**
 * `MeshType`: `.o3d` decoded to three.js space with all LOD groups; the original refuses the 6 files whose header
 * name does not match (KQ-42), the pack option `reject_name_mismatch` keeps that parity.
 */
export function meshDecodeOptions(path: string): Partial<O3dPackOptions> {
  return {
    space: 'three',
    lods: 'all',
    collision: false,
    reject_name_mismatch: true,
    requested_name: path.split('/').pop() ?? path,
  };
}

export const MeshType: RenderAssetType<O3dMeta> = {
  name: 'mesh',
  cpuKeep: false,
  decode(path) {
    return { kind: 'o3d', opts: meshDecodeOptions(path) };
  },
  dependencies: (meta) => meshTextureKeys(meta),
};

/** The same mesh type for models that need precise CPU picking (boss ranks, ctrls; Plan/41 20.3). */
export const PickableMeshType: RenderAssetType<O3dMeta> = {
  ...MeshType,
  name: 'mesh-pickable',
  cpuKeep: true,
};

/**
 * `LandType` (Plan/41 22.1): `.lnd` decoded to a `LndMeta` pack. The placement comes from the FILE NAME (`XXXX-ZZ.lnd`, X
 * first), never from the header (36 files of unloaded worlds disagree); `mpu` is the world's. The decoded blob stays
 * resident (`cpuKeep`): the CPU height field and the placements of the static objects read it. The three.js side of a
 * land (mesh, splat textures, water) is built by `terrain/Terrain.ts` as upload jobs.
 */
export function landDecodeOptions(path: string, mpu: number): LndPackOptions | null {
  const m = /(\d+)-(\d+)\.lnd$/i.exec(path);
  if (!m) return null;
  return { land_x: Number(m[1]), land_z: Number(m[2]), mpu, normals: false };
}

export function landType(mpu: number): RenderAssetType<LndMeta> {
  return {
    name: 'land',
    cpuKeep: true,
    decode(path) {
      const opts = landDecodeOptions(path, mpu);
      return opts ? { kind: 'lnd', opts } : null;
    },
  };
}

/** `SfxType` (Plan/41 22.1, WP-REND-18 consumes it): `.sfx` decoded to an effect prototype, no GPU upload. */
export const SfxType: RenderAssetType<SfxMeta> = {
  name: 'sfx',
  cpuKeep: true,
  decode() {
    return { kind: 'sfx' };
  },
};

/**
 * `SkeletonType` (WP-REND-04): `.chr` decoded to three.js space (grips and event points; the `FSK1` blob stays
 * left-handed). The decoded pack stays resident: `movers/skeleton.ts` reads the bind matrices of it and the sim gets
 * the same blob (`SimWorld::add_skeleton`).
 */
export const SkeletonType: RenderAssetType<ChrMeta> = {
  name: 'skeleton',
  cpuKeep: true,
  decode() {
    return { kind: 'chr', opts: { space: 'three' } };
  },
};
