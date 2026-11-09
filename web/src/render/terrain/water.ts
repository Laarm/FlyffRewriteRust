// Water patches, the cloud sea and the underwater overlay (Plan/41 section 10, WP-REND-10; lod.cpp:357-594, 932-1072,
// World3D.cpp:1131-1177, ref/19 section 17).
//
//   Water   patches with `tex & 3 == WTYPE_WATER`: a flat quad over the 8 x 8-cell patch at `y = byWaterHeight`, UV 0..3,
//           grouped per land and water set (`tex >> 2`); colour = the current frame texture of the set (unlit),
//           alpha = 0xAA/255 x 200/255 = 0.5229, SRCALPHA/INVSRCALPHA, cull none, depth write ON, vertex fog.
//           The set's frame clock is GLOBAL: `frame += 0.15` per 60 Hz tick, back to 0 at `frame >= count` (54 frames);
//           drawn texture = `frames[floor(min(frame, count - 1))]` (about 9 frames/s, a 6 s loop).
//   Cloud   patches with `tex == WTYPE_CLOUD` (UNMASKED compare, KEEP-QUIRK: a patch with a frame set is not a cloud):
//           quads at `y = 0` (UV 0..1 per patch) drawn TWICE with translations y = 40 and y = 80, terrain texture id 10
//           (`sky_cloud02.dds`), same alpha and states; UV offsets (texture matrix translation) advance per TICK:
//           layer 1 += 0.001, layer 2 += 0.0015 on both axes. FIX X8/PL-134: one GLOBAL offset pair (the original advanced
//           per rendered frame and per land object, so every land scrolled at its own phase and showed seams).
//   Under   if the camera is inside a water patch and below its height: a full-screen quad with terrain texture id 20
//           (the first river frame), alpha 0x7F/255, no depth, no fog. Every vertex of the original's quad has UV (0, 0)
//           (World3D.cpp:1155-1170), so the whole screen takes ONE colour: the texture sampled at its corner (four texels
//           averaged by the bilinear filter, the repeat wrap). The overlay used to spread the whole river frame over the
//           screen (a pattern of ripples and light shafts); the original shows a flat tint.
//
// Everything here is pure data or small GL objects without a renderer: the geometry builders and the clocks are
// unit-tested; `Terrain.ts` owns the instances.

import {
  BufferAttribute,
  BufferGeometry,
  DoubleSide,
  GLSL3,
  NormalBlending,
  RawShaderMaterial,
  Vector2,
  type Texture,
} from 'three';
import type { EnvUniforms } from '../gl/materials/env';
import { PATCH_SIZE, type WaterPatch } from './landData';

/** `alpha = 0xAA / 255 * 200 / 255` (vertex alpha x TFACTOR alpha, lod.cpp RenderWater). */
export const WATER_ALPHA = (0xaa / 255) * (200 / 255);
/** The underwater overlay's alpha (`TFACTOR 0x7f7fffff`... the alpha byte 0x7F). */
export const UNDERWATER_ALPHA = 0x7f / 255;
/** `fWaterFrame` of the original: frames per tick. */
export const WATER_FRAME_STEP = Math.fround(0.15);
/** Terrain texture ids used by cloud sea and the underwater overlay (Terrain.inc, ref/19 section 14). */
export const CLOUD_TEXTURE_ID = 10;
export const UNDERWATER_TEXTURE_ID = 20;
/** Cloud-sea layer heights and per-tick UV steps. */
export const CLOUD_LAYERS = [
  { y: 40, step: Math.fround(0.001) },
  { y: 80, step: Math.fround(0.0015) },
] as const;

// ---- clocks --------------------------------------------------------------------------------------------

/** The frame clock of the water sets (`CTerrainMng::m_fWaterFrame`, World.cpp:952-962), advanced per 60 Hz tick. */
export class WaterClock {
  private readonly frames: number[];

  /** `counts[s]` = number of frame textures of set `s` (54 in the data). */
  constructor(readonly counts: readonly number[]) {
    this.frames = counts.map(() => 0);
  }

  /** Advances every set by `steps` ticks: `frame += 0.15` (f32), `frame = 0` once `frame >= count`. */
  tick(steps: number): void {
    for (let n = 0; n < steps; n++) {
      this.frames.forEach((f, s) => {
        const count = this.counts[s] ?? 0;
        let next = Math.fround(f + WATER_FRAME_STEP);
        if (next >= count) next = 0;
        this.frames[s] = next;
      });
    }
  }

  /** The raw frame value of set `s`. */
  value(s: number): number {
    return this.frames[s] ?? 0;
  }

  /** The index of the texture to draw: `floor(min(frame, count - 1))`, `-1` for an unknown or empty set. */
  index(s: number): number {
    const count = this.counts[s] ?? 0;
    if (count <= 0) return -1;
    return Math.floor(Math.min(this.value(s), count - 1));
  }

  reset(): void {
    this.frames.fill(0);
  }
}

/** The global cloud-sea scroll: the offset of both layers after `ticks` ticks (exact, no accumulated error). */
export class CloudScroll {
  /** Ticks since the world was set. */
  ticks = 0;

  tick(steps: number): void {
    this.ticks += Math.max(0, Math.trunc(steps));
  }

  /** The (u, v) offset of cloud layer `layer` (0: y = 40, 1: y = 80), wrapped to 0..1 (the texture repeats). */
  offset(layer: 0 | 1): [number, number] {
    const v = (this.ticks * CLOUD_LAYERS[layer].step) % 1;
    return [v, v];
  }

  reset(): void {
    this.ticks = 0;
  }
}

// ---- geometry ------------------------------------------------------------------------------------------

/** `positions` (x y z per vertex, land-local three.js space), `uvs`, `indices` of a set of patch quads. */
export interface QuadMesh {
  readonly positions: Float32Array;
  readonly uvs: Float32Array;
  readonly indices: Uint16Array;
  readonly quads: number;
}

/**
 * One quad per patch: corners `(x0, z0) (x0, z1) (x1, z0) (x1, z1)` over the patch's 8 cells at height `height(p)`
 * (`0` for cloud quads), UV `0..uvMax` (3 for water, 1 for clouds) with `u` along +x and `v` along +z like the original.
 * Positions are land-local in three.js space (`z` mirrored); the mesh matrix translates to the land origin.
 */
export function buildQuads(
  patches: readonly WaterPatch[],
  mpu: number,
  uvMax: number,
  height: (p: WaterPatch) => number,
): QuadMesh {
  const n = patches.length;
  const positions = new Float32Array(n * 12);
  const uvs = new Float32Array(n * 8);
  const indices = new Uint16Array(n * 6);
  const span = PATCH_SIZE * mpu;
  patches.forEach((p, i) => {
    const x0 = p.px * span;
    const x1 = x0 + span;
    const z0 = -(p.pz * span);
    const z1 = z0 - span;
    const y = height(p);
    positions.set([x0, y, z0, x0, y, z1, x1, y, z0, x1, y, z1], i * 12);
    uvs.set([0, 0, 0, uvMax, uvMax, 0, uvMax, uvMax], i * 8);
    const v = i * 4;
    indices.set([v, v + 1, v + 2, v + 1, v + 3, v + 2], i * 6);
  });
  return { positions, uvs, indices, quads: n };
}

/** The water quads of one set. */
export function buildWaterQuads(
  patches: readonly WaterPatch[],
  mpu: number,
  set: number,
): QuadMesh {
  return buildQuads(
    patches.filter((p) => p.set === set),
    mpu,
    3,
    (p) => p.height,
  );
}

/** The cloud quads (all at y = 0). */
export function buildCloudQuads(patches: readonly WaterPatch[], mpu: number): QuadMesh {
  return buildQuads(patches, mpu, 1, () => 0);
}

/** The distinct water sets used by `patches`, ascending. */
export function waterSets(patches: readonly WaterPatch[]): number[] {
  return [...new Set(patches.map((p) => p.set))].sort((a, b) => a - b);
}

export function quadGeometry(q: QuadMesh): BufferGeometry {
  const g = new BufferGeometry();
  g.setAttribute('position', new BufferAttribute(q.positions, 3));
  g.setAttribute('uv', new BufferAttribute(q.uvs, 2));
  g.setIndex(new BufferAttribute(q.indices, 1));
  g.computeBoundingBox();
  g.computeBoundingSphere();
  return g;
}

// ---- materials -----------------------------------------------------------------------------------------

const WATER_VERTEX = /* glsl */ `
precision highp float;
uniform mat4 modelViewMatrix;
uniform mat4 projectionMatrix;
uniform vec2 uFogRange;
uniform vec2 uUvOffset;
in vec3 position;
in vec2 uv;
out vec2 vUv;
out float vFog;
void main() {
  vec4 mv = modelViewMatrix * vec4(position, 1.0);
  gl_Position = projectionMatrix * mv;
  vUv = uv + uUvOffset;
  float dist = length(mv.xyz);
  vFog = clamp((uFogRange.y - dist) / (uFogRange.y - uFogRange.x), 0.0, 1.0);
}
`;

const WATER_FRAGMENT = /* glsl */ `
precision highp float;
precision highp sampler2D;
uniform sampler2D uMap;
uniform vec3 uFogColor;
uniform float uWaterAlpha;
in vec2 vUv;
in float vFog;
layout(location = 0) out highp vec4 fragColor;
void main() {
  vec4 t = texture(uMap, vUv);
  // colour = texture (SELECTARG1, unlit), alpha = vertex alpha x TFACTOR alpha, then the vertex fog
  fragColor = vec4(mix(uFogColor, t.rgb, vFog), uWaterAlpha);
}
`;

export interface WaterMaterial extends RawShaderMaterial {
  readonly uniforms: {
    uMap: { value: Texture | null };
    uUvOffset: { value: Vector2 };
    uWaterAlpha: { value: number };
  } & Record<string, { value: unknown }>;
}

/** The material of water and cloud quads (the shared environment drives the fog). */
export function createWaterMaterial(env: EnvUniforms, name: string): WaterMaterial {
  const m = new RawShaderMaterial({
    name,
    glslVersion: GLSL3,
    vertexShader: WATER_VERTEX,
    fragmentShader: WATER_FRAGMENT,
    uniforms: {
      ...env,
      uMap: { value: null },
      uUvOffset: { value: new Vector2(0, 0) },
      uWaterAlpha: { value: WATER_ALPHA },
    },
    side: DoubleSide,
    transparent: true,
    blending: NormalBlending,
    depthTest: true,
    depthWrite: true,
  });
  return m as WaterMaterial;
}

const OVERLAY_VERTEX = /* glsl */ `
precision highp float;
in vec3 position;
void main() {
  gl_Position = vec4(position.xy, 0.0, 1.0);
}
`;

/** The UV of every vertex of the underwater quad (`pVertices[i].tu1 = tv1 = 0`, World3D.cpp:1155-1170). */
export const UNDERWATER_UV: readonly [number, number] = [0, 0];

const OVERLAY_FRAGMENT = /* glsl */ `
precision highp float;
precision highp sampler2D;
uniform sampler2D uMap;
uniform float uAlpha;
layout(location = 0) out highp vec4 fragColor;
void main() {
  // one flat colour for the whole screen: the texture at the UV of every vertex of the original's quad
  fragColor = vec4(texture(uMap, vec2(${UNDERWATER_UV[0].toFixed(1)}, ${UNDERWATER_UV[1].toFixed(1)})).rgb, uAlpha);
}
`;

/** The material of the full-screen underwater quad: colour = the texture at (0, 0), alpha 0x7F/255, no depth, no fog. */
export function createUnderwaterMaterial(): RawShaderMaterial & {
  readonly uniforms: { uMap: { value: Texture | null }; uAlpha: { value: number } };
} {
  return new RawShaderMaterial({
    name: 'underwater-overlay',
    glslVersion: GLSL3,
    vertexShader: OVERLAY_VERTEX,
    fragmentShader: OVERLAY_FRAGMENT,
    uniforms: { uMap: { value: null }, uAlpha: { value: UNDERWATER_ALPHA } },
    side: DoubleSide,
    transparent: true,
    blending: NormalBlending,
    depthTest: false,
    depthWrite: false,
  }) as never;
}

/** `true` when the camera is inside a water patch and below the surface (Plan/41 10.3). */
export function isUnderwater(
  water: { readonly height: number; readonly type: number } | null,
  cameraY: number,
): boolean {
  return water !== null && water.type === 2 && cameraY < water.height;
}
