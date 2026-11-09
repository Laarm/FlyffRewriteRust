// The terrain splat shader (Plan/41 section 9.5, WP-REND-09; exact equivalent of ref/19 section 16.3): ONE draw per land
// and pass composites the land's layers in the pixel shader instead of one draw per layer and patch.
//
// The original draws, per layer in file order and per patch whose enable bit is set, with
//     stage 0:  colour = layerTexture * vertexDiffuse            (MODULATE)
//     stage 1:  colour = stage0 * lightmap.rgb * 2               (MODULATE2X, clamped)
//     alpha:    the FIRST layer drawn on a patch uses the stage-0 alpha (texture.a), every later layer the alpha of
//               its 128 x 128 light/alpha map (`lightmap.a`), blend SRCALPHA / INVSRCALPHA, vertex fog per pass.
// Composited over a framebuffer `dst` that is the clear (fog) colour this is, per pixel, with `a_i` the layer alphas,
// `c_i` the layer colours and `f` the fog factor:
//     S = sum_i c_i a_i prod_{j>i} (1 - a_j),    P = prod_i (1 - a_i)
//     result = f * S + (1 - f) * fogColor * (1 - P) + P * dst
// which is what the shader outputs PREMULTIPLIED (`ONE, ONE_MINUS_SRC_ALPHA`), so splitting the layers into passes of
// at most 16 (`LAYERS_PER_PASS`; observed maximum 33 -> 3 passes) composes exactly. Pass 0 writes depth, later passes
// test with LEQUAL and do not write.
//
// Per fragment: patch = floor(cell / 8); `uMask` holds per patch the enabled layer bits (R: layers 0-31, G: 32-63) and the
// first enabled layer (B, 255 = none: the patch is a HOLE and is discarded, KEEP-QUIRK); the detail UV is
// `cell * 0.375` (the texture repeats 3 x per 8-cell patch); the light/alpha UV is `(cell * 0.875 + 0.5) / 128`
// (vertices 0..128 map to the texel centres 0..112 of the 128 x 128 maps, texels 113..127 are filler).
//
// Layer textures live in the buckets of `terrainTexturePool.ts` (`sampler2DArray`); a layer is a (bucket, slice) pair.
// Samplers cannot be indexed dynamically in GLSL ES 3.00, so the bucket is selected by a chain of uniform branches and the
// texture is read with `textureGrad` and gradients taken OUTSIDE the non-uniform control flow.
//
// Lighting is per VERTEX (Gouraud, gamma-naive) exactly like the rigid models (Plan/41 7.2):
//     L = clamp(amb + floor(amb * 255) / 255 + dif * max(0, N . (-dir)), 0, 1)
// The land mesh has no rotation, so the land-local normal is the world normal and the light direction is used as is.

import {
  CustomBlending,
  DataArrayTexture,
  DataTexture,
  FrontSide,
  GLSL3,
  LessEqualDepth,
  LinearFilter,
  NearestFilter,
  OneFactor,
  OneMinusSrcAlphaFactor,
  RawShaderMaterial,
  RedFormat,
  RGBAFormat,
  RGBAIntegerFormat,
  UnsignedByteType,
  UnsignedIntType,
  ClampToEdgeWrapping,
  type Texture,
} from 'three';
import type { EnvUniforms } from '../gl/materials/env';
import { LIGHTMAP_SIZE, PATCHES_PER_SIDE } from './landData';
import { MAX_BUCKETS } from './terrainTexturePool';

/** Layers per pass (Plan/41 R6). */
export const LAYERS_PER_PASS = 16;

const VERTEX = /* glsl */ `
precision highp float;
precision highp int;

uniform mat4 modelViewMatrix;
uniform mat4 projectionMatrix;
uniform float uMpu;
uniform vec3 uDif;
uniform vec3 uAmb;
uniform vec3 uAmbRender;
uniform vec3 uLightDir;
uniform vec2 uFogRange;

in vec3 position;
in vec3 normal;

out vec2 vCell;
out vec3 vLight;
out float vFog;

invariant gl_Position;

void main() {
  vec4 mv = modelViewMatrix * vec4(position, 1.0);
  gl_Position = projectionMatrix * mv;
  // land-local cell coordinate (three.js z is mirrored)
  vCell = vec2(position.x, -position.z) / uMpu;
  float ndl = max(0.0, dot(normal, -uLightDir));
  vLight = clamp(uAmb + uAmbRender + uDif * ndl, 0.0, 1.0);
  float dist = length(mv.xyz);
  vFog = clamp((uFogRange.y - dist) / (uFogRange.y - uFogRange.x), 0.0, 1.0);
}
`;

/** The fragment shader; `BUCKET_COUNT` samplers are declared. */
export const SPLAT_FRAGMENT = /* glsl */ `
precision highp float;
precision highp int;
precision highp sampler2D;
precision highp sampler2DArray;
precision highp usampler2D;

uniform usampler2D uMask;
uniform sampler2DArray uAlpha;
uniform sampler2D uLight;
uniform sampler2DArray uBucket0;
uniform sampler2DArray uBucket1;
uniform sampler2DArray uBucket2;
uniform sampler2DArray uBucket3;
uniform sampler2DArray uBucket4;
uniform sampler2DArray uBucket5;
uniform float uLayerBucket[${String(LAYERS_PER_PASS)}];
uniform float uLayerSlice[${String(LAYERS_PER_PASS)}];
uniform int uLayerBase;
uniform int uLayerCount;
uniform vec3 uFogColor;
uniform float uFade;

in vec2 vCell;
in vec3 vLight;
in float vFog;

layout(location = 0) out highp vec4 fragColor;

vec4 sampleBucket(int b, vec3 uvs, vec2 gx, vec2 gy) {
  if (b == 0) return textureGrad(uBucket0, uvs, gx, gy);
  if (b == 1) return textureGrad(uBucket1, uvs, gx, gy);
  if (b == 2) return textureGrad(uBucket2, uvs, gx, gy);
  if (b == 3) return textureGrad(uBucket3, uvs, gx, gy);
  if (b == 4) return textureGrad(uBucket4, uvs, gx, gy);
  return textureGrad(uBucket5, uvs, gx, gy);
}

bool layerEnabled(uvec4 m, int li) {
  if (li < 32) return ((m.r >> uint(li)) & 1u) == 1u;
  return ((m.g >> uint(li - 32)) & 1u) == 1u;
}

void main() {
  ivec2 patchXZ = clamp(ivec2(floor(vCell / 8.0)), ivec2(0), ivec2(${String(PATCHES_PER_SIDE - 1)}));
  uvec4 m = texelFetch(uMask, patchXZ, 0);
  // detail texture: 3 repeats per 8-cell patch; gradients outside the non-uniform control flow below
  vec2 dUV = vCell * 0.375;
  vec2 gx = dFdx(dUV);
  vec2 gy = dFdy(dUV);
  // light/alpha maps: vertices 0..128 -> texel centres 0..112
  vec2 lmUV = (vCell * 0.875 + 0.5) / ${String(LIGHTMAP_SIZE)}.0;
  vec3 lm = textureLod(uLight, lmUV, 0.0).rgb;
  if (m.b == 255u) discard; // a patch without layers is a hole (KEEP-QUIRK)

  vec3 S = vec3(0.0);
  float P = 1.0;
  for (int i = 0; i < ${String(LAYERS_PER_PASS)}; i++) {
    if (i >= uLayerCount) break;
    int li = uLayerBase + i;
    if (!layerEnabled(m, li)) continue;
    vec4 t = sampleBucket(int(uLayerBucket[i] + 0.5), vec3(dUV, uLayerSlice[i]), gx, gy);
    // stage 0 MODULATE (texture x vertex light), stage 1 MODULATE2X (x light map x 2), both clamped
    vec3 col = min(2.0 * min(t.rgb * vLight, 1.0) * lm, 1.0);
    // the first layer of the patch uses the stage-0 alpha, the others the alpha map
    float a = (uint(li) == m.b) ? t.a : textureLod(uAlpha, vec3(lmUV, float(li)), 0.0).r;
    S = S * (1.0 - a) + col * a;
    P *= 1.0 - a;
  }
  // premultiplied result of the per-layer SRCALPHA/INVSRCALPHA composite with vertex fog
  vec3 rgb = vFog * S + (1.0 - vFog) * uFogColor * (1.0 - P);
  fragColor = vec4(rgb, 1.0 - P) * uFade;
}
`;

/** The per-land GPU textures of the splat shader. */
export interface SplatTextures {
  /** 16 x 16 `RGBA32UI`: patch-major layer masks (`PatchMasks.data`). */
  readonly mask: DataTexture;
  /** `R8` 128 x 128 x layers: the alpha maps of the KEPT layers. */
  readonly alpha: DataArrayTexture;
  /** `RGBA8` 128 x 128: the baked light RGB. */
  readonly light: DataTexture;
}

/** Builds the three small textures of a land. `alpha` is `layers * 16384` bytes of the kept layers, `light` is RGB. */
export function createSplatTextures(
  maskData: Uint32Array,
  alpha: Uint8Array,
  layerCount: number,
  lightRgb: Uint8Array,
): SplatTextures {
  const mask = new DataTexture(
    maskData,
    PATCHES_PER_SIDE,
    PATCHES_PER_SIDE,
    RGBAIntegerFormat,
    UnsignedIntType,
  );
  mask.internalFormat = 'RGBA32UI';
  mask.minFilter = NearestFilter;
  mask.magFilter = NearestFilter;
  mask.generateMipmaps = false;
  mask.needsUpdate = true;

  const slices = Math.max(1, layerCount);
  const alphaData =
    alpha.length === slices * LIGHTMAP_SIZE * LIGHTMAP_SIZE
      ? alpha
      : new Uint8Array(slices * LIGHTMAP_SIZE * LIGHTMAP_SIZE);
  const alphaTex = new DataArrayTexture(alphaData, LIGHTMAP_SIZE, LIGHTMAP_SIZE, slices);
  alphaTex.format = RedFormat;
  alphaTex.internalFormat = 'R8';
  alphaTex.type = UnsignedByteType;
  alphaTex.minFilter = LinearFilter;
  alphaTex.magFilter = LinearFilter;
  alphaTex.wrapS = ClampToEdgeWrapping;
  alphaTex.wrapT = ClampToEdgeWrapping;
  alphaTex.generateMipmaps = false;
  alphaTex.unpackAlignment = 1;
  alphaTex.needsUpdate = true;

  const rgba = new Uint8Array(LIGHTMAP_SIZE * LIGHTMAP_SIZE * 4);
  const n = LIGHTMAP_SIZE * LIGHTMAP_SIZE;
  for (let i = 0; i < n; i++) {
    rgba[i * 4] = lightRgb[i * 3] ?? 127;
    rgba[i * 4 + 1] = lightRgb[i * 3 + 1] ?? 127;
    rgba[i * 4 + 2] = lightRgb[i * 3 + 2] ?? 127;
    rgba[i * 4 + 3] = 255;
  }
  const light = new DataTexture(rgba, LIGHTMAP_SIZE, LIGHTMAP_SIZE, RGBAFormat, UnsignedByteType);
  light.minFilter = LinearFilter;
  light.magFilter = LinearFilter;
  light.wrapS = ClampToEdgeWrapping;
  light.wrapT = ClampToEdgeWrapping;
  light.generateMipmaps = false;
  light.needsUpdate = true;
  return { mask, alpha: alphaTex, light };
}

/** What the material of one pass needs. */
export interface SplatPassSpec {
  readonly textures: SplatTextures;
  /** Per KEPT layer: its `(bucket, slice)` in the texture pool. */
  readonly layerBucket: readonly number[];
  readonly layerSlice: readonly number[];
  /** First layer of the pass and its layer count (<= 16). */
  readonly base: number;
  readonly count: number;
  readonly mpu: number;
  /** The shared model environment (light, fog), by reference. */
  readonly env: EnvUniforms;
  /** The pool's six bucket sampler textures. */
  readonly buckets: readonly Texture[];
  /** `0` draws the first pass: depth write on. */
  readonly pass: number;
}

/** Number of passes for `layers` layers. */
export function passCount(layers: number): number {
  return Math.max(1, Math.ceil(layers / LAYERS_PER_PASS));
}

/** The `RawShaderMaterial` of one pass of one land. */
export function createSplatMaterial(spec: SplatPassSpec): RawShaderMaterial {
  const layerBucket = new Float32Array(LAYERS_PER_PASS);
  const layerSlice = new Float32Array(LAYERS_PER_PASS);
  for (let i = 0; i < spec.count; i++) {
    layerBucket[i] = spec.layerBucket[spec.base + i] ?? 0;
    layerSlice[i] = spec.layerSlice[spec.base + i] ?? 0;
  }
  const uniforms: Record<string, { value: unknown }> = {
    ...spec.env,
    uMask: { value: spec.textures.mask },
    uAlpha: { value: spec.textures.alpha },
    uLight: { value: spec.textures.light },
    uLayerBucket: { value: layerBucket },
    uLayerSlice: { value: layerSlice },
    uLayerBase: { value: spec.base },
    uLayerCount: { value: spec.count },
    uMpu: { value: spec.mpu },
    uFade: { value: 1 },
  };
  for (let i = 0; i < MAX_BUCKETS; i++) {
    uniforms[`uBucket${String(i)}`] = { value: spec.buckets[i] ?? null };
  }
  const m = new RawShaderMaterial({
    name: `terrain-splat-p${String(spec.pass)}`,
    glslVersion: GLSL3,
    vertexShader: VERTEX,
    fragmentShader: SPLAT_FRAGMENT,
    uniforms,
    side: FrontSide,
    transparent: true,
    blending: CustomBlending,
    blendSrc: OneFactor,
    blendDst: OneMinusSrcAlphaFactor,
    blendSrcAlpha: OneFactor,
    blendDstAlpha: OneMinusSrcAlphaFactor,
    depthTest: true,
    depthFunc: LessEqualDepth,
    depthWrite: spec.pass === 0,
  });
  return m;
}
