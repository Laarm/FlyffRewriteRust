// The D3D9 material system: creates the `RawShaderMaterial`s of the model families from a `MaterialKey` and keeps
// the permutation statistics (Plan/41 section 7.1). Every material shares the `EnvUniforms` by reference; the
// per-block values are `uMap` (the texture), `uAlpha` (`amount / 255` or the model blend) and, for reflect blocks,
// the shared `uReflectMap` (`etc_reflect.tga`, sphere map).
//
// Culling: single-sided materials use `FrontSide`; the `onBeforeRender` of the material sets `BackSide` for draws
// whose `matrixWorld` has a negative determinant. three.js flips the winding for such meshes (so a mirrored node
// would look "correct"); D3D culled by screen-space winding and therefore showed the BACK faces of the 29 mirrored
// corpus nodes (KEEP-QUIRK, Plan/41 section 3.1, ref/18 section 14.2). `BackSide` cancels the flip, so the
// effective culling equals D3D's regardless of the determinant. Instanced draws keep their own matrix (identity
// for the InstancedMesh), hence `FrontSide`.

import {
  BackSide,
  CustomBlending,
  DoubleSide,
  DstColorFactor,
  FrontSide,
  GLSL3,
  NoBlending,
  NormalBlending,
  OneFactor,
  RawShaderMaterial,
  ZeroFactor,
  type Matrix4,
  type Object3D,
  type Texture,
  type Vector3,
} from 'three';
import { LAYER } from '../../layers';
import { createEnvUniforms, DEFAULT_ENV, type EnvLight, type EnvUniforms, applyEnv } from './env';
import { Geo, keyString, type MaterialKey } from './key';
import { modelShader } from './modelShader';

export interface MaterialFactoryDeps {
  /** The texture of untextured blocks and failed textures (the shared white). */
  readonly white: () => Texture;
  /** Neutral sphere map until `etc_reflect` has loaded (`ADDSIGNED2X` with 0.25 keeps the colour). */
  readonly neutralReflect?: () => Texture;
  readonly env?: EnvUniforms;
}

/**
 * The uniforms of palette skinning (WP-REND-04): the shared bone-palette texture of one pose source and the first
 * palette matrix of the mover. The objects are shared BY REFERENCE between the materials of one mover, so updating
 * `base.value` once per frame updates all of them.
 */
export interface SkinBinding {
  readonly palette: { value: Texture | null };
  readonly base: { value: number };
}

/** The hair ambient rule (Plan/41 section 12.5): the colour and whether world lighting is on (previews: off). */
export interface HairBinding {
  readonly color: { value: Vector3 };
  readonly lit: { value: number };
}

/** `RawShaderMaterial` with the fields the factory manages. */
export interface ModelMaterial extends RawShaderMaterial {
  userData: { key: MaterialKey; keyString: string; sharedId?: string };
}

/** Determinant of the upper 3x3 of `m` (column-major elements). */
export function det3x3(m: Matrix4): number {
  const e = m.elements;
  const a = e[0];
  const b = e[4];
  const c = e[8];
  const d = e[1];
  const f = e[5];
  const g = e[9];
  const h = e[2];
  const i = e[6];
  const j = e[10];
  return a * (f * j - g * i) - b * (d * j - g * h) + c * (d * i - f * h);
}

export class MaterialFactory {
  /** The environment shared by every material. */
  readonly env: EnvUniforms;
  private readonly reflectMap: { value: Texture | null } = { value: null };
  private readonly programs = new Set<string>();
  private readonly sharedMaterials = new Map<string, { material: ModelMaterial; refs: number }>();
  private created = 0;

  constructor(private readonly deps: MaterialFactoryDeps) {
    this.env = deps.env ?? createEnvUniforms(DEFAULT_ENV);
  }

  /** Updates the shared lighting and fog (once per drawn frame). */
  setEnvironment(env: EnvLight): void {
    applyEnv(this.env, env);
  }

  /** The sphere map of `XE_REFLECT` (`model/texture/etc_reflect`); `null` restores the neutral one. */
  setReflectTexture(t: Texture | null): void {
    this.reflectMap.value = t ?? this.deps.neutralReflect?.() ?? null;
  }

  /**
   * A shared material for `(key, texture, alpha)`: blocks with the same shader permutation, texture and constant
   * alpha use ONE material, so three.js skips the uniform refresh between their draws (a 600-draw static scene
   * pays for the material switches only). `alpha` is quantised to 8 bits. Release with `releaseShared`.
   */
  shared(key: MaterialKey, map: Texture | null, alpha: number): ModelMaterial {
    const a8 = Math.round(Math.min(1, Math.max(0, alpha)) * 255);
    const id = `${keyString(key)}#${String(map === null ? 'white' : map.id)}#${String(a8)}`;
    let e = this.sharedMaterials.get(id);
    if (!e) {
      const material = this.create(key, { map, alpha: a8 / 255, name: id });
      material.userData.sharedId = id;
      e = { material, refs: 0 };
      this.sharedMaterials.set(id, e);
    }
    e.refs++;
    return e.material;
  }

  /** Drops one reference of a material returned by {@link shared}; the last one disposes it. */
  releaseShared(m: ModelMaterial): void {
    const id = m.userData.sharedId;
    if (id === undefined) {
      m.dispose();
      return;
    }
    const e = this.sharedMaterials.get(id);
    if (!e) return;
    e.refs--;
    if (e.refs <= 0) {
      this.sharedMaterials.delete(id);
      e.material.dispose();
    }
  }

  /** Number of live shared materials. */
  get sharedCount(): number {
    return this.sharedMaterials.size;
  }

  /** Distinct shader programs (define sets) requested so far (`stats.shaderPermutations`). */
  get permutations(): number {
    return this.programs.size;
  }

  /** Materials created so far. */
  get materialCount(): number {
    return this.created;
  }

  /**
   * Creates the material of a block. `alpha` is `DerivedMaterial.alphaScale`, `map` the block texture (the shared
   * white when `null`: untextured blocks render white, ref/18 section 8.1).
   */
  create(
    key: MaterialKey,
    options: {
      map?: Texture | null;
      alpha?: number;
      name?: string;
      /** Palette skinning of a skinned key (without it the key is drawn in the bind pose). */
      skin?: SkinBinding;
      /** The hair uniforms of a `hair` key. */
      hair?: HairBinding;
      /**
       * Instanced palette skinning (`movers/MoverInstancer.ts`): the shared palette texture of the pose source; the world rows,
       * the palette base and the hair colour are per-instance attributes. Excludes `skin`.
       */
      instancedPalette?: { value: Texture | null };
      /**
       * Merged parts: the eight sampler uniforms `uMap0 .. uMap7` (one per texture slot, `movers/merge.ts`); the
       * `map` option is then unused. Requires `skin`.
       */
      slots?: readonly { value: Texture }[];
    } = {},
  ): ModelMaterial {
    const ks = keyString(key);
    const skin = key.geo === Geo.Skinned ? options.skin : undefined;
    const instancedPalette = key.geo === Geo.Skinned ? options.instancedPalette : undefined;
    const merged =
      (skin !== undefined || instancedPalette !== undefined) && options.slots !== undefined;
    const src = modelShader(
      key,
      skin || instancedPalette
        ? {
            palette: true,
            ...(merged ? { slots: true } : {}),
            ...(instancedPalette ? { instanced: true } : {}),
          }
        : {},
    );
    this.programs.add(JSON.stringify(src.defines)); // keys that differ only in their pass queue share a program
    this.created++;
    if (key.reflect && this.reflectMap.value === null) {
      this.reflectMap.value = this.deps.neutralReflect?.() ?? null;
    }
    const uniforms: Record<string, { value: unknown }> = {
      ...this.env,
      uAlpha: { value: options.alpha ?? 1 },
    };
    if (merged && options.slots) {
      options.slots.forEach((u, i) => {
        uniforms[`uMap${String(i)}`] = u;
      });
    } else {
      uniforms['uMap'] = { value: options.map ?? this.deps.white() };
    }
    if (key.reflect) uniforms['uReflectMap'] = this.reflectMap;
    if (skin) {
      uniforms['uPalette'] = skin.palette;
      uniforms['uPaletteBase'] = skin.base;
    }
    if (instancedPalette) uniforms['uPalette'] = instancedPalette;
    if (key.hair && key.geo === Geo.Skinned && options.hair) {
      // the instanced variant reads the colour from the `aHairColor` attribute: only `uHairLit` is a uniform there
      if (!instancedPalette) uniforms['uHairColor'] = options.hair.color;
      uniforms['uHairLit'] = options.hair.lit;
    }

    const m = new RawShaderMaterial({
      name: options.name ?? ks,
      glslVersion: GLSL3,
      vertexShader: src.vertex,
      fragmentShader: src.fragment,
      defines: { ...src.defines },
      uniforms,
      side: key.twoSided ? DoubleSide : FrontSide,
      depthTest: true,
      depthWrite: !key.nightOnly,
      transparent: key.blend !== 'none',
      fog: false,
      lights: false,
    }) as ModelMaterial;
    m.userData = { key, keyString: ks };
    switch (key.blend) {
      case 'none':
        m.blending = NoBlending;
        break;
      case 'alpha':
        m.blending = NormalBlending;
        break;
      case 'add':
        m.blending = CustomBlending;
        m.blendSrc = OneFactor;
        m.blendDst = OneFactor;
        m.blendSrcAlpha = OneFactor;
        m.blendDstAlpha = OneFactor;
        break;
      case 'mul':
        // XE_HIGHLIGHT_OBJ: DESTCOLOR, ZERO
        m.blending = CustomBlending;
        m.blendSrc = DstColorFactor;
        m.blendDst = ZeroFactor;
        m.blendSrcAlpha = DstColorFactor;
        m.blendDstAlpha = ZeroFactor;
        break;
    }
    if (!key.twoSided) {
      // KEEP-QUIRK culling of mirrored nodes (see the file comment)
      m.onBeforeRender = (_r, _s, _c, _g, object: Object3D) => {
        m.side = det3x3(object.matrixWorld) < 0 ? BackSide : FrontSide;
      };
    }
    return m;
  }
}

/** The layer id of a key's pass group. */
export const layerOf = (key: MaterialKey): number => LAYER[key.queue];

/** Sets the texture of a created material (a runtime texture swap, Plan/41 section 6.3). */
export function setMaterialMap(m: ModelMaterial, texture: Texture): void {
  const u = m.uniforms['uMap'];
  if (u) u.value = texture;
}

/** Sets the constant alpha factor (`amount / 255` or the model blend). */
export function setMaterialAlpha(m: ModelMaterial, alpha: number): void {
  const u = m.uniforms['uAlpha'];
  if (u) u.value = alpha;
}
