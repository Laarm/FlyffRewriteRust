// Shaders and materials of the sky, the lens flare and the weather (Plan/41 sections 14, 15; SkyBox.cpp 330-745, 850-1094).
// The fixed-function stage setup of the original, per element:
//
//   cylinder / dome   colour = texture, alpha = vertex alpha x TFACTOR alpha (the layer weight), SRCALPHA / INVSRCALPHA
//   sun disk          colour = texture x white, ONE / ONE
//   moon              colour = texture, alpha = texture alpha, SRCALPHA / ONE
//   glow, flare       colour = texture x vertex colour, alpha = texture alpha x vertex alpha, SRCALPHA / INVSRCALPHA
//   rain              untextured grey 128, alpha = vertex alpha (0 at the top, 100 at the bottom), SRCALPHA / ONE
//   snow              colour = texture x 200, alpha 200, SRCALPHA / ONE
//
// Everything is drawn with depth write off and fog off; the sky, the moon and the sun also with depth test off (the weather
// keeps the depth test: the terrain hides the drops below the ground). Colours are the original 8-bit numbers (gamma-naive).

import {
  AddEquation,
  CustomBlending,
  DoubleSide,
  GLSL3,
  NormalBlending,
  OneFactor,
  RawShaderMaterial,
  SrcAlphaFactor,
  Vector2,
  Vector3,
  Vector4,
  type Texture,
} from 'three';

const LAYER_VERTEX = /* glsl */ `
precision highp float;
uniform mat4 modelViewMatrix;
uniform mat4 projectionMatrix;
uniform vec2 uUvOffset;
in vec3 position;
in vec2 uv;
in float aAlpha;
out vec2 vUv;
out float vAlpha;
void main() {
  vUv = uv + uUvOffset;
  vAlpha = aAlpha;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}
`;

const LAYER_FRAGMENT = /* glsl */ `
precision highp float;
precision highp sampler2D;
uniform sampler2D uMap;
uniform float uWeight;
in vec2 vUv;
in float vAlpha;
layout(location = 0) out highp vec4 fragColor;
void main() {
  fragColor = vec4(texture(uMap, vUv).rgb, vAlpha * uWeight);
}
`;

const BODY_VERTEX = /* glsl */ `
precision highp float;
uniform mat4 modelViewMatrix;
uniform mat4 projectionMatrix;
in vec3 position;
in vec2 uv;
out vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}
`;

const BODY_FRAGMENT = /* glsl */ `
precision highp float;
precision highp sampler2D;
uniform sampler2D uMap;
uniform vec4 uColor;
in vec2 vUv;
layout(location = 0) out highp vec4 fragColor;
void main() {
  vec4 t = texture(uMap, vUv);
  fragColor = vec4(t.rgb * uColor.rgb, t.a * uColor.a);
}
`;

const FALL_VERTEX = /* glsl */ `
precision highp float;
uniform mat4 viewMatrix;
uniform mat4 projectionMatrix;
uniform vec3 uRight;
uniform vec3 uUp;
in vec3 position;
in vec2 uv;
in float aAlpha;
in vec3 aOffset;
out vec2 vUv;
out float vAlpha;
void main() {
  vUv = uv;
  vAlpha = aAlpha;
  vec3 p = aOffset + uRight * position.x + uUp * position.y;
  gl_Position = projectionMatrix * viewMatrix * vec4(p, 1.0);
}
`;

const RAIN_FRAGMENT = /* glsl */ `
precision highp float;
in vec2 vUv;
in float vAlpha;
layout(location = 0) out highp vec4 fragColor;
void main() {
  fragColor = vec4(vec3(128.0 / 255.0), vAlpha);
}
`;

const SNOW_FRAGMENT = /* glsl */ `
precision highp float;
precision highp sampler2D;
uniform sampler2D uMap;
in vec2 vUv;
in float vAlpha;
layout(location = 0) out highp vec4 fragColor;
void main() {
  fragColor = vec4(texture(uMap, vUv).rgb * (200.0 / 255.0), vAlpha);
}
`;

/** Uniforms of the cylinder / dome materials. */
export interface LayerUniforms {
  uMap: { value: Texture | null };
  uWeight: { value: number };
  uUvOffset: { value: Vector2 };
}

export type LayerMaterial = RawShaderMaterial & { readonly uniforms: LayerUniforms };

/** One layer of the cylinder or the dome: alpha blended, no depth. */
export function createLayerMaterial(name: string): LayerMaterial {
  return new RawShaderMaterial({
    name,
    glslVersion: GLSL3,
    vertexShader: LAYER_VERTEX,
    fragmentShader: LAYER_FRAGMENT,
    uniforms: {
      uMap: { value: null },
      uWeight: { value: 1 },
      uUvOffset: { value: new Vector2(0, 0) },
    },
    side: DoubleSide,
    transparent: true,
    blending: NormalBlending,
    depthTest: false,
    depthWrite: false,
  }) as LayerMaterial;
}

/** `ONE / ONE` (the sun disk), `SRCALPHA / ONE` (the moon) or `SRCALPHA / INVSRCALPHA` (glow, flares). */
export type BodyBlend = 'add' | 'srcAlphaOne' | 'alpha';

export interface BodyUniforms {
  uMap: { value: Texture | null };
  /** Vertex colour: rgb 0..1 and alpha 0..1. */
  uColor: { value: Vector4 };
}

export type BodyMaterial = RawShaderMaterial & { readonly uniforms: BodyUniforms };

export function createBodyMaterial(name: string, blend: BodyBlend): BodyMaterial {
  const base = {
    name,
    glslVersion: GLSL3,
    vertexShader: BODY_VERTEX,
    fragmentShader: BODY_FRAGMENT,
    uniforms: { uMap: { value: null }, uColor: { value: new Vector4(1, 1, 1, 1) } },
    side: DoubleSide,
    transparent: true,
    depthTest: false,
    depthWrite: false,
  } as const;
  if (blend === 'alpha')
    return new RawShaderMaterial({ ...base, blending: NormalBlending }) as BodyMaterial;
  return new RawShaderMaterial({
    ...base,
    blending: CustomBlending,
    blendEquation: AddEquation,
    blendSrc: blend === 'add' ? OneFactor : SrcAlphaFactor,
    blendDst: OneFactor,
  }) as BodyMaterial;
}

export interface FallUniforms {
  uRight: { value: Vector3 };
  uUp: { value: Vector3 };
  uMap: { value: Texture | null };
}

export type FallMaterial = RawShaderMaterial & { readonly uniforms: FallUniforms };

/** Rain (`snow = false`) or snow: additive-ish `SRCALPHA / ONE`, depth test on, depth write off. */
export function createFallMaterial(name: string, snow: boolean): FallMaterial {
  return new RawShaderMaterial({
    name,
    glslVersion: GLSL3,
    vertexShader: FALL_VERTEX,
    fragmentShader: snow ? SNOW_FRAGMENT : RAIN_FRAGMENT,
    uniforms: {
      uRight: { value: new Vector3(1, 0, 0) },
      uUp: { value: new Vector3(0, 1, 0) },
      uMap: { value: null },
    },
    side: DoubleSide,
    transparent: true,
    blending: CustomBlending,
    blendEquation: AddEquation,
    blendSrc: SrcAlphaFactor,
    blendDst: OneFactor,
    depthTest: true,
    depthWrite: false,
  }) as FallMaterial;
}
