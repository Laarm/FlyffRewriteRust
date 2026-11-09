// `MaterialKey`: the permutation key of the D3D9 material system (Plan/41 section 7.1) and its derivation from the
// material block state (section 7.3, ref/18 section 8.2).
//
//   model blend factor < 255 (fade, TRANSPARENT_MODE)  alpha = tex.a * factor/255, alpha test off, blend alpha
//   else XE_OPACITY                                    alpha test a >= 0xB0; amount < 255: alpha = tex.a * amount/255,
//                                                      alpha test off, blend alpha (pass 4)
//   else                                               opaque, alpha ignored
//   XE_2SIDE                                           cull none
//   XE_REFLECT (block or element)                      sphere map in stage 1, ADDSIGNED2X
//   XE_SELF_ILLUMINATE (not with reflect)              lighting off
//   XE_HIGHLIGHT_OBJ (not reflect/self-illum)          blend DESTCOLOR, ZERO, effective only with blending (model blend < 255)
//   light object (ObjectMeta.light)                    night only, unlit, cull none, no Z write, blend ONE/ONE
//   XE_SPECULAR, XE_BUMP, XE_NOBUMP, XE_MTE           no-op (DROP)

import type { Queue } from '../../layers';

/** The geometry classes of the shader families (Plan/41 section 7.1; the non-model ones belong to later WPs). */
export const Geo = {
  Rigid: 0,
  Skinned: 1,
  Terrain: 2,
  Water: 3,
  Sfx: 4,
  Sky: 5,
  Overlay: 6,
} as const;
export type Geo = (typeof Geo)[keyof typeof Geo];

/** `XE_*` bits of `Submesh.effect` (`ModelGlobal.h:4-33`). */
export const XE = {
  REFLECT: 0x1,
  OPACITY: 0x2,
  TWO_SIDE: 0x4,
  SELF_ILLUMINATE: 0x8,
  SPECULAR: 0x10,
  BUMP: 0x20,
  HIDE: 0x40,
  NOBUMP: 0x1000,
  HIGHLIGHT_OBJ: 0x2000,
  MTE: 0x200000,
} as const;

/** `OPACITY_ALPHA_REF`: the alpha test of `XE_OPACITY` passes for `a >= 0xB0`. */
export const OPACITY_ALPHA_REF = 0xb0;

export type BlendMode = 'none' | 'alpha' | 'add' | 'mul';
export type FogMode = 'vertex' | 'object' | 'off';

export interface MaterialKey {
  readonly geo: Geo;
  /** `XE_OPACITY` with `amount == 255` (reference 0xB0). */
  readonly alphaTest: boolean;
  readonly blend: BlendMode;
  readonly twoSided: boolean;
  /** Lighting off: `XE_SELF_ILLUMINATE`, light objects. */
  readonly unlit: boolean;
  readonly reflect: boolean;
  /** Hair ambient rule (section 12.5; set by WP-REND-05). */
  readonly hair: boolean;
  readonly fog: FogMode;
  /** Instanced draws (WP-REND-11). */
  readonly instanced: boolean;
  /** Projected shadow modulation (section 13.2; WP-REND-30). */
  readonly shadowRecv: boolean;
  /** Light object: drawn only at night. */
  readonly nightOnly: boolean;
  /** The pass group that draws a block with this key. */
  readonly queue: Queue;
}

/** Inputs of the derivation: one material block of one object instance. */
export interface BlockState {
  /** `Submesh.effect` (`XE_*` bits). */
  readonly effect: number;
  /** `Submesh.amount` (0..255; meaningful with `XE_OPACITY`). */
  readonly amount: number;
  /** `ObjectMeta.light`. */
  readonly light: boolean;
  readonly skinned: boolean;
  /** Extra `XE_*` bits of the element at runtime (`XE_REFLECT` of a shield, `XE_HIGHLIGHT_OBJ` ...). */
  readonly elementEffect?: number;
  /** The model blend factor 0..255 (255 = opaque, the default). */
  readonly modelBlend?: number;
  readonly fog?: FogMode;
  /** The hair element of a player (skinned only): ambient = hair colour x fblack, diffuse 0 (Plan/41 section 12.5). */
  readonly hair?: boolean;
}

/** The result of the derivation: the key and the constant alpha factor of the blend (uniform `uAlpha`). */
export interface DerivedMaterial {
  readonly key: MaterialKey;
  /** `1` unless the block or the model blends: `amount / 255` or `modelBlend / 255`. */
  readonly alphaScale: number;
}

/** Derives the material key of a block per Plan/41 section 7.3 (first matching row wins). */
export function deriveMaterial(b: BlockState): DerivedMaterial {
  const effect = b.effect | (b.elementEffect ?? 0);
  const modelBlend = b.modelBlend ?? 255;
  const reflect = (effect & XE.REFLECT) !== 0;
  const selfIllum = !reflect && (effect & XE.SELF_ILLUMINATE) !== 0;
  const highlight = !reflect && !selfIllum && (effect & XE.HIGHLIGHT_OBJ) !== 0;
  const opacity = (effect & XE.OPACITY) !== 0;
  const geo: Geo = b.skinned ? Geo.Skinned : Geo.Rigid;
  const fog: FogMode = b.fog ?? (b.skinned ? 'object' : 'vertex');
  const base = {
    geo,
    hair: b.hair === true && b.skinned,
    fog,
    instanced: false,
    shadowRecv: false,
  };

  if (b.light) {
    return {
      key: {
        ...base,
        alphaTest: false,
        blend: 'add',
        twoSided: true,
        unlit: true,
        reflect: false,
        nightOnly: true,
        queue: 'translucent',
      },
      alphaScale: 1,
    };
  }

  const twoSided = (effect & XE.TWO_SIDE) !== 0;
  const unlit = selfIllum;
  if (modelBlend < 255) {
    return {
      key: {
        ...base,
        alphaTest: false,
        blend: highlight ? 'mul' : 'alpha',
        twoSided,
        unlit,
        reflect,
        nightOnly: false,
        queue: 'translucent',
      },
      alphaScale: Math.max(0, modelBlend) / 255,
    };
  }
  if (opacity) {
    const amount = Math.min(255, Math.max(0, b.amount));
    if (amount < 255) {
      return {
        key: {
          ...base,
          alphaTest: false,
          blend: 'alpha',
          twoSided,
          unlit,
          reflect,
          nightOnly: false,
          queue: 'blended',
        },
        alphaScale: amount / 255,
      };
    }
    return {
      key: {
        ...base,
        alphaTest: true,
        blend: 'none',
        twoSided,
        unlit,
        reflect,
        nightOnly: false,
        queue: 'opaque',
      },
      alphaScale: 1,
    };
  }
  return {
    key: {
      ...base,
      alphaTest: false,
      blend: 'none',
      twoSided,
      unlit,
      reflect,
      nightOnly: false,
      queue: 'opaque',
    },
    alphaScale: 1,
  };
}

/** A stable string of a key (cache identity, probe output). */
export function keyString(k: MaterialKey): string {
  return [
    `g${String(k.geo)}`,
    k.alphaTest ? 'at' : '',
    `b:${k.blend}`,
    k.twoSided ? '2s' : '',
    k.unlit ? 'ul' : '',
    k.reflect ? 'rf' : '',
    k.hair ? 'hr' : '',
    `f:${k.fog}`,
    k.instanced ? 'in' : '',
    k.shadowRecv ? 'sh' : '',
    k.nightOnly ? 'nt' : '',
    `q:${k.queue}`,
  ]
    .filter((s) => s !== '')
    .join('|');
}
