// The instanced variant of the rigid model shader (Plan/41 section 11.3, R7). Static objects share ONE vertex buffer
// per model object and draw all their placements with a per-instance 3 x 4 matrix; the lighting, fog, alpha test,
// reflect and night rules are exactly those of the non-instanced `modelShader` because this file only PATCHES its source:
//
//   * the instance attributes are declared at the `//@SKIN_DECLARATIONS` marker and applied at `//@SKIN_APPLY`
//     (the hook points of the skinning chunk; rigid keys never use them, so the markers are still in the source);
//   * `aInst0..2` hold the three columns of the matrix (xyz) and the translation (w) in THREE.JS space, composed on the CPU
//     as `M_world * M_local(object chain)`; the model-view matrix of the instanced mesh is the plain view matrix;
//   * the normal matrix is the inverse transpose of that 3 x 3 (cofactors / determinant), NOT renormalised like D3D's
//     fixed pipeline (a scale-2 object has |N| = 1/2, KEEP-QUIRK K4); mirrored instances (negative determinant) keep the
//     screen-space winding like D3D, so the material stays `FrontSide` (Plan/41 section 3.1).
//
// `assertPatchable` fails loudly when the base shader no longer has the markers (a unit test runs it).

import type { Material } from 'three';
import { Geo, type MaterialKey } from '../gl/materials/key';
import { modelShader, type ShaderSource } from '../gl/materials/modelShader';

const DECLARATIONS = /* glsl */ `
in vec4 aInst0;
in vec4 aInst1;
in vec4 aInst2;
`;

const APPLY = /* glsl */ `
  {
    mat3 im = mat3(aInst0.xyz, aInst1.xyz, aInst2.xyz);
    float idet = dot(im[0], cross(im[1], im[2]));
    // inverse transpose = cofactor matrix / determinant (columns are the cross products of the other two columns)
    mat3 inm = mat3(cross(im[1], im[2]), cross(im[2], im[0]), cross(im[0], im[1])) / (abs(idet) > 1e-12 ? idet : 1.0);
    pos = im * pos + vec3(aInst0.w, aInst1.w, aInst2.w);
    nrm = inm * nrm;
  }
`;

/** The shader of an instanced rigid key. Throws for skinned keys (statics with skinned parts use `ModelInstance`). */
export function instancedShader(key: MaterialKey): ShaderSource {
  if (key.geo !== Geo.Rigid) throw new Error('instanced draws are rigid only');
  const base = modelShader({ ...key, instanced: true });
  assertPatchable(base.vertex);
  return {
    vertex: base.vertex
      .replace('//@SKIN_DECLARATIONS', DECLARATIONS)
      .replace('//@SKIN_APPLY', APPLY),
    fragment: base.fragment,
    defines: { ...base.defines, INSTANCED: 1 },
  };
}

/** Throws unless the vertex source carries both markers. */
export function assertPatchable(vertex: string): void {
  if (!vertex.includes('//@SKIN_DECLARATIONS') || !vertex.includes('//@SKIN_APPLY')) {
    throw new Error('modelShader lost the skinning markers the instanced variant is built on');
  }
}

/** The shader strings a material needs to switch to the instanced variant (applied after `MaterialFactory.create`). */
export function makeInstanced(
  material: Material & { vertexShader: string; defines?: object },
  key: MaterialKey,
): void {
  const src = instancedShader(key);
  material.vertexShader = src.vertex;
  material.defines = { ...src.defines };
  material.needsUpdate = true;
}
