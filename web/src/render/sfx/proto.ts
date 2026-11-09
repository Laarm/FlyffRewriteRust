// SFX prototypes (`CSfxBase`, Plan/41 section 16.1, ref/20 section 3): the decoded `.sfx` description prepared for the
// runtime. `SfxMeta` (`decode_sfx`, Plan/30 13.6) holds the parts with `effective_keys()` (sorted by frame, one key per frame) and
// the particle block; this module adds what the renderer needs: lower-case texture paths per animation frame, the first and last
// key frame of each part, and the bounding sphere for the culling (PL-123).

import type { ParticleParams, SfxKey, SfxMeta } from '../../gen/formats';
import { textureNameOfFrame } from './keys';

/** `SFXPARTBILLTYPE_*` of a part (`bill_type`). */
export const BILL_TYPE = { bill: 1, bottom: 2, pole: 3, normal: 4 } as const;
export type BillType = (typeof BILL_TYPE)[keyof typeof BILL_TYPE];

export type PartKind = 'bill' | 'particle' | 'mesh' | 'custom_mesh';

/** One part of a prepared prototype. */
export interface PartProto {
  readonly index: number;
  readonly kind: PartKind;
  /** `m_bUseing`: a hidden editor layer is never drawn (and a hidden particle part never keeps the effect alive). */
  readonly inUse: boolean;
  readonly billType: BillType;
  /** `alphaType` 2 = GLOW (additive, dest ONE); 1 = BLEND (dest INVSRCALPHA). */
  readonly additive: boolean;
  readonly texFrame: number;
  readonly texLoop: number;
  /** The effective keys (sorted by frame, one per frame). */
  readonly keys: readonly SfxKey[];
  /** `keys[0].frame` (0 without keys, as the emission window of the original). */
  readonly first: number;
  /** `keys[last].frame` (0 without keys). */
  readonly last: number;
  readonly particle: ParticleParams | null;
  /** Ring resolution of a custom mesh (`m_nPoints`, default 20). */
  readonly points: number;
  /**
   * Canonical VFS paths of the textures: one for a plain part, `texFrame` of them for a texture animation (index = animation
   * frame). Empty for mesh parts (the mesh is `meshPath`).
   */
  readonly texturePaths: readonly string[];
  /** `model/<name>` of a MESH part, `null` otherwise. */
  readonly meshPath: string | null;
}

/** A prepared prototype. */
export interface SfxProto {
  /** The VFS path of the `.sfx` (`sfx/<name>.sfx`, lower-case). */
  readonly file: string;
  readonly version: SfxMeta['version'];
  readonly parts: readonly PartProto[];
  /** The largest key frame of any part (the effect ends one frame after the last key). */
  readonly lastFrame: number;
  /** Bounding sphere radius around the effect position for the culling (PL-123), at scale 1. */
  readonly radius: number;
  /** Number of parts that draw quads (BILL and PARTICLE, in use). */
  readonly quadParts: number;
  readonly warnings: readonly string[];
}

const SFX_TEXTURE_DIR = 'sfx/texture';
const MODEL_DIR = 'model';

/** `sfx/texture/<name>`, lower-case with `/` separators (the VFS form; `g_SfxTex.Tex(name)`). */
export function sfxTexturePath(name: string): string {
  const n = name.replace(/\\/g, '/').toLowerCase();
  return `${SFX_TEXTURE_DIR}/${n}`;
}

/** `model/<name>` of a mesh part texture field. */
export function sfxMeshPath(name: string): string {
  const n = name.replace(/\\/g, '/').toLowerCase();
  return n.includes('/') ? n : `${MODEL_DIR}/${n}`;
}

const maxAbs3 = (v: readonly number[]): number =>
  Math.max(Math.abs(v[0] ?? 0), Math.abs(v[1] ?? 0), Math.abs(v[2] ?? 0));

/** The largest `|key.pos|` and `max |key.scale|` over the keys of a part (the placement the culling sphere grows by). */
function keyReach(part: PartProto): { pos: number; scale: number } {
  let pos = 0;
  let scale = 0;
  for (const k of part.keys) {
    pos = Math.max(pos, Math.hypot(k.pos[0], k.pos[1], k.pos[2]));
    scale = Math.max(scale, maxAbs3(k.scale));
  }
  return { pos, scale };
}

/**
 * The reach of a MESH part around the effect position, at instance scale 1: `max |key.pos| + extent * max |key.scale|`, where
 * `extent` is the reach of the loaded model's header box from its pivot (`SfxMeshes.extentOf`: `|centre| + |size| / 2`). The
 * culling (`SfxSystem`) multiplies the key scale by the model's extent (PL-123; #018 B9); `partRadius` has no model and uses 1.
 */
export function meshPartRadius(part: PartProto, extent: number): number {
  const { pos, scale } = keyReach(part);
  return pos + extent * scale;
}

/** The in-use MESH parts of a prototype (computed once per prototype: the culling asks every frame). */
const MESH_PARTS = new WeakMap<SfxProto, readonly PartProto[]>();
export function meshPartsOf(proto: SfxProto): readonly PartProto[] {
  let parts = MESH_PARTS.get(proto);
  if (!parts) {
    parts = proto.parts.filter((p) => p.kind === 'mesh' && p.inUse);
    MESH_PARTS.set(proto, parts);
  }
  return parts;
}

/**
 * The largest distance of anything the part draws from the effect position, at instance scale 1:
 * `max |key.pos|` + the reach of one quad (`0.71 * max |key.scale|`, a unit quad's half diagonal), custom meshes by their ring
 * radius and height, particles by `max(|xz|, |y|) * disappear + 0.5 * |accel| * disappear^2 + pos_var` plus their own size.
 * A MESH part counts its model as one unit (`meshPartRadius(part, 1)`): the culling adds the real extent once it is loaded.
 */
export function partRadius(part: PartProto): number {
  let pos = 0;
  let scale = 0;
  let ring = 0;
  let height = 0;
  for (const k of part.keys) {
    pos = Math.max(pos, Math.hypot(k.pos[0], k.pos[1], k.pos[2]));
    scale = Math.max(scale, maxAbs3(k.scale));
    ring = Math.max(ring, Math.abs(k.scale[0]), Math.abs(k.scale[2]));
    height = Math.max(height, Math.abs(k.scale[1]));
  }
  switch (part.kind) {
    case 'bill':
      return pos + 0.71 * scale;
    case 'mesh':
      return meshPartRadius(part, 1);
    case 'custom_mesh':
      return pos + Math.hypot(ring, height);
    case 'particle': {
      const p = part.particle;
      if (!p) return pos;
      const t = p.disappear;
      const speed = Math.max(
        Math.abs(p.xz_low),
        Math.abs(p.xz_high),
        Math.abs(p.y_low),
        Math.abs(p.y_high),
      );
      const accel = Math.hypot(p.accel[0], p.accel[1], p.accel[2]);
      const reach = speed * t + 0.5 * accel * t * t + Math.max(p.pos_var, p.pos_var_y);
      const grow = Math.max(maxAbs3(p.scale), 0) + maxAbs3(p.scale_speed) * t;
      return pos + reach + 0.71 * Math.max(grow, maxAbs3(p.scale_end));
    }
  }
}

function kindOf(k: SfxMeta['parts'][number]['kind']): PartKind {
  return k;
}

function billTypeOf(n: number): BillType {
  return n === 2
    ? BILL_TYPE.bottom
    : n === 3
      ? BILL_TYPE.pole
      : n === 4
        ? BILL_TYPE.normal
        : BILL_TYPE.bill;
}

/** Prepares a decoded `.sfx` (the file path is only a label). */
export function prepareProto(file: string, meta: SfxMeta): SfxProto {
  const parts: PartProto[] = meta.parts.map((p, index) => {
    const kind = kindOf(p.kind);
    const keys = p.keys;
    const first = keys[0]?.frame ?? 0;
    const last = keys[keys.length - 1]?.frame ?? 0;
    const mesh = kind === 'mesh';
    const animated = p.tex_frame > 1;
    let texturePaths: string[] = [];
    if (!mesh) {
      if (animated) {
        const names = p.frame_textures;
        texturePaths = [];
        for (let i = 0; i < p.tex_frame; i++) {
          texturePaths.push(sfxTexturePath(names[i] ?? textureNameOfFrame(p.texture, i)));
        }
      } else if (p.texture.trim() !== '') {
        // a part without a texture file name has no texture to ask for: `sfx/texture/` is the directory, not a file, and
        // asking for it counted as a missing texture of the world (integration B4); like a missing file it draws nothing. That is
        // what the original does for all 23 such parts of the corpus (`Tex("")` is NULL, and none of them has a key frame, or
        // emits particles of a size; list in `proto.test.ts`, task polish-m3)
        texturePaths = [sfxTexturePath(p.texture)];
      }
    }
    return {
      index,
      kind,
      inUse: p.in_use,
      billType: billTypeOf(p.bill_type),
      additive: p.alpha_type === 2,
      texFrame: p.tex_frame,
      texLoop: p.tex_loop,
      keys,
      first,
      last,
      particle: p.particle,
      points: p.points ?? 20,
      texturePaths,
      meshPath: mesh ? sfxMeshPath(p.texture) : null,
    };
  });
  let lastFrame = 0;
  let radius = 0;
  let quadParts = 0;
  for (const p of parts) {
    lastFrame = Math.max(lastFrame, p.last);
    if (p.inUse) {
      radius = Math.max(radius, partRadius(p));
      if (p.kind === 'bill' || p.kind === 'particle') quadParts++;
    }
  }
  return {
    file,
    version: meta.version,
    parts,
    lastFrame,
    radius,
    quadParts,
    warnings: meta.warnings,
  };
}
