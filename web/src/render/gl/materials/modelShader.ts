// GLSL ES 3.00 of the model materials (rigid and skinned-in-bind-pose; Plan/41 sections 7.2-7.4, ref/21 sections
// 6.5-6.7). One small uber shader, `#define`d by the `MaterialKey`; three.js compiles a program per distinct
// define set (at most ~40 permutations). `RawShaderMaterial` + `GLSL3`: three.js prepends `#version 300 es` and the
// defines and declares nothing else, the built-in uniforms (`modelViewMatrix`, `normalMatrix`, ...) are still filled.
//
// Lighting (per VERTEX, Gouraud, gamma-naive; material colours of the .o3d are ignored, white material):
//   rigid (fixed function):   L = clamp(amb + ambRender + dif * max(0, N.(-dir)), 0, 1)       (~ 2*amb + dif*N.L)
//   skinned (vs_1_1):         L = min(max(0, N.(-dir)) * dif + (amb + 0.4), 1)
//   unlit (self-illum, light objects): L = 1
// Rigid normals go through the inverse transpose of the model-view matrix and are NOT renormalised (D3D does the
// same without NORMALIZENORMALS, KEEP-QUIRK); skinned normals are normalised (the original shader works in object
// space with a unit light vector, bind pose = identity palette).
// The light travels along `uLightDir` (three space, world); the vertex shader rotates it into view space with the
// `viewMatrix` rotation, so no per-object light uniform is needed.
//
// Fog: linear, per-vertex factor f = clamp((end - dist) / (end - start), 0, 1) with the eye distance of the vertex
// (rigid) or ONE factor per object from the distance of the model origin (skinned, c95.w), mixed per fragment.
//
// Fragment: c = tex.rgb * L (MODULATE), reflect: c = clamp(2 * (c + sphere - 0.5), 0, 1) (ADDSIGNED2X), fog, alpha =
// tex.a * uAlpha for blended blocks, 1 otherwise (a texture with alpha but no XE_OPACITY renders opaque).

import { Geo, OPACITY_ALPHA_REF, type MaterialKey } from './key';

export interface ShaderSource {
  readonly vertex: string;
  readonly fragment: string;
  readonly defines: Readonly<Record<string, string | number | boolean>>;
}

const VERTEX = /* glsl */ `
precision highp float;
precision highp int;

uniform mat4 modelViewMatrix;
uniform mat4 projectionMatrix;
uniform mat4 viewMatrix;
uniform mat4 modelMatrix;
uniform mat3 normalMatrix;
uniform vec3 cameraPosition;

uniform vec3 uDif;
uniform vec3 uAmb;
uniform vec3 uAmbRender;
uniform vec3 uSkinAmb;
uniform vec3 uLightDir;
uniform vec2 uFogRange;
uniform float uNight;
#ifdef HAIR
#ifdef INSTANCED_SKIN
in vec3 aHairColor;
#define HAIR_COLOR aHairColor
#else
uniform vec3 uHairColor;
#define HAIR_COLOR uHairColor
#endif
uniform float uHairLit;
#endif

in vec3 position;
in vec3 normal;
in vec2 uv;
//@SKIN_DECLARATIONS
#ifdef MERGED_SLOTS
in float aSlot;
flat out float vSlot;
#endif

out vec2 vUv;
out vec3 vColor;
out float vFog;
#ifdef REFLECT
out vec2 vReflectUv;
#endif

void main() {
#ifdef NIGHT_ONLY
  if (uNight < 0.5) {
    // light objects are drawn only at night: put every vertex outside the clip volume
    gl_Position = vec4(2.0, 2.0, 2.0, 1.0);
    vUv = vec2(0.0);
    vColor = vec3(0.0);
    vFog = 1.0;
    return;
  }
#endif
  vec3 pos = position;
  vec3 nrm = normal;
  //@SKIN_APPLY
  vec4 mv = modelViewMatrix * vec4(pos, 1.0);
  gl_Position = projectionMatrix * mv;
  vUv = uv;
#ifdef MERGED_SLOTS
  vSlot = aSlot;
#endif

#ifdef UNLIT
  vColor = vec3(1.0);
#else
  #if defined(GEO_SKINNED) && defined(USE_PALETTE)
  // vs_1_1: the palette-blended normal is NOT renormalised (KEEP-QUIRK); the object scale is divided out like the
  // original's unit light vector in model space (world = scale x rotation, uniform scale)
  vec3 nWorld = (mat3(modelMatrix) * nrm) / length(modelMatrix[0].xyz);
  float ndl = max(0.0, dot(nWorld, -uLightDir));
  vColor = min(ndl * uDif + uSkinAmb, vec3(1.0));
  #else
  vec3 lightView = mat3(viewMatrix) * uLightDir;
  vec3 nView = normalMatrix * nrm;
  #ifdef GEO_SKINNED
  float ndl = max(0.0, dot(normalize(nView), -lightView));
  vColor = min(ndl * uDif + uSkinAmb, vec3(1.0));
  #else
  float ndl = max(0.0, dot(nView, -lightView));
  vColor = clamp(uAmb + uAmbRender + uDif * ndl, 0.0, 1.0);
  #endif
  #endif
  #ifdef HAIR
  // ModelObject.cpp:1147-1184: diffuse 0, ambient = hair colour x fblack, fblack = max(0.3, avg(dif x 1.5) x 0.6);
  // with lighting off (previews) the ambient is the hair colour itself
  float fblack = max(0.3, dot(uDif, vec3(0.3)));
  vColor = mix(HAIR_COLOR, min(HAIR_COLOR * fblack, vec3(1.0)), uHairLit);
  #endif
#endif

#ifdef REFLECT
  vec3 nr = normalize(normalMatrix * nrm);
  // g_mReflect: u = 0.5 * nx + 0.5, v = -0.5 * ny + 0.5 with the camera-space normal
  vReflectUv = vec2(0.5 * nr.x + 0.5, -0.5 * nr.y + 0.5);
#endif

#if defined(FOG_VERTEX)
  float dist = length(mv.xyz);
  vFog = clamp((uFogRange.y - dist) / (uFogRange.y - uFogRange.x), 0.0, 1.0);
#elif defined(FOG_OBJECT)
  #ifdef INSTANCED_SKIN
  float dist = length(vec3(aInst0.w, aInst1.w, aInst2.w) - cameraPosition);
  #else
  float dist = length(modelMatrix[3].xyz - cameraPosition);
  #endif
  vFog = clamp((uFogRange.y - dist) / (uFogRange.y - uFogRange.x), 0.0, 1.0);
#else
  vFog = 1.0;
#endif
}
`;

const FRAGMENT = /* glsl */ `
precision highp float;
precision highp int;
precision highp sampler2D;

#ifdef MERGED_SLOTS
// merged parts of one mover: the texture of a vertex is one of 8 samplers, chosen by the flat slot index
uniform sampler2D uMap0;
uniform sampler2D uMap1;
uniform sampler2D uMap2;
uniform sampler2D uMap3;
uniform sampler2D uMap4;
uniform sampler2D uMap5;
uniform sampler2D uMap6;
uniform sampler2D uMap7;
flat in float vSlot;
#else
uniform sampler2D uMap;
#endif
uniform float uAlpha;
uniform vec3 uFogColor;
#ifdef REFLECT
uniform sampler2D uReflectMap;
#endif

in vec2 vUv;
in vec3 vColor;
in float vFog;
#ifdef REFLECT
in vec2 vReflectUv;
#endif

layout(location = 0) out highp vec4 fragColor;

void main() {
#ifdef MERGED_SLOTS
  // explicit gradients: the derivatives are taken in uniform control flow, the branches only sample
  vec2 dUvX = dFdx(vUv);
  vec2 dUvY = dFdy(vUv);
  int slot = int(vSlot + 0.5);
  vec4 t;
  if (slot == 0) t = textureGrad(uMap0, vUv, dUvX, dUvY);
  else if (slot == 1) t = textureGrad(uMap1, vUv, dUvX, dUvY);
  else if (slot == 2) t = textureGrad(uMap2, vUv, dUvX, dUvY);
  else if (slot == 3) t = textureGrad(uMap3, vUv, dUvX, dUvY);
  else if (slot == 4) t = textureGrad(uMap4, vUv, dUvX, dUvY);
  else if (slot == 5) t = textureGrad(uMap5, vUv, dUvX, dUvY);
  else if (slot == 6) t = textureGrad(uMap6, vUv, dUvX, dUvY);
  else t = textureGrad(uMap7, vUv, dUvX, dUvY);
#else
  vec4 t = texture(uMap, vUv);
#endif
#ifdef ALPHA_TEST
  // half a step below the reference so that 8-bit alpha 0xB0 passes exactly
  if (t.a < ALPHA_REF) discard;
#endif
  vec3 c = t.rgb * vColor;
#ifdef REFLECT
  vec3 r = texture(uReflectMap, vReflectUv).rgb;
  c = clamp(2.0 * (c + r - 0.5), 0.0, 1.0);
#endif
#ifdef FOG
  c = mix(uFogColor, c, vFog);
#endif
#ifdef ALPHA_SCALE
  float a = t.a * uAlpha;
#else
  float a = 1.0;
#endif
  fragColor = vec4(c, a);
}
`;

/**
 * The skinning hook of WP-REND-04. Skinned objects are drawn in BIND POSE until a chunk is registered: the chunk
 * declares its uniforms and the extra attributes (`aJoints` u16 x 2 as `uvec2`, `aWeights` f32 x 2 as `vec2`, both
 * present on skinned geometry) and `apply` transforms the local variables `pos` and `nrm` (model-space position and
 * normal of the vertex) through the bone palette; `defines` are added for skinned keys (`USE_PALETTE` is always set).
 * Register `null` to go back to the bind pose.
 */
export interface SkinningChunk {
  /** GLSL declarations (uniforms, attributes, helper functions) inserted after the attribute declarations. */
  readonly declarations: string;
  /** GLSL statements that modify `pos` and `nrm`, inserted before the model-view transform. */
  readonly apply: string;
  readonly defines?: Readonly<Record<string, string | number>>;
}

let skinning: SkinningChunk | null = null;

/** Installs (or removes) the skinning chunk of the vertex shader of skinned keys. */
export function registerSkinningChunk(chunk: SkinningChunk | null): void {
  skinning = chunk;
}

/** Texels per row of the shared bone-palette texture (`movers/PaletteTexture.ts`; a multiple of 3). */
export const PALETTE_WIDTH = 1536;

/**
 * The built-in palette skinning of WP-REND-04 (Plan/41 section 12.6; the decoded D3D `vs_1_1` of ref/18 section
 * 7.2): two weights, joints are global `.chr` bone indices, `uPaletteBase` is the first matrix of the mover in the
 * shared `RGBA32F` palette texture (`uPalette`, three texels per matrix = the three rows of the 3x4 matrix);
 *
 *   pos = w1 * M[base + jA] * (v, 1) + w2 * M[base + jB] * (v, 1)      nrm likewise with w = 0 (NOT renormalised)
 *
 * (`w2 == 0` makes joint B irrelevant, the pack sets it to joint A). Texel fetches never straddle a row because
 * 1536 is a multiple of 3. A material that uses it MUST provide the `uPalette` and `uPaletteBase` uniforms and
 * the `aJoints`/`aWeights` attributes of skinned geometry (`ModelCache`).
 */
export const PALETTE_SKINNING: SkinningChunk = {
  declarations: /* glsl */ `
in uvec2 aJoints;
in vec2 aWeights;
uniform highp sampler2D uPalette;
uniform float uPaletteBase;
vec3 skinXf(int m, vec4 p) {
  int t = m * 3;
  ivec2 c = ivec2(t % ${String(PALETTE_WIDTH)}, t / ${String(PALETTE_WIDTH)});
  return vec3(
    dot(texelFetch(uPalette, c, 0), p),
    dot(texelFetch(uPalette, c + ivec2(1, 0), 0), p),
    dot(texelFetch(uPalette, c + ivec2(2, 0), 0), p)
  );
}
`,
  apply: /* glsl */ `
  {
    int skinBase = int(uPaletteBase + 0.5);
    int skinA = skinBase + int(aJoints.x);
    int skinB = skinBase + int(aJoints.y);
    vec4 skinP = vec4(pos, 1.0);
    vec4 skinN = vec4(nrm, 0.0);
    pos = aWeights.x * skinXf(skinA, skinP) + aWeights.y * skinXf(skinB, skinP);
    nrm = aWeights.x * skinXf(skinA, skinN) + aWeights.y * skinXf(skinB, skinN);
  }
`,
  defines: {},
};

/**
 * The INSTANCED variant of the palette skinning (WP-REND-12, Plan/41 section 12.7): many movers that show the same geometry and
 * textures are ONE draw. Per instance: the world matrix as the three ROWS of the render view's 3x4 matrix (`aInst0..2`, the
 * translation in `w`, so `world * p = (dot(aInst0, p4), dot(aInst1, p4), dot(aInst2, p4))`), the first palette matrix of the
 * mover (`aPalBase`) and, for hair, the colour (`aHairColor`). The skinned vertex goes through the palette first (model space),
 * then through the world matrix; the normal is divided by the uniform scale like the non-instanced `USE_PALETTE` lighting
 * (so the mesh's own `modelMatrix` is the identity there). The fog distance uses the instance origin.
 */
export const INSTANCED_PALETTE_SKINNING: SkinningChunk = {
  declarations: /* glsl */ `
in uvec2 aJoints;
in vec2 aWeights;
in vec4 aInst0;
in vec4 aInst1;
in vec4 aInst2;
in float aPalBase;
uniform highp sampler2D uPalette;
vec3 skinXf(int m, vec4 p) {
  int t = m * 3;
  ivec2 c = ivec2(t % ${String(PALETTE_WIDTH)}, t / ${String(PALETTE_WIDTH)});
  return vec3(
    dot(texelFetch(uPalette, c, 0), p),
    dot(texelFetch(uPalette, c + ivec2(1, 0), 0), p),
    dot(texelFetch(uPalette, c + ivec2(2, 0), 0), p)
  );
}
`,
  apply: /* glsl */ `
  {
    int skinBase = int(aPalBase + 0.5);
    int skinA = skinBase + int(aJoints.x);
    int skinB = skinBase + int(aJoints.y);
    vec4 skinP = vec4(pos, 1.0);
    vec4 skinN = vec4(nrm, 0.0);
    vec3 sp = aWeights.x * skinXf(skinA, skinP) + aWeights.y * skinXf(skinB, skinP);
    vec3 sn = aWeights.x * skinXf(skinA, skinN) + aWeights.y * skinXf(skinB, skinN);
    vec4 wp = vec4(sp, 1.0);
    pos = vec3(dot(aInst0, wp), dot(aInst1, wp), dot(aInst2, wp));
    float wScale = length(vec3(aInst0.x, aInst1.x, aInst2.x));
    nrm = vec3(dot(aInst0.xyz, sn), dot(aInst1.xyz, sn), dot(aInst2.xyz, sn)) / wScale;
  }
`,
  defines: { INSTANCED_SKIN: 1 },
};

/** Options of {@link modelShader}. */
export interface ModelShaderOptions {
  /**
   * Skinned keys read the bone palette (`PALETTE_SKINNING`) instead of the bind pose. Without it a skinned key uses
   * the chunk of `registerSkinningChunk` when one is registered and the bind pose otherwise.
   */
  readonly palette?: boolean;
  /**
   * Merged parts (`movers/merge.ts`): the per-vertex `aSlot` attribute selects one of the samplers `uMap0 .. uMap7`
   * (palette skinning is required: merged geometry is skinned).
   */
  readonly slots?: boolean;
  /**
   * Instanced skinned draw (`INSTANCED_PALETTE_SKINNING`): per-instance world rows, palette base and hair colour attributes
   * instead of the mesh's matrices and `uPaletteBase` / `uHairColor`. Requires `palette`.
   */
  readonly instanced?: boolean;
}

/** The shader source and defines of a model material key. Only the rigid and skinned families live here. */
export function modelShader(key: MaterialKey, options: ModelShaderOptions = {}): ShaderSource {
  if (key.geo !== Geo.Rigid && key.geo !== Geo.Skinned) {
    throw new Error(`modelShader: geo ${String(key.geo)} is not a model family`);
  }
  const defines: Record<string, string | number | boolean> = {};
  if (key.geo === Geo.Skinned) defines['GEO_SKINNED'] = 1;
  if (key.unlit) defines['UNLIT'] = 1;
  if (key.nightOnly) defines['NIGHT_ONLY'] = 1;
  if (key.reflect) defines['REFLECT'] = 1;
  if (key.hair && key.geo === Geo.Skinned) defines['HAIR'] = 1;
  if (key.alphaTest) {
    defines['ALPHA_TEST'] = 1;
    defines['ALPHA_REF'] = ((OPACITY_ALPHA_REF - 0.5) / 255).toFixed(8);
  }
  if (key.blend === 'alpha' || key.blend === 'mul') defines['ALPHA_SCALE'] = 1;
  if (key.fog === 'vertex') {
    defines['FOG'] = 1;
    defines['FOG_VERTEX'] = 1;
  } else if (key.fog === 'object') {
    defines['FOG'] = 1;
    defines['FOG_OBJECT'] = 1;
  }
  if (options.slots === true && key.geo === Geo.Skinned) defines['MERGED_SLOTS'] = 1;
  let vertex = VERTEX;
  const palette =
    options.instanced === true && options.palette === true
      ? INSTANCED_PALETTE_SKINNING
      : PALETTE_SKINNING;
  const chunk = key.geo === Geo.Skinned ? (options.palette === true ? palette : skinning) : null;
  if (chunk) {
    defines['USE_PALETTE'] = 1;
    Object.assign(defines, chunk.defines);
    vertex = vertex
      .replace('//@SKIN_DECLARATIONS', chunk.declarations)
      .replace('//@SKIN_APPLY', chunk.apply);
  }
  return { vertex, fragment: FRAGMENT, defines };
}
